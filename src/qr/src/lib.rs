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

//! qr - encode UTF-8 text as QR code SVG, read QR codes back from images.
//!
//! Encoding rides on the pure-Rust `qrcode` crate (rendering features off);
//! this core renders its own minimal SVG from the module matrix so output
//! stays one predictable shape: white background, black modules, integer
//! coordinates, `shape-rendering="crispEdges"`. Decoding rides on `rqrr`
//! (pattern detection + ECC) over a raw luma pixel frame - produced by the
//! separate `image` module from PNG/JPEG files - and every payload found is
//! reported.
//!
//! Options travel as the house options blob (see README.md): `0x01` magic,
//! little-endian length-prefixed UTF-8 key/value pairs; unknown keys ignored,
//! known keys with bad values fail the call.
//!
//! Buffer packing and blob framing come from the shared `abi` crate; only the
//! tool logic lives here.

use abi::{option_pairs, parse_pixels, parse_usize};

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side.
const MANIFEST: &str = r#"{
  "exports": {
    "encode": {
      "summary": "Encode UTF-8 text as a QR code SVG or RGBA pixel frame.",
      "options": {
        "ecc": {
          "type": "enum",
          "values": ["L", "M", "Q", "H"],
          "default": "M",
          "description": "Error correction level: L ~7%, M ~15%, Q ~25%, H ~30% recoverable."
        },
        "scale": {
          "type": "number",
          "default": 4,
          "description": "Output pixels per module (1-64)."
        },
        "margin": {
          "type": "number",
          "default": 4,
          "description": "Quiet-zone margin around the code, in modules (0-32)."
        },
        "output": {
          "type": "enum",
          "values": ["svg", "rgba"],
          "default": "svg",
          "description": "Render format: SVG document, or RGBA pixel frame (white background, black modules) that the image module can encode as PNG."
        }
      }
    },
    "decode": {
      "summary": "Decode QR payloads from a raw luma pixel frame.",
      "output": "string-array"
    }
  }
}"#;

/// Friendly ceilings on the SVG geometry knobs; anything larger is almost
/// certainly a caller mistake and would balloon memory in a bounded wasm
/// module.
const MAX_SCALE: usize = 64;
const MAX_MARGIN: usize = 32;

/// Per-call options parsed from the options blob. Missing keys fall back to
/// these defaults; unknown keys are ignored by design (forward compatibility).
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Error correction level L/M/Q/H.
    pub ecc: Ecc,
    /// Output pixels per module.
    pub scale: usize,
    /// Quiet zone in modules.
    pub margin: usize,
    /// Render format.
    pub output: Output,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ecc: Ecc::M,
            scale: 4,
            margin: 4,
            output: Output::Svg,
        }
    }
}

/// Render format for `encode`.
#[derive(Debug, PartialEq)]
pub enum Output {
    /// Standalone SVG document.
    Svg,
    /// RGBA pixel frame (white background, black modules).
    Rgba,
}

impl Output {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"svg" => Some(Output::Svg),
            b"rgba" => Some(Output::Rgba),
            _ => None,
        }
    }
}

/// Error correction level, mirroring ISO/IEC 18004 recoverable-fraction steps.
#[derive(Debug, PartialEq)]
pub enum Ecc {
    L,
    M,
    Q,
    H,
}

impl Ecc {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"L" => Some(Ecc::L),
            b"M" => Some(Ecc::M),
            b"Q" => Some(Ecc::Q),
            b"H" => Some(Ecc::H),
            _ => None,
        }
    }
}

/// Encode `text` into a QR matrix at the requested ECC level.
fn encode_matrix(text: &str, ecc: &Ecc) -> Result<qrcode::QrCode, qrcode::types::QrError> {
    let level = match ecc {
        Ecc::L => qrcode::types::EcLevel::L,
        Ecc::M => qrcode::types::EcLevel::M,
        Ecc::Q => qrcode::types::EcLevel::Q,
        Ecc::H => qrcode::types::EcLevel::H,
    };

    qrcode::QrCode::with_error_correction_level(text.as_bytes(), level)
}

