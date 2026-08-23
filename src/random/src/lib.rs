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

//! random - generate random ASCII strings, sized for password generation.
//!
//! Bare wasm32 has no entropy source and this module deliberately imports
//! nothing (that is what lets every host instantiate it with an empty import
//! list), so the CALLER supplies fresh CSPRNG bytes as the input - browser
//! `crypto.getRandomValues`, Node `crypto.randomBytes`, `/dev/urandom`. The
//! module folds the whole seed into a ChaCha20 key and stretches it into as
//! many characters as requested. Same seed means same output: hosts must use
//! a fresh seed per call. A short or empty seed is rejected rather than
//! silently weakened.
//!
//! Options travel as the house options blob (see README.md): `0x01` magic,
//! little-endian length-prefixed UTF-8 key/value pairs; unknown keys ignored,
//! known keys with bad values fail the call. The output is the string-array
//! frame documented in the manifest (`0x01`, count, then length-prefixed
//! UTF-8 strings) so callers parse one generic shape instead of splitting on
//! a separator that could collide with payload bytes.
//!
//! Buffer packing, blob framing, and the output frame come from the shared
//! `abi` crate; only the tool logic lives here.

use abi::{frame_strings, option_pairs, parse_usize};

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side. `generate.output` marks the return shape for generic hosts.
const MANIFEST: &str = r#"{
  "exports": {
    "generate": {
      "summary": "Generate random ASCII strings from seed bytes given as input.",
      "output": "string-array",
      "options": {
        "upper": {
          "type": "boolean",
          "default": true,
          "description": "Include uppercase letters A-Z."
        },
        "lower": {
          "type": "boolean",
          "default": true,
          "description": "Include lowercase letters a-z."
        },
        "numbers": {
          "type": "boolean",
          "default": true,
          "description": "Include digits 0-9."
        },
        "symbols": {
          "type": "boolean",
          "default": true,
          "description": "Include printable ASCII punctuation (everything except space, control characters and alphanumerics)."
        },
        "length": {
          "type": "number",
          "default": 16,
          "description": "Characters per string (1-1024)."
        },
        "count": {
          "type": "number",
          "default": 1,
          "description": "How many strings to generate (1-1024)."
        }
      }
    }
  }
}"#;

/// Minimum seed bytes folded into the ChaCha20 key. Fewer than this cannot
/// carry 256 bits of entropy, so the call fails instead of guessing.
const MIN_SEED: usize = 32;

/// Friendly ceilings: passwords beyond these sizes/counts are out of scope
/// for a bounded-memory wasm module and almost certainly a caller mistake.
const MAX_LENGTH: usize = 1024;
const MAX_COUNT: usize = 1024;

/// Per-call options parsed from the options blob. Missing keys fall back to
/// these defaults; unknown keys are ignored by design (forward compatibility).
#[derive(Debug, PartialEq)]
pub struct Options {
    /// Include `A-Z`.
    pub upper: bool,
    /// Include `a-z`.
    pub lower: bool,
    /// Include `0-9`.
    pub numbers: bool,
    /// Include printable ASCII punctuation: everything except space, control
    /// characters and alphanumerics.
    pub symbols: bool,
    /// Characters per string.
    pub length: usize,
    /// How many strings to generate.
    pub count: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            upper: true,
            lower: true,
            numbers: true,
            symbols: true,
            length: 16,
            count: 1,
        }
    }
}

/// Fold any seed of at least `MIN_SEED` bytes into exactly 32 key bytes by
/// XOR-folding 32-byte chunks. Extra seed bytes therefore always change the
/// key, never get truncated away.
fn fold_seed(seed: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    let mut chunks = seed.chunks_exact(32);

    for chunk in &mut chunks {
        for (k, b) in key.iter_mut().zip(chunk) {
            *k ^= b;
        }
    }

    // A trailing partial chunk still contributes its prefix bytes.
    for (k, b) in key.iter_mut().zip(chunks.remainder()) {
        *k ^= b;
    }

    key
}

