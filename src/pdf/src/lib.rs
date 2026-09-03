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

//! pdf — assemble a new PDF from pages of the input PDFs. A minimal raw-ABI
//! module: the caller writes the file-input frame into `alloc`'d linear
//! memory, calls `assemble` with an options blob, reads the output PDF from
//! the returned pointer, then `dealloc`s all buffers. No envelope, no host
//! imports — just `memory`, `alloc`, `dealloc`, `assemble`, `manifest`.
//!
//! The options blob is a flat length-prefixed key/value list so future options
//! never change the export signatures: old cores ignore unknown keys, new cores
//! default missing ones. See README.md for the wire format. The `manifest`
//! export returns the module's self-description (JSON) so consumers can call
//! it generically without hardcoding its interface.
//!
//! Buffer packing, blob framing, and the file-input frame come from the shared
//! `abi` crate; the page-tree rebuild uses lopdf (pure Rust, no C).

use abi::{option_pairs, parse_files};
use lopdf::dictionary;
use lopdf::{Document, Object, ObjectId};

/// The module's self-description as UTF-8 JSON; `JSON.parse` it on the host
/// side.
const MANIFEST: &str = r#"{
  "exports": {
    "assemble": {
      "summary": "Build a new PDF from selected pages of the input PDFs, with per-page rotation and blank pages.",
      "options": {
        "pages": {
          "type": "text",
          "default": "[]",
       "description": "JSON array of page entries in output order. Each entry is [file,page] or [file,page,rotate] where rotate is 0/90/180/270 (clockwise, added to the page's existing rotation), \"blank\" for an empty Letter page, or [\"blank\",width,height] for an empty page with dimensions in PDF points."
        }
      }
    }
  }
}"#;

/// One entry in the output page list.
#[derive(Debug, PartialEq)]
enum Entry {
    /// A page from a loaded file: (file index, 0-based page index).
    Page { file: usize, page: usize },
    /// A blank page. Source references inherit that page's MediaBox; explicit
    /// dimensions are integer PDF points.
    Blank {
        file: Option<usize>,
        page: Option<usize>,
        width: Option<u32>,
        height: Option<u32>,
    },
}

const DEFAULT_PAGE_HEIGHT: u32 = 792;
const DEFAULT_PAGE_WIDTH: u32 = 612;
const MAX_PAGE_DIMENSION: u32 = 14_400;

/// Accept dimensions large enough for normal PDF pages while rejecting values
/// that could create unreasonable allocations or invalid page geometry.
fn parse_dimension(value: &[u8]) -> Option<u32> {
    let dimension = abi::parse_usize(value)?;

    if dimension == 0 || dimension > MAX_PAGE_DIMENSION as usize {
        return None;
    }

    Some(dimension as u32)
}

/// Rotation applied to a page, in whole clockwise degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Rotation {
    Zero,
    Ninety,
    OneEighty,
    TwoSeventy,
}

impl Rotation {
    fn degrees(self) -> i64 {
        match self {
            Rotation::Zero => 0,
            Rotation::Ninety => 90,
            Rotation::OneEighty => 180,
            Rotation::TwoSeventy => 270,
        }
    }

    /// Parse a rotation option value (0/90/180/270).
    fn parse(value: &[u8]) -> Option<Rotation> {
        match value {
            b"0" => Some(Rotation::Zero),
            b"90" => Some(Rotation::Ninety),
            b"180" => Some(Rotation::OneEighty),
            b"270" => Some(Rotation::TwoSeventy),
            _ => None,
        }
    }
}

