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

//! base64 — encode and decode bytes in RFC 4648 base64 (standard or URL-safe
//! alphabet). A minimal raw-ABI module: the caller writes the input into
//! `alloc`'d linear memory, calls `encode`/`decode` with an options blob, reads
//! the output from the returned pointer, then `dealloc`s all buffers.
//! No envelope, no host imports — just `memory`, `alloc`, `dealloc`,
//! `encode`, `decode`, `manifest`.
//!
//! The options blob is a flat length-prefixed key/value list so future options
//! never change the export signatures: old cores ignore unknown keys, new cores
//! default missing ones. See README.md for the wire format. The `manifest`
//! export returns the module's self-description (JSON) so consumers can call
//! it generically without hardcoding its interface.

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side. The `version` field is kept in sync with Cargo.toml by a test below.
const MANIFEST: &str = r#"{
  "id": "base64",
  "version": "0.1.0",
  "description": "Encode and decode bytes in RFC 4648 base64.",
  "exports": {
    "encode": {
      "summary": "Encode bytes to base64 text.",
      "options": {
        "alphabet": {
          "type": "enum",
          "values": ["standard", "url"],
          "default": "standard",
          "description": "Output alphabet: standard (+/) or URL-safe (-_)."
        },
        "padding": {
          "type": "boolean",
          "default": true,
          "description": "Emit '=' padding on incomplete input groups."
        },
        "wrap": {
          "type": "number",
          "default": 0,
          "description": "Insert a newline every N output characters; 0 disables wrapping."
        }
      }
    },
    "decode": {
      "summary": "Decode base64 text to bytes.",
      "options": {
        "alphabet": {
          "type": "enum",
          "values": ["standard", "url"],
          "default": "standard",
          "description": "Alphabet accepted in the input."
        }
      }
    }
  }
}"#;

/// RFC 4648 standard alphabet (`+`/`=` line).
const STANDARD_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// RFC 4648 Section 5 URL-safe alphabet (`-`/`_` instead of `+`/`/`).
const URL_SAFE_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Magic byte every non-empty options blob must start with, so a format
/// revision can be detected instead of silently mis-parsed.
const OPTIONS_MAGIC: u8 = 0x01;

/// Per-call options parsed from the options blob. Missing keys fall back to
/// these defaults; unknown keys are ignored by design (forward compatibility).
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Emit `=` padding on encode. Decode accepts padded and unpadded input
    /// regardless of this flag.
    pub padded: bool,
    /// Use the URL-safe alphabet (`-`/`_`) instead of the standard one.
    pub url_safe: bool,
    /// Insert `\n` every `wrap` output characters on encode; 0 disables wrapping.
    pub wrap: usize,
}

impl Default for Options {
    /// RFC 4648 behavior: standard alphabet, padded, unwrapped.
    fn default() -> Self {
        Options {
            padded: true,
            url_safe: false,
            wrap: 0,
        }
    }
}

/// Pure core: encode a byte slice with the given options.
pub fn encode_bytes(input: &[u8], opts: &Options) -> String {
    let alphabet = if opts.url_safe {
        URL_SAFE_ALPHABET
    } else {
        STANDARD_ALPHABET
    };
    let mut encoded = String::with_capacity((input.len() + 2) / 3 * 4);

    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let triple = (b0 as u32) << 16 | (b1 as u32) << 8 | b2 as u32;

        encoded.push(alphabet[(triple >> 18) as usize & 63] as char);
        encoded.push(alphabet[(triple >> 12) as usize & 63] as char);

        if chunk.len() > 1 {
            encoded.push(alphabet[(triple >> 6) as usize & 63] as char);
        } else if opts.padded {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(alphabet[triple as usize & 63] as char);
        } else if opts.padded {
            encoded.push('=');
        }
    }

    // Count-and-insert wrapping: no second buffer scan, no slicing/joining,
    // and no `str` conversion that would drag panic formatting into the module.
    if opts.wrap == 0 {
        return encoded;
    }

    let mut wrapped = String::with_capacity(encoded.len() + encoded.len() / opts.wrap);
    let mut column = 0;

    for symbol in encoded.chars() {
        if column == opts.wrap {
            wrapped.push('\n');
            column = 0;
        }

        wrapped.push(symbol);
        column += 1;
    }

    wrapped
}

