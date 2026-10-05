//! H.264 from Media Foundation: the graphics card's encoder, or Windows' own.
//!
//! UwUMirror ships no codec, on either side: every Windows 10 and 11 has
//! Microsoft's H.264 encoder (in software), and every graphics card of the
//! last ten years one in hardware, registered with Media Foundation by its
//! driver (NVENC, Quick Sync, AMF). The card's is tried first — the one on the
//! card that captures, so frames never cross between cards — then Windows'
//! own.
//!
//! The two kinds work differently. Hardware encoders are asynchronous: they
//! say when they want a frame (`METransformNeedInput`) and when one is done
//! (`METransformHaveOutput`), and take frames as textures on the card. A
//! thread of its own waits for those events, so a finished frame is taken
//! out and on its way to the network the moment the encoder says so, not
//! when the picture thread next looks. Windows' encoder is synchronous and
//! takes frames in memory, so for it each NV12 frame is copied back from the
//! card first, and what comes out is taken right after.
//!
//! Settings for live pictures, latency first and bandwidth no object (it is
//! a local network): low latency mode, no B-frames, one reference frame,
//! the fastest of the encoder's presets, constant bit rate with a buffer of
//! a few frames — so no frame, not even a key frame, takes much longer to
//! send than any other — and a bit rate generous enough that speed costs no
//! sharpness ([`bit_rate_for`]). Key frames come when the receiver asks, and
//! every [`KEY_FRAME_SECONDS`] as a safety net; each goes out with SPS and
//! PPS in front of it, as from AirPlay and scrcpy, so a decoder can start
//! there.

use std::mem::ManuallyDrop;
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use windows::core::{Interface, Result, GUID, PWSTR};
use windows::Win32::Foundation::{E_FAIL, LUID, VARIANT_TRUE};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Main, CODECAPI_AVEncCommonBufferSize,
    CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonQualityVsSpeed,
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVEncVideoMaxNumRefFrame,
    CODECAPI_AVLowLatencyMode, ICodecAPI, IMF2DBuffer, IMFActivate, IMFAttributes,
    IMFDXGIDeviceManager, IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFShutdown,
    IMFTransform, METransformHaveOutput, METransformNeedInput, MFCreateAttributes,
    MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreateSample, MFMediaType_Video, MFNominalRange_16_235, MFTEnum2, MFTEnumEx,
    MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransferMatrix_BT709,
    MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_ADAPTER_LUID,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
    MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX,
    MF_SA_D3D11_AWARE, MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_UI4,
};

use super::capture::Gpu;
use crate::h264;

/// How long a hardware encoder may take to want the next frame.
const STALL: Duration = Duration::from_secs(2);
/// A key frame at least this often, even when nobody asks: a safety net for
/// a frame lost somewhere unnoticed. Rarely, since key frames are big; the
/// receiver asks for one whenever it needs it.
pub const KEY_FRAME_SECONDS: u32 = 5;
/// The bit rate for 1920 × 1080 at 60 frames a second, about 80 kB a frame:
/// on a local network bandwidth is cheap, and the more bits, the less the
/// fastest encoder settings cost in sharpness.
const BIT_RATE_1080P60: f64 = 40_000_000.0;
/// The least and the most whatever the size: below, text gets blurry; above,
/// Wi-Fi struggles, and the receiver's cache of frames since the last key
/// frame (48 MB) no longer holds [`KEY_FRAME_SECONDS`] of them.
const MIN_BIT_RATE: f64 = 12_000_000.0;
const MAX_BIT_RATE: f64 = 60_000_000.0;
/// The rate control's buffer, in frames' worth of bits: how far one frame
/// may go over the average. A key frame gets at most this much — a little
/// blurry for a few frames, but never a burst that holds up the frames
/// behind it on the network.
const BUFFER_FRAMES: u32 = 4;
/// 0 is the encoder's fastest preset, 100 its best: speed, which the bit
/// rate makes up for.
const QUALITY_VS_SPEED: u32 = 0;

/// The bit rate for a picture of this size and rate: [`BIT_RATE_1080P60`]
/// scaled by pixels a second, within [`MIN_BIT_RATE`] and [`MAX_BIT_RATE`].
pub fn bit_rate_for(width: u32, height: u32, fps: u32) -> u32 {
    let share = f64::from(width) * f64::from(height) * f64::from(fps) / (1920.0 * 1080.0 * 60.0);
    (BIT_RATE_1080P60 * share).clamp(MIN_BIT_RATE, MAX_BIT_RATE) as u32
}

