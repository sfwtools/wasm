# url

Percent-encode and decode URL text (RFC 3986).

## Module

Built by `npm run build` into `dist/url.wasm`. It exports `memory`, `alloc`,
`dealloc`, `encode`, `decode`, and `manifest` — a minimal raw-ABI module, no
envelope. Buffer packing and options-blob framing come from the repo's shared
`abi` crate.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to its
option schemas (`type`, `default`, `description`) so consumers parse it once
at load time and drive all exports generically — parameter names, types, legal
values, and defaults come from the manifest, not from caller code.

## Usage

Write the input into `alloc`'d memory, call `encode` or `decode`, read the
output, `dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`;
a result of `0` means the input or options were invalid.

Both exports take the same shape:

```
encode(inputPtr, inputLen, optsPtr, optsLen) -> u64
decode(inputPtr, inputLen, optsPtr, optsLen) -> u64
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

| Key     | Values          | Default | Applies to |
| ------- | --------------- | ------- | ---------- |
| `space` | `true`, `false` | `false` | encode     |
| `plus`  | `true`, `false` | `false` | decode     |

Encode keeps the RFC 3986 unreserved characters (`A-Z a-z 0-9 - _ . ~`)
untouched and percent-encodes every other UTF-8 byte as `%XX` (uppercase hex).
`space: true` switches to form-style encoding where spaces become `+` instead
of `%20` (application/x-www-form-urlencoded).

Decode reverses the escapes and accepts lowercase hex. A `%` not followed by
two hex digits, a cut-off escape at the end of the input, or a result that is
not valid UTF-8 text is an error. `plus: true` treats `+` as a space for
form-style payloads; by default a literal `+` decodes back as `+`.

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