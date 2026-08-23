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

//! abi - the raw-ABI transport layer every wasm tool module shares.
//!
//! This crate carries everything on the wire that is NOT tool logic: packing
//! results as `ptr << 32 | len`, handing out and reclaiming linear-memory
//! buffers, parsing the options blob, and building the string-array output
//! frame. Tool crates link it as a path dependency; each artifact still
//! compiles into its own self-contained `.wasm`.
//!
//! The exported symbols (`alloc`, `dealloc`, `manifest`) deliberately stay as
//! small `#[no_mangle]` shims inside every module rather than being defined
//! here: an export defined only in a dependency is never called internally,
//! so the wasm linker may garbage-collect it under LTO. A local shim makes
//! retention - and each module's public surface - explicit.

/// Magic byte every non-empty options blob must start with, so a format
/// revision can be detected instead of silently mis-parsed.
pub const OPTIONS_MAGIC: u8 = 0x01;

/// Magic byte of the string-array output frame, matching the options-blob
/// convention so both directions of the wire carry a revision marker.
pub const OUTPUT_MAGIC: u8 = 0x01;

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// returned pointer to a tool export and back to `free_buf` when done.
pub fn alloc_buf(len: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr() as u32;

    std::mem::forget(buf);

    ptr
}

/// Free a buffer previously handed out by `alloc_buf`.
///
/// # Safety
/// `ptr`/`len` must come from `alloc_buf` and must not have been freed before.
pub unsafe fn free_buf(ptr: u32, len: u32) {
    let buf = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);

    drop(buf);
}

/// Pack an owned byte vector as the exported `ptr << 32 | len` result,
/// leaking an exact-length allocation whose layout matches what `dealloc`
/// reconstructs.
pub fn pack(bytes: Vec<u8>) -> u64 {
    let boxed: Box<[u8]> = bytes.into_boxed_slice();
    let len = boxed.len();
    let mut boxed = boxed;
    let ptr = boxed.as_mut_ptr() as u64;

    std::mem::forget(boxed);

    ptr << 32 | len as u64
}

/// Parse one little-endian `u32` off the front of `blob`, returning it with
/// the remaining slice.
fn read_u32(blob: &[u8]) -> Option<(u32, &[u8])> {
    if blob.len() < 4 {
        return None;
    }

    Some((u32::from_le_bytes(blob[..4].try_into().ok()?), &blob[4..]))
}

/// ASCII-decimal `usize` over raw bytes - replaces `str::parse`, which drags
/// integer-parsing error machinery into the modules. Overflow or a non-digit
/// is simply no value.
pub fn parse_usize(bytes: &[u8]) -> Option<usize> {
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

/// Validate and split an options blob into its key/value pairs. An empty
/// blob means "no options" and yields no pairs; a non-empty one must start
/// with the magic byte. Any truncation or malformed length invalidates the
/// WHOLE blob (returning `None`) so callers never see half-parsed state:
/// known keys with bad values stay the caller's call to reject, but framing
/// errors are decided here, once, for every module.
pub fn option_pairs(blob: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    if blob.is_empty() {
        return Some(Vec::new());
    }

    if blob[0] != OPTIONS_MAGIC {
        return None;
    }

    let mut rest = &blob[1..];
    let mut pairs = Vec::new();

    while !rest.is_empty() {
        let (key_len, after_key_len) = read_u32(rest)?;
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

        pairs.push((key, value));
        rest = tail;
    }

    Some(pairs)
}

/// Serialize strings into the string-array wire frame: magic byte, count,
/// then length-prefixed UTF-8 entries (little-endian throughout). This is the
/// standard shape for exports returning multiple text values; manifests mark
/// such exports with `"output": "string-array"` so generic hosts can render
/// them.
pub fn frame_strings(strings: &[String]) -> Vec<u8> {
    let mut blob = Vec::new();
    blob.push(OUTPUT_MAGIC);
    blob.extend_from_slice(&(strings.len() as u32).to_le_bytes());

    for s in strings {
        blob.extend_from_slice(&(s.len() as u32).to_le_bytes());
        blob.extend_from_slice(s.as_bytes());
    }

    blob
}
