//! The size a picture is passed on in, and how it leaves the GPU's layout.
//!
//! Every picture crosses into the page uncompressed, so its size is what
//! costs: 1080p in NV12 is 3 MB, 180 MB a second at 60 frames. Anything
//! bigger is scaled down to fit 1920 × 1080 (either way round) on the graphics
//! card, while it is copied out of the player anyway.

/// The longer side of a picture handed to the page, at most.
pub const MAX_LONG_SIDE: u32 = 1920;
/// The shorter side, at most.
pub const MAX_SHORT_SIDE: u32 = 1080;

/// The size to pass a `width × height` picture on in: the same aspect, no
/// bigger than [`MAX_LONG_SIDE`] × [`MAX_SHORT_SIDE`] in its own orientation,
/// even both ways (NV12 halves the chroma both ways), never empty.
pub fn fit(width: u32, height: u32) -> (u32, u32) {
    let (long, short) = (width.max(height).max(1), width.min(height).max(1));
    let scale = (f64::from(MAX_LONG_SIDE) / f64::from(long))
        .min(f64::from(MAX_SHORT_SIDE) / f64::from(short))
        .min(1.0);
    let even = |side: u32| ((f64::from(side) * scale) as u32 & !1).max(2);
    (even(width.max(1)), even(height.max(1)))
}

/// NV12 from a mapped texture, whose rows are `pitch` bytes apart, into the
/// tightly packed form [`RawFrame`](uwumirror_core::RawFrame) carries: `height`
/// rows of luma, then `height / 2` rows of interleaved chroma, each `width`
/// bytes. The chroma plane starts right after `height` rows of luma, as
/// Direct3D lays out a mapped NV12 texture.
///
/// `None` when `mapped` is too short for that.
pub fn pack_nv12(mapped: &[u8], pitch: usize, width: usize, height: usize) -> Option<Vec<u8>> {
    let rows = height + height / 2;
    if width > pitch || rows == 0 || mapped.len() < pitch * (rows - 1) + width {
        return None;
    }
    let mut packed = Vec::with_capacity(width * rows);
    for row in mapped.chunks(pitch).take(rows) {
        packed.extend_from_slice(&row[..width]);
    }
    Some(packed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_pictures_keep_their_size() {
        assert_eq!(fit(1920, 1080), (1920, 1080));
        assert_eq!(fit(1280, 720), (1280, 720));
        assert_eq!(fit(1080, 1920), (1080, 1920));
    }

    #[test]
    fn big_pictures_fit_either_way_round() {
        assert_eq!(fit(3840, 2160), (1920, 1080));
        assert_eq!(fit(2160, 3840), (1080, 1920));
        assert_eq!(fit(2560, 1600), (1728, 1080));
        assert_eq!(fit(1080, 2400), (864, 1920));
    }

    #[test]
    fn sides_are_even_and_never_zero() {
        assert_eq!(fit(1279, 721), (1278, 720));
        assert_eq!(fit(0, 0), (2, 2));
        assert_eq!(fit(1, 1), (2, 2));
    }

    #[test]
    fn rows_lose_their_padding() {
        // 4 × 2 picture, rows 6 bytes apart: two luma rows, one chroma row.
        let mapped = [
            1, 2, 3, 4, 0, 0, //
            5, 6, 7, 8, 0, 0, //
            9, 10, 11, 12, // the last row needs no padding after it
        ];
        assert_eq!(
            pack_nv12(&mapped, 6, 4, 2).unwrap(),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
        );
        assert!(pack_nv12(&mapped[..15], 6, 4, 2).is_none());
        assert!(pack_nv12(&mapped, 3, 4, 2).is_none());
    }
}
