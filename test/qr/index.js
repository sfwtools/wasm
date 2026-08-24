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

// Runs the published image and qr modules (GitHub release) through the raw-ABI
// exports: SVG encoding across option combinations, then the double-call read
// path where image.decode hands a pixel frame to qr.decode. Exits nonzero on
// failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync }  from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { parseFrame, parsePixels, runExport, unpack }  from '../util.js';

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

  console.log(new Date().toISOString(), 'test/qr/index.js', 'loadModule', 'wasm: ' + url);

  const response = await fetch(url);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});

  return instance.exports;
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'version: ' + VERSION);

  const imageExports = await loadModule('image');
  const qrExports = await loadModule('qr');

  // First order of business: read both self-descriptions and show them.
  const readManifest = (exports) => {
    const result = unpack(exports.memory, exports.manifest());

    exports.dealloc(result.ptr, result.len);

    return JSON.parse(new TextDecoder().decode(result.bytes));
  };

  const imageManifest = readManifest(imageExports);
  const qrManifest = readManifest(qrExports);

  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'image manifest: ' + JSON.stringify(imageManifest));
  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', 'qr manifest: ' + JSON.stringify(qrManifest));

  expect(imageManifest.exports.decode && imageManifest.exports.decode.output === 'pixels',
    'image manifest does not mark decode as pixels output');
  expect(imageManifest.exports.decode.options.color.values.join(',') === 'luma,rgba',
    'image manifest color values mismatch');
  expect(qrManifest.exports.encode && qrManifest.exports.encode.options.ecc.default === 'M',
    'manifest does not describe the encode export');
  expect(qrManifest.exports.decode && qrManifest.exports.decode.output === 'string-array',
    'manifest does not mark decode as string-array output');

  // Defaults produce a standalone SVG whose canvas matches the geometry:
  // "sfw.tools" encodes as version 1 (21 modules), margin 4, scale 4 -> 116.
  const input = new TextEncoder().encode('sfw.tools');
  const svg = new TextDecoder().decode(runExport(qrExports, 'encode', input, undefined));

  expect(svg.startsWith('<?xml'), 'svg lacks an xml declaration');
  expect(svg.includes('<svg xmlns="http://www.w3.org/2000/svg"'), 'svg root is wrong');
  expect(svg.includes('viewBox="0 0 116 116"'), 'default viewBox wrong');
  expect(svg.includes('fill="#ffffff"'), 'svg lacks the background rect');
  expect(svg.endsWith('</svg>\n'), 'svg does not close cleanly');

  // Custom options resize the canvas and still close cleanly.
  const custom = new TextDecoder().decode(runExport(qrExports, 'encode',
    new TextEncoder().encode('hi'), {
      ecc:'H',
      margin:'0',
      scale:'2'
    }));

  // "hi" at H level also fits version 1: 21 modules * scale 2 = 42.
  expect(custom.includes('viewBox="0 0 42 42"'), 'custom options viewBox wrong');

  // Empty input is rejected rather than rendered as an empty code.
  expect(runExport(qrExports, 'encode', new TextEncoder().encode(''), undefined) === null,
    'empty input was not rejected');

  // Text that cannot fit any QR code is rejected cleanly.
  expect(runExport(qrExports, 'encode',
    new TextEncoder().encode('x'.repeat(70000)), undefined) === null,
    'oversized input was not rejected');

  // Invalid UTF-8 input is rejected rather than encoded loosely.
  expect(runExport(qrExports, 'encode', Uint8Array.of(0xFF, 0xFE), undefined) === null,
    'invalid utf-8 was not rejected');

  // Unknown options are ignored by design.
  const withUnknown = new TextDecoder().decode(
    runExport(qrExports, 'encode', input, { future:'whatever' }));

  expect(withUnknown === svg, 'unknown option changed the output');

  // The double-call read path: PNG bytes -> image.decode -> pixel frame ->
  // qr.decode -> payloads. The committed fixture is a clean screenshot-style
  // PNG of "https://sfw.tools/qr".
  const fixture = new Uint8Array(readFileSync(join(__dirname, 'sfw.tools.png')));
  const frame = runExport(imageExports, 'decode', fixture, undefined);

  expect(frame !== null, 'image.decode rejected the fixture');

  const decoded = parsePixels(frame);

  expect(decoded.channels === 1 && decoded.width > 0 && decoded.height > 0,
    'unexpected pixel frame shape');

  const payloads = parseFrame(runExport(qrExports, 'decode', frame, undefined));

  expect(payloads.length === 1 && payloads[0] === 'https://sfw.tools/qr',
    'fixture decode payload mismatch: ' + JSON.stringify(payloads));

  // qr.decode refuses RGBA frames: it only takes grayscale.
  const rgbaFrame = runExport(imageExports, 'decode', fixture, { color:'rgba' });

  expect(rgbaFrame !== null, 'rgba decode failed');
  expect(runExport(qrExports, 'decode', rgbaFrame, undefined) === null,
    'decode accepted a rgba frame');

  // Garbage is rejected at each stage instead of throwing or inventing data.
  expect(runExport(imageExports, 'decode', new TextEncoder().encode('not an image'), undefined) === null,
    'garbage input was not rejected');
  expect(runExport(qrExports, 'decode', new TextEncoder().encode('not a frame'), undefined) === null,
    'bad frame was not rejected');

  console.log(new Date().toISOString(), 'test/qr/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/qr/index.js', err.message);
    console.error(new Date().toISOString(), 'test/qr/index.js', '\u274c failed');

    process.exit(1);
  });
