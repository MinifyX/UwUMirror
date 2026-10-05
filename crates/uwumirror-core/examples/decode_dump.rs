//! Decodes a recorded AAC or ALAC stream with the FFmpeg UwUMirror would load,
//! to check the decoder apart from AirPlay:
//!
//!   ffprobe -v error -select_streams a -show_packets -show_data in.mp4 > packets.txt
//!   cargo run -p uwumirror-core --example decode_dump -- <ffmpeg dir or -> <aac|alac> <config hex> <rate> <channels> packets.txt
//!
//! `<ffmpeg dir>` is a folder with the app's own FFmpeg (`-` for the
//! system's), `<config hex>` the stream's extradata as ffprobe shows it
//! (`-show_streams -show_data`), rate and channels as ffprobe says them.

use uwumirror_core::decode::{self, AudioCodec, AudioDecoder};

fn hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The packets in ffprobe's `-show_packets -show_data` output.
fn packets(dump: &str) -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for line in dump.lines() {
        if line == "data=" {
            current = Some(Vec::new());
        } else if line == "[/PACKET]" {
            all.extend(current.take());
        } else if let Some(bytes) = current.as_mut() {
            // "00000000: f8e8 2000 ...   ascii": the hex sits between the
            // offset and the two spaces before the text.
            if let Some((_, rest)) = line.split_once(": ") {
                let groups = rest.get(..40).unwrap_or(rest);
                bytes.extend(hex(groups));
            }
        }
    }
    all
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, codec, config, rate, channels, file] = args.as_slice() else {
        eprintln!(
            "usage: decode_dump <ffmpeg dir|-> <aac|alac> <config hex> <rate> <channels> <packets.txt>"
        );
        std::process::exit(2);
    };
    let rate: u32 = rate.parse().expect("sample rate");
    let channels: u32 = channels.parse().expect("channels");
    if dir != "-" {
        decode::set_bundled_ffmpeg(dir.into());
    }
    let codec = match codec.as_str() {
        "aac" => AudioCodec::AacEld,
        _ => AudioCodec::Alac,
    };
    println!("FFmpeg: libavcodec {:?}", decode::ffmpeg_version());
    let mut decoder =
        AudioDecoder::with_config(codec, &hex(config), rate, channels).expect("decoder");
    let packets = packets(&std::fs::read_to_string(file).expect("packets file"));
    let (mut decoded, mut samples, mut peak) = (0, 0, 0f32);
    for (i, packet) in packets.iter().enumerate() {
        let out = decoder.decode(packet);
        if i < 3 {
            println!(
                "packet {i}: {} bytes, starts {:02x?}, {} samples, error {:?}",
                packet.len(),
                &packet[..packet.len().min(4)],
                out.len(),
                decoder.last_error
            );
        }
        if !out.is_empty() {
            decoded += 1;
        }
        samples += out.len();
        peak = out.iter().fold(peak, |p, s| p.max(s.abs()));
    }
    println!(
        "{} packets, {decoded} decoded, {samples} samples, peak {peak:.3}",
        packets.len()
    );
}
