//! AirPlay's sound through the system's own FFmpeg.
//!
//! An iPhone mirroring its screen sends AAC-ELD; AirPlay audio sends ALAC or
//! AAC. No Rust crate decodes AAC-ELD, and the one standalone decoder
//! (Fraunhofer's FDK) can't be combined with the AGPL. FFmpeg's libavcodec
//! decodes all three, is LGPL, and is on almost every Linux desktop already
//! (on macOS one `brew install ffmpeg` away; on Windows it takes a "shared"
//! build, the one with `avcodec-*.dll`, in a folder on the `PATH`).
//! So UwUMirror doesn't ship a decoder: it loads libavcodec and libavutil at
//! runtime, from the system, and only when a stream actually has sound.
//!
//! Only a handful of functions and the leading fields of three structs are
//! used — `AVPacket`'s, `AVFrame`'s and `AVCodecParameters`' — which have
//! kept their layout since FFmpeg 4 (libavcodec 58) up to FFmpeg 8
//! (libavcodec 62). Everything else is set through the functions.

use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::Arc;

use libloading::Library;
use parking_lot::Mutex;

/// The audio codecs AirPlay uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    /// Apple Lossless, 352 frames per packet, 44.1 kHz stereo.
    Alac,
    /// AAC-LC, 1024 frames per packet, 44.1 kHz stereo.
    AacLc,
    /// AAC-ELD, 480 frames per packet, 44.1 kHz stereo: screen mirroring.
    AacEld,
}

impl AudioCodec {
    /// AirPlay's `ct` field from SETUP.
    pub fn from_airplay_ct(ct: u64) -> Option<Self> {
        match ct {
            2 => Some(Self::Alac),
            4 => Some(Self::AacLc),
            8 => Some(Self::AacEld),
            _ => None,
        }
    }

    fn codec_id(self) -> c_int {
        match self {
            Self::Alac => AV_CODEC_ID_ALAC,
            Self::AacLc | Self::AacEld => AV_CODEC_ID_AAC,
        }
    }

    /// What the decoder needs to know before the first packet, as the
    /// receivers that came before us configure it.
    fn extradata(self) -> &'static [u8] {
        match self {
            // The 'alac' atom: frame length 352, 16 bit, rice parameters
            // 40/10/14, 2 channels, 44100 Hz.
            Self::Alac => &[
                0x00, 0x00, 0x00, 0x24, b'a', b'l', b'a', b'c', 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x01, 0x60, 0x00, 0x10, 0x28, 0x0a, 0x0e, 0x02, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xac, 0x44,
            ],
            // AudioSpecificConfig: AAC-LC, 44100 Hz, stereo.
            Self::AacLc => &[0x12, 0x10],
            // AudioSpecificConfig: AAC-ELD, 44100 Hz, stereo, 480 samples.
            Self::AacEld => &[0xf8, 0xe8, 0x50, 0x00],
        }
    }
}

const AV_CODEC_ID_AAC: c_int = 0x15002;
const AV_CODEC_ID_ALAC: c_int = 0x15010;
const AVMEDIA_TYPE_AUDIO: c_int = 1;
const AVERROR_EAGAIN_LINUX: c_int = -11;
const AVERROR_EAGAIN_MACOS: c_int = -35;
const AVERROR_EAGAIN_WINDOWS: c_int = -11;

const FMT_S16: c_int = 1;
const FMT_S32: c_int = 2;
const FMT_FLT: c_int = 3;
const FMT_S16P: c_int = 6;
const FMT_S32P: c_int = 7;
const FMT_FLTP: c_int = 8;

#[repr(C)]
struct AvPacketHead {
    buf: *mut c_void,
    pts: i64,
    dts: i64,
    data: *mut u8,
    size: c_int,
}

#[repr(C)]
struct AvFrameHead {
    data: [*mut u8; 8],
    linesize: [c_int; 8],
    extended_data: *mut *mut u8,
    width: c_int,
    height: c_int,
    nb_samples: c_int,
    format: c_int,
}

