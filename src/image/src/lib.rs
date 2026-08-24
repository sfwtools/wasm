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

//! image - decode PNG and JPEG images into raw pixel frames, and encode raw
//! pixel frames back into compressed PNG.
//!
//! Decoding rides on the `image` crate with only the PNG and JPEG codecs
//! enabled; everything else stays out so the artifact remains small. The
//! output is the shared pixel wire frame (see README.md): little-endian
//! width and height, a channel count, then row-major samples - no headers,
//! no compression - ready for consumers such as `qr.read`. `encode` is the
//! reverse: a pixel frame (luma or RGBA) in, a compressed PNG out.
//!
//! Options travel as the house options blob (see README.md): `0x01` magic,
//! little-endian length-prefixed UTF-8 key/value pairs; unknown keys ignored,
//! known keys with bad values fail the call.
//!
//! Buffer packing and pixel-frame framing come from the shared `abi` crate;
//! only the tool logic lives here.

use abi::{frame_pixels, option_pairs, parse_pixels};

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side.
const MANIFEST: &str = r#"{
  "exports": {
    "decode": {
      "summary": "Decode a PNG or JPEG image into a raw pixel frame.",
      "output": "pixels",
      "options": {
        "color": {
          "type": "enum",
          "values": ["luma", "rgba"],
          "default": "luma",
          "description": "Sample layout of the returned pixels: 1 byte per pixel grayscale, or 4 bytes per pixel RGBA."
        }
      }
    },
    "encode": {
      "summary": "Encode a raw pixel frame (luma or RGBA) as a compressed PNG image."
    }
  }
}"#;

/// Sample layout requested via the `color` option.
#[derive(Debug, PartialEq)]
pub enum Color {
    /// One grayscale byte per pixel.
    Luma,
    /// Four RGBA bytes per pixel.
    Rgba,
}

impl Color {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"luma" => Some(Color::Luma),
            b"rgba" => Some(Color::Rgba),
            _ => None,
        }
    }
}

/// Decode `bytes` (a PNG or JPEG file, auto-detected) into raw samples laid
/// out per `color`. Errors name the cause for hosts that surface them (the
/// ABI itself just returns 0).
pub fn decode_pixels(bytes: &[u8], color: &Color) -> Result<(u32, u32, u8, Vec<u8>), &'static str> {
    let img = image::load_from_memory(bytes).map_err(|_| "the input is not a readable PNG or JPEG image")?;
    let width = img.width();
    let height = img.height();

    let (channels, pixels) = match color {
        Color::Luma => (1, img.to_luma8().into_raw()),
        Color::Rgba => (4, img.to_rgba8().into_raw()),
    };

    Ok((width, height, channels, pixels))
}

/// Encode a raw pixel frame (see README.md) into a compressed PNG. The frame
/// may be luma (1 channel) or RGBA (4 channels); anything else is rejected.
/// Errors name the cause for hosts that surface them (the ABI itself just
/// returns 0).
///
/// The PNG container is hand-rolled around `fdeflate` (deflate) and
/// `crc32fast` (chunk checksums) instead of going through the `png` crate's
/// encoder: png 0.17 declares flate2 as a non-optional dependency, so calling
/// `png::Encoder` would link flate2/miniz_oxide into the artifact even though
/// only its `Compression::Fast` (fdeflate) path would run. Building the few
/// chunks here keeps encode on the same deflate codec the decoder already
/// ships and lets wasm-opt strip flate2 entirely.
pub fn encode_png(frame: &[u8]) -> Result<Vec<u8>, &'static str> {
    let (width, height, channels, pixels) =
        parse_pixels(frame).ok_or("the input is not a valid pixel frame")?;

    let color_type = match channels {
        1 => 0u8,
        4 => 6u8,
        _ => return Err("the pixel frame must be luma (1 channel) or RGBA (4 channels)"),
    };

    // Scanlines: each row prefixed with a filter byte, then the filtered row.
    // Filtering turns flat regions - a QR code's white background and black
    // modules - into runs of zeros before deflate, keeping the output small.
    // Sub (delta vs the previous pixel) handles horizontal flatness; Up
    // (delta vs the pixel above) handles vertical repetition, which matters
    // when `qr.encode` scales modules into repeated rows. Per row we pick
    // whichever filter produces more zero bytes - a cheap heuristic that
    // beats fixing one filter without the full adaptive cost.
    // ponytail: only Sub/Up are tried, with zero-count as the proxy for
    // compressed size, vs full PNG adaptive (None/Sub/Up/Average/Paeth + sum
    // of abs diffs). Measured ~1.5x larger than the png crate's full adaptive
    // on QR frames; upgrade only if output size on real photos matters.
    let row_len = width as usize * channels as usize;
    let mut raw = Vec::with_capacity((row_len + 1) * height as usize);
    let mut prev_row = vec![0u8; row_len];

    for row in 0..height as usize {
        let line = &pixels[row * row_len..(row + 1) * row_len];
        let mut sub = Vec::with_capacity(row_len);
        let mut up = Vec::with_capacity(row_len);

        for index in 0..row_len {
            let left = if index >= channels as usize { line[index - channels as usize] } else { 0 };
            let above = prev_row[index];

            sub.push(line[index].wrapping_sub(left));
            up.push(line[index].wrapping_sub(above));
        }

        let sub_zeros = sub.iter().filter(|&&b| b == 0).count();
        let up_zeros = up.iter().filter(|&&b| b == 0).count();

        if up_zeros > sub_zeros {
            raw.push(2);
            raw.extend_from_slice(&up);
        } else {
            raw.push(1);
            raw.extend_from_slice(&sub);
        }

        prev_row.copy_from_slice(line);
    }

    // zlib-wrapped deflate, exactly what a PNG IDAT chunk carries.
    let mut compressor = fdeflate::Compressor::new(std::io::Cursor::new(Vec::new()))
        .map_err(|_| "PNG encoding failed")?;

    compressor
        .write_data(&raw)
        .map_err(|_| "PNG encoding failed")?;
    let idat = compressor
        .finish()
        .map_err(|_| "PNG encoding failed")?
        .into_inner();

    // PNG signature + IHDR + IDAT + IEND. Each chunk is
    // length:u32BE + type + data + CRC32 over type+data.
    let mut png = Vec::with_capacity(8 + 12 + 25 + 12 + idat.len());
    png.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]);
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &idat);
    push_chunk(&mut png, b"IEND", &[]);

    Ok(png)
}