/// Parse the `pages` option: a strict JSON array of entries. An entry is
/// either a 2- or 3-element array of integers `[file, page, rotate?]`, a
/// source-sized blank `[file, page, "blank"]`, an explicit blank
/// `["blank", width, height]`, or the string `"blank"`.
/// Returns `None` on any malformed JSON, non-array entry,
/// or out-of-range index, so a bad request is a rejection (result 0), never a
/// partial document. This is deliberately a minimal parser for exactly this
/// shape - no floats, no objects, no escapes beyond what an integer can hold.
fn parse_pages(pages: &[u8]) -> Option<Vec<(Entry, Rotation)>> {
    let mut p = JsonCursor {
        bytes: pages,
        pos: 0,
    };

    p.skip_ws();
    p.expect(b'[')?;
    let mut out = Vec::new();

    loop {
        p.skip_ws();

        if p.peek() == Some(b']') {
            break;
        }

        if !out.is_empty() {
            p.expect(b',')?;
            p.skip_ws();
        }

        let (entry, rotation) = if p.peek() == Some(b'"') {
            // A blank page: "blank".
            p.expect(b'"')?;

            let mut name = Vec::new();

            loop {
                match p.peek()? {
                    b'"' => break,
                    b'\\' => return None, // No escapes in "blank".
                    byte => {
                        name.push(byte);
                        p.bump();
                    }
                }
            }

            p.expect(b'"')?;

            if name != b"blank" {
                return None;
            }

            (
                Entry::Blank {
                    file: None,
                    page: None,
                    width: None,
                    height: None,
                },
                Rotation::Zero,
            )
        } else if is_blank_array(&p) {
            // An explicit blank page: ["blank", width, height].
            p.expect(b'[')?;
            p.skip_ws();
            if p.read_string()? != b"blank" {
                return None;
            }
            p.skip_ws();
            p.expect(b',')?;
            p.skip_ws();
            let width = parse_dimension(p.read_token()?)?;
            p.skip_ws();
            p.expect(b',')?;
            p.skip_ws();
            let height = parse_dimension(p.read_token()?)?;
            p.skip_ws();
            p.expect(b']')?;

            (
                Entry::Blank {
                    file: None,
                    page: None,
                    width: Some(width),
                    height: Some(height),
                },
                Rotation::Zero,
            )
        } else {
            // A page entry: [file, page, rotate?].
            p.expect(b'[')?;
            p.skip_ws();

            let file = p.read_usize()?;
            p.skip_ws();
            p.expect(b',')?;
            p.skip_ws();

            let page = p.read_usize()?;
            let mut rotation = Rotation::Zero;
            let mut special_entry = None;

            p.skip_ws();

            if p.peek() == Some(b',') {
                p.bump();
                p.skip_ws();
                if p.peek() == Some(b'"') {
                    if p.read_string()? != b"blank" {
                        return None;
                    }
                    special_entry = Some((
                        Entry::Blank {
                            file: Some(file),
                            page: Some(page),
                            width: None,
                            height: None,
                        },
                        Rotation::Zero,
                    ));
                } else {
                    rotation = Rotation::parse(&p.read_token()?)?;
                }
                p.skip_ws();
            }

            p.expect(b']')?;

            special_entry.unwrap_or((Entry::Page { file, page }, rotation))
        };

        out.push((entry, rotation));
    }

    p.expect(b']')?;
    p.skip_ws();

    if p.peek().is_some() {
        return None;
    }

    Some(out)
}

/// Detect an explicit blank array without consuming the real cursor.
fn is_blank_array(p: &JsonCursor) -> bool {
    let mut probe = JsonCursor {
        bytes: p.bytes,
        pos: p.pos,
    };

    probe.expect(b'[');
    probe.skip_ws();

    probe.read_string().as_deref() == Some(b"blank")
}

/// A minimal byte cursor for the strict pages-JSON parser above.
struct JsonCursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonCursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) {
        self.pos += 1;
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.bump();
        }
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        if self.peek() == Some(byte) {
            self.bump();
            Some(())
        } else {
            None
        }
    }

    /// Read an ASCII-decimal integer token (no sign, no exponent).
    fn read_usize(&mut self) -> Option<usize> {
        let token = self.read_token()?;

        if token.is_empty() || !token.iter().all(|b| b.is_ascii_digit()) {
            return None;
        }

        let mut value = 0usize;

        for &digit in token {
            value = value
                .checked_mul(10)?
                .checked_add((digit - b'0') as usize)?;
        }

        Some(value)
    }

    /// Read a run of non-delimiter bytes (`[`, `]`, `,`, whitespace, quote).
    fn read_token(&mut self) -> Option<&'a [u8]> {
        let start = self.pos;

        while let Some(byte) = self.peek() {
            if matches!(
                byte,
                b'[' | b']' | b',' | b'"' | b' ' | b'\t' | b'\r' | b'\n'
            ) {
                break;
            }

            self.bump();
        }

        self.bytes.get(start..self.pos)
    }

    fn read_string(&mut self) -> Option<Vec<u8>> {
        self.expect(b'"')?;
        let mut value = Vec::new();

        while let Some(byte) = self.peek() {
            if byte == b'"' {
                self.bump();
                return Some(value);
            }
            if byte == b'\\' {
                return None;
            }
            value.push(byte);
            self.bump();
        }

        None
    }
}