#[repr(C)]
struct AvCodecParametersHead {
    codec_type: c_int,
    codec_id: c_int,
    codec_tag: u32,
    extradata: *mut u8,
    extradata_size: c_int,
}

type FindDecoder = unsafe extern "C" fn(c_int) -> *const c_void;
type AllocContext = unsafe extern "C" fn(*const c_void) -> *mut c_void;
type FreeContext = unsafe extern "C" fn(*mut *mut c_void);
type ParametersAlloc = unsafe extern "C" fn() -> *mut AvCodecParametersHead;
type ParametersFree = unsafe extern "C" fn(*mut *mut AvCodecParametersHead);
type ParametersToContext = unsafe extern "C" fn(*mut c_void, *const AvCodecParametersHead) -> c_int;
type Open2 = unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) -> c_int;
type PacketAlloc = unsafe extern "C" fn() -> *mut AvPacketHead;
type PacketFree = unsafe extern "C" fn(*mut *mut AvPacketHead);
type SendPacket = unsafe extern "C" fn(*mut c_void, *const AvPacketHead) -> c_int;
type ReceiveFrame = unsafe extern "C" fn(*mut c_void, *mut AvFrameHead) -> c_int;
type FrameAlloc = unsafe extern "C" fn() -> *mut AvFrameHead;
type FrameFree = unsafe extern "C" fn(*mut *mut AvFrameHead);
type Malloc = unsafe extern "C" fn(usize) -> *mut c_void;

struct Api {
    find_decoder: FindDecoder,
    alloc_context: AllocContext,
    free_context: FreeContext,
    parameters_alloc: ParametersAlloc,
    parameters_free: ParametersFree,
    parameters_to_context: ParametersToContext,
    open2: Open2,
    packet_alloc: PacketAlloc,
    packet_free: PacketFree,
    send_packet: SendPacket,
    receive_frame: ReceiveFrame,
    frame_alloc: FrameAlloc,
    frame_free: FrameFree,
    malloc: Malloc,
    /// Keeps the libraries loaded for as long as the pointers above are used.
    _libraries: (Library, Library),
    /// Which libavcodec this is, for the log and the settings page.
    pub version: u32,
}

/// libavcodec major version → the libavutil major it was released with.
const VERSIONS: [(u32, u32); 5] = [(62, 60), (61, 59), (60, 58), (59, 57), (58, 56)];

fn candidates(name: &str, major: u32) -> Vec<String> {
    let mut names = Vec::new();
    if cfg!(target_os = "windows") {
        names.push(format!("{name}-{major}.dll"));
    } else if cfg!(target_os = "macos") {
        for dir in ["/opt/homebrew/lib", "/usr/local/lib", "/opt/local/lib"] {
            names.push(format!("{dir}/lib{name}.{major}.dylib"));
        }
        names.push(format!("lib{name}.{major}.dylib"));
    } else {
        names.push(format!("lib{name}.so.{major}"));
    }
    names
}

fn open(name: &str, major: u32) -> Option<Library> {
    candidates(name, major).into_iter().find_map(|candidate| {
        // SAFETY: loading FFmpeg runs its (trivial) library constructors only.
        unsafe { Library::new(&candidate) }.ok()
    })
}

