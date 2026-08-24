# image

Decode PNG and JPEG images into raw pixel frames.

## Module

Built by `npm run build` into `dist/image.wasm`. It exports `memory`,
`alloc`, `dealloc`, `decode`, and `manifest` - a minimal raw-ABI module,
no envelope. Buffer packing and pixel-frame framing come from the repo's
shared `abi` crate.

Decoding uses the `image` crate with only its PNG and JPEG codecs enabled -
screenshots and phone photos. No other formats are supported, to keep the
artifact small, and encoding is not enabled at all: this module only turns
files into pixels. Consumers such as `qr.decode` take it from there; hosts
chain the two calls (`image.decode` then `qr.decode`).

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`) and marks
`decode` with `"output": "pixels"`, so consumers parse it once at load time
and drive all exports generically.

## Usage

Write the input bytes into `alloc`'d memory, call `decode`, read the output,
`dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`; a
result of `0` means the input or options were invalid.

```
decode(inputPtr, inputLen, optsPtr, optsLen) -> u64   // pixel frame
```

Pass `optsPtr = 0, optsLen = 0` for defaults. Undecodable bytes reject the
call.

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
| `color`  | `luma`,`rgba`     | `luma`  | Sample layout of the returned pixels               |

## Output: the pixel frame

A raw, uncompressed sample buffer with a small header, little-endian
throughout:

```
frame  := width:u32 height:u32 channels:u8 samples
samples := width * height * channels bytes, row-major, origin top-left
```

`channels` is `1` for luma (one grayscale byte per pixel) or `4` for RGBA.
There is no padding or stride; rows are packed. The frame is exactly what
`qr.decode` consumes as its input.

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