/// Render the QR matrix as a minimal standalone SVG document. Dark modules
/// merge into horizontal runs so typical codes emit a handful of rects
/// instead of hundreds.
fn render_svg(code: &qrcode::QrCode, opts: &Options) -> String {
    let width = code.width() as usize;
    let colors = code.to_colors();
    let size = (width + opts.margin * 2) * opts.scale;
    let mut svg = String::with_capacity(colors.len() / 2 * 24 + 160);

    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" width=\"{size}\" height=\"{size}\" shape-rendering=\"crispEdges\">\n"
    ));
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");

    for row in 0..width {
        let line = &colors[row * width..(row + 1) * width];
        let mut column = 0;

        while column < width {
            if !matches!(line[column], qrcode::Color::Dark) {
                column += 1;

                continue;
            }

            // Consume the whole dark run starting here.
            let start = column;

            while column < width && matches!(line[column], qrcode::Color::Dark) {
                column += 1;
            }

            let x = (opts.margin + start) * opts.scale;
            let y = (opts.margin + row) * opts.scale;
            let w = (column - start) * opts.scale;

            svg.push_str(&format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{}\"/>\n", opts.scale));
        }
    }

    svg.push_str("</svg>\n");

    svg
}

/// Validate the shared encode inputs and produce the matrix. Both encoders
/// (SVG and RGBA) accept the same text and bounds, so the boundary checks
/// live here once instead of twice.
fn encode_matrix_or_err(text: &str, opts: &Options) -> Result<qrcode::QrCode, &'static str> {
    if text.is_empty() {
        return Err("input text is empty");
    }

    if opts.scale == 0 || opts.scale > MAX_SCALE {
        return Err("scale must be between 1 and 64");
    }

    if opts.margin > MAX_MARGIN {
        return Err("margin must be between 0 and 32");
    }

    encode_matrix(text, &opts.ecc).map_err(|_| "the text does not fit a QR code at this error correction level")
}

/// Encode the SVG payload for `text` under `opts`. Errors name the cause for
/// hosts that surface them (the ABI itself just returns 0).
pub fn encode_svg(text: &str, opts: &Options) -> Result<String, &'static str> {
    let code = encode_matrix_or_err(text, opts)?;

    Ok(render_svg(&code, opts))
}

/// Render the QR matrix as an RGBA pixel frame: white background, black
/// modules, each module `scale` pixels, wrapped in a `margin`-module quiet
/// zone. The frame matches the wire format `abi::frame_pixels` produces, so
/// the `image` module can encode it straight to PNG.
fn render_rgba(code: &qrcode::QrCode, opts: &Options) -> Vec<u8> {
    let width = code.width() as usize;
    let colors = code.to_colors();
    let size = (width + opts.margin * 2) * opts.scale;
    let mut pixels = vec![255u8; size * size * 4];

    for row in 0..width {
        for column in 0..width {
            if !matches!(colors[row * width + column], qrcode::Color::Dark) {
                continue;
            }

            let x0 = (opts.margin + column) * opts.scale;
            let y0 = (opts.margin + row) * opts.scale;

            for dy in 0..opts.scale {
                for dx in 0..opts.scale {
                    let offset = ((y0 + dy) * size + (x0 + dx)) * 4;

                    pixels[offset] = 0;
                    pixels[offset + 1] = 0;
                    pixels[offset + 2] = 0;
                    pixels[offset + 3] = 255;
                }
            }
        }
    }

    pixels
}

/// Encode the RGBA pixel frame for `text` under `opts`. Errors name the cause
/// for hosts that surface them (the ABI itself just returns 0).
pub fn encode_rgba(text: &str, opts: &Options) -> Result<Vec<u8>, &'static str> {
    let code = encode_matrix_or_err(text, opts)?;
    let pixels = render_rgba(&code, opts);
    let size = (code.width() as usize + opts.margin * 2) * opts.scale;

    abi::frame_pixels(size as u32, size as u32, 4, &pixels).ok_or("pixel frame size overflow")
}