impl Api {
    fn load() -> Option<Api> {
        for (avcodec_major, avutil_major) in VERSIONS {
            let Some(avcodec) = open("avcodec", avcodec_major) else {
                continue;
            };
            let Some(avutil) = open("avutil", avutil_major) else {
                continue;
            };
            // SAFETY: the signatures match FFmpeg's headers for these versions.
            let api = unsafe {
                macro_rules! sym {
                    ($lib:expr, $name:literal) => {
                        match $lib.get($name) {
                            Ok(symbol) => *symbol,
                            Err(_) => continue,
                        }
                    };
                }
                Api {
                    find_decoder: sym!(avcodec, b"avcodec_find_decoder\0"),
                    alloc_context: sym!(avcodec, b"avcodec_alloc_context3\0"),
                    free_context: sym!(avcodec, b"avcodec_free_context\0"),
                    parameters_alloc: sym!(avcodec, b"avcodec_parameters_alloc\0"),
                    parameters_free: sym!(avcodec, b"avcodec_parameters_free\0"),
                    parameters_to_context: sym!(avcodec, b"avcodec_parameters_to_context\0"),
                    open2: sym!(avcodec, b"avcodec_open2\0"),
                    packet_alloc: sym!(avcodec, b"av_packet_alloc\0"),
                    packet_free: sym!(avcodec, b"av_packet_free\0"),
                    send_packet: sym!(avcodec, b"avcodec_send_packet\0"),
                    receive_frame: sym!(avcodec, b"avcodec_receive_frame\0"),
                    frame_alloc: sym!(avutil, b"av_frame_alloc\0"),
                    frame_free: sym!(avutil, b"av_frame_free\0"),
                    malloc: sym!(avutil, b"av_mallocz\0"),
                    _libraries: (avcodec, avutil),
                    version: avcodec_major,
                }
            };
            return Some(api);
        }
        None
    }
}

static API: Mutex<Option<Option<Arc<Api>>>> = Mutex::new(None);

fn api() -> Option<Arc<Api>> {
    API.lock()
        .get_or_insert_with(|| Api::load().map(Arc::new))
        .clone()
}

/// The libavcodec major version in use, or `None` when FFmpeg isn't there.
pub fn ffmpeg_version() -> Option<u32> {
    api().map(|api| api.version)
}

/// Looks for FFmpeg again, e.g. after the user installed it.
pub fn reload_ffmpeg() -> Option<u32> {
    *API.lock() = None;
    ffmpeg_version()
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("FFmpeg (libavcodec) was not found on this system")]
    NoFfmpeg,
    #[error("FFmpeg has no decoder for {0:?}")]
    NoDecoder(AudioCodec),
    #[error("FFmpeg refused to open the {0:?} decoder ({1})")]
    Open(AudioCodec, c_int),
}

/// One stream's decoder: packets in, interleaved stereo f32 out.
pub struct AudioDecoder {
    api: Arc<Api>,
    context: *mut c_void,
    packet: *mut AvPacketHead,
    frame: *mut AvFrameHead,
    channels: usize,
}

// SAFETY: the FFmpeg objects belong to this decoder alone and are only used
// through `&mut self`.
unsafe impl Send for AudioDecoder {}

impl AudioDecoder {
    pub fn new(codec: AudioCodec) -> Result<Self, DecodeError> {
        let api = api().ok_or(DecodeError::NoFfmpeg)?;
        // SAFETY: plain FFmpeg setup; every pointer is checked before use and
        // freed on the error paths (the context in `Drop`).
        unsafe {
            let decoder = (api.find_decoder)(codec.codec_id());
            if decoder.is_null() {
                return Err(DecodeError::NoDecoder(codec));
            }
            let mut context = (api.alloc_context)(decoder);
            let packet = (api.packet_alloc)();
            let frame = (api.frame_alloc)();
            let mut this = Self {
                api: api.clone(),
                context,
                packet,
                frame,
                channels: 2,
            };
            if context.is_null() || packet.is_null() || frame.is_null() {
                return Err(DecodeError::Open(codec, -12));
            }
            let mut parameters = (api.parameters_alloc)();
            if parameters.is_null() {
                return Err(DecodeError::Open(codec, -12));
            }
            let extra = codec.extradata();
            // FFmpeg reads a little past the end of extradata; it wants 64
            // zeroed bytes of padding, and frees the buffer with its own free.
            let buffer = (api.malloc)(extra.len() + 64) as *mut u8;
            if buffer.is_null() {
                (api.parameters_free)(&mut parameters);
                return Err(DecodeError::Open(codec, -12));
            }
            ptr::copy_nonoverlapping(extra.as_ptr(), buffer, extra.len());
            (*parameters).codec_type = AVMEDIA_TYPE_AUDIO;
            (*parameters).codec_id = codec.codec_id();
            (*parameters).extradata = buffer;
            (*parameters).extradata_size = extra.len() as c_int;
            let copied = (api.parameters_to_context)(context, parameters);
            (api.parameters_free)(&mut parameters);
            if copied < 0 {
                return Err(DecodeError::Open(codec, copied));
            }
            let opened = (api.open2)(context, decoder, ptr::null_mut());
            if opened < 0 {
                (api.free_context)(&mut context);
                this.context = ptr::null_mut();
                return Err(DecodeError::Open(codec, opened));
            }
            Ok(this)
        }
    }

