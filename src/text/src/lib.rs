// Copyright (C) 2026, Alex Morales
// Copyright (C) 2026, sfw.tools sfwtools.com
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

//! text - measure prose with word, sentence, and paragraph counts.
//!
//! The module accepts UTF-8 text and returns a small JSON object through the
//! shared raw ABI. It has no dependencies beyond the shared transport crate.

use abi::option_pairs;

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;

/// The module's self-description as UTF-8 JSON.
const MANIFEST: &str = r#"{
  "exports": {
    "metrics": {
      "summary": "Measure prose with word, sentence, and paragraph counts.",
      "options": {}
    }
  }
}"#;

/// Count words as Unicode alphanumeric runs, keeping internal apostrophes and
/// hyphens inside a word when they join two alphanumeric characters.
fn count_words(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut words = 0;
    let mut in_word = false;

    for (index, character) in chars.iter().enumerate() {
        if character.is_alphanumeric() {
            in_word = true;
            continue;
        }

        let joins_word = in_word
            && (*character == '\'' || *character == '-')
            && chars.get(index + 1).is_some_and(|next| next.is_alphanumeric());

        if !joins_word && in_word {
            words += 1;
            in_word = false;
        }
    }

    if in_word {
        words += 1;
    }

    words
}

/// Count runs of sentence-ending punctuation. Ellipses and punctuation runs
/// such as `?!` count as one sentence boundary.
fn count_sentences(text: &str) -> usize {
    let mut sentences = 0;
    let mut in_terminator = false;

    for character in text.chars() {
        let terminator = matches!(character, '.' | '!' | '?' | '。' | '！' | '？');

        if terminator {
            if !in_terminator {
                sentences += 1;
                in_terminator = true;
            }
        } else if !character.is_whitespace() {
            in_terminator = false;
        }
    }

    sentences
}

/// Count non-empty blocks separated by one or more blank lines.
fn count_paragraphs(text: &str) -> usize {
    let mut paragraphs = 0;
    let mut in_paragraph = false;

    for line in text.lines() {
        if line.trim().is_empty() {
            in_paragraph = false;
        } else if !in_paragraph {
            paragraphs += 1;
            in_paragraph = true;
        }
    }

    paragraphs
}

/// Render metrics as compact JSON without pulling a JSON serialization crate
/// into this small module.
fn metrics_json(text: &str) -> String {
    let paragraphs = count_paragraphs(text);
    let sentences = count_sentences(text);
    let words = count_words(text);
    let mut output = String::from("{\"paragraphs\":");

    output.push_str(&paragraphs.to_string());
    output.push_str(",\"sentences\":");
    output.push_str(&sentences.to_string());
    output.push_str(",\"words\":");
    output.push_str(&words.to_string());
    output.push('}');

    output
}

/// Allocate a write buffer of exactly `len` bytes.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    abi::alloc_buf(len)
}

/// Free a buffer previously handed out by `alloc`.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    abi::free_buf(ptr, len)
}

/// Measure UTF-8 text at `ptr..ptr+len` and return compact JSON.
#[no_mangle]
pub unsafe extern "C" fn metrics(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    if len as usize > MAX_INPUT_BYTES {
        return 0;
    }

    if opts_len != 0 {
        let options = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        match option_pairs(options) {
            Some(pairs) if pairs.is_empty() => {}
            _ => return 0,
        }
    }

    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(_) => return 0,
    };

    abi::pack(metrics_json(text).into_bytes())
}

/// Return the manifest JSON packed as `ptr << 32 | len`.
#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_prose() {
        let text = "First sentence. Second sentence!\nStill here.\n\nNew paragraph?";

        assert_eq!(count_words(text), 8);
        assert_eq!(count_sentences(text), 4);
        assert_eq!(count_paragraphs(text), 2);
        assert_eq!(metrics_json(text), "{\"paragraphs\":2,\"sentences\":4,\"words\":8}");
    }

    #[test]
    fn handles_unicode_and_joined_words() {
        assert_eq!(count_words("L'été is well-being."), 3);
        assert_eq!(count_sentences("Really?! Yes..."), 2);
    }

    #[test]
    fn counts_only_non_empty_paragraphs() {
        assert_eq!(count_paragraphs("\n  \nOne line\nTwo lines\n\n\nLast."), 2);
        assert_eq!(metrics_json(" \n\t"), "{\"paragraphs\":0,\"sentences\":0,\"words\":0}");
    }
}