/// Pure core: decode a base64 byte slice with the given options. ASCII
/// whitespace is ignored; both padded and unpadded input are accepted;
/// anything outside the selected alphabet (after padding handling) is an
/// error. Trailing bits are not checked for canonical zeroing — lenient like
/// most decoders.
pub fn decode_bytes(input: &[u8], opts: &Options) -> Result<Vec<u8>, &'static str> {
    let compact: Vec<u8> = input
        .iter()
        .copied()
        .filter(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .collect();

    let mut end = compact.len();
    let mut pads = 0;

    while end > 0 && compact[end - 1] == b'=' {
        end -= 1;
        pads += 1;
    }

    let data = &compact[..end];

    if pads > 2 || data.contains(&b'=') {
        return Err("padding character found outside the end of the input");
    }

    // Valid shapes: a trailing remainder of r data chars carries r-1 bytes and
    // canonically pads to the next 4-char boundary (r==2 wants `==`, r==3
    // wants `=`); unpadded input is accepted too. Everything else is corrupt.
    match (data.len() % 4, pads) {
        (0, 0) => {}
        (2, 0) | (2, 2) | (3, 0) | (3, 1) => {}
        _ => return Err("invalid base64 length"),
    }

    let alphabet = if opts.url_safe {
        URL_SAFE_ALPHABET
    } else {
        STANDARD_ALPHABET
    };
    let mut reverse = [0xFFu8; 256];

    for (index, &symbol) in alphabet.iter().enumerate() {
        reverse[symbol as usize] = index as u8;
    }

    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;

    for &symbol in data {
        let value = reverse[symbol as usize];

        if value == 0xFF {
            return Err("invalid base64 character");
        }

        acc = acc << 6 | value as u32;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }

    Ok(out)
}

/// Parse one little-endian `u32` off the front of `blob`, returning it with
/// the remaining slice.
fn read_u32(blob: &[u8]) -> Option<(u32, &[u8])> {
    if blob.len() < 4 {
        return None;
    }

    Some((u32::from_le_bytes(blob[..4].try_into().ok()?), &blob[4..]))
}

/// ASCII-decimal `usize` over raw bytes — replaces `str::parse`, which drags
/// integer-parsing error machinery into the module. Overflow or a non-digit
/// is simply no value.
fn parse_usize(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }

    let mut value = 0usize;

    for &digit in bytes {
        if !digit.is_ascii_digit() {
            return None;
        }

        value = value.checked_mul(10)?.checked_add((digit - b'0') as usize)?;
    }

    Some(value)
}

/// Parse the options blob straight into resolved `Options` in one pass. An
/// empty blob means "no options"; otherwise the magic byte is required.
/// Unknown keys are ignored so new callers stay compatible with older cores;
/// known keys with bad values are errors, because silently dropping a
/// requested option could corrupt results. Returns `None` when the blob is
/// truncated, malformed, or carries an unusable value.
fn resolve_options(mut blob: &[u8]) -> Option<Options> {
    if blob.is_empty() {
        return Some(Options::default());
    }

    if blob[0] != OPTIONS_MAGIC {
        return None;
    }

    blob = &blob[1..];
    let mut opts = Options::default();

    while !blob.is_empty() {
        let (key_len, after_key_len) = read_u32(blob)?;
        let key_len = key_len as usize;

        if after_key_len.len() < key_len {
            return None;
        }

        let (key, after_key) = after_key_len.split_at(key_len);
        let (value_len, after_value_len) = read_u32(after_key)?;
        let value_len = value_len as usize;

        if after_value_len.len() < value_len {
            return None;
        }

        let (value, tail) = after_value_len.split_at(value_len);

        match key {
            b"alphabet" => match value {
                b"standard" => opts.url_safe = false,
                b"url" => opts.url_safe = true,
                _ => return None,
            },
            b"padding" => match value {
                b"true" => opts.padded = true,
                b"false" => opts.padded = false,
                _ => return None,
            },
            b"wrap" => match parse_usize(value) {
                Some(width) => opts.wrap = width,
                None => return None,
            },
            _ => {}
        }

        blob = tail;
    }

    Some(opts)
}