/// Find the first page attribute on a page or one of its `/Pages` ancestors.
/// lopdf exposes resource inheritance separately, but these page attributes
/// share the same parent-chain rule and must be materialized before the source
/// page tree is discarded.
fn inherited_page_attribute(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut current = page_id;
    let mut seen = std::collections::BTreeSet::new();

    while seen.insert(current) {
        let dict = doc.get_dictionary(current).ok()?;

        if let Ok(value) = dict.get(key) {
            return Some(value.clone());
        }

        current = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }

    None
}

/// Copy inheritable page attributes onto every page while the original page
/// tree is still available. The new output tree has a fresh root, so leaving
/// these values on discarded ancestors would make otherwise valid content
/// invisible after export.
fn materialize_page_attributes(doc: &mut Document, page_order: &[ObjectId]) {
    const ATTRIBUTES: &[&[u8]] = &[b"Resources", b"MediaBox", b"CropBox", b"Rotate"];
    let inherited: Vec<(ObjectId, Vec<(&[u8], Object)>)> = page_order
        .iter()
        .map(|page_id| {
            let values = ATTRIBUTES
                .iter()
                .filter_map(|key| {
                    inherited_page_attribute(doc, *page_id, key).map(|value| (*key, value))
                })
                .collect();
            (*page_id, values)
        })
        .collect();

    for (page_id, values) in inherited {
        if let Ok(dict) = doc.get_dictionary_mut(page_id) {
            for (key, value) in values {
                if !dict.has(key) {
                    dict.set(key, value);
                }
            }
        }
    }
}

