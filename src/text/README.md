# text

Measure prose with word, sentence, and paragraph counts.

## Module

Built by `npm run build` into `dist/text.wasm`. It exports `memory`, `alloc`,
`dealloc`, `metrics`, and `manifest`. The input is UTF-8 text and the output is
compact UTF-8 JSON. The module imports nothing at runtime.

## Usage

```text
metrics(inputPtr, inputLen, optsPtr, optsLen) -> u64
```

The options blob must be empty. Invalid UTF-8, malformed options, non-empty
options, and inputs larger than 16 MiB are rejected with result `0`.

Example output:

```json
{"paragraphs":2,"sentences":4,"words":8}
```

Words are Unicode alphanumeric runs. An apostrophe or hyphen between two
alphanumeric characters remains inside the word. Sentence counts treat runs
of `.`, `!`, `?`, and their common CJK equivalents as one boundary. Paragraphs
are non-empty blocks separated by one or more blank lines; a normal line break
does not end a paragraph.

## License

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
at your option any later version.
