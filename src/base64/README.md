# base64

Encode a string to base64.

## Module

Built by `npm run build` into `dist/base64.wasm`. It exports `memory`,
`alloc`, `dealloc`, and `encode` — a minimal raw-ABI module, no envelope.

## Usage

Write the input string into `alloc`'d memory, call `encode`, read the output,
`dealloc` both buffers. The packed `u64` result is `ptr << 32 | len`.

## License

AGPL-3.0. Author: Alex Morales, sfw.tools, sfwtools.com.
