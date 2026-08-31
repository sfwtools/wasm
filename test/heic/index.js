// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Runs the published heic module through the raw ABI. Local fixtures cover
// HEIF transforms and EXIF; the sequence fixture is fetched only at test time
// because the upstream Nokia corpus has no explicit redistribution license.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { parsePixels, runExport, unpack } from '../util.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/heic.wasm';
const SEQUENCE = 'https://raw.githubusercontent.com/nokiatech/heif_conformance/master/conformance_files/C026.heic';

const main = async () => {
  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', 'wasm: ' + WASM);
  const response = await fetch(WASM);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  const manifestResult = unpack(instance.exports.memory, instance.exports.manifest());
  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', 'manifest: ' + new TextDecoder().decode(manifestResult.bytes));
  instance.exports.dealloc(manifestResult.ptr, manifestResult.len);

  const fixture = readFileSync(join(__dirname, 'fixtures', 'orientation.heic'));
  const frame = runExport(instance.exports, 'decode', fixture);

  if(!frame)
    throw new Error('fixture decode failed');

  const pixels = parsePixels(frame);

  if(pixels.channels !== 4 || pixels.width !== 40 || pixels.height !== 64)
    throw new Error('unexpected RGBA frame');

  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', 'decoded: ' + pixels.width + 'x' + pixels.height + ' RGBA');
  const exif = runExport(instance.exports, 'decode', readFileSync(join(__dirname, 'fixtures', 'exif.heic')));

  if(!exif || parsePixels(exif).channels !== 4)
    throw new Error('EXIF fixture decode failed');

  const sequenceResponse = await fetch(SEQUENCE);

  if(!sequenceResponse.ok)
    throw new Error('sequence fixture fetch failed: ' + sequenceResponse.status);

  const sequence = runExport(instance.exports, 'decode', new Uint8Array(await sequenceResponse.arrayBuffer()));

  if(!sequence)
    throw new Error('sequence fixture decode failed');

  const firstFrame = parsePixels(sequence);

  if(firstFrame.channels !== 4 || firstFrame.width !== 1280 || firstFrame.height !== 720)
    throw new Error('unexpected first-frame dimensions');

  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', 'first frame: ' + firstFrame.width + 'x' + firstFrame.height + ' RGBA');
  console.log(new Date().toISOString(), 'test/heic/index.js', 'main', '\u2705 ok');
};

main().catch((error) => {
  console.error(new Date().toISOString(), 'test/heic/index.js', error.message);
  console.error(new Date().toISOString(), 'test/heic/index.js', '\u274c failed');
  process.exit(1);
});
