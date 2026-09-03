// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Runs the published convert module through the raw ABI and checks conversions.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { runExport } from '../util.js';

const directory = dirname(fileURLToPath(import.meta.url));
const version = JSON.parse(readFileSync(join(directory, '..', '..', 'package.json'), 'utf8')).version;
const url = 'https://github.com/sfwtools/wasm/releases/download/v' + version + '/convert.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/convert/index.js', 'main', 'version: ' + version);
  console.log(new Date().toISOString(), 'test/convert/index.js', 'main', 'wasm: ' + url);
  const response = await fetch(url);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const result = runExport(instance.exports, 'convert', new Uint8Array(0), {
    value: '10',
    from: 'Meters (m)',
    to: 'Feet (ft)',
  });
  const output = JSON.parse(new TextDecoder().decode(result));

  expect(output.unit === 'Feet (ft)', 'wrong output unit');
  expect(Math.abs(output.value - 32.80839895) < 0.000001, 'wrong length conversion');
  expect(runExport(instance.exports, 'convert', new Uint8Array(0), {
    value: '1',
    from: 'Meters (m)',
    to: 'Kilograms (kg)',
  }) === null, 'incompatible units were accepted');
  console.log(new Date().toISOString(), 'test/convert/index.js', 'main', '\u2705 ok');
};

main().catch((error) => {
  console.error(new Date().toISOString(), 'test/convert/index.js', error.message);
  console.error(new Date().toISOString(), 'test/convert/index.js', '\u274c failed');
  process.exit(1);
});
