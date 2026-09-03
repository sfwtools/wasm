# sfw.tools sfwtools.com

Standalone WebAssembly modules.

Built by `npm run build` into `dist/<id>.wasm`.

| Tool | Name | Description |
| --- | --- | --- |
| `base64` | base64 | Encode/decode base64. |
| `random` | random | Generate random strings. |
| `qr` | qr | QR codes: SVG or RGBA frames, decode from luma frames. |
| `image` | image | Decode 11 image formats to pixel frames; encode 8 formats. |
| `barcode` | barcode | 1D barcodes: SVG or RGBA frames (encode only). |
| `url` | url | Percent-encode/decode URL text. |
| `pdf` | pdf | Assemble a PDF from pages of the inputs (reorder/delete/merge/rotate/blank). |
| `heic` | heic | Decode the first HEIC/HEIF image to an RGBA pixel frame. |
| `text` | text | Measure document counts, estimates, and approximate token usage. |

## Testing

Each tool has a test in `test/<id>/index.js` that fetches the published release
of its module from GitHub (the version comes from `package.json`) and checks the
output through the raw-ABI exports. Run a single tool's test with:

```bash
node test/base64/index.js
```

The test exits nonzero on failure.

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
