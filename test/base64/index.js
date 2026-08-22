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
// exports and checks the output. Exits nonzero on failure.

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

const main = async () => {
  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', 'wasm: ' + WASM);

  const response = await fetch(WASM);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const { memory, alloc, dealloc, encode } = instance.exports;

  const input = new TextEncoder().encode(INPUT);
  const ptr = alloc(input.length);

  new Uint8Array(memory.buffer, ptr, input.length).set(input);

  const packed = encode(ptr, input.length);
  const outPtr = Number(packed >> 32n);
  const outLen = Number(packed & 0xFFFFFFFFn);
  const output = new TextDecoder().decode(new Uint8Array(memory.buffer, outPtr, outLen));

  dealloc(ptr, input.length);
  dealloc(outPtr, outLen);

  const expected = Buffer.from(INPUT).toString('base64');

  if(output !== expected)
    throw new Error('encode mismatch: got "' + output + '", want "' + expected + '"');

  console.log(new Date().toISOString(), 'test/base64/index.js', 'main', '✅ ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/base64/index.js', err.message);
    console.error(new Date().toISOString(), 'test/base64/index.js', '❌ failed');

    process.exit(1);
  });
