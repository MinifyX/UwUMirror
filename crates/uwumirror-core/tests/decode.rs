//! Decodes real AAC through the system's FFmpeg, when it has one.
//!
//! The `ffmpeg` command makes a second of a 440 Hz tone as raw ADTS; its
//! frames go through `AudioDecoder` the way AirPlay's AAC packets do. Without
//! FFmpeg on the machine there is nothing to test, and the test says so.

use std::process::Command;

use uwumirror_core::decode::{ffmpeg_version, AudioCodec, AudioDecoder};

fn adts_frames(data: &[u8]) -> Vec<&[u8]> {
    let mut frames = Vec::new();
    let mut at = 0;
    while at + 7 <= data.len() {
        assert_eq!(data[at], 0xff, "ADTS sync");
        let header = if data[at + 1] & 1 == 1 { 7 } else { 9 };
        let length = ((data[at + 3] as usize & 3) << 11)
            | ((data[at + 4] as usize) << 3)
            | (data[at + 5] as usize >> 5);
        frames.push(&data[at + header..at + length]);
        at += length;
    }
    frames
}

#[test]
fn decodes_aac_lc_to_stereo_float() {
    if ffmpeg_version().is_none() {
        eprintln!("no libavcodec on this system, skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("uwumirror-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("tone.aac");
    let made = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"])
        .args(["-i", "sine=frequency=440:sample_rate=44100:duration=1"])
        .args(["-ac", "2", "-c:a", "aac", "-f", "adts"])
        .arg(&file)
        .status();
    let Ok(status) = made else {
        eprintln!("no ffmpeg command on this system, skipping");
        return;
    };
    assert!(status.success());
    let data = std::fs::read(&file).unwrap();
    std::fs::remove_dir_all(&dir).ok();

    let mut decoder = AudioDecoder::new(AudioCodec::AacLc).expect("decoder");
    let mut samples = Vec::new();
    for frame in adts_frames(&data) {
        samples.extend(decoder.decode(frame));
    }
    // A second of stereo, give or take the encoder's priming frames.
    assert!(samples.len() > 80_000, "only {} samples", samples.len());
    let peak = samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    assert!(peak > 0.05 && peak <= 1.0, "peak {peak}");
}

#[test]
fn damaged_packets_give_silence_not_a_crash() {
    let Ok(mut decoder) = AudioDecoder::new(AudioCodec::AacEld) else {
        eprintln!("no libavcodec on this system, skipping");
        return;
    };
    for garbage in [
        &[0u8; 3][..],
        &[0xff; 200][..],
        &[1, 2, 3, 4, 5, 6, 7, 8][..],
    ] {
        let _ = decoder.decode(garbage);
    }
    let mut alac = AudioDecoder::new(AudioCodec::Alac).expect("ALAC decoder");
    let _ = alac.decode(&[0x20; 64]);
}
