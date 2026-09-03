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

//! text - measure prose with document statistics.
//!
//! The module accepts UTF-8 text and returns a small JSON object through the
//! shared raw ABI. Token counts use a dependency-free character estimate.

use std::collections::HashSet;

use abi::{option_pairs, parse_usize};

const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_WPM: usize = 1000;
const MIN_WPM: usize = 1;
const READING_WPM: usize = 200;
const SPEAKING_WPM: usize = 40;

/// The module's self-description as UTF-8 JSON.
const MANIFEST: &str = r#"{
  "exports": {
    "metrics": {
      "summary": "Measure document text with word, character, line, sentence, paragraph, time, and page statistics.",
      "options": {
        "reading_wpm": {
          "type": "number",
          "default": 200,
          "description": "Reading speed in words per minute (1-1000)."
        },
        "speaking_wpm": {
          "type": "number",
          "default": 40,
          "description": "Speaking speed in words per minute (1-1000)."
        }
      }
    }
  }
}"#;

/// Count words as Unicode alphanumeric runs, keeping internal apostrophes and
/// hyphens inside a word when they join two alphanumeric characters.
fn word_tokens(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut words = Vec::new();
    let mut start = None;
    let mut in_word = false;

    for (index, character) in chars.iter().enumerate() {
        if character.is_alphanumeric() {
            if start.is_none() {
                start = Some(index);
            }

            in_word = true;
            continue;
        }

        let joins_word = in_word
            && (*character == '\'' || *character == '-')
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphanumeric());

        if !joins_word && in_word {
            let end = index;
            words.push(chars[start.take().unwrap()..end].iter().collect());
            in_word = false;
        }
    }

    if in_word {
        words.push(chars[start.unwrap()..].iter().collect());
    }

    words
}

/// Count sentence-ending punctuation after sentence content. Decimal points
/// between two alphanumeric characters are not boundaries; abbreviations are
/// treated as sentence-ending periods because recognizing them needs language
/// data outside the standard library.
fn count_sentences(text: &str) -> usize {
    let mut sentences = 0;
    let chars: Vec<char> = text.chars().collect();
    let mut has_content = false;
    let mut in_terminator = false;

    for (index, character) in chars.iter().enumerate() {
        let terminator = matches!(character, '.' | '!' | '?' | '。' | '！' | '？');
        let decimal_point = *character == '.'
            && chars
                .get(index.wrapping_sub(1))
                .is_some_and(|previous| previous.is_alphanumeric())
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphanumeric());

        if terminator && !decimal_point {
            if has_content && !in_terminator {
                sentences += 1;
                in_terminator = true;
            }
        } else if !character.is_whitespace() {
            has_content = true;
            in_terminator = false;
        }
    }

    if sentences == 0
        && text.chars().any(|character| {
            !character.is_whitespace() && !matches!(character, '.' | '!' | '?' | '。' | '！' | '？')
        })
    {
        return count_paragraphs(text);
    }

    sentences
}

/// Count lines split by `\n`; a non-empty trailing line is represented by the
/// final empty line, matching conventional text-editor behavior.
fn count_lines(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }

    text.split('\n').count()
}