/// Decode every QR payload found in a raw luma pixel frame (see README.md).
/// Payloads are UTF-8 text; codes whose payload fails to decode (damaged
/// beyond ECC) are skipped rather than failing the whole image. An error is
/// returned when nothing readable was found at all.
pub fn read_frame(frame: &[u8]) -> Result<Vec<String>, &'static str> {
    let (width, height, channels, pixels) =
        parse_pixels(frame).ok_or("the input is not a valid pixel frame")?;

    if channels != 1 {
        return Err("the pixel frame must be grayscale (1 channel)");
    }

    let stride = width as usize;
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(width as usize, height as usize, |x, y| {
        pixels[y * stride + x]
    });
    let mut payloads: Vec<String> = Vec::new();

    for grid in prepared.detect_grids() {
        if let Ok((_meta, text)) = grid.decode() {
            payloads.push(text);
        }
    }

    if payloads.is_empty() {
        return Err("no QR code found in the image");
    }

    Ok(payloads)
}

/// Parse the options blob straight into resolved `Options`. Framing (magic
/// byte, length prefixes) is validated by the shared `option_pairs`; unknown
/// keys are ignored so new callers stay compatible with older cores; known
/// keys with bad values are errors, because silently dropping a requested
/// option could change results. Returns `None` when the blob is malformed
/// or carries an unusable value.
fn resolve_options(blob: &[u8]) -> Option<Options> {
    let mut opts = Options::default();

    for (key, value) in option_pairs(blob)? {
        match key {
            b"ecc" => match Ecc::parse(value) {
                Some(ecc) => opts.ecc = ecc,
                None => return None,
            },
            b"scale" => match parse_usize(value) {
                Some(n) => opts.scale = n,
                None => return None,
            },
            b"margin" => match parse_usize(value) {
                Some(n) => opts.margin = n,
                None => return None,
            },
            b"output" => match Output::parse(value) {
                Some(output) => opts.output = output,
                None => return None,
            },
            _ => {}
        }
    }

    Some(opts)
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `create` and back to `dealloc` when done.
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

/// Encode the UTF-8 text at `ptr..ptr+len` as a QR code packed as
/// `ptr << 32 | len`. The `output` option selects the format: SVG by default,
/// or an RGBA pixel frame (`output=rgba`) that the `image` module can encode
/// as PNG. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); empty input, a
/// malformed blob, an unusable option value, or text that does not fit the
/// chosen error correction level returns 0. The caller reads the output and
/// deallocs both buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn encode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(_) => return 0,
    };

    let opts = if opts_len == 0 {
        Options::default()
    } else {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        match resolve_options(blob) {
            Some(opts) => opts,
            None => return 0,
        }
    };

    match opts.output {
        Output::Svg => match encode_svg(text, &opts) {
            Ok(svg) => abi::pack(svg.into_bytes()),
            Err(_) => 0,
        },
        Output::Rgba => match encode_rgba(text, &opts) {
            Ok(frame) => abi::pack(frame),
            Err(_) => 0,
        },
    }
}

