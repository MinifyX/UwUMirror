//! Decoded pictures into the page (Miracast's).
//!
//! H.264 is small; a decoded 1080p picture is 3 MB in NV12, 180 MB a second
//! at 60 frames. Tauri's channel hands each message over by copying it
//! through WebView2's resource handler, and that couldn't keep up (see
//! `docs/architecture.md`). So on Windows the pictures travel through
//! WebView2's **shared buffers**: memory mapped into both processes, written
//! here and read by the page's `VideoFrame` without another copy in between.
//!
//! There are three slots, each the size of the biggest picture
//! (1920 × 1080). A picture goes into a free slot, and WebView2 posts that
//! slot to the page with the picture's size and time
//! (`PostSharedBufferToScript`, an event on the page). The first byte of a
//! slot says who has it: 1 while the page has it, 0 again once the page has
//! made its `VideoFrame` from it. A picture that finds no slot free is
//! dropped — a page that falls behind skips pictures, never queues them.
//!
//! Where shared buffers aren't there (another system, an old WebView2), the
//! pictures go through the video channel like H.264, at most two at a time:
//! the page says when it has drawn one (`frame_done`).

use std::sync::atomic::{AtomicU32, Ordering};

#[cfg(windows)]
use parking_lot::Mutex;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::AppHandle;
use uwumirror_core::RawFrame;

/// Pictures sent through the channel that the page hasn't drawn yet, at most.
const MAX_IN_FLIGHT: u32 = 2;

/// `counter` changed by `change`, unless it says no (`None`): whether it did.
/// (Spelled out because `fetch_update` is deprecated in newer Rust and its
/// successor `try_update` isn't in older.)
fn update(counter: &AtomicU32, change: impl Fn(u32) -> Option<u32>) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = change(current) else {
            return false;
        };
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

/// Bytes in front of a picture in a shared slot; byte 0 is the owner flag.
/// Keep in step with `FRAME_HEADER` in `lib/api.ts`.
#[cfg_attr(not(windows), allow(dead_code))]
pub const SLOT_HEADER: usize = 64;

/// A picture as a message on the video channel; the layout is in `hub.rs`.
pub fn encode_frame(id: u64, frame: &RawFrame) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(25 + frame.data.len());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.push(2);
    bytes.extend_from_slice(&frame.pts_us.to_le_bytes());
    bytes.extend_from_slice(&frame.width.to_le_bytes());
    bytes.extend_from_slice(&frame.height.to_le_bytes());
    bytes.extend_from_slice(&frame.data);
    bytes
}

pub struct FrameOut {
    #[cfg_attr(not(windows), allow(dead_code))]
    app: AppHandle,
    in_flight: AtomicU32,
    #[cfg(windows)]
    shared: Mutex<shared::Ring>,
}

impl FrameOut {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            in_flight: AtomicU32::new(0),
            // The channel can be forced, to compare the two.
            #[cfg(windows)]
            shared: Mutex::new(
                if std::env::var_os("UWUMIRROR_FRAMES_VIA_CHANNEL").is_some() {
                    shared::Ring::Unavailable
                } else {
                    shared::Ring::default()
                },
            ),
        }
    }

    /// The page (re)subscribed: whatever the old one held is free again.
    pub fn reset(&self) {
        self.in_flight.store(0, Ordering::Relaxed);
        #[cfg(windows)]
        self.shared.lock().reset();
    }

    /// The page drew a picture that came through the channel.
    pub fn done(&self) {
        update(&self.in_flight, |n| n.checked_sub(1));
    }

    /// Hands `frame` to the page, or drops it when the page is busy.
    pub fn send(
        self: &std::sync::Arc<Self>,
        id: u64,
        frame: &RawFrame,
        channel: Option<&Channel<InvokeResponseBody>>,
    ) {
        #[cfg(windows)]
        match shared::send(self, id, frame) {
            shared::Sent::Yes | shared::Sent::Dropped => return,
            shared::Sent::NoSharedBuffers => {}
        }
        let Some(channel) = channel else { return };
        if !update(&self.in_flight, |n| (n < MAX_IN_FLIGHT).then_some(n + 1)) {
            return;
        }
        if channel
            .send(InvokeResponseBody::Raw(encode_frame(id, frame)))
            .is_err()
        {
            self.done();
        }
    }
}

