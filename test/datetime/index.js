// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Runs the published datetime module through the raw ABI and checks calendar
// arithmetic, elapsed differences, and calendar information.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { runExport } from '../util.js';

const directory = dirname(fileURLToPath(import.meta.url));
const version = JSON.parse(readFileSync(join(directory, '..', '..', 'package.json'), 'utf8')).version;
const url = 'https://github.com/sfwtools/wasm/releases/download/v' + version + '/datetime.wasm';

const expect = (condition, message) => {
  if(!condition)
    throw new Error(message);
};

const call = (exports, operation, options) => {
  const output = runExport(exports, operation, new Uint8Array(0), options);

  if(output === null)
    throw new Error(operation + ' returned an error');

  return JSON.parse(new TextDecoder().decode(output));
};

const main = async () => {
  console.log(new Date().toISOString(), 'test/datetime/index.js', 'main', 'version: ' + version);
  console.log(new Date().toISOString(), 'test/datetime/index.js', 'main', 'wasm: ' + url);
  const response = await fetch(url);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const bytes = await response.arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  expect(call(instance.exports, 'add', { date: '2026-03-09', unit: 'months', amount: '1' }).date === '2026-04-09', 'month addition mismatch');
  expect(call(instance.exports, 'add', { date: '2026-01-31', unit: 'months', amount: '1' }).date === '2026-02-28', 'month-end clamp mismatch');
  expect(call(instance.exports, 'difference', { start: '2026-03-09', end: '2026-04-09' }).calendar_months === 1, 'calendar difference mismatch');
  expect(call(instance.exports, 'difference', { start: '2026-03-09T09:00:00Z', end: '2026-03-09T12:30:00Z' }).elapsed_minutes === 210, 'elapsed difference mismatch');
  expect(call(instance.exports, 'calendar_info', { date: '2024-02-29' }).is_leap_year === true, 'leap-year information mismatch');
  console.log(new Date().toISOString(), 'test/datetime/index.js', 'main', '\u2705 ok');
};

main().catch((error) => {
  console.error(new Date().toISOString(), 'test/datetime/index.js', error.message);
  console.error(new Date().toISOString(), 'test/datetime/index.js', '\u274c failed');
  process.exit(1);
});
