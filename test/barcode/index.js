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

// Runs the published barcode module (GitHub release) through the raw-ABI
// exports: SVG encoding across symbologies and option combinations, plus the
// RGBA frame path that the image module can turn into a PNG. Exits nonzero on
// failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { parsePixels, runExport, unpack } from '../util.js';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const RELEASE = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION;

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const loadModule = async (name) => {
  const url = RELEASE + '/' + name + '.wasm';

  console.log(new Date().toISOString(), 'test/barcode/index.js', 'loadModule', 'wasm: ' + url);

  const response = await fetch(url);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});

  return instance.exports;
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/barcode/index.js', 'main', 'version: ' + VERSION);

  const barcodeExports = await loadModule('barcode');

  // First order of business: read the self-description and show it.
  const readManifest = (exports) => {
    const result = unpack(exports.memory, exports.manifest());

    exports.dealloc(result.ptr, result.len);

    return JSON.parse(new TextDecoder().decode(result.bytes));
  };

  const manifest = readManifest(barcodeExports);

  console.log(new Date().toISOString(), 'test/barcode/index.js', 'main', 'manifest: ' + JSON.stringify(manifest));

  expect(manifest.exports.encode && manifest.exports.encode.options.type.default === 'code128',
    'manifest does not describe the encode export');
  expect(manifest.exports.encode.options.output.values.join(',') === 'svg,rgba',
    'encode output option values mismatch');

  // Defaults produce a standalone SVG whose geometry matches code128 of
  // "SFW.TOOLS": default scale 2, margin 4, height 80.
  const input = new TextEncoder().encode('SFW.TOOLS');
  const svg = new TextDecoder().decode(runExport(barcodeExports, 'encode', input, undefined));

  expect(svg.startsWith('<?xml'), 'svg lacks an xml declaration');
  expect(svg.includes('<svg xmlns="http://www.w3.org/2000/svg"'), 'svg root is wrong');
  expect(svg.includes('height="80"'), 'default height wrong');
  expect(svg.includes('fill="#ffffff"'), 'svg lacks the background rect');
  expect(svg.endsWith('</svg>\n'), 'svg does not close cleanly');
  expect(svg.matches('<rect').count > 3, 'expected bar rects');

  // Each symbology produces a valid SVG for data it can encode.
  const cases = [
    ['code128', 'SFW.TOOLS'],
    ['ean13', '5901234123457'],
    ['ean8', '9638507'],
    ['code39', 'SFW TOOLS'],
    ['itf', '1234567890'],
    ['codabar', 'A1234B']
  ];

  for(const [type, data] of cases) {
    const out = new TextDecoder().decode(runExport(barcodeExports, 'encode',
      new TextEncoder().encode(data), { type }));

    expect(out.startsWith('<?xml') && out.endsWith('</svg>\n'), type + ' svg broken');
    expect(out.includes('height="80"'), type + ' svg height wrong');
  }

  // UPC-A is not in barcoders 2.0.0; the module encodes it as EAN-13 with a
  // leading zero, so 12 digits must succeed.
  const upca = runExport(barcodeExports, 'encode', new TextEncoder().encode('03600029145'), { type:'upca' });

  expect(upca !== null, 'upca 12-digit encode rejected');

  // Invalid data for a symbology is rejected cleanly.
  expect(runExport(barcodeExports, 'encode', new TextEncoder().encode('ABC'),
    { type:'ean13' }) === null, 'ean13 accepted letters');
  expect(runExport(barcodeExports, 'encode', new TextEncoder().encode(''),
    undefined) === null, 'empty input was not rejected');
  expect(runExport(barcodeExports, 'encode', new TextEncoder().encode('x'),
    { type:'qrcode' }) === null, 'unknown symbology was not rejected');

  // Unknown options are ignored by design.
  const withUnknown = new TextDecoder().decode(
    runExport(barcodeExports, 'encode', input, { future:'whatever' }));

  expect(withUnknown === svg, 'unknown option changed the output');

  // The RGBA path produces a 4-channel frame with expected dimensions.
  const frame = runExport(barcodeExports, 'encode', input, { output:'rgba', scale:'1', margin:'0' });

  expect(frame !== null, 'rgba encode rejected');
  const decoded = parsePixels(frame);
  expect(decoded.channels === 4 && decoded.height === 80, 'rgba frame shape wrong: '
    + JSON.stringify({ width:decoded.width, height:decoded.height, channels:decoded.channels }));

  // Bad output values are rejected.
  expect(runExport(barcodeExports, 'encode', input, { output:'png' }) === null,
    'encode accepted an unknown output value');

  console.log(new Date().toISOString(), 'test/barcode/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/barcode/index.js', err.message);
    console.error(new Date().toISOString(), 'test/barcode/index.js', '\u274c failed');

    process.exit(1);
  });