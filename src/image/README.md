# image

Decode PNG and JPEG images into raw pixel frames, and encode raw pixel frames
back into compressed PNG.

## Module

Built by `npm run build` into `dist/image.wasm`. It exports `memory`,
`alloc`, `dealloc`, `decode`, `encode`, and `manifest` - a minimal raw-ABI
module, no envelope. Buffer packing and pixel-frame framing come from the
repo's shared `abi` crate.

Decoding uses the `image` crate with only its PNG and JPEG codecs enabled -
screenshots and phone photos. No other formats are supported, to keep the
artifact small. Encoding turns a raw pixel frame (luma or RGBA) into a
compressed PNG: the PNG container is built directly on the repo's `fdeflate`
(deflate) and `crc32fast` (checksums) dependencies, with a per-row Sub/Up
filter heuristic, instead of going through the `png` crate's encoder - that
keeps `flate2`/`miniz_oxide` out of the artifact (png 0.17 declares flate2 as
a non-optional dependency, so any use of `png::Encoder` would link it). The
QR flow produces a PNG by chaining `qr.encode` (as RGBA frame) then
`image.encode`. Consumers such as `qr.decode` take the decode output from
there; hosts chain the calls (`image.decode` then `qr.decode`).

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`) and marks
`decode` with `"output": "pixels"`, so consumers parse it once at load time
and drive all exports generically.

## Usage

Write the input bytes into `alloc`'d memory, call the export, read the
output, `dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`;
a result of `0` means the input or options were invalid.

```
decode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // pixel frame
encode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // PNG bytes
```

Pass `optsPtr = 0, optsLen = 0` for defaults. Undecodable bytes reject the
call. `encode` takes the pixel wire frame (below) and returns compressed PNG
bytes; it accepts no options today, so the blob is only walked for framing
validation.

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
| `color`  | `luma`,`rgba`     | `luma`  | Sample layout of the returned pixels (`decode` only) |

`encode` takes no options; the frame's channel byte declares the layout.

## Output: the pixel frame

A raw, uncompressed sample buffer with a small header, little-endian
throughout:

```
frame  := width:u32 height:u32 channels:u8 samples
samples := width * height * channels bytes, row-major, origin top-left
```

`channels` is `1` for luma (one grayscale byte per pixel) or `4` for RGBA.
There is no padding or stride; rows are packed. The frame is exactly what
`qr.decode` consumes as its input, and what `encode` accepts to produce a
PNG.

## Output

`decode` returns the pixel frame described above. `encode` returns a
compressed PNG (deflate via `fdeflate`, per-row Sub/Up adaptive filtering),
suitable for saving as `.png` or serving directly.

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
