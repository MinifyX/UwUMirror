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
//! (`METransformHaveOutput`), and take frames as textures on the card.
//! Windows' encoder is synchronous and takes frames in memory, so for it each
//! NV12 frame is copied back from the card first.
//!
//! Settings for live pictures: low latency (one frame in, one out, no
//! B-frames), constant bit rate, Main profile, a key frame every two seconds
//! and whenever the receiver asks. Every key frame goes out with SPS and PPS
//! in front of it, as from AirPlay and scrcpy, so a decoder can start there.

use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

use windows::core::{Interface, Result, GUID, PWSTR};
use windows::Win32::Foundation::{E_FAIL, LUID};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Media::MediaFoundation::{
    eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Main, CODECAPI_AVEncCommonMeanBitRate,
    CODECAPI_AVEncCommonRateControlMode, CODECAPI_AVEncMPVDefaultBPictureCount,
    CODECAPI_AVEncMPVGOPSize, CODECAPI_AVEncVideoForceKeyFrame, CODECAPI_AVLowLatencyMode,
    ICodecAPI, IMF2DBuffer, IMFActivate, IMFAttributes, IMFDXGIDeviceManager,
    IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFTransform, METransformHaveOutput,
    METransformNeedInput, MFCreateAttributes, MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer,
    MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFMediaType_Video,
    MFNominalRange_16_235, MFTEnum2, MFTEnumEx, MFT_FRIENDLY_NAME_Attribute, MFVideoFormat_H264,
    MFVideoFormat_NV12, MFVideoInterlace_Progressive, MFVideoPrimaries_BT709,
    MFVideoTransferMatrix_BT709, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_ADAPTER_LUID,
    MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER, MFT_ENUM_FLAG_SYNCMFT,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_END_OF_STREAM, MFT_MESSAGE_NOTIFY_END_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER, MFT_OUTPUT_DATA_BUFFER,
    MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
    MFT_REGISTER_TYPE_INFO, MF_EVENT_FLAG_NO_WAIT, MF_E_NO_EVENTS_AVAILABLE,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AVG_BITRATE,
    MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_MPEG2_PROFILE, MF_MT_MPEG_SEQUENCE_HEADER, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
    MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES, MF_MT_YUV_MATRIX, MF_SA_D3D11_AWARE,
    MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK,
};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::{VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_UI4};

use super::capture::Gpu;
use crate::h264;

/// How long a hardware encoder may take to want the next frame.
const STALL: Duration = Duration::from_secs(2);
/// How long to wait for a frame just given to a hardware encoder; one that
/// isn't done by then goes out with the next.
const OUTPUT_WAIT: Duration = Duration::from_millis(20);

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
    pub pts_us: u64,
    /// Annex B, SPS and PPS in front of every key frame.
    pub data: Vec<u8>,
}

pub struct Encoder {
    transform: IMFTransform,
    /// Shut down when done, which hardware encoders want.
    activate: IMFActivate,
    codec: Option<ICodecAPI>,
    /// Asynchronous (hardware) encoders tell what they want through events.
    events: Option<IMFMediaEventGenerator>,
    /// How many frames the encoder asked for and hasn't got yet.
    wanted: u32,
    /// Takes frames as textures; otherwise in memory.
    on_gpu: bool,
    _manager: Option<IMFDXGIDeviceManager>,
    staging: Option<ID3D11Texture2D>,
    provides_samples: bool,
    output_size: u32,
    settings: Settings,
    parameter_sets: Vec<u8>,
    /// "NVIDIA H.264 Encoder MFT", "H264 Encoder MFT", …
    pub name: String,
    pub hardware: bool,
}

