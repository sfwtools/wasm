# base64

Encode a string to base64.

## Module

Built by `npm run build` into `dist/base64.wasm`. It exports `memory`,
`alloc`, `dealloc`, and `encode` — a minimal raw-ABI module, no envelope.

## Usage

Write the input string into `alloc`'d memory, call `encode`, read the output,
`dealloc` both buffers. The packed `u64` result is `ptr << 32 | len`.

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
