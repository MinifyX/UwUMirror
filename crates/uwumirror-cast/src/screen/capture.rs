//! The screen, from Windows.Graphics.Capture.
//!
//! The newer of Windows' two ways to record the screen (the other is DXGI
//! desktop duplication): it draws the mouse pointer into the picture itself,
//! works across graphics cards on laptops with two, and shows the yellow
//! frame around the screen that tells the person in front of it that it is
//! being recorded. Frames come only when something changes; the latest is
//! kept in a texture of our own, so a still screen is still a picture.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use windows::core::{Interface, Result};
use windows::Foundation::Metadata::ApiInformation;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, LUID, POINT};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11Texture2D,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Frames Windows may have ready at once. Each is copied out and handed back
/// at once, so two are plenty: one being filled while the other is copied.
/// More would only let frames wait.
const BUFFERS: i32 = 2;

/// The graphics card everything runs on: capture, conversion, and — when its
/// encoder takes textures — encoding.
pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    /// Which card, so the encoder found is this card's.
    pub luid: LUID,
}

impl Gpu {
    pub fn new() -> Result<Self> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&[
                    D3D_FEATURE_LEVEL_11_1,
                    D3D_FEATURE_LEVEL_11_0,
                    D3D_FEATURE_LEVEL_10_1,
                    D3D_FEATURE_LEVEL_10_0,
                ]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device: ID3D11Device = device.expect("D3D11CreateDevice gave a device");
        let context = context.expect("D3D11CreateDevice gave a context");
        // A hardware encoder uses the device from threads of its own.
        if let Ok(multithread) = device.cast::<ID3D11Multithread>() {
            unsafe {
                let _ = multithread.SetMultithreadProtected(true);
            }
        }
        let luid = unsafe {
            device
                .cast::<IDXGIDevice>()?
                .GetAdapter()?
                .GetDesc()?
                .AdapterLuid
        };
        Ok(Self {
            device,
            context,
            luid,
        })
    }

    /// A BGRA texture the video processor can read.
    pub fn bgra_texture(&self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut texture))?
        };
        Ok(texture.expect("CreateTexture2D gave a texture"))
    }
}

/// Recording the primary screen until dropped.
pub struct Capture {
    device: IDirect3DDevice,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    size: SizeInt32,
    /// The latest frame, ours to read whenever.
    pub texture: ID3D11Texture2D,
    /// When Windows handed over the frame last taken into the texture, on
    /// [`counter`]'s clock; taken by whoever stamps it.
    pub captured: Option<i64>,
    arrivals: Arc<Arrivals>,
    /// The `FrameArrived` registration, removed when dropped.
    token: i64,
}

/// What the capture's `FrameArrived` handler tells the picture thread.
#[derive(Default)]
struct Arrivals {
    /// When the newest frame not yet taken came.
    latest: Mutex<Option<i64>>,
    came: Condvar,
}

/// The performance counter in 100 ns units: the clock everything on the
/// sending side is stamped with.
///
/// Not the frames' own `SystemRelativeTime`: on a 165 Hz screen that runs
/// ahead of this clock by up to 10 ms, a target time rather than a moment
/// that has passed. When Windows hands a frame over is what we can measure.
pub fn counter() -> i64 {
    static FREQUENCY: OnceLock<i64> = OnceLock::new();
    let frequency = *FREQUENCY.get_or_init(|| {
        let mut frequency = 0;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut frequency);
        }
        frequency.max(1)
    });
    let mut ticks = 0;
    unsafe {
        let _ = QueryPerformanceCounter(&mut ticks);
    }
    (i128::from(ticks) * 10_000_000 / i128::from(frequency)) as i64
}

fn pixels(size: SizeInt32) -> (u32, u32) {
    (size.Width.max(1) as u32, size.Height.max(1) as u32)
}

