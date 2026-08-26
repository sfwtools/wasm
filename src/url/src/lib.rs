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

//! url — percent-encode and decode URL text (RFC 3986). A minimal raw-ABI
//! module: the caller writes the input into `alloc`'d linear memory, calls
//! `encode`/`decode` with an options blob, reads the output from the returned
//! pointer, then `dealloc`s all buffers. No envelope, no host imports — just
//! `memory`, `alloc`, `dealloc`, `encode`, `decode`, `manifest`.
//!
//! The options blob is a flat length-prefixed key/value list so future options
//! never change the export signatures: old cores ignore unknown keys, new cores
//! default missing ones. See README.md for the wire format. The `manifest`
//! export returns the module's self-description (JSON) so consumers can call
//! it generically without hardcoding its interface.
//!
//! Buffer packing, blob framing, and the ABI exports' plumbing come from the
//! shared `abi` crate; only the tool logic lives here.

use abi::option_pairs;

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side.
const MANIFEST: &str = r#"{
  "exports": {
    "encode": {
      "summary": "Percent-encode text for use in a URL.",
      "options": {
        "space": {
          "type": "boolean",
          "default": false,
          "description": "Encode spaces as '+' (application/x-www-form-urlencoded) instead of '%20'."
        }
      }
    },
    "decode": {
      "summary": "Decode percent-encoded text.",
      "options": {
        "plus": {
          "type": "boolean",
          "default": false,
          "description": "Treat '+' as a space (application/x-www-form-urlencoded)."
        }
      }
    }
  }
}"#;

/// RFC 3986 unreserved characters pass through percent-encoding untouched.
fn is_unreserved(byte: u8) -> bool {
    matches!(byte,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~')
}

/// Hex digit for a nibble, uppercase per RFC 3986.
fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + (nibble - 10),
    }
}

/// Numeric value of an ASCII hex digit.
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Per-call options parsed from the options blob. Missing keys fall back to
/// these defaults; unknown keys are ignored by design (forward compatibility).
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Encode spaces as `+` (form-style) instead of `%20`.
    pub plus_spaces: bool,
    /// On decode, treat `+` as a space (form-style).
    pub plus_is_space: bool,
}

impl Default for Options {
    /// RFC 3986 behavior: `%20` spaces, `+` is a literal plus on decode.
    fn default() -> Self {
        Options {
            plus_spaces: false,
            plus_is_space: false,
        }
    }
}

/// Pure core: percent-encode a byte slice with the given options. Every byte
/// outside the RFC 3986 unreserved set becomes `%XX` (uppercase hex); `+` for
/// spaces is opt-in for form-encoding contexts.
pub fn encode_bytes(input: &[u8], opts: &Options) -> String {
    let mut encoded = String::with_capacity(input.len());

    for &byte in input {
        if is_unreserved(byte) {
            encoded.push(byte as char);
        } else if byte == b' ' && opts.plus_spaces {
            encoded.push('+');
        } else {
            encoded.push('%');
            encoded.push(hex_digit(byte >> 4) as char);
            encoded.push(hex_digit(byte & 0x0F) as char);
        }
    }

    encoded
}

/// Pure core: percent-decode a byte slice with the given options. A `%` must
/// be followed by two hex digits; `+` becomes a space only when requested. The
/// result must be valid UTF-8, since this module's outputs are text.
pub fn decode_bytes(input: &[u8], opts: &Options) -> Result<Vec<u8>, &'static str> {
    let mut decoded = Vec::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        match input[index] {
            b'%' => {
                if index + 2 >= input.len() {
                    return Err("percent-escape is cut off at the end of the input");
                }

                let high = hex_value(input[index + 1])
                    .ok_or("a percent-escape is not followed by two hex digits")?;
                let low = hex_value(input[index + 2])
                    .ok_or("a percent-escape is not followed by two hex digits")?;

                decoded.push(high << 4 | low);
                index += 3;
            }
            b'+' if opts.plus_is_space => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    match std::str::from_utf8(&decoded) {
        Ok(_) => Ok(decoded),
        Err(_) => Err("the decoded bytes are not valid UTF-8 text"),
    }
}

/// Parse the options blob straight into resolved `Options`. Framing (magic
/// byte, length prefixes) is validated by the shared `option_pairs`; unknown
/// keys are ignored so new callers stay compatible with older cores; known
/// keys with bad values are errors, because silently dropping a requested
/// option could corrupt results. Returns `None` when the blob is malformed
/// or carries an unusable value.
fn resolve_options(blob: &[u8]) -> Option<Options> {
    let mut opts = Options::default();

    for (key, value) in option_pairs(blob)? {
        match key {
            b"space" => match value {
                b"true" => opts.plus_spaces = true,
                b"false" => opts.plus_spaces = false,
                _ => return None,
            },
            b"plus" => match value {
                b"true" => opts.plus_is_space = true,
                b"false" => opts.plus_is_space = false,
                _ => return None,
            },
            _ => {}
        }
    }

    Some(opts)
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `encode`/`decode` and back to `dealloc` when done.
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

/// Encode the input bytes at `ptr..ptr+len` as percent-encoded text and return
/// the output packed as `ptr << 32 | len`. Options come from the blob at
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
        return abi::pack(encode_bytes(input, &Options::default()).into_bytes());
    }

    let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

    match resolve_options(blob) {
        Some(opts) => abi::pack(encode_bytes(input, &opts).into_bytes()),
        None => 0,
    }
}

