//! Sound out of the default output device.
//!
//! Each stream with sound gets an [`AudioPlayer`]: the source pushes PCM as it
//! arrives, the player converts it to the device's rate and channels and keeps
//! a short buffer. Mirroring is live, so latency wins over completeness: a
//! buffer that grows past a few hundred milliseconds (a device that sends a
//! little faster than we play, a hiccup on Wi-Fi) is cut back instead of
//! letting the sound drift further and further behind the picture.
//!
//! cpal's stream isn't `Send` on every system, so it lives on a thread of its
//! own for as long as the player does.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use parking_lot::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no sound output device")]
    NoDevice,
    #[error("the sound output device refused: {0}")]
    Device(String),
}

/// Played before the first sound comes out, and again after running dry.
const PRIME_SECONDS: f32 = 0.06;
/// More than this waiting in the buffer, and it is cut back to `TRIM_TO_SECONDS`.
const MAX_SECONDS: f32 = 0.3;
const TRIM_TO_SECONDS: f32 = 0.1;

struct Shared {
    buffer: Mutex<VecDeque<f32>>,
    rate: u32,
    channels: usize,
    /// Linear gain as f32 bits.
    volume: AtomicU32,
    primed: AtomicBool,
}

impl Shared {
    fn samples(&self, seconds: f32) -> usize {
        (self.rate as f32 * seconds) as usize * self.channels
    }