/// What the encoder is asked for.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bit_rate: u32,
}

/// One encoded frame.
#[derive(Debug)]
pub struct Encoded {
    pub key: bool,
    /// The PTS the frame went in with.
    pub pts_us: u64,
    /// Annex B, SPS and PPS in front of every key frame.
    pub data: Vec<u8>,
}

/// Where encoded frames go, from whichever thread has them.
pub type Sink = Box<dyn FnMut(Encoded) + Send>;

/// What the picture thread and an asynchronous encoder's event thread share.
#[derive(Default)]
struct Shared {
    /// How many frames the encoder asked for and hasn't got yet.
    wanted: Mutex<u32>,
    asked: Condvar,
    sink: Mutex<Option<Sink>>,
    /// The event thread's error, for the picture thread to report.
    failed: Mutex<Option<windows::core::Error>>,
}

impl Shared {
    fn deliver(&self, encoded: Encoded) {
        if let Some(sink) = self.sink.lock().as_mut() {
            sink(encoded);
        }
    }
}

/// The encoder's output side: everything `ProcessOutput` needs.
struct Output {
    transform: IMFTransform,
    provides_samples: bool,
    output_size: u32,
    parameter_sets: Vec<u8>,
    name: String,
}

/// Media Foundation objects handed to another thread. An asynchronous
/// encoder is built to be called from any thread (Media Foundation's own
/// pipeline does so from its work queues); the bindings just can't know.
struct FreeThreaded<T>(T);

unsafe impl<T> Send for FreeThreaded<T> {}

impl<T> FreeThreaded<T> {
    /// Taken as a whole, so a closure moves the wrapper, not its parts.
    fn into_inner(self) -> T {
        self.0
    }
}

/// What one `ProcessOutput` gave.
enum Step {
    /// Something came out (nothing usable, sometimes).
    Frame(Option<Encoded>),
    /// The encoder has nothing (more).
    Empty,
}

pub struct Encoder {
    transform: IMFTransform,
    /// Shut down when done, which hardware encoders want.
    activate: IMFActivate,
    codec: Option<ICodecAPI>,
    shared: Arc<Shared>,
    /// A synchronous encoder's output side; an asynchronous one's is on its
    /// event thread.
    output: Option<Output>,
    /// The event thread, and a channel that closes when it ends.
    events: Option<(JoinHandle<()>, mpsc::Receiver<()>)>,
    /// Takes frames as textures; otherwise in memory.
    on_gpu: bool,
    _manager: Option<IMFDXGIDeviceManager>,
    staging: Option<ID3D11Texture2D>,
    settings: Settings,
    /// "NVIDIA H.264 Encoder MFT", "H264 Encoder MFT", …
    pub name: String,
    pub hardware: bool,
}

fn variant(vt: VARENUM, value: VARIANT_0_0_0) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: value,
            }),
        },
    }
}

fn variant_u32(value: u32) -> VARIANT {
    variant(VT_UI4, VARIANT_0_0_0 { ulVal: value })
}

fn variant_true() -> VARIANT {
    variant(
        VT_BOOL,
        VARIANT_0_0_0 {
            boolVal: VARIANT_TRUE,
        },
    )
}

fn pack(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

fn error(text: &str) -> windows::core::Error {
    windows::core::Error::new(E_FAIL, text)
}

/// Every encoder Media Foundation offers for NV12 to H.264 with `flags`.
fn enumerate(flags: u32, adapter: Option<LUID>) -> Vec<IMFActivate> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut array: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    let flags = windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG(flags as i32);
    let result = unsafe {
        match adapter {
            Some(luid) => {
                let mut attributes: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut attributes, 1).and_then(|()| {
                    let attributes = attributes.expect("attributes");
                    let bytes: [u8; 8] = std::mem::transmute(luid);
                    attributes.SetBlob(&MFT_ENUM_ADAPTER_LUID, &bytes)?;
                    MFTEnum2(
                        MFT_CATEGORY_VIDEO_ENCODER,
                        flags,
                        Some(&input),
                        Some(&output),
                        &attributes,
                        &mut array,
                        &mut count,
                    )
                })
            }
            None => MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                flags,
                Some(&input),
                Some(&output),
                &mut array,
                &mut count,
            ),
        }
    };
    if result.is_err() || array.is_null() {
        return Vec::new();
    }
    // Each element is taken over (and released when dropped), the array
    // itself freed.
    unsafe {
        let found: Vec<IMFActivate> = (0..count as usize)
            .filter_map(|i| std::ptr::read(array.add(i)))
            .collect();
        CoTaskMemFree(Some(array as *const _));
        found
    }
}