impl Capture {
    pub fn primary(gpu: &Gpu) -> Result<Self> {
        if !GraphicsCaptureSession::IsSupported()? {
            return Err(windows::core::Error::new(
                windows::Win32::Foundation::E_NOTIMPL,
                "this Windows can't record the screen (Windows 10 1903 or newer needed)",
            ));
        }
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(monitor)? };
        let device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&gpu.device.cast::<IDXGIDevice>()?)? }
                .cast()?;
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            BUFFERS,
            size,
        )?;
        let session = pool.CreateCaptureSession(&item)?;
        // The pointer is in the picture by default; said anyway, where the
        // setting exists (Windows 10 2004 and newer).
        if ApiInformation::IsPropertyPresent(
            &"Windows.Graphics.Capture.GraphicsCaptureSession".into(),
            &"IsCursorCaptureEnabled".into(),
        )
        .unwrap_or(false)
        {
            let _ = session.SetIsCursorCaptureEnabled(true);
        }
        // Free-threaded: the handler runs on a thread of Windows' pool the
        // moment a frame is ready, and only notes it; the frame is taken on
        // the picture thread.
        let arrivals = Arc::new(Arrivals::default());
        let token = pool.FrameArrived(&TypedEventHandler::new({
            let arrivals = arrivals.clone();
            move |_, _| {
                *arrivals.latest.lock() = Some(counter());
                arrivals.came.notify_one();
                Ok(())
            }
        }))?;
        // `MinUpdateInterval` (newer Windows 11) stays at its 16 ms. On a
        // 165 Hz screen that skips some of a 60 fps video's frames (10-20 %
        // in tests); lower, every change comes, the pointer's too, and the
        // frames that then wait their turn behind those cost more: capture
        // to received went from 4.4 to 9.8 ms at the 95th percentile.
        session.StartCapture()?;
        let (width, height) = pixels(size);
        let texture = gpu.bgra_texture(width, height)?;
        Ok(Self {
            device,
            pool,
            session,
            size,
            texture,
            captured: None,
            arrivals,
            token,
        })
    }

    /// The screen's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        pixels(self.size)
    }

    /// Waits until Windows has a new frame, at most `timeout`. True when
    /// there is one for [`Capture::update`].
    pub fn wait(&self, timeout: Duration) -> bool {
        let mut latest = self.arrivals.latest.lock();
        if latest.is_none() {
            self.arrivals.came.wait_for(&mut latest, timeout);
        }
        latest.is_some()
    }

    /// Takes the newest frame, if one came, into [`Capture::texture`]. True
    /// when the screen changed size and the texture is a new one.
    pub fn update(&mut self, gpu: &Gpu) -> Result<bool> {
        // The stamp first: a frame that comes in meanwhile is the newest
        // taken below, and counts as a little older than it is, never younger.
        let arrived = self.arrivals.latest.lock().take();
        let mut newest = None;
        // Only the newest counts; older ones go straight back to the pool.
        while let Ok(frame) = self.pool.TryGetNextFrame() {
            if let Some(older) = newest.replace(frame) {
                let _ = older.Close();
            }
        }
        let Some(frame) = newest else {
            return Ok(false);
        };
        let size = frame.ContentSize()?;
        if size != self.size && size.Width > 0 && size.Height > 0 {
            let _ = frame.Close();
            self.pool.Recreate(
                &self.device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                BUFFERS,
                size,
            )?;
            self.size = size;
            let (width, height) = pixels(size);
            self.texture = gpu.bgra_texture(width, height)?;
            return Ok(true);
        }
        let surface = frame.Surface()?;
        let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let source: ID3D11Texture2D = unsafe { access.GetInterface()? };
        unsafe { gpu.context.CopyResource(&self.texture, &source) };
        // Back to the pool at once: the copy is queued before the frame can
        // be reused, and capture never waits for a buffer of ours.
        let _ = frame.Close();
        self.captured = Some(arrived.unwrap_or_else(counter));
        Ok(false)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}
