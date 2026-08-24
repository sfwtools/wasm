# qr

Encode UTF-8 text as a QR code SVG or RGBA pixel frame, read QR codes back
from images.

## Module

Built by `npm run build` into `dist/qr.wasm`. It exports `memory`,
`alloc`, `dealloc`, `encode`, `decode`, and `manifest` - a minimal raw-ABI
module, no envelope. Buffer packing, options-blob framing, the pixel-frame
parser, and the string-array output frame come from the repo's shared `abi`
crate.

Encoding uses the pure-Rust `qrcode` crate with its rendering features off;
this core renders its own minimal SVG (white background, black modules,
integer coordinates, `shape-rendering="crispEdges"`) so the output is one
predictable shape. Dark modules in a row merge into single rects. With the
`output=rgba` option it instead returns an RGBA pixel frame (white
background, black modules, each module `scale` pixels, wrapped in the quiet
zone) that the `image` module encodes straight to compressed PNG - so PNG
output is a two-call chain (`qr.encode` then `image.encode`) rather than a
feature inside qr.

Decoding uses `rqrr` (finder-pattern detection, perspective handling, ECC)
over a raw luma pixel frame. File-format decoding lives in the separate
`image` module: hosts chain the calls (`image.decode` then `qr.decode`). The
name pairs with `encode` like base64 does; it deliberately is not `read`,
because a `#[no_mangle] extern "C"` symbol named `read` interposes
libSystem's own `read(2)` on Darwin and segfaults hosts at their first
stdout write - avoid libc-reserved names for exports everywhere. Every QR
code found in the frame contributes one payload; codes damaged beyond ECC
are skipped rather than failing the whole frame.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`) and marks
`decode` with `"output": "string-array"`, so consumers parse it once at load
time and drive all exports generically.

## Usage

Write the input bytes into `alloc`'d memory, call the export, read the
output, `dealloc` every buffer. The packed `u64` result is
`ptr << 32 | len`; a result of `0` means the input or options were invalid.

```
encode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // SVG document or RGBA pixel frame
decode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // string-array frame
```

Pass `optsPtr = 0, optsLen = 0` for defaults. `encode` takes UTF-8 text and
returns a standalone SVG document, or - with `output=rgba` - an RGBA pixel
frame (the wire format `image.encode` takes). `decode` takes the pixel frame
produced by `image.decode` (`width:u32 height:u32 channels:u8 samples`,
little-endian, row-major, luma only) and returns the string-array frame
(magic byte, count, length-prefixed UTF-8 entries), one entry per QR code
found in the frame. A malformed frame or one without any readable code
rejects the call.

## Options blob

A flat length-prefixed key/value list, little-endian throughout:

```
blob  := 0x01 pair*
pair  := keyLen:u32 keyBytes valueLen:u32 valueBytes   // UTF-8 keys and values
```

The leading `0x01` magic byte identifies format revision 1; an empty blob
(0/0) means defaults. Unknown keys are ignored so new callers keep working
with older cores; a known key with a bad value fails the call (result `0`)
rather than being silently dropped. `decode` takes no options today; its
blob is walked only for framing validation so a future option can appear
without breaking callers. `decode` takes no options today.

| Key      | Values            | Default | Description                                        |
| -------- | ----------------- | ------- | -------------------------------------------------- |
| `ecc`    | `L`,`M`,`Q`,`H`   | `M`     | Error correction level (~7/15/25/30% recoverable)  |
| `scale`  | column number     | `4`     | Output pixels per module (1-64)                    |
| `margin` | column number     | `4`     | Quiet zone around the code, in modules (0-32)      |
| `output` | `svg`,`rgba`      | `svg`   | Render format: SVG document, or RGBA pixel frame   |

## Output

`encode` produces a standalone SVG document (`<?xml ...?>` header included),
safe to save as `.svg` and open anywhere:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 W H" width="W" height="H" shape-rendering="crispEdges">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <rect x="..." y="..." width="..." height="..."/>
</svg>
```

With `output=rgba` it produces the pixel wire frame (little-endian `width:u32
height:u32 channels:u8`, then row-major samples, 4 channels RGBA), which the
`image` module's `encode` export turns into a compressed PNG. Example chain
for a PNG:

```
qr.encode(text, { output:'rgba' }) -> frame
image.encode(frame)                -> PNG bytes
```

`decode` produces a string-array frame of payloads in reading order, e.g.
one code in the frame yields exactly one entry.

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
