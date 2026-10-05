//! Playing a `MediaSource` and catching its pictures.
//!
//! `MediaPlayer` in frame-server mode draws nothing itself: it says when a
//! picture is ready (`VideoFrameAvailable`, on a worker thread of its own), and
//! `CopyFrameToVideoSurface` copies it — decoded, converted and scaled on the
//! graphics card — into a Direct3D texture we hand it. From there it is
//! copied into a staging texture the processor can read, packed and passed
//! on. No window, no dispatcher, no UI thread involved.
//!
//! Two settings matter: `RealTimePlayback`, without which the player buffers
//! like for a film and Miracast's picture stalls after a few frames; and the
//! texture in NV12, half the bytes of BGRA, which the page's `VideoFrame`
//! takes as it is.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use uwumirror_core::{EventSink, RawFrame, StreamEvent};
use windows::core::{IInspectable, Interface, Result};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::DirectX::Direct3D11::IDirect3DSurface;
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlayer, MediaPlayerFailedEventArgs};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGISurface;
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11SurfaceFromDXGISurface;

use crate::frame::{fit, pack_nv12};

/// How the player's pictures have been doing, for the log and the tests.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaybackStats {
    pub frames: u64,
    /// Pictures that couldn't be copied out.
    pub failed: u64,
    /// Time spent copying pictures out of the player, in total.
    pub copy_us: u64,
}

/// The textures one picture size needs.
struct Target {
    width: u32,
    height: u32,
    /// What the player draws into: on the card, a render target.
    texture: ID3D11Texture2D,
    /// Its copy the processor can map.
    staging: ID3D11Texture2D,
    /// `texture` as the WinRT surface `CopyFrameToVideoSurface` takes.
    surface: IDirect3DSurface,
}

struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    target: Option<Target>,
    /// The size the sender's picture has, before any scaling.
    natural: (u32, u32),
}

// SAFETY: a D3D11 device is free-threaded; its immediate context is not, and
// is only ever used under the mutex that holds this, as are the textures.
unsafe impl Send for Gpu {}

fn new_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
    staging: bool,
) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: if staging {
            D3D11_USAGE_STAGING
        } else {
            D3D11_USAGE_DEFAULT
        },
        BindFlags: if staging {
            0
        } else {
            (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32
        },
        CPUAccessFlags: if staging {
            D3D11_CPU_ACCESS_READ.0 as u32
        } else {
            0
        },
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
    texture.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_POINTER))
}

impl Gpu {
    fn new() -> Result<Self> {
        // The graphics card, or Windows' software rasterizer where there is
        // none (a VM, a server).
        let mut last = None;
        for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
            match Self::with_driver(driver) {
                Ok(gpu) => return Ok(gpu),
                Err(error) => last = Some(error),
            }
        }
        Err(last.expect("two drivers tried"))
    }

    fn with_driver(driver: D3D_DRIVER_TYPE) -> Result<Self> {
        let (mut device, mut context) = (None, None);
        unsafe {
            D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let missing = || windows::core::Error::from(windows::Win32::Foundation::E_POINTER);
        let device: ID3D11Device = device.ok_or_else(missing)?;
        let context: ID3D11DeviceContext = context.ok_or_else(missing)?;
        // Media Foundation may touch the device from its own threads while it
        // copies; ours are serialized by the mutex anyway.
        if let Ok(multithread) = context.cast::<ID3D11Multithread>() {
            unsafe {
                let _ = multithread.SetMultithreadProtected(true);
            }
        }
        Ok(Self {
            device,
            context,
            target: None,
            natural: (0, 0),
        })
    }

    fn target(&mut self, width: u32, height: u32) -> Result<&Target> {
        if !matches!(&self.target, Some(t) if t.width == width && t.height == height) {
            self.target = None;
            let texture = new_texture(&self.device, width, height, false)?;
            let staging = new_texture(&self.device, width, height, true)?;
            let dxgi: IDXGISurface = texture.cast()?;
            let surface: IDirect3DSurface =
                unsafe { CreateDirect3D11SurfaceFromDXGISurface(&dxgi)? }.cast()?;
            self.target = Some(Target {
                width,
                height,
                texture,
                staging,
                surface,
            });
        }
        Ok(self.target.as_ref().expect("just made"))
    }
}

/// Counted as pictures come, read whenever.
#[derive(Default)]
pub(crate) struct Counters {
    frames: AtomicU64,
    failed: AtomicU64,
    copy_us: AtomicU64,
}

impl Counters {
    pub(crate) fn snapshot(&self) -> PlaybackStats {
        PlaybackStats {
            frames: self.frames.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            copy_us: self.copy_us.load(Ordering::Relaxed),
        }
    }
}

/// Catches one player's pictures and passes them on as one stream's frames.
struct Grabber {
    id: u64,
    sink: EventSink,
    started: Instant,
    gpu: Mutex<Gpu>,
    counters: Arc<Counters>,
}

impl Grabber {
    fn frame(&self, player: &MediaPlayer) {
        let started = Instant::now();
        let counters = &self.counters;
        match self.copy(player) {
            Ok(Some(frame)) => {
                counters
                    .copy_us
                    .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
                if counters.frames.fetch_add(1, Ordering::Relaxed) == 0 {
                    tracing::info!(
                        id = self.id,
                        width = frame.width,
                        height = frame.height,
                        "Miracast: first picture"
                    );
                }
                (self.sink)(StreamEvent::Frame { id: self.id, frame });
            }
            Ok(None) => {}
            Err(error) => {
                if counters.failed.fetch_add(1, Ordering::Relaxed) == 0 {
                    tracing::warn!(%error, id = self.id, "Miracast: a picture couldn't be copied");
                }
            }
        }
    }

