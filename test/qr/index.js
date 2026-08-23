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

// Runs the published qr module (GitHub release) through the raw-ABI exports -
// SVG creation across the option combinations - and checks the outputs.
// Exits nonzero on failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { runExport, unpack }  from '../util.js';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/qr.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'wasm: ' + WASM);

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

  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'manifest: ' + manifestText);

  const manifest = JSON.parse(manifestText);

  expect(manifest.exports && manifest.exports.create,
    'manifest does not describe the create export');
  expect(manifest.exports.create.options.ecc.values.join(',') === 'L,M,Q,H',
    'manifest ecc values mismatch');
  expect(manifest.exports.create.options.ecc.default === 'M',
    'manifest default ecc mismatch');

  // Defaults produce a standalone SVG whose canvas matches the geometry:
  // "sfw.tools" encodes as version 1 (21 modules), margin 4, scale 4 -> 116.
  const input = new TextEncoder().encode('sfw.tools');
  const svg = new TextDecoder().decode(runExport(instance.exports, 'create', input, undefined));

  expect(svg.startsWith('<?xml'), 'svg lacks an xml declaration');
  expect(svg.includes('<svg xmlns="http://www.w3.org/2000/svg"'), 'svg root is wrong');
  expect(svg.includes('viewBox="0 0 116 116"'), 'default viewBox wrong');
  expect(svg.includes('fill="#ffffff"'), 'svg lacks the background rect');
  expect(svg.endsWith('</svg>\n'), 'svg does not close cleanly');

  // Custom options resize the canvas and still close cleanly.
  const custom = new TextDecoder().decode(runExport(instance.exports, 'create',
    new TextEncoder().encode('hi'), { ecc:'H', scale:'2', margin:'0' }));

  // "hi" at H level also fits version 1: 21 modules * scale 2 = 42.
  expect(custom.includes('viewBox="0 0 42 42"'), 'custom options viewBox wrong');

  // Empty input is rejected rather than rendered as an empty code.
  expect(runExport(instance.exports, 'create', new TextEncoder().encode(''), undefined) === null,
    'empty input was not rejected');

  // Text that cannot fit any QR code is rejected cleanly.
  expect(runExport(instance.exports, 'create',
    new TextEncoder().encode('x'.repeat(70000)), undefined) === null,
    'oversized input was not rejected');

  // Invalid UTF-8 input is rejected rather than encoded loosely.
  expect(runExport(instance.exports, 'create', Uint8Array.of(0xFF, 0xFE), undefined) === null,
    'invalid utf-8 was not rejected');

  // Unknown options are ignored by design.
  const withUnknown = new TextDecoder().decode(
    runExport(instance.exports, 'create', input, { future:'whatever' }));

  expect(withUnknown === svg, 'unknown option changed the output');

  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/qr/index.js', err.message);
    console.error(new Date().toISOString(), 'test/qr/index.js', '\u274c failed');

    process.exit(1);
  });
