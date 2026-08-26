# barcode

Encode text as common 1D barcodes, rendered as SVG or as an RGBA pixel frame
for the `image` module to turn into PNG.

## Module

Built by `npm run build` into `dist/barcode.wasm`. It exports `memory`,
`alloc`, `dealloc`, `encode`, and `manifest` - a minimal raw-ABI module, no
envelope. Buffer packing, options-blob framing, and the pixel frame come from
the repo's shared `abi` crate.

Encoding rides on the pure-Rust `barcoders` crate (2.0) with its generator
features (ascii/json/svg/image) off; this core renders its own minimal SVG
(white background, black bars, integer coordinates,
`shape-rendering="crispEdges"`) and RGBA frames from the module bits, exactly
like `qr` renders QR codes itself. Dark bars merge into horizontal runs so
typical codes emit a handful of rects instead of hundreds.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`), so consumers
parse it once at load time and drive all exports generically.

## Usage

Write the input bytes into `alloc`'d memory, call the export, read the
output, `dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`;
a result of `0` means the input or options were invalid.

```
encode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // SVG document or RGBA pixel frame
```

Pass `optsPtr = 0, optsLen = 0` for defaults. `encode` takes UTF-8 text and
returns a standalone SVG document, or - with `output=rgba` - an RGBA pixel
frame (the wire format `image.encode` takes) for PNG output.

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

| Key      | Values                                   | Default | Description                             |
| -------- | ---------------------------------------- | ------- | --------------------------------------- |
| `type`   | `code128`,`ean13`,`upca`,`ean8`,`code39`,`itf`,`codabar` | `code128` | Barcode symbology |
| `output` | `svg`,`rgba`                             | `svg`   | Render format                           |
| `scale`  | column number                            | `2`     | Output pixels per module (1-32)         |
| `height` | column number                            | `80`    | Bar height in pixels (8-512)            |
| `margin` | column number                            | `4`     | Quiet zone around the code, in modules (0-32) |

`code128` follows the `barcoders` convention of a leading character-set
marker: if the text does not start with one of `À` (set A, U+00C0), `Ɓ`
(set B, U+0181), or `Ć` (set C, U+0106), the module prepends `Ɓ` (set B, full
ASCII) automatically. `upca` is not a separate `barcoders` module; UPC-A is
encoded as EAN-13 with a leading zero, which is the identical bar pattern.

Each symbology validates its data (length, digits vs letters, checksum) and
rejects what it cannot encode with a friendly error: `ean13` takes 12 or 13
digits, `ean8` 7 or 8, `upca` 11 or 12, `itf` digits (odd lengths get a
checksum digit appended by the crate), `codabar` letters A-D as start/stop
with digits and `-/. :+$` in between.

## Output

`encode` produces a standalone SVG document (`<?xml ...?>` header included),
safe to save as `.svg` and open anywhere:

```xml
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 W H" width="W" height="H" shape-rendering="crispEdges">
  <rect width="100%" height="100%" fill="#ffffff"/>
  <rect x="..." y="0" width="..." height="80"/>
</svg>
```

With `output=rgba` it produces the pixel wire frame (little-endian `width:u32
height:u32 channels:u8`, then row-major samples, 4 channels RGBA), which the
`image` module's `encode` export turns into a PNG. Example chain for a PNG:

```
barcode.encode(text, { output:'rgba' }) -> frame
image.encode(frame)                      -> PNG bytes
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