fn friendly_name(activate: &IMFActivate) -> String {
    let mut text = PWSTR::null();
    let mut len = 0;
    unsafe {
        if activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut text, &mut len)
            .is_err()
        {
            return "?".into();
        }
        let name = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        name
    }
}

impl Encoder {
    /// The card's encoder if it has a working one, otherwise Windows' own.
    pub fn open(gpu: &Gpu, settings: Settings, software_fps: u32, hardware: bool) -> Result<Self> {
        let hardware = if hardware {
            enumerate(
                (MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0) as u32,
                Some(gpu.luid),
            )
        } else {
            Vec::new()
        };
        for activate in hardware {
            let name = friendly_name(&activate);
            match Self::start(gpu, activate, settings, true) {
                Ok(encoder) => return Ok(encoder),
                Err(error) => tracing::info!(%name, %error, "hardware encoder unusable"),
            }
        }
        // Windows' own works on the processor: fewer frames, and the bit
        // rate for those.
        let fps = settings.fps.min(software_fps);
        let settings = Settings {
            fps,
            bit_rate: (u64::from(settings.bit_rate) * u64::from(fps)
                / u64::from(settings.fps.max(1))) as u32,
            ..settings
        };
        let software = enumerate(
            (MFT_ENUM_FLAG_SYNCMFT.0 | MFT_ENUM_FLAG_SORTANDFILTER.0) as u32,
            None,
        );
        let mut last = error("no H.264 encoder on this system");
        for activate in software {
            let name = friendly_name(&activate);
            match Self::start(gpu, activate, settings, false) {
                Ok(encoder) => return Ok(encoder),
                Err(error) => {
                    tracing::info!(%name, %error, "encoder unusable");
                    last = error;
                }
            }
        }
        Err(last)
    }

    fn start(gpu: &Gpu, activate: IMFActivate, settings: Settings, hardware: bool) -> Result<Self> {
        let name = friendly_name(&activate);
        let transform: IMFTransform = unsafe { activate.ActivateObject()? };
        let configured =
            Self::configure(gpu, &transform, settings, hardware, &name).and_then(|configured| {
                let mut output = Output {
                    transform: transform.clone(),
                    provides_samples: false,
                    output_size: 0,
                    parameter_sets: Vec::new(),
                    name: name.clone(),
                };
                output.read_stream_info()?;
                unsafe {
                    // Nothing to flush yet; Windows' own encoder even refuses.
                    let _ = transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                    transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                    transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                }
                Ok((configured, output))
            });
        let ((events, on_gpu, manager, codec), output) = match configured {
            Ok(parts) => parts,
            Err(error) => {
                unsafe {
                    let _ = activate.ShutdownObject();
                }
                return Err(error);
            }
        };
        let shared = Arc::new(Shared::default());
        let (output, events) = match events {
            Some(events) => {
                let (done_tx, done_rx) = mpsc::channel::<()>();
                let shared = shared.clone();
                let parts = FreeThreaded((events, output));
                let thread = std::thread::Builder::new()
                    .name("uwumirror-encoder".into())
                    .spawn(move || {
                        let _done = done_tx;
                        let (events, output) = parts.into_inner();
                        event_thread(&events, output, &shared);
                    })
                    .map_err(|_| error("no thread for the encoder"))?;
                (None, Some((thread, done_rx)))
            }
            None => (Some(output), None),
        };
        tracing::info!(
            %name,
            hardware,
            textures = on_gpu,
            width = settings.width,
            height = settings.height,
            fps = settings.fps,
            bit_rate = settings.bit_rate,
            "H.264 encoder"
        );
        Ok(Self {
            transform,
            activate,
            codec,
            shared,
            output,
            events,
            on_gpu,
            _manager: manager,
            staging: None,
            settings,
            name,
            hardware,
        })
    }