/// One ChaCha20 quarter round over four state words (RFC 8439).
fn quarter_round(state: &mut [u32; 16], a: usize, b: usize, c:usize, d: usize) {
    state[a] = state[a].wrapping_add(state[b]);
    state[d] = (state[d] ^ state[a]).rotate_left(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_left(12);
    state[a] = state[a].wrapping_add(state[b]);
    state[d] = (state[d] ^ state[a]).rotate_left(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_left(7);
}

/// The ChaCha20 block function: 20 rounds over the working state, then add
/// the original state. Output words serialize little-endian.
fn chacha20_block(key: &[u8; 32], counter: u32) -> [u8; 64] {
    let mut state = [
        0x61707865u32, 0x3320646eu32, 0x79622d32u32, 0x6b206574u32,
        u32::from_le_bytes(key[0..4].try_into().unwrap()),
        u32::from_le_bytes(key[4..8].try_into().unwrap()),
        u32::from_le_bytes(key[8..12].try_into().unwrap()),
        u32::from_le_bytes(key[12..16].try_into().unwrap()),
        u32::from_le_bytes(key[16..20].try_into().unwrap()),
        u32::from_le_bytes(key[20..24].try_into().unwrap()),
        u32::from_le_bytes(key[24..28].try_into().unwrap()),
        u32::from_le_bytes(key[28..32].try_into().unwrap()),
        counter,
        0,
        0,
        0,
    ];
    let start = state;

    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];

    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.wrapping_add(start[i]).to_le_bytes());
    }

    out
}

/// An endless keystream over the folded key: consecutive ChaCha20 blocks with
/// a zero nonce and a rising counter.
struct Stream {
    key: [u8; 32],
    counter: u32,
    block: [u8; 64],
    used: usize,
}

impl Stream {
    fn new(key: [u8; 32]) -> Self {
        Stream {
            key,
            counter: 0,
            block: chacha20_block(&key, 0),
            used: 0,
        }
    }

    fn next_byte(&mut self) -> u8 {
        if self.used == 64 {
            self.counter += 1;
            self.block = chacha20_block(&self.key, self.counter);
            self.used = 0;
        }

        let b = self.block[self.used];
        self.used += 1;

        b
    }
}

/// The ASCII character class definitions behind the boolean options.
fn class_range(enabled: bool, range: impl Iterator<Item = u8>) -> impl Iterator<Item = u8> {
    range.filter(move |_| enabled)
}

/// Build the alphabet the strings are drawn from: the union of the enabled
/// classes. Symbols are every printable ASCII byte that no other class claims.
fn alphabet(opts: &Options) -> Vec<u8> {
    let mut set: Vec<u8> = Vec::with_capacity(94);

    for b in class_range(opts.upper, b'A'..=b'Z')
        .chain(class_range(opts.lower, b'a'..=b'z'))
        .chain(class_range(opts.numbers, b'0'..=b'9'))
    {
        set.push(b);
    }

    if opts.symbols {
        for b in 0x21u8..=0x7E {
            if !b.is_ascii_alphanumeric() {
                set.push(b);
            }
        }
    }

    set
}

/// Generate `count` strings of `length` chars each from `alphabet`, drawing
/// uniformly via rejection sampling (bytes >= the largest multiple of the
/// alphabet size are discarded, so no character is favored). Returns the
/// strings joined into the output frame's entry list.
pub fn generate_strings(seed: &[u8], opts: &Options) -> Result<Vec<String>, &'static str> {
    if seed.len() < MIN_SEED {
        return Err("need at least 32 seed bytes of host entropy");
    }

    if opts.length == 0 || opts.length > MAX_LENGTH {
        return Err("length must be between 1 and 1024");
    }

    if opts.count == 0 || opts.count > MAX_COUNT {
        return Err("count must be between 1 and 1024");
    }

    let alphabet = alphabet(opts);

    if alphabet.is_empty() {
        return Err("at least one character class must be enabled");
    }

    // Largest multiple of the alphabet size within one byte: draws below it
    // map onto alphabet indices without bias.
    let limit = 256 - 256 % alphabet.len();
    let mut stream = Stream::new(fold_seed(seed));
    let mut out = Vec::with_capacity(opts.count);

    for _ in 0..opts.count {
        let mut s = String::with_capacity(opts.length);

        for _ in 0..opts.length {
            let b = loop {
                let b = stream.next_byte();

                if (b as usize) < limit {
                    break b;
                }
            };

            s.push(alphabet[b as usize % alphabet.len()] as char);
        }

        out.push(s);
    }

    Ok(out)
}