/// Append one PNG chunk (`type` + `data`) with its length and CRC to `png`.
fn push_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    png.extend_from_slice(kind);
    png.extend_from_slice(data);

    let mut crc = crc32fast::Hasher::new();

    crc.update(kind);
    crc.update(data);
    png.extend_from_slice(&crc.finalize().to_be_bytes());
}

/// Parse the options blob straight into a resolved `Color`. Framing (magic
/// byte, length prefixes) is validated by the shared `option_pairs`; unknown
/// keys are ignored so new callers stay compatible with older cores; known
/// keys with bad values are errors, because silently dropping a requested
/// option could change results. Returns `None` when the blob is malformed
/// or carries an unusable value.
fn resolve_color(blob: &[u8]) -> Option<Color> {
    let mut color = Color::Luma;

    for (key, value) in option_pairs(blob)? {
        if key == b"color" {
            color = Color::parse(value)?;
        }
    }

    Some(color)
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `decode` and back to `dealloc` when done.
///
/// # Safety
/// The returned pointer is only valid inside this module's linear memory and
/// must be released with `dealloc`.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    abi::alloc_buf(len)
}

/// Free a buffer previously handed out by `alloc`.
///
/// # Safety
/// `ptr`/`len` must come from `alloc` and must not have been freed before.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    abi::free_buf(ptr, len)
}

/// Decode the PNG or JPEG bytes at `ptr..ptr+len` into a pixel frame packed
/// as `ptr << 32 | len`: little-endian width and height, one channel-count
/// byte, then row-major samples. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); undecodable input,
/// a malformed blob, or an unusable option value returns 0. The caller reads
/// the output and deallocs both buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn decode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    let color = if opts_len == 0 {
        Color::Luma
    } else {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        match resolve_color(blob) {
            Some(color) => color,
            None => return 0,
        }
    };

    match decode_pixels(input, &color) {
        Ok((width, height, channels, pixels)) => match frame_pixels(width, height, channels, &pixels) {
            Some(frame) => abi::pack(frame),
            None => 0,
        },
        Err(_) => 0,
    }
}

/// Encode the raw pixel frame at `ptr..ptr+len` as a compressed PNG packed as
/// `ptr << 32 | len`: little-endian width and height, one channel-count byte
/// (1 = luma, 4 = RGBA), then row-major samples. Options come from the blob
/// at `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults; the export takes
/// no options today, so the blob is walked only for framing validation); a
/// malformed frame, a bad blob, or an unsupported channel count returns 0.
/// The caller reads the output and deallocs both buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn encode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if opts_len != 0 {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        if option_pairs(blob).is_none() {
            return 0;
        }
    }

    match encode_png(input) {
        Ok(png) => abi::pack(png),
        Err(_) => 0,
    }
}

