//! What the computer plays, from WASAPI's loopback.
//!
//! cpal records an output device's mix when it is asked for an input stream
//! on it — WASAPI's loopback mode — in the device's own format, usually
//! 32-bit float at 48 kHz, sometimes with more than two channels. That
//! becomes the 16-bit 48 kHz stereo UwUCast carries. When nothing plays,
//! WASAPI sends nothing; the receiver's player just waits.
//!
//! As with playing sound, cpal's stream lives on a thread of its own.

use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use uwumirror_core::audio::Resampler;

use crate::protocol::{AUDIO_CHANNELS, AUDIO_RATE};
use crate::sender::Outbox;

/// Records until dropped.
pub struct Loopback {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Loopback {
    pub fn start(outbox: Outbox, clock: Instant) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("uwumirror-loopback".into())
            .spawn(move || {
                let stream = match record(outbox, clock) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        stream
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                let _ = stop_rx.recv();
                drop(stream);
            })
            .map_err(|error| error.to_string())?;
        ready_rx
            .recv()
            .map_err(|_| "the sound thread ended".to_owned())??;
        Ok(Self {
            stop: Some(stop_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn record(outbox: Outbox, clock: Instant) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("no sound output device")?;
    let supported = device
        .default_output_config()
        .map_err(|error| error.to_string())?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    tracing::info!(
        rate = config.sample_rate.0,
        channels = config.channels,
        ?format,
        "recording what the computer plays"
    );
    match format {
        cpal::SampleFormat::F32 => input::<f32>(&device, &config, outbox, clock),
        cpal::SampleFormat::I16 => input::<i16>(&device, &config, outbox, clock),
        cpal::SampleFormat::I32 => input::<i32>(&device, &config, outbox, clock),
        cpal::SampleFormat::U16 => input::<u16>(&device, &config, outbox, clock),
        other => Err(format!("unsupported sample format {other}")),
    }
    .and_then(|stream| {
        stream.play().map_err(|error| error.to_string())?;
        Ok(stream)
    })
}

fn input<T: SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    outbox: Outbox,
    clock: Instant,
) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let mut resampler = Resampler::new(
        config.sample_rate.0,
        config.channels as usize,
        AUDIO_RATE,
        AUDIO_CHANNELS,
    );
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let floats: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
                let converted = resampler.convert(&floats);
                if converted.is_empty() {
                    return;
                }
                let samples: Vec<i16> = converted
                    .iter()
                    .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
                    .collect();
                outbox.audio(clock.elapsed().as_micros() as u64, samples);
            },
            |error| tracing::warn!(%error, "recording what the computer plays"),
            None,
        )
        .map_err(|error| error.to_string())
}