#[cfg(windows)]
mod shared {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use tauri::Manager;
    use uwumirror_core::RawFrame;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment12, ICoreWebView2SharedBuffer, ICoreWebView2_17,
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
    };
    use windows_core::{Interface, HSTRING};

    use super::{FrameOut, SLOT_HEADER};

    const SLOTS: usize = 3;
    /// The biggest picture a slot takes: 1920 × 1080 either way round.
    const SLOT_PICTURE: usize = (uwumirror_miracast::MAX_LONG_SIDE as usize)
        * (uwumirror_miracast::MAX_SHORT_SIDE as usize)
        * 3
        / 2;
    /// A slot the page hasn't given back for this long, it has lost (it was
    /// reloaded, say): it is ours again.
    const RECLAIM_AFTER: Duration = Duration::from_secs(1);

    pub struct Slot {
        buffer: ICoreWebView2SharedBuffer,
        memory: *mut u8,
        given: Option<Instant>,
    }

    impl Slot {
        fn flag(&self) -> &AtomicU8 {
            // SAFETY: `memory` is the start of the buffer's mapping, which
            // lives as long as `buffer`; the page writes the same byte.
            unsafe { AtomicU8::from_ptr(self.memory) }
        }
    }

    #[derive(Default)]
    pub enum Ring {
        /// Nothing asked yet: made on the first picture.
        #[default]
        Unknown,
        Creating,
        Ready {
            webview: ICoreWebView2_17,
            slots: Vec<Slot>,
        },
        /// This WebView2 has no shared buffers; the channel it is.
        Unavailable,
    }

    // SAFETY: the WebView2 objects in here are only ever called on the main
    // thread (inside `with_webview`); other threads only read and write the
    // mapped memory, and the flag byte decides whose it is.
    unsafe impl Send for Ring {}

    impl Ring {
        pub fn reset(&mut self) {
            if let Ring::Ready { slots, .. } = self {
                for slot in slots {
                    slot.flag().store(0, Ordering::Release);
                    slot.given = None;
                }
            }
        }
    }

    pub enum Sent {
        Yes,
        Dropped,
        NoSharedBuffers,
    }

    pub fn send(out: &Arc<FrameOut>, id: u64, frame: &RawFrame) -> Sent {
        let mut ring = out.shared.lock();
        let slot = match &mut *ring {
            Ring::Unavailable => return Sent::NoSharedBuffers,
            Ring::Creating => return Sent::Dropped,
            Ring::Unknown => {
                *ring = Ring::Creating;
                drop(ring);
                create(out);
                return Sent::Dropped;
            }
            Ring::Ready { slots, .. } => {
                if frame.data.len() > SLOT_PICTURE {
                    return Sent::NoSharedBuffers;
                }
                let free = slots.iter().position(|slot| {
                    slot.flag().load(Ordering::Acquire) == 0
                        || slot.given.is_some_and(|at| at.elapsed() > RECLAIM_AFTER)
                });
                let Some(index) = free else {
                    return Sent::Dropped;
                };
                let slot = &mut slots[index];
                // SAFETY: the slot is free, so the page doesn't read it, and
                // the picture fits behind the header (checked above).
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        frame.data.as_ptr(),
                        slot.memory.add(SLOT_HEADER),
                        frame.data.len(),
                    );
                }
                slot.flag().store(1, Ordering::Release);
                slot.given = Some(Instant::now());
                index
            }
        };
        drop(ring);
        let sent = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let meta = format!(
            r#"{{"uwumirror":"frame","id":{id},"width":{},"height":{},"pts":{},"sent":{sent:.1}}}"#,
            frame.width, frame.height, frame.pts_us
        );
        let window = out.app.get_webview_window("main");
        let posted = window.map(|window| {
            let out = out.clone();
            window.with_webview(move |_| post(&out, slot, &meta))
        });
        if !matches!(posted, Some(Ok(()))) {
            free(out, slot);
        }
        Sent::Yes
    }

    fn free(out: &FrameOut, index: usize) {
        if let Ring::Ready { slots, .. } = &mut *out.shared.lock() {
            slots[index].flag().store(0, Ordering::Release);
            slots[index].given = None;
        }
    }

    /// On the main thread: hands slot `index` to the page.
    fn post(out: &FrameOut, index: usize, meta: &str) {
        let result = match &*out.shared.lock() {
            Ring::Ready { webview, slots } => unsafe {
                webview.PostSharedBufferToScript(
                    &slots[index].buffer,
                    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_WRITE,
                    &HSTRING::from(meta),
                )
            },
            _ => return,
        };
        if let Err(error) = result {
            tracing::debug!(%error, "picture to the page");
            free(out, index);
        }
    }

    /// Makes the slots, on the main thread, where WebView2 wants it.
    fn create(out: &Arc<FrameOut>) {
        let Some(window) = out.app.get_webview_window("main") else {
            *out.shared.lock() = Ring::Unavailable;
            return;
        };
        let target = out.clone();
        let asked = window.with_webview(move |platform| {
            let made = (|| -> windows_core::Result<Ring> {
                let environment: ICoreWebView2Environment12 = platform.environment().cast()?;
                let webview: ICoreWebView2_17 =
                    unsafe { platform.controller().CoreWebView2()? }.cast()?;
                let mut slots = Vec::with_capacity(SLOTS);
                for _ in 0..SLOTS {
                    let buffer = unsafe {
                        environment.CreateSharedBuffer((SLOT_HEADER + SLOT_PICTURE) as u64)?
                    };
                    let mut memory = std::ptr::null_mut();
                    unsafe { buffer.Buffer(&mut memory)? };
                    slots.push(Slot {
                        buffer,
                        memory,
                        given: None,
                    });
                }
                Ok(Ring::Ready { webview, slots })
            })();
            *target.shared.lock() = match made {
                Ok(ring) => {
                    tracing::info!("pictures go to the page through shared buffers");
                    ring
                }
                Err(error) => {
                    tracing::warn!(%error, "no shared buffers; pictures go through the channel");
                    Ring::Unavailable
                }
            };
        });
        if asked.is_err() {
            *out.shared.lock() = Ring::Unavailable;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_message_layout() {
        let bytes = encode_frame(
            7,
            &RawFrame {
                width: 2,
                height: 2,
                pts_us: 9,
                data: vec![1, 2, 3, 4, 5, 6],
            },
        );
        assert_eq!(&bytes[..8], &7u64.to_le_bytes());
        assert_eq!(bytes[8], 2);
        assert_eq!(&bytes[9..17], &9u64.to_le_bytes());
        assert_eq!(&bytes[17..21], &2u32.to_le_bytes());
        assert_eq!(&bytes[21..25], &2u32.to_le_bytes());
        assert_eq!(&bytes[25..], &[1, 2, 3, 4, 5, 6]);
    }
}