fn variant_u32(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
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
        // Windows' own works on the processor: fewer frames.
        let settings = Settings {
            fps: settings.fps.min(software_fps),
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
        let configured = Self::configure(gpu, &transform, settings, hardware);
        match configured {
            Ok((events, on_gpu, manager, codec)) => {
                let mut encoder = Self {
                    transform,
                    activate,
                    codec,
                    events,
                    wanted: 0,
                    on_gpu,
                    _manager: manager,
                    staging: None,
                    provides_samples: false,
                    output_size: 0,
                    settings,
                    parameter_sets: Vec::new(),
                    name,
                    hardware,
                };
                encoder.read_stream_info()?;
                unsafe {
                    // Nothing to flush yet; Windows' own encoder even refuses.
                    let _ = encoder
                        .transform
                        .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
                    encoder
                        .transform
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
                    encoder
                        .transform
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
                }
                tracing::info!(
                    name = %encoder.name,
                    hardware,
                    textures = encoder.on_gpu,
                    width = settings.width,
                    height = settings.height,
                    fps = settings.fps,
                    "H.264 encoder"
                );
                Ok(encoder)
            }
            Err(error) => {
                unsafe {
                    let _ = activate.ShutdownObject();
                }
                Err(error)
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn configure(
        gpu: &Gpu,
        transform: &IMFTransform,
        settings: Settings,
        hardware: bool,
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
                let set = |api: &GUID, value: u32| {
                    let variant = variant_u32(value);
                    if let Err(error) = codec.SetValue(api, &variant) {
                        tracing::debug!(?api, %error, "encoder setting refused");
                    }
                };
                set(
                    &CODECAPI_AVEncCommonRateControlMode,
                    eAVEncCommonRateControlMode_CBR.0 as u32,
                );
                set(&CODECAPI_AVEncCommonMeanBitRate, settings.bit_rate);
                set(&CODECAPI_AVEncMPVGOPSize, settings.fps * 2);
                set(&CODECAPI_AVEncMPVDefaultBPictureCount, 0);
                // A VARIANT_BOOL's true is all ones; as VT_UI4 every encoder
                // tried takes 1 as well.
                set(&CODECAPI_AVLowLatencyMode, 1);
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

    /// The size frames are encoded at.
    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Encodes one NV12 frame; what comes out (usually that frame, sometimes
    /// the one before) is returned.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        frame: &ID3D11Texture2D,
        pts_us: u64,
        key: bool,
    ) -> Result<Vec<Encoded>> {
        let mut out = Vec::new();
        if key {
            if let Some(codec) = &self.codec {
                unsafe {
                    let _ = codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &variant_u32(1));
                }
            }
        }
        let sample = self.sample(gpu, frame, pts_us)?;
        if self.events.is_some() {
            let deadline = Instant::now() + STALL;
            self.pump(&mut out, |e, _| e.wanted > 0, deadline)?;
            if self.wanted == 0 {
                return Err(error("the encoder stopped taking frames"));
            }
            unsafe { self.transform.ProcessInput(0, &sample, 0)? };
            self.wanted -= 1;
            let before = out.len();
            self.pump(
                &mut out,
                move |_, out| out.len() > before,
                Instant::now() + OUTPUT_WAIT,
            )?;
        } else {
            unsafe { self.transform.ProcessInput(0, &sample, 0)? };
            self.drain(&mut out)?;
        }
        Ok(out)
    }

    /// Handles an asynchronous encoder's events until `done` or `deadline`.
    fn pump(
        &mut self,
        out: &mut Vec<Encoded>,
        done: impl Fn(&Self, &Vec<Encoded>) -> bool,
        deadline: Instant,
    ) -> Result<()> {
        let events = self.events.clone().expect("an asynchronous encoder");
        loop {
            match unsafe { events.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => {
                    let kind = unsafe { event.GetType()? };
                    unsafe { event.GetStatus()?.ok()? };
                    if kind == METransformNeedInput.0 as u32 {
                        self.wanted += 1;
                    } else if kind == METransformHaveOutput.0 as u32 {
                        self.output(out)?;
                    }
                }
                Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                    if done(self, out) || Instant::now() >= deadline {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Takes everything a synchronous encoder has.
    fn drain(&mut self, out: &mut Vec<Encoded>) -> Result<()> {
        while self.output(out)? {}
        Ok(())
    }

    /// One `ProcessOutput`. False when the encoder has nothing (more).
    fn output(&mut self, out: &mut Vec<Encoded>) -> Result<bool> {
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
                    if let Some(sample) = sample {
                        if let Some(encoded) = self.read(&sample)? {
                            out.push(encoded);
                        }
                    }
                    return Ok(true);
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(false),
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
