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

// Runs the published pdf module (GitHub release) through the raw-ABI exports -
// assemble across reorder / blank / rotate / merge combos - and checks the
// outputs. Exits nonzero on failure.

// --- imports: Node built-ins + the shared host helpers ----------------------
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { frameFiles, parseFiles, runExport, unpack } from '../util.js';

// --- module globals: CommonJS-style path helpers derived from import.meta ---
const __filename = fileURLToPath(import.meta.url);
const __dirname  = dirname(__filename);

// --- configuration: only what the operator tweaks --------------------------
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/pdf.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

// Parse a PDF far enough to count its pages and read each page's /Rotate.
// Kept intentionally small (no lopdf in the harness): a header regex for the
// page count and a rotation check is enough to verify assembly.
const inspectPdf = (bytes) => {
  const text = Buffer.from(bytes).toString('latin1');

  // Count /Type /Page (not /Pages) objects.
  const pages = (text.match(/\/Type\s*\/Page[^s]/g) || []).length;
  const rotates = [...text.matchAll(/\/Rotate\s+(\d+)/g)].map((m) => Number(m[1]));
  const mediaBoxes = [...text.matchAll(/\/MediaBox\s*\[\s*0\s+0\s+(\d+)\s+(\d+)\s*\]/g)]
    .map((m) => [Number(m[1]), Number(m[2])]);

  return { mediaBoxes, pages, rotates };
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/pdf/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/pdf/index.js', 'main', 'wasm: ' + WASM);

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

  console.log(new Date().toISOString(), 'test/pdf/index.js', 'main', 'manifest: ' + manifestText);

  const manifest = JSON.parse(manifestText);

  expect(manifest.exports && manifest.exports.assemble,
    'manifest does not describe the assemble export');

  // A tiny valid single-page PDF (base64): catalog + pages + one page with a
  // proper xref table, so lopdf's loader accepts it. Built once by hand with
  // the same generator the other harnesses use; kept inline so the test is
  // self-contained (no fixture files).
  const singlePage = Buffer.from(
    'JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFs2IDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjQgMCBvYmoKPDwgL1R5cGUgL0ZvbnQgL1N1YnR5cGUgL1R5cGUxIC9CYXNlRm9udCAvQ291cmllciA+PgplbmRvYmoKNSAwIG9iago8PCAvRm9udCA8PCAvRjEgNCAwIFIgPj4gPj4KZW5kb2JqCjYgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA2MTIgNzkyXSAvUmVzb3VyY2VzIDUgMCBSIC9Db250ZW50cyA3IDAgUiA+PgplbmRvYmoKNyAwIG9iago8PCAvTGVuZ3RoIDQyID4+CnN0cmVhbQpCVCAvRjEgMjQgVGYgNzIgNzIwIFRkIChoZWxsbyB3b3JsZCkgVGogRVQKZW5kc3RyZWFtCmVuZG9iagp4cmVmCjAgOAowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAxODMgMDAwMDAgbiAKMDAwMDAwMDIyNiAwMDAwMCBuIAowMDAwMDAwMzMwIDAwMDAwIG4gCnRyYWlsZXIKPDwgL1NpemUgOCAvUm9vdCAxIDAgUiA+PgpzdGFydHhyZWYKNDIyCiUlRU9GCg==',
    'base64'
  );

  const frame = frameFiles([{ name:'single.pdf', data:new Uint8Array(singlePage) }]);

  // Round-trip the frame through the JS parser first: cheap sanity that the
  // host and module agree on the wire format.
  expect(parseFiles(frame).length === 1, 'file frame round-trip mismatch');

  // A blank-only document: one empty Letter page.
  const blankOut = runExport(instance.exports, 'assemble', frame, { pages:'["blank"]' });
  expect(blankOut, 'blank-only assemble failed');
  expect(inspectPdf(blankOut).pages === 1, 'blank document page count mismatch');

  // An explicit blank page must preserve the requested dimensions.
  const customBlank = runExport(instance.exports, 'assemble', frame, { pages:'[["blank",1000,500]]' });
  expect(customBlank, 'custom blank assemble failed');
  expect(inspectPdf(customBlank).mediaBoxes.some(([width, height]) => width === 1000 && height === 500),
    'custom blank dimensions mismatch');

  // Reordering a two-page source is not possible with a single-page fixture,
  // so feed the same file twice (as two distinct inputs) and interleave.
  const twoFrame = frameFiles([
    { name:'a.pdf', data:new Uint8Array(singlePage) },
    { name:'b.pdf', data:new Uint8Array(singlePage) }
  ]);

  const merged = runExport(instance.exports, 'assemble', twoFrame, { pages:'[[1,0],[0,0]]' });
  expect(merged, 'cross-file assemble failed');
  expect(inspectPdf(merged).pages === 2, 'merged document page count mismatch');

  // Rotation: request a 90-degree turn and confirm /Rotate 90 in the output.
  const rotated = runExport(instance.exports, 'assemble', frame, { pages:'[[0,0,90]]' });
  expect(rotated, 'rotated assemble failed');
  expect(inspectPdf(rotated).rotates.includes(90), 'rotation not applied');

  // Errors are rejected with 0, never a partial document.
  expect(runExport(instance.exports, 'assemble', frame, { pages:'[[0,9]]' }) === null,
    'out-of-range page index was not rejected');
  expect(runExport(instance.exports, 'assemble', frame, { pages:'[[1,0]]' }) === null,
    'out-of-range file index was not rejected');
  expect(runExport(instance.exports, 'assemble', frame, { pages:'[[0,0,45]]' }) === null,
    'invalid rotation was not rejected');
  expect(runExport(instance.exports, 'assemble', frame, { pages:'[not json' }) === null,
    'malformed pages JSON was not rejected');
  expect(runExport(instance.exports, 'assemble', frame, { pages:'[["blank",0,500]]' }) === null,
    'invalid blank dimensions were not rejected');

  // Unknown options are ignored by design.
  const withUnknown = runExport(instance.exports, 'assemble', frame, { future:'whatever', pages:'[[0,0]]' });
  expect(withUnknown, 'unknown option broke assembly');

  // Save one assembled output for manual inspection (e.g. opening in a reader).
  writeFileSync(join(__dirname, 'out.pdf'), Buffer.from(withUnknown));

  console.log(new Date().toISOString(), 'test/pdf/index.js', 'main', '\u2705 ok');
};

main()
  .catch((err) => {
    console.error(new Date().toISOString(), 'test/pdf/index.js', err.message);
    console.error(new Date().toISOString(), 'test/pdf/index.js', '\u274c failed');

    process.exit(1);
  });
