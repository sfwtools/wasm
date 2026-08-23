# random

Generate random ASCII strings - sized for password generation - from
host-provided entropy.

## Module

Built by `npm run build` into `dist/random.wasm`. It exports `memory`,
`alloc`, `dealloc`, `generate`, and `manifest` - a minimal raw-ABI module,
no envelope. Buffer packing, options-blob framing, and the string-array
output frame come from the repo's shared `abi` crate.

The module imports NOTHING (the property that lets every host instantiate it
with an empty import list), and bare wasm has no entropy source, so the
CALLER supplies fresh CSPRNG bytes as the input: browser
`crypto.getRandomValues`, Node `crypto.randomBytes`, `/dev/urandom`. The core
folds the whole seed into a ChaCha20 key (RFC 8439 block function) and
stretches it into the requested characters with rejection sampling, so every
enabled character is equally likely. At least 32 seed bytes are required;
fewer fails the call instead of silently weakening output. Same seed means
same output - hosts must use a fresh seed per call.

## Manifest

The `manifest()` export returns the module's self-description as UTF-8 JSON,
packed like every other result (`ptr << 32 | len`). It maps each export to
its option schemas (`type`, `values`, `default`, `description`) so consumers
parse it once at load time and drive all exports generically. The
`generate.output` field (`"string-array"`) tells generic hosts how to render
the return frame.

## Usage

Write the seed into `alloc`'d memory, call `generate`, read the output,
`dealloc` every buffer. The packed `u64` result is `ptr << 32 | len`; a
result of `0` means the input or options were invalid.

```
generate(inputPtr, inputLen, optsPtr, optsLen) -> u64
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

| Key        | Values         | Default | Description                                        |
| ---------- | -------------- | ------- | -------------------------------------------------- |
| `upper`    | `true`,`false` | `true`  | Include uppercase letters A-Z                      |
| `lower`    | `true`,`false` | `true`  | Include lowercase letters a-z                      |
| `numbers`  | `true`,`false` | `true`  | Include digits 0-9                                 |
| `symbols`  | `true`,`false` | `true`  | Include printable ASCII punctuation                |
| `length`   | column number  | `16`    | Characters per string (1-1024)                     |
| `count`    | column number  | `1`     | How many strings to generate (1-1024)              |

`symbols` means every printable ASCII character that is not alphanumeric:
`! " # $ % & ' ( ) * + , - . / : ; < = > ? @ [ \ ] ^ _ \` { | } ~`.
Space, newline and other control characters never appear in output.
At least one class must stay enabled.

## Output frame (string-array)

```
frame := 0x01 count:u32 entry*
entry := len:u32 bytes[len]      // UTF-8, little-endian lengths
```

The magic byte marks the frame revision; the explicit count lets callers
validate the entry list before walking it.

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
