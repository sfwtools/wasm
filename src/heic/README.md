# heic

Decode HEIC and HEIF images into RGBA8 pixel frames.

## Module

Built by `npm run build` into `dist/heic.wasm`. It exports `memory`, `alloc`,
`dealloc`, `decode`, and `manifest`. The module uses the pure-Rust `heic`
decoder's pure-Rust backend with its `fallible-alloc` feature. It has no C
dependencies and imports nothing at runtime. The dependency's 0.1.x line
contains the pure-Rust backend directly; its later 0.2.0 release is yanked.

`decode` selects the first displayable image/frame, applies HEIF container
transforms, applies the decoder's supported display orientation, and returns
the shared RGBA8 pixel frame. HEIC/HEIF encoding is intentionally not provided.

## Limits

The module rejects empty or larger-than-256 MiB inputs, dimensions above
16,384 pixels, images above 64 million pixels, and estimated RGBA output above
512 MiB. Limits are checked before full-frame decoding. Malformed, unsupported,
encrypted, or otherwise undecodable input returns `0`.

## Output

The result is the shared pixel frame:

```
frame := width:u32 height:u32 channels:u8 samples
channels := 4
samples := width * height * 4 bytes, row-major, origin top-left
```

## License and patents

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
at your option any later version.

HEVC/H.265 and its use in HEIF may be covered by third-party patents. The
decoder dependency grants no patent rights; operators must assess applicable
licensing requirements for their jurisdiction and use.
