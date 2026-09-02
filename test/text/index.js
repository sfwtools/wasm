// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Runs the published text module through the raw ABI and checks prose metrics.

import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { runExport, unpack } from '../util.js';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERSION = JSON.parse(readFileSync(join(__dirname, '..', '..', 'package.json'), 'utf8')).version;
const WASM = 'https://github.com/sfwtools/wasm/releases/download/v' + VERSION + '/text.wasm';

const main = async () => {
  console.log(new Date().toISOString(), 'test/text/index.js', 'main', 'version: ' + VERSION);
  console.log(new Date().toISOString(), 'test/text/index.js', 'main', 'wasm: ' + WASM);
  const response = await fetch(WASM);

  if(!response.ok)
    throw new Error('fetch failed: ' + response.status);

  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  const manifestResult = unpack(instance.exports.memory, instance.exports.manifest());
  console.log(new Date().toISOString(), 'test/text/index.js', 'main', 'manifest: ' + new TextDecoder().decode(manifestResult.bytes));
  instance.exports.dealloc(manifestResult.ptr, manifestResult.len);

  const input = new TextEncoder().encode('First sentence. Second sentence!\nStill here.\n\nNew paragraph?');
  const output = runExport(instance.exports, 'metrics', input, { reading_wpm: 200, speaking_wpm: 40 });

  if(!output)
    throw new Error('metrics rejected valid text');

  const metrics = JSON.parse(new TextDecoder().decode(output));

  if(metrics.paragraphs !== 2 || metrics.sentences !== 4 || metrics.words !== 8 || metrics.unique_words !== 7 || metrics.pages !== 0.016)
    throw new Error('unexpected metrics: ' + JSON.stringify(metrics));

  console.log(new Date().toISOString(), 'test/text/index.js', 'main', '\u2705 ok');
};

main().catch((error) => {
  console.error(new Date().toISOString(), 'test/text/index.js', error.message);
  console.error(new Date().toISOString(), 'test/text/index.js', '\u274c failed');
  process.exit(1);
});