/// Pack an owned byte vector as the exported `ptr << 32 | len` result,
/// leaking an exact-length allocation whose layout matches what `dealloc`
/// reconstructs.
fn pack(bytes: Vec<u8>) -> u64 {
    let boxed: Box<[u8]> = bytes.into_boxed_slice();
    let len = boxed.len();
    let mut boxed = boxed;
    let ptr = boxed.as_mut_ptr() as u64;

    std::mem::forget(boxed);

    ptr << 32 | len as u64
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `encode`/`decode` and back to `dealloc` when done.
///
/// # Safety
/// The returned pointer is only valid inside this module's linear memory and
/// must be released with `dealloc`.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr() as u32;

    std::mem::forget(buf);

    ptr
}

/// Free a buffer previously handed out by `alloc`.
///
/// # Safety
/// `ptr`/`len` must come from `alloc` and must not have been freed before.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    let buf = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);

    drop(buf);
}

/// Encode the input bytes at `ptr..ptr+len` as base64 and return the output
/// packed as `ptr << 32 | len`. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); a malformed blob or
/// bad option value returns 0. The caller reads the output and deallocs both
/// buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn encode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if opts_len == 0 {
        return pack(encode_bytes(input, &Options::default()).into_bytes());
    }

    let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

    match resolve_options(blob) {
        Some(opts) => pack(encode_bytes(input, &opts).into_bytes()),
        None => 0,
    }
}

/// Decode the base64 input at `ptr..ptr+len` and return the decoded bytes
/// packed as `ptr << 32 | len`. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); invalid input, a
/// malformed blob, or a bad option value returns 0. The caller reads the
/// output and deallocs both buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn decode(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    if opts_len == 0 {
        return match decode_bytes(input, &Options::default()) {
            Ok(bytes) => pack(bytes),
            Err(_) => 0,
        };
    }

    let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

    match resolve_options(blob) {
        Some(opts) => match decode_bytes(input, &opts) {
            Ok(bytes) => pack(bytes),
            Err(_) => 0,
        },
        None => 0,
    }
}

