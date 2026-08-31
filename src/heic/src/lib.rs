// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

//! heic - decode the first HEIC/HEIF image into an RGBA8 pixel frame.
//!
//! The module uses the pure-Rust `heic` backend and exposes only the shared
//! raw ABI. Resource limits are checked before the decoder can allocate the
//! full image, and all failures return the ABI's rejected result (`0`).

use abi::frame_pixels;
use heic::{DecoderConfig, ImageInfo, Limits, PixelLayout};

const MAX_INPUT_BYTES: usize = 256 * 1024 * 1024;
const MAX_MEMORY_BYTES: u64 = 512 * 1024 * 1024;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;
const MAX_DIMENSION: u64 = 16_384;

/// The module's self-description as UTF-8 JSON.
const MANIFEST: &str = r#"{
  "exports": {
    "decode": {
      "summary": "Decode the first HEIC or HEIF image into an RGBA8 pixel frame with container transforms applied.",
      "output": "pixels",
      "options": {}
    }
  }
}"#;

/// Reject dimensions before the full decoder is invoked.
fn within_limits(info: &ImageInfo) -> bool {
    let width = u64::from(info.width);
    let height = u64::from(info.height);
    let pixels = width.checked_mul(height);
    let memory = pixels.and_then(|count| count.checked_mul(4));

    width <= MAX_DIMENSION
        && height <= MAX_DIMENSION
        && pixels.is_some_and(|count| count <= MAX_PIXELS)
        && memory.is_some_and(|bytes| bytes <= MAX_MEMORY_BYTES)
}

/// Read a little- or big-endian integer from a TIFF payload.
fn read_tiff_u16(data: &[u8], offset: usize, little_endian: bool) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let bytes = data.get(offset..end)?;

    Some(if little_endian {
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        u16::from_be_bytes([bytes[0], bytes[1]])
    })
}

/// Read the EXIF orientation tag from the first TIFF IFD, if present.
fn exif_orientation(exif: &[u8]) -> Option<u16> {
    let little_endian = match exif.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };

    if read_tiff_u16(exif, 2, little_endian)? != 42 {
        return None;
    }

    let offset_bytes = exif.get(4..8)?;
    let ifd_offset = if little_endian {
        u32::from_le_bytes(offset_bytes.try_into().ok()?)
    } else {
        u32::from_be_bytes(offset_bytes.try_into().ok()?)
    } as usize;
    let entries = read_tiff_u16(exif, ifd_offset, little_endian)? as usize;

    for index in 0..entries {
        let entry = ifd_offset.checked_add(2)?.checked_add(index.checked_mul(12)?)?;

        if read_tiff_u16(exif, entry, little_endian)? != 0x0112 {
            continue;
        }

        if read_tiff_u16(exif, entry.checked_add(2)?, little_endian)? != 3
            || read_tiff_u16(exif, entry.checked_add(4)?, little_endian)? == 0
        {
            return None;
        }

        let value = read_tiff_u16(exif, entry.checked_add(8)?, little_endian)?;

        return (1..=8).contains(&value).then_some(value);
    }

    None
}

/// Apply an EXIF orientation to tightly packed RGBA pixels.
fn apply_orientation(width: u32, height: u32, pixels: &[u8], orientation: u16) -> Option<(u32, u32, Vec<u8>)> {
    if orientation == 1 {
        return Some((width, height, pixels.to_vec()));
    }

    let (output_width, output_height) = if matches!(orientation, 5..=8) {
        (height, width)
    } else {
        (width, height)
    };
    let pixel_count = (output_width as usize).checked_mul(output_height as usize)?;
    let byte_count = pixel_count.checked_mul(4)?;
    let mut output = Vec::new();
    output.try_reserve_exact(byte_count).ok()?;
    output.resize(byte_count, 0);

    for y in 0..output_height {
        for x in 0..output_width {
            let (source_x, source_y) = match orientation {
                2 => (width - 1 - x, y),
                3 => (width - 1 - x, height - 1 - y),
                4 => (x, height - 1 - y),
                5 => (y, x),
                6 => (y, height - 1 - x),
                7 => (width - 1 - y, height - 1 - x),
                8 => (width - 1 - y, x),
                _ => (x, y),
            };
            let source = ((source_y as usize) * width as usize + source_x as usize) * 4;
            let target = ((y as usize) * output_width as usize + x as usize) * 4;

            output[target..target + 4].copy_from_slice(&pixels[source..source + 4]);
        }
    }

    Some((output_width, output_height, output))
}

/// Decode the first displayable image into the shared RGBA8 pixel frame.
fn decode_rgba(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return None;
    }

    let info = ImageInfo::from_bytes(bytes).ok()?;

    if !within_limits(&info) {
        return None;
    }

    let mut limits = Limits::default();
    limits.max_width = Some(MAX_DIMENSION);
    limits.max_height = Some(MAX_DIMENSION);
    limits.max_pixels = Some(MAX_PIXELS);
    limits.max_memory_bytes = Some(MAX_MEMORY_BYTES);

    let output = DecoderConfig::new()
        .decode_request(bytes)
        .with_output_layout(PixelLayout::Rgba8)
        .with_limits(&limits)
        .decode()
        .ok()?;

    let orientation = info.exif.as_deref().and_then(exif_orientation).unwrap_or(1);
    let (width, height, pixels) = apply_orientation(output.width, output.height, &output.data, orientation)?;

    frame_pixels(width, height, 4, &pixels)
}

/// Allocate a write buffer of exactly `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    abi::alloc_buf(len)
}

/// Free a buffer previously handed out by `alloc`.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    abi::free_buf(ptr, len)
}

/// Decode HEIC/HEIF bytes at `ptr..ptr+len` and return a packed RGBA8 frame.
#[no_mangle]
pub unsafe extern "C" fn decode(ptr: u32, len: u32, _opts_ptr: u32, opts_len: u32) -> u64 {
    if opts_len != 0 {
        return 0;
    }

    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    match decode_rgba(input) {
        Some(frame) => abi::pack(frame),
        None => 0,
    }
}

/// Return the manifest JSON packed as `ptr << 32 | len`.
#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_oversized_input() {
        assert_eq!(decode_rgba(&[]), None);
        assert_eq!(decode_rgba(&vec![0; MAX_INPUT_BYTES + 1]), None);
    }

    #[test]
    fn rejects_dimensions_over_the_pixel_limit() {
        let info = ImageInfo {
            width: 16_384,
            height: 16_384,
            has_alpha: false,
            bit_depth: 8,
            chroma_format: 1,
            has_exif: false,
            has_xmp: false,
            has_thumbnail: false,
            color_primaries: 2,
            transfer_characteristics: 2,
            matrix_coefficients: 2,
            video_full_range: false,
            has_icc_profile: false,
            has_depth: false,
            has_gain_map: false,
            exif: None,
            xmp: None,
            icc_profile: None,
        };

        assert!(!within_limits(&info));
    }

    #[test]
    fn reads_exif_orientation_and_rotates_rgba() {
        let exif = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0,
            0, 0, 0, 0,
        ];
        let pixels = [
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255,
            6, 0, 0, 255,
        ];

        assert_eq!(exif_orientation(&exif), Some(6));
        let (width, height, rotated) = apply_orientation(2, 3, &pixels, 6).unwrap();

        assert_eq!((width, height), (3, 2));
        assert_eq!(rotated[0], 5);
        assert_eq!(rotated[4], 3);
        assert_eq!(rotated[8], 1);
    }
}
