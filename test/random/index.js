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

// Runs the published random module (GitHub release) through the raw-ABI
// exports - generation across the option combinations - and checks the
// outputs. Exits nonzero on failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { parseFrame, runExport, unpack }  from '../util.js';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/random.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/random/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/random/index.js', 'main', 'wasm: ' + WASM);

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

  console.log(new Date().toISOString(), 'test/random/index.js', 'main', 'manifest: ' + manifestText);

  const manifest = JSON.parse(manifestText);

  expect(manifest.exports && manifest.exports.generate,
    'manifest does not describe the generate export');
  expect(manifest.exports.generate.output === 'string-array',
    'manifest does not declare the string-array output');
  expect(manifest.exports.generate.options.length.default === 16,
    'manifest default length mismatch');

  // Defaults: one string, 16 printable ASCII characters.
  const seed = crypto.getRandomValues(new Uint8Array(32));
  const encoded = runExport(instance.exports, 'generate', seed, undefined);

  expect(encoded, 'generate returned an error');

  const strings = parseFrame(encoded);

  expect(strings.length === 1 && strings[0].length === 16,
    'defaults should give one 16-char string');
  expect(/^[\x21-\x7e]+$/.test(strings[0]), 'output left the printable ASCII range');

  // Same seed reproduces; a different seed differs.
  expect(parseFrame(runExport(instance.exports, 'generate', seed, undefined))[0] === strings[0],
    'same seed did not reproduce the output');
  expect(parseFrame(runExport(instance.exports, 'generate',
    crypto.getRandomValues(new Uint8Array(32)), undefined))[0] !== strings[0],
    'a fresh seed produced identical output');

  // Digits-only options are honored across shape and content.
  const digits = parseFrame(runExport(instance.exports, 'generate',
    crypto.getRandomValues(new Uint8Array(48)),
    { upper:false, lower:false, symbols:false, length:'24', count:'3' }));

  expect(digits.length === 3 && digits.every((s) => /^\d{24}$/.test(s)),
    'digits-only options broken: ' + JSON.stringify(digits));

  // A too-short seed must be rejected, never silently weakened.
  expect(runExport(instance.exports, 'generate', new Uint8Array(31), undefined) === null,
    'short seed was not rejected');

  // Unknown options are ignored by design.
  const withUnknown = parseFrame(runExport(instance.exports, 'generate', seed, { future:'whatever' }));

  expect(withUnknown.length === 1 && withUnknown[0] === strings[0],
    'unknown option broke generation');

  console.log(new Date().toISOString(), 'test/random/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/random/index.js', err.message);
    console.error(new Date().toISOString(), 'test/random/index.js', '\u274c failed');

    process.exit(1);
  });