/// Return the manifest JSON packed as `ptr << 32 | len`. The caller reads the
/// text, then deallocs the buffer.
#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn encodes_hello_world_with_defaults() {
        assert_eq!(encode_bytes(b"hello world", &Options::default()), "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn encodes_empty_to_empty() {
        assert_eq!(encode_bytes(b"", &Options::default()), "");
    }

    #[test]
    fn encodes_url_safe_alphabet() {
        let url = Options {
            url_safe: true,
            ..Options::default()
        };

        // 0xFB 0xEF 0xBE splits into four sextets of 62: `+` standard, `-` URL-safe.
        assert_eq!(encode_bytes(&[0xFB, 0xEF, 0xBE], &Options::default()), "++++");
        assert_eq!(encode_bytes(&[0xFB, 0xEF, 0xBE], &url), "----");

        // A sextet of 63 exercises the other special symbol of each alphabet.
        assert_eq!(encode_bytes(&[0xFF, 0xEF, 0xBE], &Options::default()), "/+++");
        assert_eq!(encode_bytes(&[0xFF, 0xEF, 0xBE], &url), "_---");
    }

    #[test]
    fn omits_padding_when_disabled() {
        let opts = Options {
            padded: false,
            ..Options::default()
        };

        assert_eq!(encode_bytes(b"hello world", &opts), "aGVsbG8gd29ybGQ");
        assert_eq!(encode_bytes(b"a", &opts), "YQ");
    }

    #[test]
    fn wraps_output_columns() {
        let opts = Options {
            wrap: 4,
            ..Options::default()
        };

        assert_eq!(encode_bytes(b"hello world", &opts), "aGVs\nbG8g\nd29y\nbGQ=");
    }

    #[test]
    fn decodes_hello_world_with_defaults() {
        assert_eq!(
            decode_bytes(b"aGVsbG8gd29ybGQ=", &Options::default()),
            Ok(b"hello world".to_vec())
        );
    }

    #[test]
    fn decodes_empty_to_empty() {
        assert_eq!(decode_bytes(b"", &Options::default()), Ok(Vec::new()));
    }

    #[test]
    fn decodes_accepts_unpadded_and_whitespace() {
        assert_eq!(
            decode_bytes(b"aGVsbG8gd29ybGQ", &Options::default()),
            Ok(b"hello world".to_vec())
        );
        assert_eq!(
            decode_bytes(b"aGVs\nbG8g\r\nd29y\tbGQ= \n", &Options::default()),
            Ok(b"hello world".to_vec())
        );
    }

    #[test]
    fn decodes_url_safe_alphabet_only() {
        assert_eq!(
            decode_bytes(b"----", &Options {
                url_safe: true,
                ..Options::default()
            }),
            Ok(vec![0xFB, 0xEF, 0xBE])
        );
        assert!(decode_bytes(b"----", &Options::default()).is_err());
        assert!(decode_bytes(b"++++", &Options {
            url_safe: true,
            ..Options::default()
        })
        .is_err());
    }

    #[test]
    fn round_trips_binary_data_across_option_combos() {
        let sample: &[u8] = &[
            0x00, 0x01, 0xFB, 0xEF, 0xBE, 0x7F, 0x80, 0xFF, b'a', b'b', b'c', b'd', b'e',
        ];

        for url_safe in [false, true] {
            for padded in [false, true] {
                let opts = Options {
                    url_safe,
                    padded,
                    wrap: 0,
                };

                assert_eq!(
                    decode_bytes(encode_bytes(sample, &opts).as_bytes(), &opts),
                    Ok(sample.to_vec())
                );
            }
        }
    }

    #[test]
    fn rejects_invalid_characters() {
        assert!(decode_bytes(b"aGVs*bG8=", &Options::default()).is_err());
    }

    #[test]
    fn rejects_impossible_lengths() {
        assert!(decode_bytes(b"A", &Options::default()).is_err());
        assert!(decode_bytes(b"ABCDE", &Options::default()).is_err());
        assert!(decode_bytes(b"A===", &Options::default()).is_err());
        assert!(decode_bytes(b"ABCD=", &Options::default()).is_err());
        // A 2-char remainder pads with two `=`; one is corrupt.
        assert!(decode_bytes(b"YQ=", &Options::default()).is_err());
        // And its canonical form decodes.
        assert_eq!(
            decode_bytes(b"YQ==", &Options::default()),
            Ok(b"a".to_vec())
        );
    }

    #[test]
    fn rejects_padding_outside_the_end() {
        assert!(decode_bytes(b"aGVs=bG8gd29ybGQ=", &Options::default()).is_err());
    }

    #[test]
    fn resolves_options_blob() {
        assert_eq!(
            resolve_options(&blob(&[pair("alphabet", "url"), pair("padding", "false"), pair("wrap", "76")])),
            Some(Options {
                url_safe: true,
                padded: false,
                wrap: 76
            })
        );
    }

    #[test]
    fn empty_blob_means_defaults_without_magic() {
        assert_eq!(resolve_options(b""), Some(Options::default()));
    }

    #[test]
    fn rejects_malformed_blobs_and_values() {
        assert_eq!(resolve_options(&[0x02]), None);
        assert_eq!(resolve_options(&blob(&[pair("alphabet", "url")])[..4]), None);
        assert_eq!(resolve_options(&[OPTIONS_MAGIC, 0, 0, 0, 5, b'a']), None);
        assert_eq!(resolve_options(&blob(&[pair("alphabet", "rot13")])), None);
        assert_eq!(resolve_options(&blob(&[pair("padding", "yes")])), None);
        assert_eq!(resolve_options(&blob(&[pair("wrap", "12x")])), None);
        assert_eq!(resolve_options(&blob(&[pair("wrap", "")])), None);

        // Overflowing usize is not a column count either.
        assert_eq!(resolve_options(&blob(&[pair("wrap", "99999999999999999999999")])), None);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(
            resolve_options(&blob(&[pair("future", "whatever"), pair("wrap", "10")])),
            Some(Options {
                wrap: 10,
                ..Options::default()
            })
        );
    }

    #[test]
    fn manifest_version_matches_crate_version() {
        // The manifest is a hand-written string; this is the tripwire that
        // keeps its `version` field from drifting away from Cargo.toml.
        assert!(MANIFEST.contains(env!("CARGO_PKG_VERSION")));
    }
}
