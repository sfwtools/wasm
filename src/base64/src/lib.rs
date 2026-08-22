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

//! base64 — encode a string to base64. A minimal raw-ABI module: the caller
//! writes the input string into `alloc`'d linear memory, calls `encode`, reads
//! the base64 output from the returned pointer, then `dealloc`s both buffers.
//! No envelope, no host imports — just `memory`, `alloc`, `dealloc`, `encode`.

/// RFC 4648 standard base64 alphabet.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Pure core: base64-encode a byte slice. The `encode` export is a thin pointer
/// wrapper around this, so the logic is unit-testable without the ABI.
pub fn encode_bytes(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);

    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let triple = (b0 as u32) << 16 | (b1 as u32) << 8 | b2 as u32;

        out.push(ALPHABET[(triple >> 18) as usize & 63] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 63] as char
        } else {
            '='
        });
    }

    out
}

/// Allocate a write buffer of exactly `len` bytes. The caller passes the
/// pointer to `encode` and back to `dealloc` when done.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    let mut buf: Vec<u8> = Vec::with_capacity(len as usize);
    let ptr = buf.as_mut_ptr() as u32;

    std::mem::forget(buf);

    ptr
}

/// Free a buffer previously handed out by `alloc`.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    let buf = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);

    drop(buf);
}

/// Encode the input bytes at `ptr..ptr+len` as base64 and return the output
/// packed as `ptr << 32 | len`. The caller reads the output and deallocs both
/// buffers.
#[no_mangle]
pub unsafe extern "C" fn encode(ptr: u32, len: u32) -> u64 {
    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let output = encode_bytes(input).into_bytes();
    let boxed: Box<[u8]> = output.into_boxed_slice();
    let out_len = boxed.len();
    let mut boxed = boxed;
    let out_ptr = boxed.as_mut_ptr() as u32;

    std::mem::forget(boxed);

    ((out_ptr as u64) << 32) | (out_len as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_hello_world() {
        assert_eq!(encode_bytes(b"hello world"), "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn encodes_empty() {
        assert_eq!(encode_bytes(b""), "");
    }
}