    fn copy(&self, player: &MediaPlayer) -> Result<Option<RawFrame>> {
        let session = player.PlaybackSession()?;
        let natural = (session.NaturalVideoWidth()?, session.NaturalVideoHeight()?);
        if natural.0 == 0 || natural.1 == 0 {
            return Ok(None);
        }
        let mut gpu = self.gpu.lock();
        if gpu.natural != natural {
            gpu.natural = natural;
            (self.sink)(StreamEvent::VideoSize {
                id: self.id,
                width: natural.0,
                height: natural.1,
            });
        }
        let (width, height) = fit(natural.0, natural.1);
        let context = gpu.context.clone();
        let target = gpu.target(width, height)?;
        player.CopyFrameToVideoSurface(&target.surface)?;
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let data = unsafe {
            context.CopyResource(&target.staging, &target.texture);
            context.Map(&target.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let pitch = mapped.RowPitch as usize;
            let rows = height as usize * 3 / 2;
            // SAFETY: a mapped NV12 staging texture holds `rows` rows of
            // `pitch` bytes, the last one at least `width` long.
            let bytes = std::slice::from_raw_parts(
                mapped.pData as *const u8,
                pitch * (rows - 1) + width as usize,
            );
            let data = pack_nv12(bytes, pitch, width as usize, height as usize);
            context.Unmap(&target.staging, 0);
            data
        };
        Ok(data.map(|data| RawFrame {
            width,
            height,
            pts_us: self.started.elapsed().as_micros() as u64,
            data,
        }))
    }
}

/// A `MediaPlayer` playing one source, its pictures going to one stream.
pub(crate) struct Playback {
    player: MediaPlayer,
    grabber: Arc<Grabber>,
    tokens: [i64; 3],
}

impl Playback {
    /// Plays `source`; `on_end` is told once if playing fails (with why) or
    /// the source ends (without).
    pub(crate) fn start(
        source: &MediaSource,
        id: u64,
        sink: EventSink,
        audio: bool,
        looping: bool,
        on_end: Arc<dyn Fn(Option<String>) + Send + Sync>,
    ) -> Result<Self> {
        let grabber = Arc::new(Grabber {
            id,
            sink,
            started: Instant::now(),
            gpu: Mutex::new(Gpu::new()?),
            counters: Arc::default(),
        });
        let player = MediaPlayer::new()?;
        // Without it the player buffers ahead like for a film, and Miracast's
        // picture stalls.
        player.SetRealTimePlayback(true)?;
        player.SetIsVideoFrameServerEnabled(true)?;
        player.SetIsMuted(!audio)?;
        player.SetIsLoopingEnabled(looping)?;
        let frame_token =
            player.VideoFrameAvailable(&TypedEventHandler::<MediaPlayer, IInspectable>::new({
                let grabber = grabber.clone();
                move |player, _| {
                    if let Some(player) = player.as_ref() {
                        grabber.frame(player);
                    }
                    Ok(())
                }
            }))?;
        let failed_token = player.MediaFailed(&TypedEventHandler::<
            MediaPlayer,
            MediaPlayerFailedEventArgs,
        >::new({
            let on_end = on_end.clone();
            move |_, args| {
                let reason = args
                    .as_ref()
                    .and_then(|args| args.ErrorMessage().ok())
                    .map(|message| message.to_string())
                    .filter(|message| !message.is_empty())
                    .unwrap_or_else(|| "the picture couldn't be played".into());
                // What Windows says beyond the message, which is often empty:
                // which kind of failure, and the HRESULT behind it.
                let error = args
                    .as_ref()
                    .and_then(|args| args.Error().ok())
                    .map(|e| e.0);
                let code = args
                    .as_ref()
                    .and_then(|args| args.ExtendedErrorCode().ok())
                    .map(|code| format!("{:#010x}", code.0 as u32));
                tracing::warn!(%reason, ?error, ?code, id, "Miracast: playback failed");
                // The code goes along: the page recognises a firewall that
                // holds the picture back by it (0xc00d4278, "the server
                // didn't answer in time").
                on_end(Some(match code {
                    Some(code) => format!("{reason}, {code}"),
                    None => reason,
                }));
                Ok(())
            }
        }))?;
        let ended_token = player.MediaEnded(
            &TypedEventHandler::<MediaPlayer, IInspectable>::new(move |_, _| {
                on_end(None);
                Ok(())
            }),
        )?;
        player.SetSource(source)?;
        player.Play()?;
        Ok(Self {
            player,
            grabber,
            tokens: [frame_token, failed_token, ended_token],
        })
    }

    pub(crate) fn set_audio(&self, on: bool) {
        if let Err(error) = self.player.SetIsMuted(!on) {
            tracing::warn!(%error, "Miracast: muting");
        }
    }

    pub(crate) fn counters(&self) -> Arc<Counters> {
        self.grabber.counters.clone()
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        let [frame, failed, ended] = self.tokens;
        let _ = self.player.RemoveVideoFrameAvailable(frame);
        let _ = self.player.RemoveMediaFailed(failed);
        let _ = self.player.RemoveMediaEnded(ended);
        let _ = self.player.Close();
        let stats = self.grabber.counters.snapshot();
        tracing::info!(
            id = self.grabber.id,
            frames = stats.frames,
            failed = stats.failed,
            copy_ms_per_frame = stats.copy_us as f64 / stats.frames.max(1) as f64 / 1000.0,
            "Miracast: playback over"
        );
    }
}