    #[allow(clippy::type_complexity)]
    fn configure(
        gpu: &Gpu,
        transform: &IMFTransform,
        settings: Settings,
        hardware: bool,
        name: &str,
    ) -> Result<(
        Option<IMFMediaEventGenerator>,
        bool,
        Option<IMFDXGIDeviceManager>,
        Option<ICodecAPI>,
    )> {
        unsafe {
            let attributes = transform.GetAttributes().ok();
            let mut events = None;
            if let Some(attributes) = &attributes {
                if attributes.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0 {
                    attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
                    events = Some(transform.cast::<IMFMediaEventGenerator>()?);
                }
                // Media Foundation's own word for "live": no frames held back.
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }
            if hardware && events.is_none() {
                // Every hardware encoder is asynchronous; one that isn't is
                // something else.
                return Err(error("a synchronous hardware encoder"));
            }

            // Frames as textures, when the encoder takes them.
            let mut manager = None;
            let d3d11 = attributes
                .as_ref()
                .is_some_and(|a| a.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) != 0);
            if hardware && d3d11 {
                let mut token = 0u32;
                let mut created: Option<IMFDXGIDeviceManager> = None;
                MFCreateDXGIDeviceManager(&mut token, &mut created)?;
                let created = created.expect("a device manager");
                created.ResetDevice(&gpu.device, token)?;
                if transform
                    .ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, created.as_raw() as usize)
                    .is_ok()
                {
                    manager = Some(created);
                }
            }
            let on_gpu = manager.is_some();
            if hardware && !on_gpu {
                return Err(error("a hardware encoder that takes no textures"));
            }

            // Before the media types: some encoders only take these then.
            let codec = transform.cast::<ICodecAPI>().ok();
            if let Some(codec) = &codec {
                let mut refused = Vec::new();
                let mut set = |what: &'static str, api: &GUID, value: &VARIANT| {
                    if codec.SetValue(api, value).is_err() {
                        refused.push(what);
                    }
                };
                // A VARIANT_BOOL, as documented; some encoders only took a
                // number for it, so that comes second.
                if codec
                    .SetValue(&CODECAPI_AVLowLatencyMode, &variant_true())
                    .is_err()
                {
                    set("low latency", &CODECAPI_AVLowLatencyMode, &variant_u32(1));
                }
                set(
                    "constant bit rate",
                    &CODECAPI_AVEncCommonRateControlMode,
                    &variant_u32(eAVEncCommonRateControlMode_CBR.0 as u32),
                );
                set(
                    "bit rate",
                    &CODECAPI_AVEncCommonMeanBitRate,
                    &variant_u32(settings.bit_rate),
                );
                set(
                    "buffer size",
                    &CODECAPI_AVEncCommonBufferSize,
                    &variant_u32(settings.bit_rate / settings.fps.max(1) * BUFFER_FRAMES),
                );
                set(
                    "key frame interval",
                    &CODECAPI_AVEncMPVGOPSize,
                    &variant_u32(settings.fps * KEY_FRAME_SECONDS),
                );
                set(
                    "no B-frames",
                    &CODECAPI_AVEncMPVDefaultBPictureCount,
                    &variant_u32(0),
                );
                set(
                    "one reference frame",
                    &CODECAPI_AVEncVideoMaxNumRefFrame,
                    &variant_u32(1),
                );
                set(
                    "speed",
                    &CODECAPI_AVEncCommonQualityVsSpeed,
                    &variant_u32(QUALITY_VS_SPEED),
                );
                if !refused.is_empty() {
                    tracing::info!(name, ?refused, "encoder settings refused");
                }
            }

            let output = MFCreateMediaType()?;
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, settings.bit_rate)?;
            describe(&output, settings, hardware)?;
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;
            transform.SetOutputType(0, &output, 0)?;

            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            describe(&input, settings, hardware)?;
            transform.SetInputType(0, &input, 0)?;
            Ok((events, on_gpu, manager, codec))
        }
    }

    /// The size frames are encoded at.
    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Where encoded frames go from now on. They may come from another
    /// thread.
    pub fn set_sink(&self, sink: Sink) {
        *self.shared.sink.lock() = Some(sink);
    }

    /// Encodes one NV12 frame. What comes out goes to the sink: an
    /// asynchronous encoder's from its event thread as soon as it is done, a
    /// synchronous one's before this returns.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        frame: &ID3D11Texture2D,
        pts_us: u64,
        key: bool,
    ) -> Result<()> {
        if let Some(error) = self.shared.failed.lock().take() {
            return Err(error);
        }
        if key {
            if let Some(codec) = &self.codec {
                unsafe {
                    let _ = codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &variant_u32(1));
                }
            }
        }
        let sample = self.sample(gpu, frame, pts_us)?;
        if self.events.is_some() {
            {
                let mut wanted = self.shared.wanted.lock();
                if *wanted == 0 {
                    self.shared.asked.wait_for(&mut wanted, STALL);
                }
                if *wanted == 0 {
                    return Err(self
                        .shared
                        .failed
                        .lock()
                        .take()
                        .unwrap_or_else(|| error("the encoder stopped taking frames")));
                }
                *wanted -= 1;
            }
            unsafe { self.transform.ProcessInput(0, &sample, 0)? };
        } else {
            unsafe { self.transform.ProcessInput(0, &sample, 0)? };
            let output = self
                .output
                .as_mut()
                .expect("a synchronous encoder's output");
            while let Step::Frame(encoded) = output.step()? {
                if let Some(encoded) = encoded {
                    self.shared.deliver(encoded);
                }
            }
        }
        Ok(())
    }

    /// The frame as the encoder takes it: a texture on the card, or a copy
    /// in memory.
    fn sample(&mut self, gpu: &Gpu, frame: &ID3D11Texture2D, pts_us: u64) -> Result<IMFSample> {
        let Settings {
            width, height, fps, ..
        } = self.settings;
        unsafe {
            let buffer = if self.on_gpu {
                let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, frame, 0, false)?;
                let len = buffer.cast::<IMF2DBuffer>()?.GetContiguousLength()?;
                buffer.SetCurrentLength(len)?;
                buffer
            } else {
                let staging = match &self.staging {
                    Some(staging) => staging.clone(),
                    None => {
                        let staging = staging_texture(gpu, width, height)?;
                        self.staging = Some(staging.clone());
                        staging
                    }
                };
                // Map waits for the card to finish the conversion and the
                // copy: a millisecond or two, and only for Windows' own
                // encoder, which needs the frame in memory before it can
                // start anyway.
                gpu.context.CopyResource(&staging, frame);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                gpu.context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let size = (width * height * 3 / 2) as usize;
                let buffer = MFCreateMemoryBuffer(size as u32)?;
                let mut target = std::ptr::null_mut();
                buffer.Lock(&mut target, None, None)?;
                let pitch = mapped.RowPitch as usize;
                let source = mapped.pData as *const u8;
                let (w, h) = (width as usize, height as usize);
                // Brightness rows, then the colour rows below them.
                for row in 0..h + h / 2 {
                    std::ptr::copy_nonoverlapping(source.add(row * pitch), target.add(row * w), w);
                }
                buffer.Unlock()?;
                gpu.context.Unmap(&staging, 0);
                buffer.SetCurrentLength(size as u32)?;
                buffer
            };
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(pts_us as i64 * 10)?;
            sample.SetSampleDuration(10_000_000 / i64::from(fps.max(1)))?;
            Ok(sample)
        }
    }
}

