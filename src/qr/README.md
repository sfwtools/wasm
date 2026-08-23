# qr

Create QR codes as SVG from UTF-8 text.

## Module

Built by `npm run build` into `dist/qr.wasm`. It exports `memory`,
`alloc`, `dealloc`, `create`, and `manifest` - a minimal raw-ABI module,
no envelope. Buffer packing and options-blob framing come from the repo's
shared `abi` crate.

The matrix encoding uses the pure-Rust `qrcode` crate with its rendering
features off; this core renders its own minimal SVG (white background, black
modules, integer coordinates, `shape-rendering="crispEdges"`) so the output
is one predictable shape. Dark modules in a row merge into single rects.

Reading QR images is deliberately out of scope for now: decoding is image
processing, a different problem than encoding.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`) so consumers
parse it once at load time and drive all exports generically.

## Usage

Write the UTF-8 text into `alloc`'d memory, call `create`, read the output,
`dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`; a
result of `0` means the input or options were invalid, or the text does not
fit a QR code at the chosen error correction level.

```
create(inputPtr, inputLen, optsPtr, optsLen) -> u64
```

Pass `optsPtr = 0, optsLen = 0` for defaults.

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

| Key      | Values            | Default | Description                                        |
| -------- | ----------------- | ------- | -------------------------------------------------- |
| `ecc`    | `L`,`M`,`Q`,`H`   | `M`     | Error correction level (~7/15/25/30% recoverable)  |
| `scale`  | column number     | `4`     | Output pixels per module (1-64)                    |
| `margin` | column number     | `4`     | Quiet zone around the code, in modules (0-32)      |

## Output

A standalone SVG document (`<?xml ...?>` header included), safe to save as
`.svg` and open anywhere:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 W H" width="W" height="H" shape-rendering="crispEdges">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <rect x="..." y="..." width="..." height="..."/>
</svg>
```

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
