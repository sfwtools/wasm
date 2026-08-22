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

// Compiles every crate in src/ to dist/<id>.wasm, then shrinks each with the
// wasm-opt binary from binaryen (-Os, bulk-memory enabled: Rust's memcpy emits
// memory.copy/fill that the feature section doesn't declare). Cargo reads
// .cargo/config.toml from the working dir, so cargo runs with cwd = the crate.

// --- imports: Node built-ins + binaryen (ships the wasm-opt binary) ---------
import { execSync }                                                                from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync }               from 'node:fs';
import { homedir }                                                                 from 'node:os';
import { join }                                                                    from 'node:path';
import { fileURLToPath }                                                           from 'node:url';

// --- paths: resolved from the repo root (this file lives at the root) -------
const root = process.cwd();
const distDir = join(root, 'dist');
const srcDir = join(root, 'src');

// --- wasm-opt: the binary ships inside the binaryen npm package -------------
const wasmOpt = join(fileURLToPath(new URL('./node_modules/', import.meta.url)), 'binaryen', 'bin', 'wasm-opt');

// --- environment: prepend the user cargo bin so rustup-managed cargo is found
// regardless of PATH (mirrors how the main sfw.tools build resolves cargo).
const env = {
  ...process.env,
  PATH:join(homedir(), '.cargo/bin') + ':' + (process.env.PATH ?? '')
};

// --- cargo: locate the binary (rustup installs to ~/.cargo/bin) -------------
const cargo = (() => {
  // PATH name first, then the rustup home; the first that runs wins.
  const candidates = ['cargo', join(homedir(), '.cargo/bin/cargo')];

  for(const c of candidates) {
    try {
      execSync(c + ' --version', {
        env,
        stdio:'ignore'
      });

      return c;
    } catch { /* try next */ }
  }

  throw new Error('cargo not found. Install Rust via rustup.');
})();

// --- build loop: one crate per src/<id> dir -> one wasm in dist/<id>.wasm ---
let count = 0;

for(const dir of readdirSync(srcDir)) {
  const crateDir = join(srcDir, dir);
  const manifestPath = join(crateDir, 'Cargo.toml');

  // non-crate dirs (none today) are skipped
  if(!existsSync(manifestPath))
    continue;

  console.log(new Date().toISOString(), 'build.js', '[build] ' + dir);

  // Compile to wasm. cwd = the crate dir: cargo reads .cargo/config.toml
  // (which exports linear memory) from the working dir.
  execSync(cargo + ' build --release --target wasm32-unknown-unknown', {
    cwd:crateDir,
    env,
    stdio:'inherit'
  });

  // Read the crate name: it selects the built artifact file below.
  const manifest = readFileSync(manifestPath, 'utf8');
  const crateName = manifest.match(/^name\s*=\s*"([^"]+)"/m)?.[1];

  if(!crateName)
    throw new Error('no crate name in ' + manifestPath);

  // Cargo emits the cdylib under the snake_case crate name (kebab ids get
  // normalized: my-tool -> my_tool.wasm), so the artifact may be `name.wasm`
  // or `libname.wasm` depending on the linker's naming.
  const snake = crateName.replace(/-/g, '_');
  const releaseDir = join(crateDir, 'target', 'wasm32-unknown-unknown', 'release');
  const wasm = [join(releaseDir, snake + '.wasm'), join(releaseDir, 'lib' + snake + '.wasm')].find((f) => existsSync(f));

  if(!wasm)
    throw new Error('expected wasm in ' + releaseDir);

  // Copy the built module to dist/<id>.wasm (kebab-case id, matching the dir).
  mkdirSync(distDir, { recursive:true });
  cpSync(wasm, join(distDir, dir + '.wasm'));

  // Shrink with wasm-opt -Os. --enable-bulk-memory: Rust's memcpy emits
  // memory.copy/fill that the module's feature section doesn't declare.
  const distWasm = join(distDir, dir + '.wasm');
  execSync(wasmOpt + ' -Os --enable-bulk-memory ' + distWasm + ' -o ' + distWasm, {
    stdio:'inherit'
  });

  count++;
}

console.log(new Date().toISOString(), 'build.js', '[build] done (' + count + ' tools)');
