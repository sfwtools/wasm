# text

Measure document text with word, character, line, sentence, paragraph, time,
and page statistics.

## Module

Built by `npm run build` into `dist/text.wasm`. It exports `memory`, `alloc`,
`dealloc`, `metrics`, and `manifest`. The input is UTF-8 text and the output is
compact UTF-8 JSON. The module imports nothing at runtime.

## Usage

```text
metrics(inputPtr, inputLen, optsPtr, optsLen) -> u64
```

The options blob may contain `reading_wpm` and `speaking_wpm`, each an ASCII
decimal integer from 1 through 1000. Defaults are 200 and 40 respectively.
Unknown options are ignored. Invalid UTF-8, malformed options, invalid known
options, and inputs larger than 16 MiB are rejected with result `0`.

Example output:

```json
{"words":8,"unique_words":7,"characters":60,"characters_no_spaces":52,"lines":4,"non_empty_lines":3,"sentences":4,"paragraphs":2,"average_sentence_length":2,"average_paragraph_length":4,"reading_time_seconds":3,"writing_time_seconds":12,"pages":0.016}
```

Words are Unicode alphanumeric runs. An apostrophe or hyphen between two
alphanumeric characters remains inside the word. `unique_words` lowercases
those same tokens with Rust's standard-library conversion; accents are not
removed. `characters` and `characters_no_spaces` count Unicode scalar values,
not UTF-8 bytes or grapheme clusters. Whitespace uses Rust's Unicode
`is_whitespace` definition.

Lines are separated by `\n`. Empty input has zero lines; otherwise a trailing
newline creates a final empty line. `non_empty_lines` excludes lines containing
only whitespace. Paragraphs are non-empty blocks separated by one or more blank
lines; a normal line break does not end a paragraph.

Sentence counts treat runs of `.`, `!`, `?`, and their common CJK equivalents as
one boundary, but punctuation-only input has zero sentences. A terminator at
the end of input or before whitespace counts when content precedes it. A period
between two alphanumeric characters is treated as a decimal point, not a
boundary. Abbreviations are not language-detected and their periods count as
boundaries. Average lengths are words per counted sentence or paragraph.

Reading and speaking durations use words per minute and round positive results
up to the next whole second. The speaking duration is returned in the required
`writing_time_seconds` field. Pages use a fixed 500 words per page and are not
rounded.

## License

Copyright (C) 2026, Alex Morales
Copyright (C) 2026, sfw.tools sfwtools.com

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as published by
the Free Software Foundation, either version 3 of the License, or
at your option any later version.
