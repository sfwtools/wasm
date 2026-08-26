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

//! barcode - encode text as common 1D barcodes, rendered as SVG or as an
//! RGBA pixel frame for the `image` module to turn into PNG.
//!
//! Encoding rides on the pure-Rust `barcoders` crate (its generator features
//! stay off); this core renders its own minimal SVG and RGBA frames from the
//! module bits, exactly like `qr` renders QR codes itself. Output is one
//! predictable shape: white background, black bars, integer coordinates,
//! `shape-rendering="crispEdges"` for SVG.
//!
//! Options travel as the house options blob (see README.md): `0x01` magic,
//! little-endian length-prefixed UTF-8 key/value pairs; unknown keys ignored,
//! known keys with bad values fail the call.
//!
//! Buffer packing, blob framing, and the pixel frame come from the shared
//! `abi` crate; only the tool logic lives here.

use abi::{option_pairs, parse_usize};

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side.
const MANIFEST: &str = r#"{
  "exports": {
    "encode": {
      "summary": "Encode text as a 1D barcode SVG or RGBA pixel frame.",
      "options": {
        "type": {
          "type": "enum",
          "values": ["code128", "ean13", "upca", "ean8", "code39", "itf", "codabar"],
          "default": "code128",
          "description": "Barcode symbology. code128 auto-selects character set B unless the text begins with a set marker (A=U+00C0, B=U+0181, C=U+0106)."
        },
        "output": {
          "type": "enum",
          "values": ["svg", "rgba"],
          "default": "svg",
          "description": "Render format: SVG document, or RGBA pixel frame that the image module can encode as PNG."
        },
        "scale": {
          "type": "number",
          "default": 2,
          "description": "Output pixels per module (1-32)."
        },
        "height": {
          "type": "number",
          "default": 80,
          "description": "Bar height in pixels (8-512)."
        },
        "margin": {
          "type": "number",
          "default": 4,
          "description": "Quiet-zone margin around the code, in modules (0-32)."
        }
      }
    }
  }
}"#;

/// Friendly ceilings on the geometry knobs; anything larger is almost
/// certainly a caller mistake and would balloon memory in a bounded wasm
/// module.
const MAX_SCALE: usize = 32;
const MAX_HEIGHT: usize = 512;
const MAX_MARGIN: usize = 32;

/// Per-call options parsed from the options blob. Missing keys fall back to
/// these defaults; unknown keys are ignored by design (forward compatibility).
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Barcode symbology.
    pub symbol: Symbol,
    /// Render format.
    pub output: Output,
    /// Output pixels per module.
    pub scale: usize,
    /// Bar height in pixels.
    pub height: usize,
    /// Quiet zone in modules.
    pub margin: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            symbol: Symbol::Code128,
            output: Output::Svg,
            scale: 2,
            height: 80,
            margin: 4,
        }
    }
}

/// Supported 1D symbologies, one-to-one with `barcoders` modules.
#[derive(Debug, PartialEq)]
pub enum Symbol {
    Code128,
    Ean13,
    Upca,
    Ean8,
    Code39,
    Itf,
    Codabar,
}

impl Symbol {
    fn parse(value: &[u8]) -> Option<Self> {
        match value {
            b"code128" => Some(Symbol::Code128),
            b"ean13" => Some(Symbol::Ean13),
            b"upca" => Some(Symbol::Upca),
            b"ean8" => Some(Symbol::Ean8),
            b"code39" => Some(Symbol::Code39),
            b"itf" => Some(Symbol::Itf),
            b"codabar" => Some(Symbol::Codabar),
            _ => None,
        }
    }
}

/// Render format for `encode`.
#[derive(Debug, PartialEq)]
pub enum Output {
    /// Standalone SVG document.
    Svg,
    /// RGBA pixel frame (white background, black bars).
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

/// Map a `barcoders` error to a friendly, user-readable message.
fn friendly_error(err: barcoders::error::Error) -> &'static str {
    match err {
        barcoders::error::Error::Character => "the text contains characters this barcode type cannot encode",
        barcoders::error::Error::Length => "the text length is invalid for this barcode type",
        barcoders::error::Error::Checksum => "the text has an invalid checksum digit",
        barcoders::error::Error::Generate => "the barcode could not be generated",
    }
}