/// Return the manifest JSON packed as `ptr << 32 | len`. The caller reads the
/// text, then deallocs the buffer.
///
/// # Safety
/// The returned pointer references this module's linear memory.
#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi::{OPTIONS_MAGIC, parse_pixels};

    /// Build one wire-format pair, matching `resolve_color` expectations.
    fn pair(key: &str, value: &str) -> Vec<u8> {
        let mut blob = (key.len() as u32).to_le_bytes().to_vec();

        blob.extend_from_slice(key.as_bytes());
        blob.extend_from_slice(&(value.len() as u32).to_le_bytes());
        blob.extend_from_slice(value.as_bytes());

        blob
    }

    fn blob(pairs: &[Vec<u8>]) -> Vec<u8> {
        let mut all = vec![OPTIONS_MAGIC];

        for item in pairs {
            all.extend_from_slice(item);
        }

        all
    }

    /// Encode a small grayscale gradient as PNG bytes in memory.
    fn png_bytes() -> Vec<u8> {
        let mut img = image::GrayImage::new(3, 2);

        img.put_pixel(0, 0, image::Luma([0]));
        img.put_pixel(1, 0, image::Luma([64]));
        img.put_pixel(2, 0, image::Luma([128]));
        img.put_pixel(0, 1, image::Luma([192]));
        img.put_pixel(1, 1, image::Luma([255]));
        img.put_pixel(2, 1, image::Luma([32]));

        let mut bytes = Vec::new();

        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();

        bytes
    }

    #[test]
    fn decodes_png_to_luma_frame() {
        let (width, height, channels, pixels) = decode_pixels(&png_bytes(), &Color::Luma).unwrap();

        assert_eq!((width, height, channels), (3, 2, 1));
        assert_eq!(pixels, vec![0, 64, 128, 192, 255, 32]);
    }

    #[test]
    fn decodes_png_to_rgba_frame() {
        let (width, height, channels, pixels) = decode_pixels(&png_bytes(), &Color::Rgba).unwrap();

        assert_eq!((width, height, channels), (3, 2, 4));

        // Grayscale source expands to R=G=B with a full alpha channel.
        assert_eq!(pixels[0..4], [0, 0, 0, 255]);
        assert_eq!(pixels[4 * 4..4 * 4 + 4], [255, 255, 255, 255]);
    }

    /// Encode an RGB view of a grayscale gradient as lossy JPEG bytes.
    fn jpeg_bytes() -> Vec<u8> {
        let mut img = image::GrayImage::new(3, 2);

        img.put_pixel(0, 0, image::Luma([0]));
        img.put_pixel(1, 0, image::Luma([64]));
        img.put_pixel(2, 0, image::Luma([128]));
        img.put_pixel(0, 1, image::Luma([192]));
        img.put_pixel(1, 1, image::Luma([255]));
        img.put_pixel(2, 1, image::Luma([32]));

        let mut bytes = Vec::new();

        image::DynamicImage::ImageLuma8(img)
            .to_rgb8()
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg)
            .unwrap();

        bytes
    }

    #[test]
    fn decodes_jpeg_to_luma_frame() {
        // Lossy compression means exact samples cannot be asserted, but the
        // dimensions, channel count, and rough ordering must survive.
        let (width, height, channels, pixels) = decode_pixels(&jpeg_bytes(), &Color::Luma).unwrap();

        assert_eq!((width, height, channels), (3, 2, 1));
        assert!(pixels[0] < pixels[1], "dark-to-light order lost: {:?}", pixels);
        assert!(pixels.iter().max().unwrap() > &200, "white point lost: {:?}", pixels);
    }

    #[test]
    fn frames_round_trip_through_parse_pixels() {
        let (_, _, _, pixels) = decode_pixels(&png_bytes(), &Color::Luma).unwrap();
        let frame = frame_pixels(3, 2, 1, &pixels).unwrap();

        assert_eq!(parse_pixels(&frame), Some((3, 2, 1, pixels.as_slice())));
    }

    #[test]
    fn rejects_garbage_and_empty_input() {
        assert!(decode_pixels(b"not an image at all", &Color::Luma).is_err());
        assert!(decode_pixels(&[], &Color::Luma).is_err());
    }

    #[test]
    fn encode_png_round_trips_luma_and_rgba() {
        for color in [Color::Luma, Color::Rgba] {
            let (width, height, channels, pixels) = decode_pixels(&png_bytes(), &color).unwrap();
            let frame = frame_pixels(width, height, channels, &pixels).unwrap();
            let png = encode_png(&frame).unwrap();

            // PNG magic; the output must decode back to the same pixels.
            assert_eq!(&png[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A], "png signature");

            let (w2, h2, ch2, pixels2) = decode_pixels(&png, &color).unwrap();

            assert_eq!((w2, h2, ch2), (width, height, channels));
            assert_eq!(pixels2, pixels);
        }
    }

    #[test]
    fn encode_png_rejects_bad_frames() {
        assert!(encode_png(b"not a frame at all").is_err());
        assert!(encode_png(&[]).is_err());

        // 3 channels is not a supported sample layout.
        let bad = frame_pixels(2, 2, 3, &[0; 12]).unwrap();

        assert!(encode_png(&bad).is_err());
    }

    #[test]
    fn resolves_color_blob() {
        assert_eq!(resolve_color(&blob(&[pair("color", "rgba")])), Some(Color::Rgba));
        assert_eq!(resolve_color(&blob(&[pair("color", "luma")])), Some(Color::Luma));
        assert_eq!(resolve_color(b""), Some(Color::Luma));
        assert_eq!(resolve_color(&blob(&[pair("future", "whatever")])), Some(Color::Luma));
    }

    #[test]
    fn rejects_malformed_blobs_and_values() {
        assert_eq!(resolve_color(&[0x02]), None);
        assert_eq!(resolve_color(&blob(&[pair("color", "cmyk")])), None);
        assert_eq!(resolve_color(&blob(&[pair("color", "")])), None);
    }
}