/// Assemble a new PDF from the input files and page selection. Each source
/// document is loaded, its non-page objects merged into one output document,
/// then the output page tree is rebuilt to contain exactly the selected pages
/// in order (blank pages created as empty Letter pages). Returns the assembled
/// PDF bytes.
fn assemble_pdf(
    files: &[(Vec<u8>, Vec<u8>)],
    pages: &[(Entry, Rotation)],
) -> Result<Vec<u8>, &'static str> {
    if files.is_empty() {
        return Err("no input files");
    }

    if pages.is_empty() {
        return Err("no pages selected");
    }

    let mut sources = Vec::with_capacity(files.len());

    for (_, data) in files {
        let doc = Document::load_mem(data).map_err(|_| "unable to read a PDF file")?;
        sources.push(doc);
    }

    let mut out = Document::with_version("1.5");
    let mut max_id = 1u32;

    // First, merge every non-page object from all sources into the output,
    // renumbering each source's ids so they never collide. Page objects are
    // collected per source (keyed by their renumbered id) before the source's
    // objects are drained, so the selection loop below can find them.
    let mut page_objects: Vec<std::collections::BTreeMap<ObjectId, Object>> =
        Vec::with_capacity(sources.len());
    // Per-source ordered page ids (original document order), captured before
    // the source's objects are drained so the selection loop can index into it.
    let mut page_order: Vec<Vec<ObjectId>> = Vec::with_capacity(sources.len());

    for source in sources.iter_mut() {
        source.renumber_objects_with(max_id);
        max_id = source.max_id + 1;

        let order: Vec<ObjectId> = source.page_iter().collect();
        materialize_page_attributes(source, &order);
        let mut pages = std::collections::BTreeMap::new();

        for (id, object) in std::mem::take(&mut source.objects) {
            match object.type_name().unwrap_or(b"") {
                b"Page" => {
                    pages.insert(id, object);
                }
                b"Pages" | b"Catalog" => continue, // Rebuilt below.
                b"Outlines" | b"Outline" => continue, // Outlines carry cross-doc refs; dropped.
                _ => {
                    out.objects.insert(id, object);
                }
            }
        }

        page_objects.push(pages);
        page_order.push(order);
    }

    // Build the output page tree root first, so every copied page can point at
    // a real Parent. The copied source objects were inserted directly, so keep
    // lopdf's allocator above their highest ID or new page objects overwrite
    // source XObjects and content streams.
    out.max_id = max_id;
    let pages_id = out.new_object_id();
    let catalog_id = out.new_object_id();

    out.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Vec::<Object>::new(),
            "Count" => 0,
        }),
    );

    out.set_object(
        catalog_id,
        dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        },
    );
    out.trailer.set("Root", catalog_id);

    // Collect the selected page object ids, in output order, and their source
    // doc references so rotation can be applied before the tree is written.
    let mut kid_ids: Vec<ObjectId> = Vec::new();

    for (entry, rotation) in pages {
        match entry {
            Entry::Blank {
                file,
                page,
                width,
                height,
            } => {
                let page_id = out.new_object_id();
                let media_box = match (width, height) {
                    (Some(width), Some(height)) => {
                        vec![0.into(), 0.into(), (*width).into(), (*height).into()].into()
                    }
                    _ => file
                        .and_then(|file| {
                            page.and_then(|page| {
                                page_order
                                    .get(file)
                                    .and_then(|order| order.get(page).copied())
                            })
                        })
                        .and_then(|page_id| {
                            page_objects.iter().find_map(|pages| pages.get(&page_id))
                        })
                        .and_then(|object| object.as_dict().ok())
                        .and_then(|dict| dict.get(b"MediaBox").ok())
                        .cloned()
                        .unwrap_or_else(|| {
                            vec![
                                0.into(),
                                0.into(),
                                DEFAULT_PAGE_WIDTH.into(),
                                DEFAULT_PAGE_HEIGHT.into(),
                            ]
                            .into()
                        }),
                };

                out.set_object(
                    page_id,
                    dictionary! {
                        "Type" => "Page",
                        "Parent" => pages_id,
                        "MediaBox" => media_box,
                    },
                );
                kid_ids.push(page_id);
            }
            Entry::Page { file, page } => {
                let order = page_order.get(*file).ok_or("file index out of range")?;
                let page_id = *order.get(*page).ok_or("page index out of range")?;

                // Copy the page object into the output, reparented to the new
                // tree and rotated as requested.
                let object = page_objects[*file]
                    .get(&page_id)
                    .ok_or("page object missing")?;
                let mut dict = object
                    .as_dict()
                    .map_err(|_| "page object is not a dictionary")?
                    .clone();
                dict.set("Parent", pages_id);

                if *rotation != Rotation::Zero {
                    let current = dict.get(b"Rotate").and_then(Object::as_i64).unwrap_or(0);
                    dict.set("Rotate", (current + rotation.degrees()) % 360);
                }

                let output_page_id = out.new_object_id();
                out.set_object(output_page_id, Object::Dictionary(dict));
                kid_ids.push(output_page_id);
            }
        }
    }

    // Set the tree's Kids/Count from the collected selection.
    let page_objs: Vec<Object> = kid_ids.iter().map(|id| Object::Reference(*id)).collect();

    out.set_object(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_objs,
            "Count" => kid_ids.len() as u32,
        }),
    );

    let mut bytes = Vec::new();
    out.save_to(&mut bytes)
        .map_err(|_| "unable to write the output PDF")?;

    Ok(bytes)
}

/// Parse the options blob straight into the page selection. Framing (magic
/// byte, length prefixes) is validated by the shared `option_pairs`; unknown
/// keys are ignored so new callers stay compatible with older cores; known
/// keys with bad values are errors.
fn resolve_options(blob: &[u8]) -> Option<Vec<(Entry, Rotation)>> {
    let mut pages: Option<Vec<(Entry, Rotation)>> = None;

    for (key, value) in option_pairs(blob)? {
        match key {
            b"pages" => pages = Some(parse_pages(value)?),
            _ => {}
        }
    }

    Some(pages.unwrap_or_default())
}

/// Allocate a write buffer of exactly `len` bytes.
///
/// # Safety
/// The returned pointer is only valid inside this module's linear memory and
/// must be released with `dealloc`.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: u32) -> u32 {
    abi::alloc_buf(len)
}

/// Free a buffer previously handed out by `alloc`.
///
/// # Safety
/// `ptr`/`len` must come from `alloc` and must not have been freed before.
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: u32, len: u32) {
    abi::free_buf(ptr, len)
}

