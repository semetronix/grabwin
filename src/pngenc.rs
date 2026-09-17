use crate::error::{Error, Result};
use crate::frame::{convert, PixelFormat};

/// Encodes tightly packed BGRA as an RGB PNG (alpha dropped).
///
/// `compression`: 0 = stored, 1 = fast (default), 2–5 = balanced, 6–9 = high.
pub fn encode_png(bgra: &[u8], width: u32, height: u32, compression: u8) -> Result<Vec<u8>> {
    if compression > 9 {
        return Err(Error::Invalid(format!(
            "compression must be 0..=9, got {compression}"
        )));
    }
    let expected = width as usize * height as usize * 4;
    if bgra.len() != expected {
        return Err(Error::Invalid(format!(
            "bgra buffer is {} bytes, expected {expected} for {width}x{height}",
            bgra.len()
        )));
    }
    let rgb = convert(bgra, PixelFormat::Rgb);

    let (level, filter) = match compression {
        0 => (png::Compression::NoCompression, png::Filter::Sub),
        1 => (png::Compression::Fast, png::Filter::Sub),
        2..=5 => (png::Compression::Balanced, png::Filter::Adaptive),
        _ => (png::Compression::High, png::Filter::Adaptive),
    };

    let mut out = Vec::with_capacity(rgb.len() / 2);
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(level);
        enc.set_filter(filter);
        let mut writer = enc
            .write_header()
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer
            .write_image_data(&rgb)
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer.finish().map_err(|e| Error::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(data: &[u8]) -> (u32, u32, png::ColorType, Vec<u8>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(data));
        let mut reader = decoder.read_info().unwrap();
        // png 0.18: output_buffer_size() -> Option<usize>; если компилятор скажет usize — убрать unwrap()
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        (info.width, info.height, info.color_type, buf)
    }

    // 2x2 BGRA: red, green / blue, white
    const BGRA: [u8; 16] = [
        0, 0, 255, 255, 0, 255, 0, 255, //
        255, 0, 0, 255, 255, 255, 255, 255,
    ];
    const RGB: [u8; 12] = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];

    #[test]
    fn roundtrip_all_levels() {
        for level in [0u8, 1, 3, 9] {
            let data = encode_png(&BGRA, 2, 2, level).unwrap();
            assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "level {level}");
            let (w, h, ct, px) = decode(&data);
            assert_eq!((w, h), (2, 2));
            assert_eq!(ct, png::ColorType::Rgb);
            assert_eq!(px, RGB.to_vec(), "level {level}");
        }
    }

    #[test]
    fn level_above_nine_is_rejected() {
        assert!(encode_png(&BGRA, 2, 2, 10).is_err());
    }

    #[test]
    fn size_mismatch_is_rejected() {
        assert!(encode_png(&BGRA, 3, 2, 1).is_err());
    }
}