    fn fill<T: SizedSample + FromSample<f32>>(&self, out: &mut [T]) {
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let mut buffer = self.buffer.lock();
        if !self.primed.load(Ordering::Relaxed) {
            if buffer.len() >= self.samples(PRIME_SECONDS) {
                self.primed.store(true, Ordering::Relaxed);
            } else {
                out.fill(T::from_sample(0.0));
                return;
            }
        }
        for slot in out.iter_mut() {
            match buffer.pop_front() {
                Some(sample) => *slot = T::from_sample((sample * volume).clamp(-1.0, 1.0)),
                None => {
                    *slot = T::from_sample(0.0);
                    self.primed.store(false, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Linear resampling with channel mapping, carried over from chunk to chunk.
struct Converter {
    in_rate: u32,
    in_channels: usize,
    out_rate: u32,
    out_channels: usize,
    /// The last input frame of the previous chunk, already mapped.
    last: Option<Vec<f32>>,
    /// Position between `last` and the next frame, in input frames.
    t: f64,
}

impl Converter {
    fn map(&self, frame: &[f32], out: &mut Vec<f32>) {
        match (self.in_channels, self.out_channels) {
            (a, b) if a == b => out.extend_from_slice(frame),
            (_, 1) => out.push(frame.iter().sum::<f32>() / frame.len() as f32),
            (1, n) => out.extend(std::iter::repeat_n(frame[0], n)),
            (_, n) => {
                // Stereo onto more speakers: front left and right, the rest silent.
                out.push(frame[0]);
                out.push(frame[1]);
                out.extend(std::iter::repeat_n(0.0, n - 2));
            }
        }
    }

    fn convert(&mut self, input: &[f32]) -> Vec<f32> {
        let mut mapped = Vec::with_capacity(input.len() / self.in_channels * self.out_channels);
        for frame in input.chunks_exact(self.in_channels) {
            self.map(frame, &mut mapped);
        }
        if self.in_rate == self.out_rate {
            return mapped;
        }
        let n = self.out_channels;
        let mut frames: Vec<f32> = self.last.take().unwrap_or_default();
        frames.extend_from_slice(&mapped);
        let count = frames.len() / n;
        if count < 2 {
            if count == 1 {
                self.last = Some(frames);
            }
            return Vec::new();
        }
        let step = self.in_rate as f64 / self.out_rate as f64;
        let mut out = Vec::with_capacity(((count as f64 / step) as usize + 2) * n);
        while self.t + 1.0 < count as f64 {
            let index = self.t as usize;
            let frac = (self.t - index as f64) as f32;
            for c in 0..n {
                let a = frames[index * n + c];
                let b = frames[(index + 1) * n + c];
                out.push(a + (b - a) * frac);
            }
            self.t += step;
        }
        self.t -= (count - 1) as f64;
        self.last = Some(frames[(count - 1) * n..].to_vec());
        out
    }
}

/// The same conversion for sound that goes elsewhere than the speakers: a
/// sending UwUMirror turns whatever its output device mixes into the 48 kHz
/// stereo UwUCast carries.
pub struct Resampler(Converter);

impl Resampler {
    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Self {
        Self(Converter {
            in_rate,
            in_channels: in_channels.max(1),
            out_rate,
            out_channels: out_channels.max(1),
            last: None,
            t: 0.0,
        })
    }

    /// Interleaved samples in, interleaved samples out.
    pub fn convert(&mut self, input: &[f32]) -> Vec<f32> {
        self.0.convert(input)
    }
}

/// Plays one stream's sound until dropped.
pub struct AudioPlayer {
    shared: Arc<Shared>,
    converter: Mutex<Converter>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl AudioPlayer {
    /// Opens the default output for PCM at `rate` with `channels` channels.
    pub fn open(rate: u32, channels: usize) -> Result<Self, AudioError> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<Arc<Shared>, AudioError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("uwumirror-audio".into())
            .spawn(move || {
                let stream = match build_stream() {
                    Ok((stream, shared)) => {
                        let _ = ready_tx.send(Ok(shared));
                        stream
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                // Blocks until the player is dropped (or its sender is).
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|error| AudioError::Device(error.to_string()))?;
        let shared = ready_rx
            .recv()
            .map_err(|_| AudioError::Device("the sound thread ended".into()))??;
        let converter = Converter {
            in_rate: rate,
            in_channels: channels.max(1),
            out_rate: shared.rate,
            out_channels: shared.channels,
            last: None,
            t: 0.0,
        };
        Ok(Self {
            shared,
            converter: Mutex::new(converter),
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }

    /// Interleaved samples in -1.0..=1.0.
    pub fn push_f32(&self, samples: &[f32]) {
        let converted = self.converter.lock().convert(samples);
        let mut buffer = self.shared.buffer.lock();
        buffer.extend(converted);
        if buffer.len() > self.shared.samples(MAX_SECONDS) {
            let excess = buffer.len() - self.shared.samples(TRIM_TO_SECONDS);
            // Whole frames only, or left and right swap places.
            let excess = excess - excess % self.shared.channels;
            buffer.drain(..excess);
        }
    }

    /// Interleaved signed 16-bit samples, as Android sends them.
    pub fn push_i16(&self, samples: &[i16]) {
        let floats: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        self.push_f32(&floats);
    }

    /// Linear gain, 0.0 (silent) to 1.0 (as sent).
    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    /// Throws away what is waiting, e.g. when the device skips to another track.
    pub fn flush(&self) {
        self.shared.buffer.lock().clear();
        self.shared.primed.store(false, Ordering::Relaxed);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn build_stream() -> Result<(cpal::Stream, Arc<Shared>), AudioError> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(AudioError::NoDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|error| AudioError::Device(error.to_string()))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let shared = Arc::new(Shared {
        buffer: Mutex::new(VecDeque::new()),
        rate: config.sample_rate.0,
        channels: config.channels.max(1) as usize,
        volume: AtomicU32::new(1.0f32.to_bits()),
        primed: AtomicBool::new(false),
    });
    let stream = match format {
        cpal::SampleFormat::F32 => output::<f32>(&device, &config, &shared),
        cpal::SampleFormat::I16 => output::<i16>(&device, &config, &shared),
        cpal::SampleFormat::U16 => output::<u16>(&device, &config, &shared),
        cpal::SampleFormat::I32 => output::<i32>(&device, &config, &shared),
        other => Err(AudioError::Device(format!(
            "unsupported sample format {other}"
        ))),
    }?;
    stream
        .play()
        .map_err(|error| AudioError::Device(error.to_string()))?;
    Ok((stream, shared))
}

fn output<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, AudioError> {
    let shared = shared.clone();
    device
        .build_output_stream(
            config,
            move |out: &mut [T], _| shared.fill(out),
            |error| tracing::warn!(%error, "sound output"),
            None,
        )
        .map_err(|error| AudioError::Device(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn converter(
        in_rate: u32,
        in_channels: usize,
        out_rate: u32,
        out_channels: usize,
    ) -> Converter {
        Converter {
            in_rate,
            in_channels,
            out_rate,
            out_channels,
            last: None,
            t: 0.0,
        }
    }

    #[test]
    fn same_rate_passes_through() {
        let mut c = converter(48_000, 2, 48_000, 2);
        assert_eq!(c.convert(&[0.1, 0.2, 0.3, 0.4]), vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn resampling_keeps_the_rate_over_many_chunks() {
        let mut c = converter(44_100, 2, 48_000, 2);
        let chunk = vec![0.5f32; 352 * 2];
        let mut frames = 0;
        for _ in 0..1000 {
            frames += c.convert(&chunk).len() / 2;
        }
        let expected = 352_000.0 * 48_000.0 / 44_100.0;
        assert!(
            (frames as f64 - expected).abs() < 3.0,
            "{frames} vs {expected}"
        );
    }

    #[test]
    fn stereo_to_mono_and_surround() {
        let mut mono = converter(48_000, 2, 48_000, 1);
        assert_eq!(mono.convert(&[1.0, 0.0]), vec![0.5]);
        let mut six = converter(48_000, 2, 48_000, 6);
        assert_eq!(
            six.convert(&[0.25, 0.75]),
            vec![0.25, 0.75, 0.0, 0.0, 0.0, 0.0]
        );
    }
}