/// Count lines containing at least one non-whitespace Unicode scalar value.
fn count_non_empty_lines(text: &str) -> usize {
    text.split('\n')
        .filter(|line| line.chars().any(|character| !character.is_whitespace()))
        .count()
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

/// Round a positive duration up so non-zero work never reports zero seconds.
fn duration_seconds(words: usize, words_per_minute: usize) -> usize {
    if words == 0 {
        return 0;
    }

    (words * 60).div_ceil(words_per_minute)
}

/// Resolve known options. Unknown keys are ignored for forward compatibility.
fn resolve_options(blob: &[u8]) -> Option<(usize, usize)> {
    let mut reading_wpm = READING_WPM;
    let mut speaking_wpm = SPEAKING_WPM;

    for (key, value) in option_pairs(blob)? {
        let parsed = match parse_usize(value) {
            Some(value) if (MIN_WPM..=MAX_WPM).contains(&value) => value,
            _ if key == b"reading_wpm" || key == b"speaking_wpm" => return None,
            _ => continue,
        };

        match key {
            b"reading_wpm" => reading_wpm = parsed,
            b"speaking_wpm" => speaking_wpm = parsed,
            _ => {}
        }
    }

    Some((reading_wpm, speaking_wpm))
}

/// Render metrics as compact JSON without pulling a JSON serialization crate
/// into this small module. Character values are Unicode scalar values, not
/// UTF-8 bytes or grapheme clusters.
fn metrics_json(text: &str, reading_wpm: usize, speaking_wpm: usize) -> String {
    let words_list = word_tokens(text);
    let words = words_list.len();
    let unique_words = words_list
        .iter()
        .map(|word| word.to_lowercase())
        .collect::<HashSet<_>>()
        .len();
    let paragraphs = count_paragraphs(text);
    let sentences = count_sentences(text);
    let lines = count_lines(text);
    let non_empty_lines = count_non_empty_lines(text);
    let characters = text.chars().count();
    let characters_no_spaces = text
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    let average_sentence_length = if sentences == 0 {
        0.0
    } else {
        words as f64 / sentences as f64
    };
    let average_paragraph_length = if paragraphs == 0 {
        0.0
    } else {
        words as f64 / paragraphs as f64
    };
    let effective_words = words.max(characters_no_spaces.div_ceil(5));
    let reading_time_seconds = duration_seconds(effective_words, reading_wpm);
    let speaking_time_seconds = duration_seconds(effective_words, speaking_wpm);
    let pages = effective_words as f64 / 500.0;
    let approximate_tokens = characters.div_ceil(4);
    let mut output = String::from("{\"words\":");

    output.push_str(&words.to_string());
    output.push_str(",\"unique_words\":");
    output.push_str(&unique_words.to_string());
    output.push_str(",\"characters\":");
    output.push_str(&characters.to_string());
    output.push_str(",\"characters_no_spaces\":");
    output.push_str(&characters_no_spaces.to_string());
    output.push_str(",\"lines\":");
    output.push_str(&lines.to_string());
    output.push_str(",\"non_empty_lines\":");
    output.push_str(&non_empty_lines.to_string());
    output.push_str(",\"sentences\":");
    output.push_str(&sentences.to_string());
    output.push_str(",\"paragraphs\":");
    output.push_str(&paragraphs.to_string());
    output.push_str(",\"average_sentence_length\":");
    output.push_str(&average_sentence_length.to_string());
    output.push_str(",\"average_paragraph_length\":");
    output.push_str(&average_paragraph_length.to_string());
    output.push_str(",\"reading_time_seconds\":");
    output.push_str(&reading_time_seconds.to_string());
    output.push_str(",\"speaking_time_seconds\":");
    output.push_str(&speaking_time_seconds.to_string());
    output.push_str(",\"pages\":");
    output.push_str(&pages.to_string());
    output.push_str(",\"approximate_tokens\":");
    output.push_str(&approximate_tokens.to_string());
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

        if resolve_options(options).is_none() {
            return 0;
        }
    }

    let input = std::slice::from_raw_parts(ptr as *const u8, len as usize);
    let text = match std::str::from_utf8(input) {
        Ok(text) => text,
        Err(_) => return 0,
    };

    let options = match resolve_options(std::slice::from_raw_parts(
        opts_ptr as *const u8,
        opts_len as usize,
    )) {
        Some(options) => options,
        None => return 0,
    };

    abi::pack(metrics_json(text, options.0, options.1).into_bytes())
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

        assert_eq!(word_tokens(text).len(), 8);
        assert_eq!(count_sentences(text), 4);
        assert_eq!(count_paragraphs(text), 2);
        assert_eq!(metrics_json(text, READING_WPM, SPEAKING_WPM), "{\"words\":8,\"unique_words\":7,\"characters\":60,\"characters_no_spaces\":52,\"lines\":4,\"non_empty_lines\":3,\"sentences\":4,\"paragraphs\":2,\"average_sentence_length\":2,\"average_paragraph_length\":4,\"reading_time_seconds\":4,\"speaking_time_seconds\":17,\"pages\":0.022,\"approximate_tokens\":15}");
    }

    #[test]
    fn handles_unicode_and_joined_words() {
        assert_eq!(
            word_tokens("L'été is well-being."),
            vec!["L'été", "is", "well-being"]
        );
        assert_eq!(
            word_tokens("One one O'NEIL o'neil well-being well-being")
                .iter()
                .map(|word| word.to_lowercase())
                .collect::<HashSet<_>>()
                .len(),
            3
        );
        assert_eq!(count_sentences("Really?! Yes..."), 2);
    }

    #[test]
    fn counts_only_non_empty_paragraphs() {
        assert_eq!(count_paragraphs("\n  \nOne line\nTwo lines\n\n\nLast."), 2);
        assert_eq!(metrics_json(" \n\t", READING_WPM, SPEAKING_WPM), "{\"words\":0,\"unique_words\":0,\"characters\":3,\"characters_no_spaces\":0,\"lines\":2,\"non_empty_lines\":0,\"sentences\":0,\"paragraphs\":0,\"average_sentence_length\":0,\"average_paragraph_length\":0,\"reading_time_seconds\":0,\"speaking_time_seconds\":0,\"pages\":0,\"approximate_tokens\":1}");
    }

    #[test]
    fn counts_lines_and_unicode_scalars() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("one"), 1);
        assert_eq!(count_lines("one\n\n two\n"), 4);
        assert_eq!(count_non_empty_lines("one\n \t\n\ntwo\n"), 2);
        assert_eq!("Café 🦀".chars().count(), 6);
        assert_eq!(
            "Café 🦀 \t\n"
                .chars()
                .filter(|character| !character.is_whitespace())
                .count(),
            5
        );
    }

    #[test]
    fn sentence_boundaries_require_content_and_skip_decimals() {
        assert_eq!(count_sentences("?!..."), 0);
        assert_eq!(count_sentences("One sentence\n\nAnother one"), 2);
        assert_eq!(count_sentences("One sentence\n\n\nTwo sentences"), 2);
        assert_eq!(count_sentences("One?! ... Two。"), 2);
        assert_eq!(count_sentences("Value 1.25. Next?"), 2);
        assert_eq!(count_sentences("One.\nTwo!"), 2);
    }

    #[test]
    fn calculates_averages_durations_and_pages() {
        let json = metrics_json("One two. Three four.", 100, 50);
        let one_sentence = metrics_json("One two.", READING_WPM, SPEAKING_WPM);
        let multi_page = metrics_json(&"word ".repeat(501), READING_WPM, SPEAKING_WPM);

        assert!(one_sentence.contains("\"average_sentence_length\":2"));
        assert!(json.contains("\"average_sentence_length\":2"));
        assert!(json.contains("\"average_paragraph_length\":4"));
        assert!(json.contains("\"reading_time_seconds\":3"));
        assert!(json.contains("\"speaking_time_seconds\":5"));
        assert!(json.contains("\"pages\":0.008"));
        assert!(multi_page.contains("\"pages\":1.002"));
        let long_word = metrics_json("antidisestablishmentarianism", READING_WPM, SPEAKING_WPM);
        assert!(long_word.contains("\"reading_time_seconds\":2"));
        assert!(long_word.contains("\"pages\":0.012"));
        assert!(long_word.contains("\"approximate_tokens\":7"));
        assert_eq!(resolve_options(&[]), Some((READING_WPM, SPEAKING_WPM)));
        assert_eq!(resolve_options(&[1, 0, 0, 0]), None);
    }
}