/// An asynchronous encoder's events, until it is shut down: counts the
/// frames it asks for, and takes each finished frame out at once.
fn event_thread(events: &IMFMediaEventGenerator, mut output: Output, shared: &Shared) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let result = (|| -> Result<()> {
        loop {
            // Blocks until the next event; fails once the encoder is shut
            // down, which ends this thread.
            let Ok(event) = (unsafe { events.GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0)) })
            else {
                return Ok(());
            };
            let kind = unsafe { event.GetType()? };
            unsafe { event.GetStatus()?.ok()? };
            if kind == METransformNeedInput.0 as u32 {
                *shared.wanted.lock() += 1;
                shared.asked.notify_one();
            } else if kind == METransformHaveOutput.0 as u32 {
                if let Step::Frame(Some(encoded)) = output.step()? {
                    shared.deliver(encoded);
                }
            }
        }
    })();
    if let Err(error) = result {
        tracing::warn!(name = %output.name, %error, "encoder");
        *shared.failed.lock() = Some(error);
        // Wakes a picture thread waiting for the encoder to want a frame.
        shared.asked.notify_one();
    }
    drop(output);
    unsafe { CoUninitialize() };
}

impl Output {
    fn read_stream_info(&mut self) -> Result<()> {
        unsafe {
            let info = self.transform.GetOutputStreamInfo(0)?;
            let provides = (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0
                | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0) as u32;
            self.provides_samples = info.dwFlags & provides != 0;
            self.output_size = info.cbSize.max(1024 * 1024);
            // SPS and PPS, when the encoder tells them up front.
            if let Ok(current) = self.transform.GetOutputCurrentType(0) {
                if let Some(header) = blob(&current, &MF_MT_MPEG_SEQUENCE_HEADER) {
                    let sets = h264::parameter_sets(&header);
                    if !sets.is_empty() {
                        self.parameter_sets = sets;
                    }
                }
            }
        }
        Ok(())
    }