/// Decode QR payloads from the luma pixel frame at `ptr..ptr+len` (as produced
/// by the `image` module) and return them as a string-array frame packed as
/// `ptr << 32 | len`; every code found contributes one entry. A malformed
/// frame or an image without any readable QR code returns 0. The options
/// blob carries no keys today; it is walked only for framing validation so a
/// future option can be added without breaking callers. The caller reads the
/// output and deallocs both buffers.
///
/// The name pairs with `encode` like base64 does. It deliberately is NOT
/// named `read`: a `#[no_mangle] extern "C"` symbol named `read` interposes
/// libSystem's own `read(2)` on Darwin, which segfaults any host process at
/// its first stdout write. Avoid libc-reserved names for exports everywhere.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn decode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if opts_len != 0 {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        if option_pairs(blob).is_none() {
            return 0;
        }
    }

    match read_frame(input) {
        Ok(payloads) => abi::pack(abi::frame_strings(&payloads)),
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
    use abi::OPTIONS_MAGIC;

    /// Build one wire-format pair, matching `resolve_options` expectations.
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

    #[test]
    fn encodes_svg_with_defaults() {
        let svg = encode_svg("sfw.tools", &Options::default()).expect("default encode works");

        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("viewBox=\"0 0 "));
        assert!(svg.contains("fill=\"#ffffff\""));
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.matches("<rect").count() > 20, "expected module rects");
    }

    #[test]
    fn scale_and_margin_size_the_canvas() {
        // A version-1 code is 21 modules wide; M-level "sfw.tools" fits v1.
        let small = encode_svg("sfw.tools", &Options { scale: 1, margin: 0, ..Options::default() }).unwrap();
        let big = encode_svg("sfw.tools", &Options { scale: 8, margin: 2, ..Options::default() }).unwrap();
        let small_view = small.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let big_view = big.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();

        assert_eq!(small_view, "0 0 21 21");
        assert_eq!(big_view, "0 0 200 200");
    }

    #[test]
    fn rejects_empty_input_and_bad_bounds() {
        assert!(encode_svg("", &Options::default()).is_err());

        let zero = Options {
            scale: 0,
            ..Options::default()
        };
        assert!(encode_svg("x", &zero).is_err());

        let wide = Options {
            margin: 33,
            ..Options::default()
        };
        assert!(encode_svg("x", &wide).is_err());
    }

    #[test]
    fn oversized_payload_fails_cleanly() {
        // Version-40 caps byte capacity well below 64 KiB at every ECC level.
        let huge = "x".repeat(70_000);

        assert!(encode_svg(&huge, &Options::default()).is_err());
    }

    #[test]
    fn resolves_options_blob() {
        assert_eq!(
            resolve_options(&blob(&[pair("ecc", "H"), pair("scale", "12"), pair("margin", "0")])),
            Some(Options {
                ecc: Ecc::H,
                scale: 12,
                margin: 0,
                ..Options::default()
            })
        );
    }

    #[test]
    fn resolves_output_option() {
        assert_eq!(
            resolve_options(&blob(&[pair("output", "rgba")])),
            Some(Options {
                output: Output::Rgba,
                ..Options::default()
            })
        );
        assert_eq!(
            resolve_options(&blob(&[pair("output", "svg")])),
            Some(Options::default())
        );
        assert_eq!(resolve_options(&blob(&[pair("output", "pdf")])), None);
    }

    #[test]
    fn empty_blob_means_defaults_without_magic() {
        assert_eq!(resolve_options(b""), Some(Options::default()));
    }

    #[test]
    fn rejects_malformed_blobs_and_values() {
        assert_eq!(resolve_options(&[0x02]), None);
        assert_eq!(resolve_options(&blob(&[pair("ecc", "X")])), None);
        assert_eq!(resolve_options(&blob(&[pair("scale", "")])), None);
        assert_eq!(resolve_options(&blob(&[pair("scale", "4x")])), None);
        assert_eq!(resolve_options(&blob(&[pair("margin", "-1")])), None);

        // Overflowing usize is not a scale either.
        assert_eq!(
            resolve_options(&blob(&[pair("scale", "99999999999999999999999")])),
            None
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(
            resolve_options(&blob(&[pair("future", "whatever"), pair("scale", "6")])),
            Some(Options {
                scale: 6,
                ..Options::default()
            })
        );
    }
    /// Render `text` as a luma pixel buffer exactly like a clean screenshot
    /// of one of our own SVGs: white background, black modules, quiet zone.
    /// Returns `(width, height, samples)` ready for `frame_pixels`.
    fn render_gray(text: &str, scale: usize, margin: usize) -> (usize, usize, Vec<u8>) {
        let code = qrcode::QrCode::new(text.as_bytes()).unwrap();
        let width = code.width();
        let colors = code.to_colors();
        let size = (width + margin * 2) * scale;
        let mut pixels = vec![255u8; size * size];

        for row in 0..width {
            for col in 0..width {
                if !matches!(colors[row * width + col], qrcode::Color::Dark) {
                    continue;
                }

                for dy in 0..scale {
                    let y = (margin + row) * scale + dy;

                    for dx in 0..scale {
                        pixels[y * size + (margin + col) * scale + dx] = 0;
                    }
                }
            }
        }

        (size, size, pixels)
    }

    #[test]
    fn round_trips_a_luma_frame() {
        let (width, height, pixels) = render_gray("https://sfw.tools/qr", 8, 4);
        let frame = abi::frame_pixels(width as u32, height as u32, 1, &pixels).unwrap();
        let payloads = read_frame(&frame).unwrap();

        assert_eq!(payloads, vec!["https://sfw.tools/qr".to_string()]);
    }

    #[test]
    fn rgba_frame_has_expected_geometry_and_colors() {
        // "sfw.tools" is version 1: 21 modules, margin 4 -> 29 modules wide,
        // scale 4 -> 116 px. The frame must be RGBA with white background and
        // black modules.
        let frame = encode_rgba("sfw.tools", &Options::default()).unwrap();
        let (width, height, channels, pixels) = parse_pixels(&frame).unwrap();

        assert_eq!((width, height, channels), (116, 116, 4));
        assert_eq!(pixels.len(), 116 * 116 * 4);
        assert_eq!(&pixels[0..4], &[255, 255, 255, 255], "top-left corner is white");

        // The finder pattern's top-left dark module sits at pixel (16,16):
        // margin 4 modules * scale 4.
        let finder_origin = (16 * 116 + 16) * 4;

        assert_eq!(&pixels[finder_origin..finder_origin + 4], &[0, 0, 0, 255], "finder origin is black");
    }

    #[test]
    fn rgba_frame_rejects_bad_options_like_svg() {
        let zero = Options {
            scale: 0,
            ..Options::default()
        };

        assert!(encode_rgba("x", &zero).is_err());
        assert!(encode_rgba("", &Options::default()).is_err());
    }

    #[test]
    fn reads_multiple_codes_from_one_frame() {
        let (lw, lh, lp) = render_gray("left code", 4, 4);
        let (rw, rh, rp) = render_gray("right code", 4, 4);
        let gap = 16;
        let height = lh.max(rh);
        let width = lw + gap + rw;
        let mut combined = vec![255u8; width * height];

        for row in 0..lh {
            combined[row * width..row * width + lw].copy_from_slice(&lp[row * lw..(row + 1) * lw]);
        }

        let offset = lw + gap;

        for row in 0..rh {
            combined[row * width + offset..row * width + offset + rw]
                .copy_from_slice(&rp[row * rw..(row + 1) * rw]);
        }

        let frame = abi::frame_pixels(width as u32, height as u32, 1, &combined).unwrap();
        let mut payloads = read_frame(&frame).unwrap();

        payloads.sort();

        assert_eq!(payloads, vec!["left code".to_string(), "right code".to_string()]);
    }

    #[test]
    fn rejects_garbage_qrless_and_non_luma_frames() {
        assert!(read_frame(b"not a pixel frame at all").is_err());

        // A valid frame of plain white carries no codes.
        let blank = abi::frame_pixels(64, 64, 1, &vec![255u8; 64 * 64]).unwrap();

        assert!(read_frame(&blank).is_err());

        // RGBA frames are refused: this reader only takes grayscale.
        let rgba = abi::frame_pixels(4, 4, 4, &vec![255u8; 4 * 4 * 4]).unwrap();

        assert!(read_frame(&rgba).is_err());
        assert!(read_frame(&[]).is_err());
    }
}