/// Parse the options blob straight into resolved `Options`. Framing (magic
/// byte, length prefixes) is validated by the shared `option_pairs`; unknown
/// keys are ignored so new callers stay compatible with older cores; known
/// keys with bad values are errors, because silently dropping a requested
/// option could weaken results. Returns `None` when the blob is malformed
/// or carries an unusable value.
fn resolve_options(blob: &[u8]) -> Option<Options> {
    let mut opts = Options::default();

    for (key, value) in option_pairs(blob)? {
        match key {
            b"upper" => match value {
                b"true" => opts.upper = true,
                b"false" => opts.upper = false,
                _ => return None,
            },
            b"lower" => match value {
                b"true" => opts.lower = true,
                b"false" => opts.lower = false,
                _ => return None,
            },
            b"numbers" => match value {
                b"true" => opts.numbers = true,
                b"false" => opts.numbers = false,
                _ => return None,
            },
            b"symbols" => match value {
                b"true" => opts.symbols = true,
                b"false" => opts.symbols = false,
                _ => return None,
            },
            b"length" => match parse_usize(value) {
                Some(n) => opts.length = n,
                None => return None,
            },
            b"count" => match parse_usize(value) {
                Some(n) => opts.count = n,
                None => return None,
            },
            _ => {}
        }
    }

    Some(opts)
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `generate` and back to `dealloc` when done.
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

/// Generate random strings from the seed at `ptr..ptr+len` and return the
/// string-array frame packed as `ptr << 32 | len`. Options come from the blob
/// at `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults); a too-short
/// seed, a malformed blob, or an unusable option value returns 0. The caller
/// reads the output and deallocs both buffers.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn generate(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let seed = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    let opts = if opts_len == 0 {
        Options::default()
    } else {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        match resolve_options(blob) {
            Some(opts) => opts,
            None => return 0,
        }
    };

    match generate_strings(seed, &opts) {
        Ok(strings) => abi::pack(frame_strings(&strings)),
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
    use std::collections::HashSet;

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

    const SEED: &[u8] = &[0x5Au8; 64];

    #[test]
    fn defaults_generate_one_sixteen_char_string() {
        let out = generate_strings(SEED, &Options::default()).expect("defaults should work");

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 16);
    }

    #[test]
    fn output_stays_printable_ascii() {
        let opts = Options::default();
        let out = generate_strings(&[0xA7u8; 64], &opts).expect("generation works");

        for s in out {
            for b in s.bytes() {
                assert!((0x21..=0x7E).contains(&b), "non-printable byte {b:#04x}");
            }
        }
    }

    #[test]
    fn class_options_narrow_the_alphabet() {
        let digits = Options {
            upper: false,
            lower: false,
            symbols: false,
            ..Options::default()
        };
        let out = generate_strings(SEED, &digits).expect("digits-only works");

        assert!(out[0].bytes().all(|b| b.is_ascii_digit()));
        assert_eq!(alphabet(&digits).len(), 10);

        let none = Options {
            upper: false,
            lower: false,
            numbers: false,
            symbols: false,
            ..Options::default()
        };
        assert_eq!(alphabet(&none).len(), 0);
        assert!(generate_strings(SEED, &none).is_err());
    }

    #[test]
    fn symbol_class_is_punctuation_only() {
        let syms = alphabet(&Options {
            upper: false,
            lower: false,
            numbers: false,
            symbols: true,
            ..Options::default()
        });

        assert_eq!(syms.len(), 32);
        assert!(syms.iter().all(|b| !b.is_ascii_alphanumeric()));
        assert!(syms.contains(&b'!'));
        assert!(syms.contains(&b'~'));
        assert!(!syms.contains(&b' '));
    }

    #[test]
    fn same_seed_reproduces_and_different_seed_differs() {
        // Seeds must be varied: constant-byte seeds of 32-multiple length
        // would XOR-fold to the same key, which is exactly what this test
        // guards against conflating.
        let a = generate_strings(&test_seed(0x5A), &Options::default()).unwrap();
        let b = generate_strings(&test_seed(0x5A), &Options::default()).unwrap();
        let c = generate_strings(&test_seed(0xA5), &Options::default()).unwrap();

        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    /// A 48-byte varied seed: two full chunks plus a trailing partial one,
    /// so both fold paths are exercised.
    fn test_seed(tag: u8) -> Vec<u8> {
        (0..48).map(|i| tag.wrapping_add((i as u8).wrapping_mul(7))).collect()
    }

    #[test]
    fn distribution_uses_the_whole_alphabet() {
        // 40 chars over a 26-letter alphabet: every letter appearing is the
        // expected case (p(all present) ~ 87%); a broken sampler misses many.
        let opts = Options {
            upper: false,
            numbers: false,
            symbols: false,
            length: 400,
            count: 1,
            ..Options::default()
        };
        let out = generate_strings(SEED, &opts).unwrap();
        let seen: HashSet<u8> = out[0].bytes().collect();

        assert_eq!(seen.len(), 26, "sampler missed letters: {out:?}");
    }

    #[test]
    fn rejects_short_seeds_and_bad_bounds() {
        assert!(generate_strings(&[0u8; 31], &Options::default()).is_err());

        let long = Options {
            length: 1025,
            ..Options::default()
        };
        assert!(generate_strings(SEED, &long).is_err());

        let many = Options {
            count: 0,
            ..Options::default()
        };
        assert!(generate_strings(SEED, &many).is_err());
    }

    #[test]
    fn frames_strings_as_a_counted_length_prefixed_array() {
        let frame = frame_strings(&["ab".to_string(), String::new()]);

        assert_eq!(
            frame,
            vec![0x01, 2, 0, 0, 0, 2, 0, 0, 0, b'a', b'b', 0, 0, 0, 0]
        );
    }

    #[test]
    fn resolves_options_blob() {
        assert_eq!(
            resolve_options(&blob(&[
                pair("lower", "false"),
                pair("symbols", "false"),
                pair("length", "24"),
                pair("count", "5")
            ])),
            Some(Options {
                lower: false,
                symbols: false,
                length: 24,
                count: 5,
                ..Options::default()
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
        assert_eq!(resolve_options(&blob(&[pair("length", "")])), None);
        assert_eq!(resolve_options(&blob(&[pair("length", "2x")])), None);
        assert_eq!(resolve_options(&blob(&[pair("upper", "yes")])), None);

        // Overflowing usize is not a length either.
        assert_eq!(
            resolve_options(&blob(&[pair("length", "99999999999999999999999")])),
            None
        );
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(
            resolve_options(&blob(&[pair("future", "whatever"), pair("count", "3")])),
            Some(Options {
                count: 3,
                ..Options::default()
            })
        );
    }

    #[test]
    fn chacha_block_matches_rfc8439_test_vector() {
        // RFC 8439 section 2.3.2: block function on the zero key, counter 0.
        let key = [0u8; 32];
        let block = chacha20_block(&key, 0);

        let expect: [u8; 64] = [
            0x76, 0xb8, 0xe0, 0xad, 0xa0, 0xf1, 0x3d, 0x90, 0x40, 0x5d, 0x6a, 0xe5, 0x53, 0x86,
            0xbd, 0x28, 0xbd, 0xd2, 0x19, 0xb8, 0xa0, 0x8d, 0xed, 0x1a, 0xa8, 0x36, 0xef, 0xcc,
            0x8b, 0x77, 0x0d, 0xc7, 0xda, 0x41, 0x59, 0x7c, 0x51, 0x57, 0x48, 0x8d, 0x77, 0x24,
            0xe0, 0x3f, 0xb8, 0xd8, 0x4a, 0x37, 0x6a, 0x43, 0xb8, 0xf4, 0x15, 0x18, 0xa1, 0x1c,
            0xc3, 0x87, 0xb6, 0x69, 0xb2, 0xee, 0x65, 0x86,
        ];

        assert_eq!(block, expect);
    }
}
