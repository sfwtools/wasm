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
// util.js - the raw-ABI host helpers every test harness shares: serializing
// options to the wire blob, copying packed results out of linear memory,
// running an export end to end, and walking a string-array frame. Mirrors
// what the repo's abi crate does inside the modules.

// Serialize a plain object into the modules' options-blob wire format:
// 0x01 magic, then keyLen/key/valueLen/value pairs with little-endian u32
// lengths.
export const encodeOptions = (options) => {
  const encoder = new TextEncoder();
  const chunks = [Uint8Array.of(0x01)];

  for(const [key, value] of Object.entries(options)) {
    const keyBytes = encoder.encode(key);
    const valueBytes = encoder.encode(String(value));
    const chunk = new Uint8Array(4 + keyBytes.length + 4 + valueBytes.length);
    const view = new DataView(chunk.buffer);

    view.setUint32(0, keyBytes.length, true);
    chunk.set(keyBytes, 4);
    view.setUint32(4 + keyBytes.length, valueBytes.length, true);
    chunk.set(valueBytes, 8 + keyBytes.length);

    chunks.push(chunk);
  }

  const blob = new Uint8Array(chunks.reduce((sum, chunk) => sum + chunk.length, 0));
  let offset = 0;

  for(const chunk of chunks) {
    blob.set(chunk, offset);
    offset += chunk.length;
  }

  return blob;
};

// Copy an output out of linear memory before anything can move or free it.
// Returns { bytes, len, ptr } so the caller can dealloc afterwards.
export const unpack = (memory, packed) => {
  const outPtr = Number(packed >> 32n);
  const outLen = Number(packed & 0xFFFFFFFFn);

  return {
    bytes: new Uint8Array(memory.buffer, outPtr, outLen).slice(),
    len: outLen,
    ptr: outPtr
  };
};

// Call one export end to end: write input (+ options) into alloc'd memory,
// unpack the ptr<<32|len result, copy the output out, dealloc everything.
// Returns the output bytes, or null when the module rejected the call (0).
export const runExport = (exports, name, input, options) => {
  const optionsBlob = options ? encodeOptions(options) : new Uint8Array(0);
  const inPtr = exports.alloc(input.length);
  const optsPtr = optionsBlob.length ? exports.alloc(optionsBlob.length) : 0;

  new Uint8Array(exports.memory.buffer, inPtr, input.length).set(input);

  if(optionsBlob.length)
    new Uint8Array(exports.memory.buffer, optsPtr, optionsBlob.length).set(optionsBlob);

  const packed = exports[name](inPtr, input.length, optsPtr, optionsBlob.length);
  const result = packed === 0n ? null : unpack(exports.memory, packed);

  exports.dealloc(inPtr, input.length);

  if(optionsBlob.length)
    exports.dealloc(optsPtr, optionsBlob.length);

  if(result)
    exports.dealloc(result.ptr, result.len);

  return result ? result.bytes : null;
};

// Walk the string-array frame: 0x01 magic, count, then length-prefixed UTF-8
// entries (little-endian u32s throughout).
export const parseFrame = (frame) => {
  if(frame[0] !== 0x01)
    throw new Error('frame magic mismatch');

  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const count = view.getUint32(1, true);
  let offset = 5;
  const strings = [];

  for(let i = 0; i < count; i += 1) {
    const length = view.getUint32(offset, true);

    offset += 4;
    strings.push(new TextDecoder().decode(frame.subarray(offset, offset + length)));
    offset += length;
  }

  return strings;
};

// Walk the pixel frame produced by image.decode: little-endian u32 width and
// height, one channel byte (1 = luma, 4 = RGBA), then row-major samples.
export const parsePixels = (frame) => {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const width = view.getUint32(0, true);
  const height = view.getUint32(4, true);
  const channels = frame[8];
  const expected = width * height * channels;

  if(frame.byteLength !== 9 + expected || expected === 0)
    throw new Error('pixel frame length mismatch');

  return {
    channels,
    height,
    pixels: frame.subarray(9),
    width
  };
};