/// Assemble the input file frame at `ptr..ptr+len` into a new PDF and return
/// the output packed as `ptr << 32 | len`. Options come from the blob at
/// `opts_ptr..opts_ptr+opts_len` (pass 0/0 for defaults). Returns 0 on any
/// malformed input, bad option value, or assembly error.
///
/// # Safety
/// All pointers must reference this module's linear memory with exact lengths.
#[no_mangle]
pub unsafe extern "C" fn assemble(ptr: u32, len: u32, opts_ptr: u32, opts_len: u32) -> u64 {
    let frame = std::slice::from_raw_parts(ptr as *const u8, len as usize);

    let files: Vec<(Vec<u8>, Vec<u8>)> = match parse_files(frame) {
        Some(files) => files
            .into_iter()
            .map(|(n, d)| (n.to_vec(), d.to_vec()))
            .collect(),
        None => return 0,
    };

    let pages = if opts_len == 0 {
        Vec::new()
    } else {
        let blob = std::slice::from_raw_parts(opts_ptr as *const u8, opts_len as usize);

        match resolve_options(blob) {
            Some(pages) => pages,
            None => return 0,
        }
    };

    match assemble_pdf(&files, &pages) {
        Ok(bytes) => abi::pack(bytes),
        Err(_) => 0,
    }
}

