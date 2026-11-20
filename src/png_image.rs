//! Bounded decoding of static RGB and RGBA fixture PNGs.

use png::{BitDepth, ColorType, Decoder, Limits, Transformations};
use std::collections::BTreeMap;
use std::io::Cursor;

pub const MAX_DECODED_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_ENCODED_BYTES: usize = MAX_DECODED_BYTES + 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 2 * MAX_DECODED_BYTES;
const MAX_RESOURCES: usize = 64;

#[derive(Debug)]
pub struct PngImage {
    width: u32,
    height: u32,
    color: ColorType,
    samples: Vec<u8>,
}

impl PngImage {
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        Self::decode_with_budget(bytes, MAX_DECODED_BYTES)
    }

    fn decode_with_budget(bytes: &[u8], budget: usize) -> Result<Self, String> {
        if bytes.len() > MAX_ENCODED_BYTES {
            return Err(format!(
                "PNG encoded data exceeds {MAX_ENCODED_BYTES} bytes"
            ));
        }
        let mut decoder = Decoder::new_with_limits(
            Cursor::new(bytes),
            Limits {
                bytes: MAX_DECODED_BYTES,
            },
        );
        decoder.set_ignore_text_chunk(true);
        decoder.ignore_checksums(false);
        decoder.set_transformations(Transformations::EXPAND);
        let header = decoder
            .read_header_info()
            .map_err(|error| format!("invalid PNG header: {error}"))?;
        let (width, height) = header.size();
        let rgba_bytes = decoded_size(width, height)?;
        if rgba_bytes > budget {
            return Err(format!(
                "document PNG resources exceed {MAX_DOCUMENT_BYTES} decoded bytes"
            ));
        }
        if header.bit_depth != BitDepth::Eight
            || !matches!(header.color_type, ColorType::Rgb | ColorType::Rgba)
        {
            return Err("fixture PNG requires 8-bit RGB or RGBA samples".to_owned());
        }
        let mut reader = decoder
            .read_info()
            .map_err(|error| format!("invalid PNG metadata: {error}"))?;
        validate_metadata(reader.info())?;
        let size = reader
            .output_buffer_size()
            .filter(|size| *size <= rgba_bytes)
            .ok_or_else(|| "invalid PNG output buffer size".to_owned())?;
        let mut samples = vec![0; size];
        let frame = reader
            .next_frame(&mut samples)
            .map_err(|error| format!("invalid PNG image data: {error}"))?;
        reader
            .finish()
            .map_err(|error| format!("invalid PNG ending: {error}"))?;
        validate_metadata(reader.info())?;
        if frame.width != width
            || frame.height != height
            || frame.bit_depth != BitDepth::Eight
            || !matches!(frame.color_type, ColorType::Rgb | ColorType::Rgba)
            || frame.buffer_size() != size
        {
            return Err("PNG frame differs from the expected image format and size".to_owned());
        }
        Ok(Self {
            width,
            height,
            color: frame.color_type,
            samples,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub(crate) fn rgba(&self, x: u32, y: u32) -> [u8; 4] {
        debug_assert!(x < self.width && y < self.height);
        let channels = self.color.samples();
        let offset = (y as usize * self.width as usize + x as usize) * channels;
        let pixel = &self.samples[offset..offset + channels];
        [
            pixel[0],
            pixel[1],
            pixel[2],
            pixel.get(3).copied().unwrap_or(255),
        ]
    }
}

fn decoded_size(width: u32, height: u32) -> Result<usize, String> {
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|bytes| (1..=MAX_DECODED_BYTES).contains(bytes));
    bytes.ok_or_else(|| {
        format!("PNG dimensions exceed the {MAX_DECODED_BYTES}-byte decoded size limit")
    })
}

fn validate_metadata(info: &png::Info<'_>) -> Result<(), String> {
    if info.animation_control.is_some() || info.frame_control.is_some() {
        return Err("animated PNG is outside the fixture image grammar".to_owned());
    }
    if info.icc_profile.is_some()
        || info.coding_independent_code_points.is_some()
        || info.mastering_display_color_volume.is_some()
        || info.content_light_level.is_some()
        || (info.srgb.is_none() && (info.gama_chunk.is_some() || info.chrm_chunk.is_some()))
    {
        return Err("fixture PNG requires untagged or sRGB color".to_owned());
    }
    if info.exif_metadata.is_some() {
        return Err("PNG EXIF orientation is outside the fixture image grammar".to_owned());
    }
    Ok(())
}

/// Images identified by exact, relative fixture labels. The renderer never
/// resolves a label to a filesystem path or network address.
#[derive(Default)]
pub struct PngResources {
    images: BTreeMap<String, PngImage>,
    decoded_bytes: usize,
}

impl PngResources {
    pub fn insert(&mut self, name: &str, source: &[u8]) -> Result<(), String> {
        if name.is_empty()
            || name.len() > 128
            || matches!(name, "." | "..")
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err("PNG resource name must be a simple fixture label".to_owned());
        }
        if self.images.contains_key(name) {
            return Err(format!("duplicate PNG resource: {name}"));
        }
        if self.images.len() >= MAX_RESOURCES {
            return Err(format!("document exceeds {MAX_RESOURCES} PNG resources"));
        }
        let image = PngImage::decode_with_budget(source, MAX_DOCUMENT_BYTES - self.decoded_bytes)
            .map_err(|error| format!("PNG resource {name}: {error}"))?;
        self.decoded_bytes += decoded_size(image.width, image.height)?;
        self.images.insert(name.to_owned(), image);
        Ok(())
    }

    pub(crate) fn get(&self, name: &str) -> Option<&PngImage> {
        self.images.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECKER: &[u8] = include_bytes!("../fixtures/m1/assets/checker.png");

    #[test]
    fn pinned_image_decodes_including_an_alpha_zero_sample() {
        let image = PngImage::decode(CHECKER).unwrap();
        assert_eq!(image.size(), (2, 2));
        assert_eq!(image.rgba(0, 0), [255, 0, 0, 255]);
        assert_eq!(image.rgba(1, 0), [0, 255, 0, 255]);
        assert_eq!(image.rgba(0, 1), [0, 0, 255, 255]);
        assert_eq!(image.rgba(1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn rejects_truncated_header_and_image_ending() {
        let truncated = include_bytes!("../fixtures/m1/assets/truncated.png");
        assert!(
            PngImage::decode(truncated)
                .unwrap_err()
                .starts_with("invalid PNG header:")
        );
        assert!(PngImage::decode(&CHECKER[..CHECKER.len() - 5]).is_err());
        let mut corrupt = CHECKER.to_vec();
        corrupt[45] ^= 1;
        assert!(PngImage::decode(&corrupt).is_err());
    }

    #[test]
    fn validates_zlib_adler_checksum_even_with_a_valid_idat_crc() {
        let mut corrupt = CHECKER.to_vec();
        let idat = corrupt
            .windows(4)
            .position(|bytes| bytes == b"IDAT")
            .unwrap();
        let length = u32::from_be_bytes(corrupt[idat - 4..idat].try_into().unwrap()) as usize;
        let end = idat + 4 + length;
        // Modify only the zlib trailer, then regenerate the enclosing PNG CRC.
        corrupt[end - 1] ^= 1;
        let mut crc = !0u32;
        for &byte in &corrupt[idat..end] {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
            }
        }
        corrupt[end..end + 4].copy_from_slice(&(!crc).to_be_bytes());
        let error = PngImage::decode(&corrupt).unwrap_err();
        assert!(error.starts_with("invalid PNG image data:"), "{error}");
    }

    #[test]
    fn bounds_preflight_normalized_bytes_and_preserve_existing_resources() {
        assert_eq!(decoded_size(2048, 2047).unwrap(), MAX_DECODED_BYTES - 8192);
        assert_eq!(decoded_size(2048, 2048).unwrap(), MAX_DECODED_BYTES);
        assert!(decoded_size(2048, 2049).is_err());
        assert!(decoded_size(u32::MAX, u32::MAX).is_err());
        assert!(
            PngImage::decode_with_budget(CHECKER, 15)
                .unwrap_err()
                .contains("document PNG resources")
        );
        let mut resources = PngResources::default();
        resources.insert("checker.png", CHECKER).unwrap();
        assert!(resources.insert("../checker.png", CHECKER).is_err());
        assert!(
            resources
                .insert("checker.png", b"bad data")
                .unwrap_err()
                .contains("duplicate")
        );
        assert_eq!(resources.get("checker.png").unwrap().size(), (2, 2));
    }

    #[test]
    fn decoder_accepts_the_frame_cap_and_rejects_an_oversized_header() {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2048, 2048);
            encoder.set_color(ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&vec![0; MAX_DECODED_BYTES])
                .unwrap();
            writer.finish().unwrap();
        }
        let decoded = PngImage::decode(&encoded).unwrap();
        assert_eq!(decoded.size(), (2048, 2048));
        assert_eq!(decoded.rgba(2047, 2047), [0; 4]);

        let mut oversized = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut oversized, 2048, 2049);
            encoder.set_color(ColorType::Rgba);
            let _header = encoder.write_header().unwrap();
        }
        assert!(
            PngImage::decode(&oversized)
                .unwrap_err()
                .contains("16777216-byte decoded size limit")
        );
    }
}