/// Decode the percent-encoded input at `ptr..ptr+len` and return the decoded
/// bytes packed as `ptr << 32 | len`. Options come from the blob at
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
            Ok(bytes) => abi::pack(bytes),
            Err(_) => 0,
        };
    }

    let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

    match resolve_options(blob) {
        Some(opts) => match decode_bytes(input, &opts) {
            Ok(bytes) => abi::pack(bytes),
            Err(_) => 0,
        },
        None => 0,
    }
}

/// Return the manifest JSON packed as `ptr << 32 | len`. The caller reads the
/// text, then deallocs the buffer.
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
    fn encodes_unreserved_characters_untouched() {
        assert_eq!(
            encode_bytes(b"AZaz09-_.~", &Options::default()),
            "AZaz09-_.~"
        );
    }

    #[test]
    fn encodes_spaces_and_reserved_chars_percent() {
        assert_eq!(
            encode_bytes(b"hello world", &Options::default()),
            "hello%20world"
        );
        assert_eq!(
            encode_bytes(b"a&b=c/d?e", &Options::default()),
            "a%26b%3Dc%2Fd%3Fe"
        );
    }

    #[test]
    fn encodes_non_ascii_as_utf8_bytes() {
        // 'é' is 0xC3 0xA9; a multi-byte char encodes every byte.
        assert_eq!(encode_bytes("café".as_bytes(), &Options::default()), "caf%C3%A9");
    }

    #[test]
    fn encodes_spaces_as_plus_when_requested() {
        let opts = Options {
            plus_spaces: true,
            ..Options::default()
        };

        assert_eq!(encode_bytes(b"hello world", &opts), "hello+world");
    }

    #[test]
    fn encodes_empty_to_empty() {
        assert_eq!(encode_bytes(b"", &Options::default()), "");
    }

    #[test]
    fn decodes_percent_escapes() {
        assert_eq!(
            decode_bytes(b"hello%20world", &Options::default()),
            Ok(b"hello world".to_vec())
        );
        assert_eq!(
            decode_bytes(b"caf%C3%A9", &Options::default()),
            Ok("café".as_bytes().to_vec())
        );
    }

    #[test]
    fn decodes_lowercase_hex_too() {
        assert_eq!(
            decode_bytes(b"%c3%a9", &Options::default()),
            Ok("é".as_bytes().to_vec())
        );
    }

    #[test]
    fn decodes_plus_only_when_requested() {
        assert_eq!(
            decode_bytes(b"hello+world", &Options::default()),
            Ok(b"hello+world".to_vec())
        );

        let opts = Options {
            plus_is_space: true,
            ..Options::default()
        };

        assert_eq!(decode_bytes(b"hello+world", &opts), Ok(b"hello world".to_vec()));
    }

    #[test]
    fn round_trips_mixed_text() {
        let sample = "café + naïve / 100% safe";

        assert_eq!(
            decode_bytes(encode_bytes(sample.as_bytes(), &Options::default()).as_bytes(), &Options::default()),
            Ok(sample.as_bytes().to_vec())
        );
    }

    #[test]
    fn rejects_cut_off_escapes() {
        assert!(decode_bytes(b"%", &Options::default()).is_err());
        assert!(decode_bytes(b"%2", &Options::default()).is_err());
    }

    #[test]
    fn rejects_non_hex_escapes() {
        assert!(decode_bytes(b"%zz", &Options::default()).is_err());
        assert!(decode_bytes(b"a%2g", &Options::default()).is_err());
    }

    #[test]
    fn rejects_invalid_utf8_after_decode() {
        // %FF decodes to a lone 0xFF byte, which is not valid UTF-8.
        assert!(decode_bytes(b"%FF", &Options::default()).is_err());
    }

    #[test]
    fn resolves_options_blob() {
        assert_eq!(
            resolve_options(&blob(&[pair("space", "true"), pair("plus", "true")])),
            Some(Options {
                plus_spaces: true,
                plus_is_space: true
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
        assert_eq!(resolve_options(&blob(&[pair("space", "url")])[..4]), None);
        assert_eq!(resolve_options(&[OPTIONS_MAGIC, 0, 0, 0, 5, b'a']), None);
        assert_eq!(resolve_options(&blob(&[pair("space", "yes")])), None);
        assert_eq!(resolve_options(&blob(&[pair("plus", "1")])), None);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(
            resolve_options(&blob(&[pair("future", "whatever"), pair("space", "true")])),
            Some(Options {
                plus_spaces: true,
                ..Options::default()
            })
        );
    }
}