/// Return the manifest JSON packed as `ptr << 32 | len`.
#[no_mangle]
pub unsafe extern "C" fn manifest() -> u64 {
    abi::pack(MANIFEST.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use abi::OPTIONS_MAGIC;

    fn pair(key: &str, value: &str) -> Vec<u8> {
        let mut blob = (key.len() as u32).to_le_bytes().to_vec();

        blob.extend_from_slice(key.as_bytes());
        blob.extend_from_slice(&(value.len() as u32).to_le_bytes());
        blob.extend_from_slice(value.as_bytes());

        blob
    }

    fn blob(pairs: &[Vec<u8>]) -> Vec<u8> {
        let mut all = vec![OPTIONS_MAGIC];

        for item in pairs {
            all.extend_from_slice(item);
        }

        all
    }

    fn make_pdf(text: &str) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let _resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let content = lopdf::content::Content {
            operations: vec![
                lopdf::content::Operation::new("BT", vec![]),
                lopdf::content::Operation::new("Tf", vec!["F1".into(), 48.into()]),
                lopdf::content::Operation::new("Td", vec![100.into(), 600.into()]),
                lopdf::content::Operation::new("Tj", vec![Object::string_literal(text)]),
                lopdf::content::Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(lopdf::Stream::new(
            dictionary! {},
            content.encode().unwrap(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn make_inherited_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let content = lopdf::content::Content {
            operations: vec![
                lopdf::content::Operation::new("BT", vec![]),
                lopdf::content::Operation::new("Tf", vec!["F1".into(), 48.into()]),
                lopdf::content::Operation::new("Td", vec![100.into(), 600.into()]),
                lopdf::content::Operation::new("Tj", vec![Object::string_literal("inherited")]),
                lopdf::content::Operation::new("ET", vec![]),
            ],
        };
        let content_id = doc.add_object(lopdf::Stream::new(
            dictionary! {},
            content.encode().unwrap(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page_id.into()],
                "Count" => 1,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 1000.into(), 500.into()],
                "CropBox" => vec![0.into(), 0.into(), 900.into(), 400.into()],
                "Rotate" => 90,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn parse_pages_basic_entries() {
        assert_eq!(
            parse_pages(b"[[0,1],[0,2,90],[\"blank\",612,792],\"blank\"]"),
            Some(vec![
                (Entry::Page { file: 0, page: 1 }, Rotation::Zero),
                (Entry::Page { file: 0, page: 2 }, Rotation::Ninety),
                (
                    Entry::Blank {
                        file: None,
                        page: None,
                        width: Some(612),
                        height: Some(792)
                    },
                    Rotation::Zero
                ),
                (
                    Entry::Blank {
                        file: None,
                        page: None,
                        width: None,
                        height: None
                    },
                    Rotation::Zero
                ),
            ])
        );
    }

    #[test]
    fn parse_pages_whitespace_and_empty() {
        assert_eq!(parse_pages(b"[]"), Some(vec![]));
        assert_eq!(
            parse_pages(b"  [ [ 0 , 1 ] ]  "),
            Some(vec![(Entry::Page { file: 0, page: 1 }, Rotation::Zero)])
        );
    }

    #[test]
    fn parse_pages_source_sized_blank() {
        assert_eq!(
            parse_pages(b"[[0,0,\"blank\"]]"),
            Some(vec![(
                Entry::Blank {
                    file: Some(0),
                    page: Some(0),
                    width: None,
                    height: None
                },
                Rotation::Zero
            )])
        );
    }

    #[test]
    fn parse_pages_rejects_malformed() {
        assert_eq!(parse_pages(b""), None);
        assert_eq!(parse_pages(b"[0,1]"), None);
        assert_eq!(parse_pages(b"[[0]]"), None);
        assert_eq!(parse_pages(b"[[0,1,2]]"), None); // No rotate=2.
        assert_eq!(parse_pages(b"[[0,-1]]"), None); // No negative pages.
        assert_eq!(parse_pages(b"[[0,1],]"), None); // Trailing comma.
        assert_eq!(parse_pages(b"[\"x\"]"), None);
        assert_eq!(parse_pages(b"[[0,1]"), None); // Unclosed.
        assert_eq!(parse_pages(b"[[0,1]]x"), None); // Trailing garbage.
        assert_eq!(parse_pages(b"[[\"blank\",612,0]]"), None); // Zero dimension.
        assert_eq!(parse_pages(b"[[\"blank\",14401,792]]"), None); // Excessive dimension.
        assert_eq!(parse_pages(b"[[\"blank\",612]]"), None); // Missing height.
        assert_eq!(parse_pages(b"[[\"blank\",612.5,792]]"), None); // No fractional points.
    }

    #[test]
    fn parse_pages_all_rotations() {
        assert_eq!(
            parse_pages(b"[[0,0,0]]"),
            Some(vec![(Entry::Page { file: 0, page: 0 }, Rotation::Zero)])
        );
        assert_eq!(
            parse_pages(b"[[0,0,90]]"),
            Some(vec![(Entry::Page { file: 0, page: 0 }, Rotation::Ninety)])
        );
        assert_eq!(
            parse_pages(b"[[0,0,180]]"),
            Some(vec![(
                Entry::Page { file: 0, page: 0 },
                Rotation::OneEighty
            )])
        );
        assert_eq!(
            parse_pages(b"[[0,0,270]]"),
            Some(vec![(
                Entry::Page { file: 0, page: 0 },
                Rotation::TwoSeventy
            )])
        );
    }

    #[test]
    fn assemble_empty_selection_is_rejected() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf)];
        let pages = vec![];

        assert!(assemble_pdf(&files, &pages).is_err());
    }

    #[test]
    fn assemble_pdf_blank_page_round_trip() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf.clone())];
        let pages = vec![(
            Entry::Blank {
                file: None,
                page: None,
                width: None,
                height: None,
            },
            Rotation::Zero,
        )];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");
        assert_eq!(out.get_pages().len(), 1);

        // The blank page is empty: no Contents.
        let page_id = out.get_pages().get(&1).copied().expect("page 1");
        let dict = out.get_object(page_id).unwrap().as_dict().unwrap();
        assert!(!dict.has(b"Contents"));
    }

    #[test]
    fn assemble_pdf_custom_blank_page_uses_dimensions() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf)];
        let pages = vec![(
            Entry::Blank {
                file: None,
                page: None,
                width: Some(1000),
                height: Some(500),
            },
            Rotation::Zero,
        )];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");
        let page_id = out.get_pages().get(&1).copied().expect("page 1");
        let dict = out.get_object(page_id).unwrap().as_dict().unwrap();

        assert_eq!(
            dict.get(b"MediaBox").unwrap(),
            &vec![0.into(), 0.into(), 1000.into(), 500.into()].into()
        );
        assert!(!dict.has(b"Contents"));
    }

    #[test]
    fn assemble_pdf_reorders_pages() {
        // Two pages from the same single-page source cannot exist; build a
        // two-page source instead.
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });
        let _resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });
        let mut kids = Vec::new();

        for text in ["first", "second"] {
            let content = lopdf::content::Content {
                operations: vec![
                    lopdf::content::Operation::new("BT", vec![]),
                    lopdf::content::Operation::new("Tf", vec!["F1".into(), 48.into()]),
                    lopdf::content::Operation::new("Td", vec![100.into(), 600.into()]),
                    lopdf::content::Operation::new("Tj", vec![Object::string_literal(text)]),
                    lopdf::content::Operation::new("ET", vec![]),
                ],
            };
            let content_id = doc.add_object(lopdf::Stream::new(
                dictionary! {},
                content.encode().unwrap(),
            ));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            });
            kids.push(page_id.into());
        }

        let pages_dict = dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 2,
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut two_bytes = Vec::new();
        doc.save_to(&mut two_bytes).unwrap();

        let files = vec![(b"two.pdf".to_vec(), two_bytes)];
        let pages = vec![
            (Entry::Page { file: 0, page: 1 }, Rotation::Zero),
            (Entry::Page { file: 0, page: 0 }, Rotation::Zero),
        ];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");

        assert_eq!(out.get_pages().len(), 2);
    }

    #[test]
    fn assemble_pdf_duplicates_page_with_same_dimensions() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf)];
        let pages = vec![
            (Entry::Page { file: 0, page: 0 }, Rotation::Zero),
            (Entry::Page { file: 0, page: 0 }, Rotation::Zero),
        ];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");
        let page_ids: Vec<ObjectId> = out.get_pages().values().copied().collect();
        let first = out.get_object(page_ids[0]).unwrap().as_dict().unwrap();
        let second = out.get_object(page_ids[1]).unwrap().as_dict().unwrap();

        assert_ne!(page_ids[0], page_ids[1]);
        assert_eq!(
            first.get(b"MediaBox").unwrap(),
            second.get(b"MediaBox").unwrap()
        );
    }

    #[test]
    fn assemble_pdf_materializes_inherited_page_attributes() {
        let pdf = make_inherited_pdf();
        let files = vec![(b"inherited.pdf".to_vec(), pdf)];
        let pages = vec![
            (Entry::Page { file: 0, page: 0 }, Rotation::Zero),
            (Entry::Page { file: 0, page: 0 }, Rotation::Ninety),
            (
                Entry::Blank {
                    file: Some(0),
                    page: Some(0),
                    width: None,
                    height: None,
                },
                Rotation::Zero,
            ),
        ];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");
        let page_ids: Vec<ObjectId> = out.get_pages().values().copied().collect();
        let first = out.get_object(page_ids[0]).unwrap().as_dict().unwrap();
        let second = out.get_object(page_ids[1]).unwrap().as_dict().unwrap();
        let blank = out.get_object(page_ids[2]).unwrap().as_dict().unwrap();

        assert!(first.has(b"Resources"));
        assert_eq!(
            first.get(b"MediaBox").unwrap(),
            &vec![0.into(), 0.into(), 1000.into(), 500.into()].into()
        );
        assert_eq!(
            first.get(b"CropBox").unwrap(),
            &vec![0.into(), 0.into(), 900.into(), 400.into()].into()
        );
        assert_eq!(first.get(b"Rotate").and_then(Object::as_i64).unwrap(), 90);
        assert_eq!(second.get(b"Rotate").and_then(Object::as_i64).unwrap(), 180);
        assert_eq!(
            blank.get(b"MediaBox").unwrap(),
            &vec![0.into(), 0.into(), 1000.into(), 500.into()].into()
        );
    }

    #[test]
    fn assemble_pdf_rotates_page() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf)];
        let pages = vec![(Entry::Page { file: 0, page: 0 }, Rotation::Ninety)];

        let bytes = assemble_pdf(&files, &pages).expect("assemble");
        let out = Document::load_mem(&bytes).expect("load output");
        let page_id = out.get_pages().get(&1).copied().expect("page 1");
        let dict = out.get_object(page_id).unwrap().as_dict().unwrap();
        assert_eq!(dict.get(b"Rotate").and_then(Object::as_i64).unwrap(), 90);
    }

    #[test]
    fn assemble_pdf_rejects_bad_indices() {
        let pdf = make_pdf("hello");
        let files = vec![(b"a.pdf".to_vec(), pdf)];

        assert!(assemble_pdf(
            &files,
            &[(Entry::Page { file: 0, page: 5 }, Rotation::Zero)]
        )
        .is_err());
        assert!(assemble_pdf(
            &files,
            &[(Entry::Page { file: 1, page: 0 }, Rotation::Zero)]
        )
        .is_err());
    }

    #[test]
    fn resolve_options_parses_pages() {
        assert_eq!(
            resolve_options(&blob(&[pair("pages", "[[0,0,90]]")])),
            Some(vec![(Entry::Page { file: 0, page: 0 }, Rotation::Ninety)])
        );
        assert_eq!(resolve_options(&blob(&[pair("pages", "[")])), None);
        assert_eq!(
            resolve_options(&blob(&[pair("future", "x"), pair("pages", "[]")])),
            Some(vec![])
        );
    }
}
