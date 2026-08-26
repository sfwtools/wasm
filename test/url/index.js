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

// Runs the published url module (GitHub release) through the raw-ABI exports -
// percent-encode/decode across the option combinations - and checks the
// outputs. Exits nonzero on failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { runExport, unpack }  from '../util.js';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const INPUT = 'café + naïve / 100% safe';
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/url.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/url/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/url/index.js', 'main', 'wasm: ' + WASM);

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

  console.log(new Date().toISOString(), 'test/url/index.js', 'main', 'manifest: ' + manifestText);

  const manifest = JSON.parse(manifestText);

  expect(manifest.exports && manifest.exports.encode && manifest.exports.decode,
    'manifest does not describe the encode/decode exports');

  const input = new TextEncoder().encode(INPUT);

  // Encode with defaults matches a reference: unreserved pass through,
  // everything else percent-encoded, space as %20, uppercase hex.
  const encoded = runExport(instance.exports, 'encode', input, undefined);
  const encodedText = new TextDecoder().decode(encoded);
  const expectedEncoded = 'caf%C3%A9%20%2B%20na%C3%AFve%20%2F%20100%25%20safe';

  expect(encoded, 'encode returned an error');
  expect(encodedText === expectedEncoded,
    'encode mismatch: got "' + encodedText + '", want "' + expectedEncoded + '"');

  // Encode keeps unreserved characters untouched and hex is uppercase.
  expect(/%[a-f]/.test(encodedText) === false, 'encode produced lowercase hex');

  // Decode round-trips the default output.
  const decoded = runExport(instance.exports, 'decode', encoded, undefined);
  expect(decoded, 'decode returned an error');
  expect(new TextDecoder().decode(decoded) === INPUT, 'decode round-trip mismatch');

  // Form-style encode: space as +. And its matching decode (plus -> space).
  const formEncoded = runExport(instance.exports, 'encode', input, { space:true });
  const formText = new TextDecoder().decode(formEncoded);
  expect(formText.includes('+') && !formText.includes('%20'),
    'space:true did not encode spaces as plus: "' + formText + '"');
  expect(new TextDecoder().decode(runExport(instance.exports, 'decode', formEncoded, { plus:true })) === INPUT,
    'form-style round-trip mismatch');

  // Without plus:true, a literal + in the input decodes back as +, not space.
  const literalPlus = runExport(instance.exports, 'decode', new TextEncoder().encode('a+b'), undefined);
  expect(new TextDecoder().decode(literalPlus) === 'a+b', 'default decode should keep + literal');

  // Lowercase hex decodes too.
  const lowerHex = runExport(instance.exports, 'decode', new TextEncoder().encode('caf%C3%a9'), undefined);
  expect(new TextDecoder().decode(lowerHex) === 'café', 'lowercase hex decode mismatch');

  // Invalid percent-escapes are rejected with 0, not garbage.
  expect(runExport(instance.exports, 'decode', new TextEncoder().encode('bad %zz escape'), undefined) === null,
    'invalid escape did not produce an error result');
  expect(runExport(instance.exports, 'decode', new TextEncoder().encode('cut off %2'), undefined) === null,
    'cut-off escape did not produce an error result');

  // Decoding bytes that are not valid UTF-8 text is rejected.
  expect(runExport(instance.exports, 'decode', new TextEncoder().encode('%FF'), undefined) === null,
    'invalid UTF-8 did not produce an error result');

  // Unknown options are ignored by design.
  const withUnknown = runExport(instance.exports, 'encode', input, { future:'whatever' });
  expect(withUnknown && new TextDecoder().decode(withUnknown) === expectedEncoded,
    'unknown option broke encoding');

  console.log(new Date().toISOString(), 'test/url/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/url/index.js', err.message);
    console.error(new Date().toISOString(), 'test/url/index.js', '\u274c failed');

    process.exit(1);
  });