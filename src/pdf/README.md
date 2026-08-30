# pdf

Assemble a new PDF from selected pages of the input PDFs — reorder, delete,
merge, rotate, and insert blank pages.

## Module

Built by `npm run build` into `dist/pdf.wasm`. It exports `memory`, `alloc`,
`dealloc`, `assemble`, and `manifest` — a minimal raw-ABI module, no envelope.
Buffer packing and options-blob framing come from the repo's shared `abi`
crate; the page-tree rebuild uses lopdf 0.44 (pure Rust, no C).

The module imports nothing (`getrandom_backend="unsupported"` in
`.cargo/config.toml` keeps lopdf's encryption deps import-free on wasm — see
the comment there). Encrypted PDFs cannot be loaded; everything else assembles
fine.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to its
option schemas so consumers parse it once at load time and drive all exports
generically — parameter names, types, legal values, and defaults come from the
manifest, not from caller code.

## Usage

Write the file-input frame into `alloc`'d memory, call `assemble`, read the
output PDF, `dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`;
a result of `0` means the input or options were invalid.

```
assemble(inputPtr, inputLen, optsPtr, optsLen) -> u64
```

Pass `optsPtr = 0, optsLen = 0` for an empty page selection (an error).

## File-input frame

The input is one or more PDFs packed into a single buffer (the same frame the
`abi` crate's `frame_files`/`parse_files` describe):

```
frame := 0x01 count:u32 (nameLen:u32 name dataLen:u32 data)*
```

Each entry is a named file; `name` is used only for human-readable errors. An
empty payload is rejected. The host serializes the frame with the shared
helpers in `test/util.js` (`frameFiles`), and the module parses it with
`parse_files`.

## Options blob

A flat length-prefixed key/value list, little-endian throughout:

```
blob  := 0x01 pair*
pair  := keyLen:u32 keyBytes valueLen:u32 valueBytes   // UTF-8 keys and values
```

The leading `0x01` magic byte identifies format revision 1; an empty blob
(0/0) means defaults. Unknown keys are ignored so new callers keep working
with older cores; a known key with a bad value fails the call (result `0`)
rather than being silently dropped.

| Key     | Values        | Default | Applies to |
| ------- | ------------- | ------- | ---------- |
| `pages` | JSON array    | `[]`    | assemble   |

The `pages` option is a strict JSON array of page entries in output order.
Each entry is one of:

- `[file, page]` — the page at 0-based `page` from the 0-based `file` input.
- `[file, page, rotate]` — same, with `rotate` in `0`/`90`/`180`/`270`
  (clockwise, added to the page's existing rotation).
- `"blank"` — an empty Letter page (612 x 792 pt).

Examples:

- Reorder `[[0,2],[0,0],[0,1]]`
- Delete pages by omitting them `[[0,0],[0,2]]`
- Merge two files `[[0,0],[0,1],[1,0],[1,2]]`
- Insert a blank separator `[[0,0],"blank",[0,1]]`
- Rotate a page `[[0,1,90]]`

Out-of-range file/page indices, a bad rotation, malformed JSON, or an empty
selection are all rejected (result `0`); a request never produces a partial
document. Page order follows the selection exactly. Each source PDF's own
MediaBox and resources are preserved; outlines are dropped (their cross-doc
references cannot survive a rebuild).

## License

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <http://www.gnu.org/licenses/>.