/// Encode `text` into the module bits for the requested symbology: one byte
/// per module, `1` = dark bar, `0` = light space.
fn encode_bits(text: &str, symbol: &Symbol) -> Result<Vec<u8>, &'static str> {
    match symbol {
        // barcoders requires a leading character-set marker (A/B/C); default
        // to set B (full ASCII) unless the caller already picked a set.
        Symbol::Code128 => {
            let mut data = text.to_string();

            if !matches!(data.chars().next(), Some('\u{00C0}') | Some('\u{0181}') | Some('\u{0106}')) {
                data.insert(0, '\u{0181}');
            }

            barcoders::sym::code128::Code128::new(data)
                .map(|code| code.encode())
                .map_err(friendly_error)
        }
        Symbol::Ean13 => barcoders::sym::ean13::EAN13::new(text)
            .map(|code| code.encode())
            .map_err(friendly_error),
        Symbol::Upca => {
            // barcoders 2.0.0 has no UPCA module; UPC-A is EAN-13 with a
            // leading 0 (11-12 digits -> 13-digit EAN-13), so encode it as
            // such. `new` appends the checksum when 12 digits are given.
            let mut data = text.to_string();

            if data.len() == 11 || data.len() == 12 {
                data.insert(0, '0');
            }

            barcoders::sym::ean13::EAN13::new(data)
                .map(|code| code.encode())
                .map_err(friendly_error)
        }
        Symbol::Ean8 => barcoders::sym::ean8::EAN8::new(text)
            .map(|code| code.encode())
            .map_err(friendly_error),
        Symbol::Code39 => barcoders::sym::code39::Code39::new(text)
            .map(|code| code.encode())
            .map_err(friendly_error),
        Symbol::Itf => barcoders::sym::tf::TF::interleaved(text)
            .map(|code| code.encode())
            .map_err(friendly_error),
        Symbol::Codabar => barcoders::sym::codabar::Codabar::new(text)
            .map(|code| code.encode())
            .map_err(friendly_error),
    }
}

/// Render the module bits as a minimal standalone SVG document. Dark bars
/// merge into horizontal runs so typical codes emit a handful of rects
/// instead of hundreds.
fn render_svg(bits: &[u8], opts: &Options) -> String {
    let width = (bits.len() + opts.margin * 2) * opts.scale;
    let height = opts.height;
    let mut svg = String::with_capacity(bits.len() / 2 * 24 + 160);

    svg.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {height}\" width=\"{width}\" height=\"{height}\" shape-rendering=\"crispEdges\">\n"
    ));
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");

    let mut index = 0;

    while index < bits.len() {
        if bits[index] == 0 {
            index += 1;

            continue;
        }

        let start = index;

        while index < bits.len() && bits[index] == 1 {
            index += 1;
        }

        let x = (opts.margin + start) * opts.scale;
        let w = (index - start) * opts.scale;

        svg.push_str(&format!("<rect x=\"{x}\" y=\"0\" width=\"{w}\" height=\"{}\"/>\n", opts.height));
    }

    svg.push_str("</svg>\n");

    svg
}

/// Render the module bits as an RGBA pixel frame: white background, black
/// bars, each module `scale` pixels wide and `height` pixels tall, wrapped in
/// a `margin`-module quiet zone. The frame matches the wire format
/// `abi::frame_pixels` produces, so the `image` module can encode it to PNG.
fn render_rgba(bits: &[u8], opts: &Options) -> Vec<u8> {
    let width = (bits.len() + opts.margin * 2) * opts.scale;
    let height = opts.height;
    let mut pixels = vec![255u8; width * height * 4];

    for (index, &bit) in bits.iter().enumerate() {
        if bit == 0 {
            continue;
        }

        let x0 = (opts.margin + index) * opts.scale;

        for y in 0..height {
            for x in x0..x0 + opts.scale {
                let offset = (y * width + x) * 4;

                pixels[offset] = 0;
                pixels[offset + 1] = 0;
                pixels[offset + 2] = 0;
                pixels[offset + 3] = 255;
            }
        }
    }

    pixels
}

/// Encode `text` into an SVG under `opts`. Errors name the cause for hosts
/// that surface them (the ABI itself just returns 0).
pub fn encode_svg(text: &str, opts: &Options) -> Result<String, &'static str> {
    if text.is_empty() {
        return Err("input text is empty");
    }

    if opts.scale == 0 || opts.scale > MAX_SCALE {
        return Err("scale must be between 1 and 32");
    }

    if opts.height < 8 || opts.height > MAX_HEIGHT {
        return Err("height must be between 8 and 512");
    }

    if opts.margin > MAX_MARGIN {
        return Err("margin must be between 0 and 32");
    }

    let bits = encode_bits(text, &opts.symbol)?;

    Ok(render_svg(&bits, opts))
}

/// Encode `text` into an RGBA pixel frame under `opts`. Errors name the cause
/// for hosts that surface them (the ABI itself just returns 0).
pub fn encode_rgba(text: &str, opts: &Options) -> Result<Vec<u8>, &'static str> {
    if text.is_empty() {
        return Err("input text is empty");
    }

    if opts.scale == 0 || opts.scale > MAX_SCALE {
        return Err("scale must be between 1 and 32");
    }

    if opts.height < 8 || opts.height > MAX_HEIGHT {
        return Err("height must be between 8 and 512");
    }

    if opts.margin > MAX_MARGIN {
        return Err("margin must be between 0 and 32");
    }

    let bits = encode_bits(text, &opts.symbol)?;
    let width = (bits.len() + opts.margin * 2) * opts.scale;
    let pixels = render_rgba(&bits, opts);

    abi::frame_pixels(width as u32, opts.height as u32, 4, &pixels).ok_or("pixel frame size overflow")
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
            b"type" => match Symbol::parse(value) {
                Some(symbol) => opts.symbol = symbol,
                None => return None,
            },
            b"output" => match Output::parse(value) {
                Some(output) => opts.output = output,
                None => return None,
            },
            b"scale" => match parse_usize(value) {
                Some(n) => opts.scale = n,
                None => return None,
            },
            b"height" => match parse_usize(value) {
                Some(n) => opts.height = n,
                None => return None,
            },
            b"margin" => match parse_usize(value) {
                Some(n) => opts.margin = n,
                None => return None,
            },
            _ => {}
        }
    }

    Some(opts)
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `encode` and back to `dealloc` when done.
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