    /// Decodes one packet; returns interleaved stereo samples in -1.0..=1.0.
    /// A packet the decoder rejects gives silence for it, not an error: one
    /// damaged packet on Wi-Fi must not end the sound for the whole stream.
    pub fn decode(&mut self, data: &[u8]) -> Vec<f32> {
        let mut out = Vec::new();
        if data.is_empty() || self.context.is_null() {
            return out;
        }
        // SAFETY: the packet borrows `data` only for the send call, which
        // copies it (the packet has no `buf`, so FFmpeg treats it as not
        // reference-counted); frame data is read within the frame's bounds.
        unsafe {
            (*self.packet).data = data.as_ptr() as *mut u8;
            (*self.packet).size = data.len() as c_int;
            let sent = (self.api.send_packet)(self.context, self.packet);
            (*self.packet).data = ptr::null_mut();
            (*self.packet).size = 0;
            if sent < 0 && !is_again(sent) {
                return out;
            }
            while (self.api.receive_frame)(self.context, self.frame) >= 0 {
                self.read_frame(&mut out);
            }
        }
        out
    }

    unsafe fn read_frame(&self, out: &mut Vec<f32>) {
        let frame = &*self.frame;
        let samples = frame.nb_samples.max(0) as usize;
        let channels = self.channels;
        let planes = frame.extended_data;
        if planes.is_null() || samples == 0 {
            return;
        }
        let plane = |c: usize| -> *const u8 { *planes.add(c) };
        out.reserve(samples * channels);
        match frame.format {
            FMT_FLTP | FMT_S16P | FMT_S32P => {
                let left = plane(0);
                let right = if plane(1).is_null() { left } else { plane(1) };
                for i in 0..samples {
                    for p in [left, right] {
                        out.push(match frame.format {
                            FMT_FLTP => *(p as *const f32).add(i),
                            FMT_S16P => *(p as *const i16).add(i) as f32 / 32768.0,
                            _ => *(p as *const i32).add(i) as f32 / 2_147_483_648.0,
                        });
                    }
                }
            }
            FMT_FLT | FMT_S16 | FMT_S32 => {
                let p = plane(0);
                for i in 0..samples * channels {
                    out.push(match frame.format {
                        FMT_FLT => *(p as *const f32).add(i),
                        FMT_S16 => *(p as *const i16).add(i) as f32 / 32768.0,
                        _ => *(p as *const i32).add(i) as f32 / 2_147_483_648.0,
                    });
                }
            }
            other => tracing::warn!(format = other, "FFmpeg gave an unexpected sample format"),
        }
    }
}

fn is_again(code: c_int) -> bool {
    code == AVERROR_EAGAIN_LINUX || code == AVERROR_EAGAIN_MACOS || code == AVERROR_EAGAIN_WINDOWS
}

impl Drop for AudioDecoder {
    fn drop(&mut self) {
        // SAFETY: each pointer was allocated by FFmpeg and is freed once.
        unsafe {
            if !self.frame.is_null() {
                (self.api.frame_free)(&mut self.frame);
            }
            if !self.packet.is_null() {
                (self.api.packet_free)(&mut self.packet);
            }
            if !self.context.is_null() {
                (self.api.free_context)(&mut self.context);
            }
        }
    }
}