    /// One `ProcessOutput`.
    fn step(&mut self) -> Result<Step> {
        for _ in 0..3 {
            let sample = if self.provides_samples {
                None
            } else {
                unsafe {
                    let sample = MFCreateSample()?;
                    sample.AddBuffer(&MFCreateMemoryBuffer(self.output_size)?)?;
                    Some(sample)
                }
            };
            let mut buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0u32;
            let result = unsafe {
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status)
            };
            let sample = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));
            match result {
                Ok(()) => {
                    return Ok(Step::Frame(match sample {
                        Some(sample) => self.read(&sample)?,
                        None => None,
                    }));
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(Step::Empty),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                    // The encoder settled on its output type: take it, and try again.
                    unsafe {
                        let offered = self.transform.GetOutputAvailableType(0, 0)?;
                        self.transform.SetOutputType(0, &offered, 0)?;
                    }
                    self.read_stream_info()?;
                }
                Err(e) => return Err(e),
            }
        }
        Err(error("the encoder keeps changing its output"))
    }

    fn read(&mut self, sample: &IMFSample) -> Result<Option<Encoded>> {
        let data = unsafe {
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut pointer = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut pointer, None, Some(&mut len))?;
            let data = std::slice::from_raw_parts(pointer, len as usize).to_vec();
            buffer.Unlock()?;
            data
        };
        let pts_us = unsafe { sample.GetSampleTime() }.unwrap_or(0).max(0) as u64 / 10;
        if data.is_empty() {
            return Ok(None);
        }
        let Some((key, data)) = h264::prepare(data, &mut self.parameter_sets) else {
            tracing::warn!(name = %self.name, "encoder output isn't Annex B");
            return Ok(None);
        };
        Ok(Some(Encoded { key, pts_us, data }))
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            let _ = self.activate.ShutdownObject();
            // Shut down, an asynchronous encoder's event queue ends, and with
            // it the event thread; said once more in case the activation
            // object didn't pass it on.
            if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                let _ = shutdown.Shutdown();
            }
        }
        *self.shared.sink.lock() = None;
        if let Some((thread, done)) = self.events.take() {
            // Joined only once it has ended: an encoder that never lets go
            // mustn't hang the end of sending.
            if let Err(mpsc::RecvTimeoutError::Disconnected) =
                done.recv_timeout(Duration::from_secs(1))
            {
                let _ = thread.join();
            } else {
                tracing::warn!(name = %self.name, "the encoder's event thread didn't end");
            }
        }
    }
}

/// Size, rate, shape and colour, the same for both sides of the encoder.
unsafe fn describe(media_type: &IMFMediaType, settings: Settings, hardware: bool) -> Result<()> {
    unsafe {
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack(settings.width, settings.height))?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, pack(settings.fps, 1))?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        // What the video processor makes for the graphics cards' encoders
        // (see `convert.rs`), for them to write into the stream. Windows'
        // own writes nothing either way.
        if hardware {
            let _ = media_type.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32);
            let _ = media_type.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32);
            let _ =
                media_type.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32);
        }
    }
    Ok(())
}

fn blob(attributes: &IMFMediaType, key: &GUID) -> Option<Vec<u8>> {
    unsafe {
        let mut pointer = std::ptr::null_mut();
        let mut len = 0u32;
        attributes
            .GetAllocatedBlob(key, &mut pointer, &mut len)
            .ok()?;
        let data = std::slice::from_raw_parts(pointer, len as usize).to_vec();
        CoTaskMemFree(Some(pointer as *const _));
        Some(data)
    }
}

fn staging_texture(gpu: &Gpu, width: u32, height: u32) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe {
        gpu.device
            .CreateTexture2D(&desc, None, Some(&mut texture))?
    };
    Ok(texture.expect("CreateTexture2D gave a texture"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_rate_scales_with_pixels_and_stays_within_bounds() {
        assert_eq!(bit_rate_for(1920, 1080, 60), 40_000_000);
        assert_eq!(bit_rate_for(1920, 1080, 30), 20_000_000);
        assert_eq!(bit_rate_for(1280, 720, 60), 17_777_777);
        assert_eq!(bit_rate_for(1280, 720, 30), MIN_BIT_RATE as u32);
        assert_eq!(bit_rate_for(3840, 2160, 60), MAX_BIT_RATE as u32);
    }
}