/// Encode the UTF-8 text at `ptr..ptr+len` as a barcode packed as
/// `ptr << 32 | len`. The `output` option selects the format: SVG by default,
/// or an RGBA pixel frame (`output=rgba`) that the `image` module can encode
/// as PNG. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); empty input, a
/// malformed blob, an unusable option value, or text the symbology cannot
/// encode returns 0. The caller reads the output and deallocs both buffers.
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
    fn encodes_code128_svg_with_defaults() {
        let svg = encode_svg("SFW.TOOLS", &Options::default()).expect("default encode works");

        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("fill=\"#ffffff\""));
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.matches("<rect").count() > 3, "expected bar rects");
    }

    #[test]
    fn geometry_reflects_options() {
        // code128 of a short string is a handful of modules; scale and margin
        // must scale the canvas.
        let small = encode_svg("12", &Options { scale: 1, margin: 0, ..Options::default() }).unwrap();
        let big = encode_svg("12", &Options { scale: 4, margin: 2, ..Options::default() }).unwrap();
        let small_view = small.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let big_view = big.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();

        let small_w: usize = small_view.split(' ').nth(2).unwrap().parse().unwrap();
        let small_bits = encode_bits("12", &Symbol::Code128).unwrap();
        let expected_w = (small_bits.len() + 0) * 1;

        assert_eq!(small_w, expected_w);
        assert_ne!(small_view, big_view, "scale/margin must change the canvas");
    }

    #[test]
    fn rgba_frame_matches_expected_geometry() {
        // code128 "12" at scale 1, margin 0 -> width = module count, height 80.
        let frame = encode_rgba("12", &Options { scale: 1, margin: 0, output: Output::Rgba, ..Options::default() }).unwrap();
        let (width, height, channels, pixels) = abi::parse_pixels(&frame).unwrap();

        assert_eq!(height, 80);
        assert_eq!(channels, 4);
        assert_eq!(width as usize, encode_bits("12", &Symbol::Code128).unwrap().len());
        assert_eq!(pixels.len(), width as usize * 80 * 4);
    }

    #[test]
    fn rejects_empty_input_and_bad_bounds() {
        assert!(encode_svg("", &Options::default()).is_err());

        let zero_scale = Options { scale: 0, ..Options::default() };
        assert!(encode_svg("x", &zero_scale).is_err());

        let tall = Options { height: 600, ..Options::default() };
        assert!(encode_svg("x", &tall).is_err());

        let wide = Options { margin: 33, ..Options::default() };
        assert!(encode_svg("x", &wide).is_err());
    }

    #[test]
    fn rejects_data_each_symbology_cannot_encode() {
        // EAN-13 wants exactly 12/13 digits; letters must fail.
        assert!(encode_svg("ABC", &Options { symbol: Symbol::Ean13, ..Options::default() }).is_err());
        // EAN-13 wrong length must fail.
        assert!(encode_svg("123", &Options { symbol: Symbol::Ean13, ..Options::default() }).is_err());
        // ITF is digits-only.
        assert!(encode_svg("12A", &Options { symbol: Symbol::Itf, ..Options::default() }).is_err());
    }

    #[test]
    fn ean13_accepts_valid_checksum() {
        // 12 digits: the crate computes and appends the checksum.
        assert!(encode_svg("590123412345", &Options { symbol: Symbol::Ean13, ..Options::default() }).is_ok());
        // 13 digits with a bad checksum must fail.
        assert!(encode_svg("5901234123459", &Options { symbol: Symbol::Ean13, ..Options::default() }).is_err());
    }

    #[test]
    fn resolves_options_blob() {
        assert_eq!(
            resolve_options(&blob(&[pair("type", "ean13"), pair("output", "rgba"), pair("scale", "3"), pair("height", "50"), pair("margin", "2")])),
            Some(Options {
                symbol: Symbol::Ean13,
                output: Output::Rgba,
                scale: 3,
                height: 50,
                margin: 2,
            })
        );
    }

    #[test]
    fn empty_blob_means_defaults() {
        assert_eq!(resolve_options(b""), Some(Options::default()));
    }

    #[test]
    fn rejects_malformed_blobs_and_values() {
        assert_eq!(resolve_options(&[0x02]), None);
        assert_eq!(resolve_options(&blob(&[pair("type", "qrcode")])), None);
        assert_eq!(resolve_options(&blob(&[pair("output", "png")])), None);
        assert_eq!(resolve_options(&blob(&[pair("scale", "4x")])), None);
        assert_eq!(resolve_options(&blob(&[pair("height", "-1")])), None);
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
}