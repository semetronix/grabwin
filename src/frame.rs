use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra,
    Rgba,
    Rgb,
    Bgr,
}

impl PixelFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "bgra" => Ok(Self::Bgra),
            "rgba" => Ok(Self::Rgba),
            "rgb" => Ok(Self::Rgb),
            "bgr" => Ok(Self::Bgr),
            other => Err(Error::Invalid(format!(
                "unknown pixel format {other:?}; expected bgra, rgba, rgb or bgr"
            ))),
        }
    }

    pub fn channels(self) -> usize {
        match self {
            Self::Bgra | Self::Rgba => 4,
            Self::Rgb | Self::Bgr => 3,
        }
    }
}

/// A tightly packed BGRA frame (row stride == width * 4).
#[derive(Debug, Clone)]
pub struct Frame {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Frame {
    pub fn to_format(&self, fmt: PixelFormat) -> Vec<u8> {
        convert(&self.bgra, fmt)
    }
}

/// Copies `height` rows of `width * 4` bytes out of a D3D11 mapped buffer with the given row pitch.
pub fn unpack_rows(src: &[u8], row_pitch: usize, width: u32, height: u32) -> Vec<u8> {
    let row_bytes = width as usize * 4;
    if row_pitch == row_bytes {
        return src[..row_bytes * height as usize].to_vec();
    }
    let mut out = Vec::with_capacity(row_bytes * height as usize);
    for y in 0..height as usize {
        let start = y * row_pitch;
        out.extend_from_slice(&src[start..start + row_bytes]);
    }
    out
}

/// Converts tightly packed BGRA bytes into the requested layout.
pub fn convert(bgra: &[u8], fmt: PixelFormat) -> Vec<u8> {
    match fmt {
        PixelFormat::Bgra => bgra.to_vec(),
        PixelFormat::Rgba => {
            let mut out = bgra.to_vec();
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            out
        }
        PixelFormat::Rgb => {
            let mut out = Vec::with_capacity(bgra.len() / 4 * 3);
            for px in bgra.chunks_exact(4) {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            }
            out
        }
        PixelFormat::Bgr => {
            let mut out = Vec::with_capacity(bgra.len() / 4 * 3);
            for px in bgra.chunks_exact(4) {
                out.extend_from_slice(&px[..3]);
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2x1 image: pixel0 = B1 G2 R3 A4, pixel1 = B5 G6 R7 A8
    const BGRA: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    #[test]
    fn parse_formats() {
        assert!(matches!(PixelFormat::parse("bgra"), Ok(PixelFormat::Bgra)));
        assert!(matches!(PixelFormat::parse("RGB"), Ok(PixelFormat::Rgb)));
        assert!(matches!(PixelFormat::parse("rgba"), Ok(PixelFormat::Rgba)));
        assert!(matches!(PixelFormat::parse("bgr"), Ok(PixelFormat::Bgr)));
        assert!(PixelFormat::parse("yuv").is_err());
    }

    #[test]
    fn channels() {
        assert_eq!(PixelFormat::Bgra.channels(), 4);
        assert_eq!(PixelFormat::Rgba.channels(), 4);
        assert_eq!(PixelFormat::Rgb.channels(), 3);
        assert_eq!(PixelFormat::Bgr.channels(), 3);
    }

    #[test]
    fn convert_bgra_is_copy() {
        assert_eq!(convert(&BGRA, PixelFormat::Bgra), BGRA.to_vec());
    }

    #[test]
    fn convert_rgba_swaps_r_b() {
        assert_eq!(
            convert(&BGRA, PixelFormat::Rgba),
            vec![3, 2, 1, 4, 7, 6, 5, 8]
        );
    }

    #[test]
    fn convert_rgb_drops_alpha_and_swaps() {
        assert_eq!(convert(&BGRA, PixelFormat::Rgb), vec![3, 2, 1, 7, 6, 5]);
    }

    #[test]
    fn convert_bgr_drops_alpha() {
        assert_eq!(convert(&BGRA, PixelFormat::Bgr), vec![1, 2, 3, 5, 6, 7]);
    }

    #[test]
    fn unpack_rows_strips_pitch_padding() {
        // width 1 px (4 bytes), height 2, pitch 8 (4 bytes padding per row)
        let src = [1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8, 99, 99, 99, 99];
        assert_eq!(unpack_rows(&src, 8, 1, 2), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn unpack_rows_tight_pitch_is_identity() {
        let src = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(unpack_rows(&src, 4, 1, 2), src.to_vec());
    }

    #[test]
    fn frame_to_format() {
        let f = Frame {
            bgra: BGRA.to_vec(),
            width: 2,
            height: 1,
        };
        assert_eq!(f.to_format(PixelFormat::Rgb), vec![3, 2, 1, 7, 6, 5]);
    }
}
