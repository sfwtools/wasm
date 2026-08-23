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

// Runs the published base64 module (GitHub release) through the raw-ABI
// exports — encode/decode across the option combinations — and checks the
// outputs. Exits nonzero on failure.

// --- imports: Node built-ins only (no dependencies) ------------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const INPUT = 'sfw.tools';
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/base64.wasm';

// Serialize a plain object into the module's options-blob wire format:
// 0x01 magic, then keyLen/key/valueLen/value pairs with little-endian u32
// lengths (see src/base64/README.md).
const encodeOptions = (options) => {
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
const unpack = (memory, packed) => {
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
const runExport = (exports, name, input, options) => {
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

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', 'wasm: ' + WASM);

  const response = await fetch(WASM);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});

  // First order of business: read the module's self-description and show it.
  // The manifest is the debug ground truth for everything this test does next.
  const manifestResult = unpack(instance.exports.memory, instance.exports.manifest());
  const manifestText = new TextDecoder().decode(manifestResult.bytes);

  instance.exports.dealloc(manifestResult.ptr, manifestResult.len);

  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', 'manifest: ' + manifestText);

  const manifest = JSON.parse(manifestText);

  expect(manifest.id === 'base64', 'manifest id mismatch: ' + manifest.id);
  expect(manifest.exports && manifest.exports.encode && manifest.exports.decode,
    'manifest does not describe the encode/decode exports');
  expect(manifest.exports.encode.options.alphabet.default === 'standard',
    'manifest default alphabet mismatch');

  const expected = Buffer.from(INPUT).toString('base64');
  const input = new TextEncoder().encode(INPUT);

  // Encode with defaults matches the platform reference.
  const encoded = runExport(instance.exports, 'encode', input, undefined);
  expect(encoded, 'encode returned an error');
  expect(new TextDecoder().decode(encoded) === expected,
    'encode mismatch: got "' + new TextDecoder().decode(encoded) + '", want "' + expected + '"');

  // Decode round-trips the standard output.
  const decoded = runExport(instance.exports, 'decode', encoded, undefined);
  expect(decoded, 'decode returned an error');
  expect(new TextDecoder().decode(decoded) === INPUT, 'decode round-trip mismatch');

  // URL-safe alphabet emits neither '+' nor '/', and round-trips.
  const urlEncoded = runExport(instance.exports, 'encode', input, { alphabet:'url' });
  const urlText = new TextDecoder().decode(urlEncoded);
  expect(!/[+/]/.test(urlText), 'url-safe output contains standard-only symbols: "' + urlText + '"');
  expect(new TextDecoder().decode(runExport(instance.exports, 'decode', urlEncoded, { alphabet:'url' })) === INPUT,
    'url-safe round-trip mismatch');

  // Unpadded output carries no '=' and still decodes.
  const bare = runExport(instance.exports, 'encode', input, { padding:false });
  expect(!bare.includes(0x3D), 'unpadded output contains "="');
  expect(new TextDecoder().decode(runExport(instance.exports, 'decode', bare, undefined)) === INPUT,
    'unpadded round-trip mismatch');

  // Wrapped output splits at the column and decode ignores the newlines.
  const wrapped = runExport(instance.exports, 'encode', input, { wrap:4 });
  const wrappedText = new TextDecoder().decode(wrapped);
  expect(wrappedText.split('\n').every((line) => line.length <= 4),
    'wrap produced a longer line: "' + wrappedText + '"');
  expect(wrappedText.replace(/\n/g, '') === expected, 'wrapped output changed the payload');
  expect(new TextDecoder().decode(runExport(instance.exports, 'decode', wrapped, undefined)) === INPUT,
    'wrapped round-trip mismatch');

  // Invalid base64 is rejected with 0, not garbage.
  expect(runExport(instance.exports, 'decode', new TextEncoder().encode('definitely * not * base64'), undefined) === null,
    'invalid input did not produce an error result');

  // Unknown options are ignored by design.
  const withUnknown = runExport(instance.exports, 'encode', input, { future:'whatever' });
  expect(withUnknown && new TextDecoder().decode(withUnknown) === expected,
    'unknown option broke encoding');

  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', '✅ ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/base64/index.js', err.message);
    console.error(new Date().toISOString(), 'test/base64/index.js', '❌ failed');

    process.exit(1);
  });
