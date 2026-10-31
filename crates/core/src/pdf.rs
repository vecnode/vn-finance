//! A small PDF reader, large enough for the documents this application imports.
//!
//! The scope is deliberate. This is not a PDF library: it answers one question —
//! "what text does this document say, and roughly where does it say it" — and it says
//! so when it cannot. Everything it cannot do is **reported** rather than guessed: an
//! encrypted file, an object-stream layout, a font with no character map, a stream that
//! will not inflate. A tax assistant that invents a figure out of a PDF it half
//! understood is worse than one that refuses.
//!
//! What is implemented:
//!
//! - the object graph (`N 0 obj << … >> stream … endstream`), with xref-free lookup;
//! - the page tree, with inherited `/Resources`, so a page's fonts are known;
//! - `FlateDecode` streams, through `miniz_oxide` — no other filter is needed by the
//!   generators this targets, and an unknown filter is reported, not ignored;
//! - content streams: `Tf`, `TL`, `Td`, `TD`, `Tm`, `T*`, `Tj`, `TJ`, `'`, `"`, inside
//!   `q`/`Q`/`cm`;
//! - text decoding via `/ToUnicode` (bfchar/bfrange) when present, otherwise cp1252 for
//!   one-byte fonts, and a stated problem for a two-byte font with no character map (a
//!   glyph id is not a character);
//! - rows: items sharing a baseline become one line, ordered left to right, which is
//!   what turns a positioned form into text a parser can read.
//!
//! ## The one divergence from the reference
//!
//! The reference inflates through `node:zlib`, which is a C library behind a JavaScript
//! API. This port uses `miniz_oxide`, which is pure Rust, so the "no C in the shipped
//! binary" property survives the only place in the whole application that needed it.
//! Everything else here is a line-for-line port, and the golden fixtures check the
//! output byte for byte.
//!
//! The whole file is parsed as **bytes**, not as text. The reference did the same thing
//! with a `latin1` string, because that is the only decoding under which one byte is
//! one character; writing it as bytes in Rust makes that structural instead of
//! conventional.

use std::collections::BTreeMap;

/// One run of text, with the position it was placed at.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfTextItem {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position, measured up from the page's origin.
    pub y: f64,
    /// The type size it was shown at.
    pub size: f64,
    /// What it says.
    pub text: String,
}

/// One page.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPageText {
    /// Zero-based page index.
    pub index: usize,
    /// Every positioned run on it.
    pub items: Vec<PdfTextItem>,
    /// The rows those runs assemble into.
    pub rows: Vec<String>,
}

/// What a PDF says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PdfText {
    /// How many pages were readable.
    pub page_count: usize,
    /// The pages.
    pub pages: Vec<PdfPageText>,
    /// Every page's rows, in document order.
    pub rows: Vec<String>,
    /// Those rows joined with newlines.
    pub text: String,
    /// Everything the reader could not do, in Portuguese.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Name(String),
    Number(f64),
    Str(Vec<u8>),
    Ref(u32, u32),
    Array(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Bool(bool),
    Null,
}

impl Value {
    fn dict_get(&self, key: &str) -> Option<&Value> {
        match self {
            Self::Dict(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    fn as_name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name),
            _ => None,
        }
    }

    fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(number) => Some(*number),
            _ => None,
        }
    }

    fn as_dict(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Dict(entries) => Some(entries),
            _ => None,
        }
    }
}

/// One byte of the file and where the cursor is.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.data.get(self.pos + offset).copied()
    }

    fn rest(&self) -> &[u8] {
        self.data.get(self.pos..).unwrap_or(&[])
    }

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.rest().starts_with(needle)
    }
}

/// The whitespace set the PDF grammar names, which is also ASCII whitespace.
fn is_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

/// The characters that end a token.
fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn skip_whitespace(cursor: &mut Cursor<'_>) {
    while let Some(byte) = cursor.peek() {
        if is_whitespace(byte) {
            cursor.pos += 1;
            continue;
        }
        if byte == b'%' {
            while let Some(inner) = cursor.peek() {
                if inner == b'\n' || inner == b'\r' {
                    break;
                }
                cursor.pos += 1;
            }
            continue;
        }
        return;
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn read_hex_string(cursor: &mut Cursor<'_>) -> Vec<u8> {
    cursor.pos += 1; // the '<'
    let mut digits: Vec<u8> = Vec::new();
    while let Some(byte) = cursor.peek() {
        if byte == b'>' {
            break;
        }
        if hex_value(byte).is_some() {
            digits.push(byte);
        }
        cursor.pos += 1;
    }
    cursor.pos += 1; // the '>'
    if digits.len() % 2 == 1 {
        digits.push(b'0');
    }
    digits
        .chunks(2)
        .filter_map(|pair| {
            let high = hex_value(*pair.first()?)?;
            let low = hex_value(*pair.get(1)?)?;
            Some(high * 16 + low)
        })
        .collect()
}

fn read_literal_string(cursor: &mut Cursor<'_>) -> Vec<u8> {
    cursor.pos += 1; // the '('
    let mut depth = 1_i32;
    let mut out: Vec<u8> = Vec::new();
    while let Some(byte) = cursor.peek() {
        cursor.pos += 1;
        if byte == b'\\' {
            let Some(escape) = cursor.peek() else {
                break;
            };
            cursor.pos += 1;
            match escape {
                b'n' => out.push(b'\n'),
                b'r' => out.push(b'\r'),
                b't' => out.push(b'\t'),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'\n' => {}
                b'\r' => {
                    if cursor.peek() == Some(b'\n') {
                        cursor.pos += 1;
                    }
                }
                b'0'..=b'7' => {
                    let mut octal = vec![escape];
                    while octal.len() < 3 {
                        match cursor.peek() {
                            Some(next @ b'0'..=b'7') => {
                                octal.push(next);
                                cursor.pos += 1;
                            }
                            _ => break,
                        }
                    }
                    let text = String::from_utf8_lossy(&octal);
                    if let Ok(value) = u8::from_str_radix(&text, 8) {
                        out.push(value);
                    }
                }
                other => out.push(other),
            }
            continue;
        }
        if byte == b'(' {
            depth += 1;
            out.push(byte);
            continue;
        }
        if byte == b')' {
            depth -= 1;
            if depth == 0 {
                break;
            }
            out.push(byte);
            continue;
        }
        out.push(byte);
    }
    out
}

fn read_name(cursor: &mut Cursor<'_>) -> String {
    cursor.pos += 1; // the '/'
    let mut out: Vec<u8> = Vec::new();
    while let Some(byte) = cursor.peek() {
        if is_whitespace(byte) || is_delimiter(byte) {
            break;
        }
        cursor.pos += 1;
        if byte == b'#' {
            let first = cursor.peek().and_then(hex_value);
            let second = cursor.peek_at(1).and_then(hex_value);
            if let (Some(high), Some(low)) = (first, second) {
                out.push(high * 16 + low);
                cursor.pos += 2;
                continue;
            }
        }
        out.push(byte);
    }
    latin1_to_string(&out)
}

fn read_number_token(cursor: &mut Cursor<'_>) -> Option<f64> {
    let start = cursor.pos;
    while let Some(byte) = cursor.peek() {
        if matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9') {
            cursor.pos += 1;
        } else {
            break;
        }
    }
    if cursor.pos == start {
        return None;
    }
    let text = latin1_to_string(&cursor.data[start..cursor.pos]);
    match text.parse::<f64>() {
        Ok(value) if value.is_finite() => Some(value),
        _ => {
            cursor.pos = start;
            None
        }
    }
}

fn read_value(cursor: &mut Cursor<'_>, depth: usize) -> Value {
    skip_whitespace(cursor);
    if depth > 64 {
        return Value::Null;
    }
    let Some(byte) = cursor.peek() else {
        return Value::Null;
    };

    if byte == b'<' {
        if cursor.peek_at(1) == Some(b'<') {
            cursor.pos += 2;
            let mut entries: Vec<(String, Value)> = Vec::new();
            loop {
                skip_whitespace(cursor);
                let Some(next) = cursor.peek() else {
                    break;
                };
                if next == b'>' && cursor.peek_at(1) == Some(b'>') {
                    cursor.pos += 2;
                    break;
                }
                if next != b'/' {
                    // Not a key: consume one token so a malformed dictionary cannot
                    // spin forever.
                    let before = cursor.pos;
                    read_value(cursor, depth + 1);
                    if cursor.pos == before {
                        cursor.pos += 1;
                    }
                    continue;
                }
                let key = read_name(cursor);
                let value = read_value(cursor, depth + 1);
                entries.push((key, value));
            }
            return Value::Dict(entries);
        }
        return Value::Str(read_hex_string(cursor));
    }

    if byte == b'/' {
        return Value::Name(read_name(cursor));
    }

    if byte == b'[' {
        cursor.pos += 1;
        let mut items: Vec<Value> = Vec::new();
        loop {
            skip_whitespace(cursor);
            let Some(next) = cursor.peek() else {
                break;
            };
            if next == b']' {
                cursor.pos += 1;
                break;
            }
            let before = cursor.pos;
            items.push(read_value(cursor, depth + 1));
            if cursor.pos == before {
                cursor.pos += 1;
            }
        }
        return Value::Array(items);
    }

    if byte == b'(' {
        return Value::Str(read_literal_string(cursor));
    }

    if cursor.starts_with(b"true") {
        cursor.pos += 4;
        return Value::Bool(true);
    }
    if cursor.starts_with(b"false") {
        cursor.pos += 5;
        return Value::Bool(false);
    }
    if cursor.starts_with(b"null") {
        cursor.pos += 4;
        return Value::Null;
    }

    // A number — which may be the first half of an indirect reference ("12 0 R").
    let Some(first) = read_number_token(cursor) else {
        cursor.pos = (cursor.pos + 1).min(cursor.data.len());
        return Value::Null;
    };
    let after_first = cursor.pos;
    skip_whitespace(cursor);
    if let Some(second) = read_number_token(cursor) {
        skip_whitespace(cursor);
        if cursor.peek() == Some(b'R') {
            cursor.pos += 1;
            let num = if first >= 0.0 { first as u32 } else { 0 };
            let generation = if second >= 0.0 { second as u32 } else { 0 };
            return Value::Ref(num, generation);
        }
    }
    cursor.pos = after_first;
    Value::Number(first)
}

/// One byte per character, which is the only decoding under which one byte is one char.
fn latin1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// One object of the file.
struct PdfObject {
    num: u32,
    dict: Value,
    stream_start: Option<usize>,
    stream_end: Option<usize>,
    decoded: Option<Vec<u8>>,
    decode_failed: bool,
}

/// Whether a file claims to be a PDF.
#[must_use]
pub fn looks_like_pdf(bytes: &[u8]) -> bool {
    let head = latin1_to_string(&bytes[..bytes.len().min(1024)]);
    let trimmed = head.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
    trimmed.starts_with("%PDF-") && {
        let rest = &trimmed[5..];
        let mut chars = rest.chars();
        let digits = chars.by_ref().take_while(char::is_ascii_digit).count();
        digits > 0 && rest[digits..].starts_with('.')
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Find the next `N G obj` header at or after `from`.
fn find_object_header(data: &[u8], from: usize) -> Option<(u32, u32, usize)> {
    let mut index = from;
    while index < data.len() {
        // The number, one to ten digits.
        let number_start = index;
        let mut cursor = index;
        let mut digits = 0;
        while cursor < data.len() && data[cursor].is_ascii_digit() && digits < 10 {
            cursor += 1;
            digits += 1;
        }
        if digits == 0 || digits == 10 {
            index += 1;
            continue;
        }
        let num: u32 = latin1_to_string(&data[number_start..cursor]).parse().unwrap_or(0);
        // Whitespace, then the generation, at most five digits.
        let mut generation_start = cursor;
        while generation_start < data.len() && is_whitespace(data[generation_start]) {
            generation_start += 1;
        }
        if generation_start == cursor {
            index += 1;
            continue;
        }
        let mut generation_end = generation_start;
        let mut generation_digits = 0;
        while generation_end < data.len() && data[generation_end].is_ascii_digit() && generation_digits < 5 {
            generation_end += 1;
            generation_digits += 1;
        }
        if generation_digits == 0 || generation_digits == 5 {
            index += 1;
            continue;
        }
        let generation: u32 = latin1_to_string(&data[generation_start..generation_end])
            .parse()
            .unwrap_or(0);
        // Whitespace, then `obj`.
        let mut keyword = generation_end;
        while keyword < data.len() && is_whitespace(data[keyword]) {
            keyword += 1;
        }
        if keyword == generation_end || !data[keyword..].starts_with(b"obj") {
            index += 1;
            continue;
        }
        let after = keyword + 3;
        // `\b` after the keyword: the next byte must not be a word character.
        if data.get(after).copied().is_some_and(is_word_byte) {
            index += 1;
            continue;
        }
        return Some((num, generation, after));
    }
    None
}

fn parse_objects(data: &[u8]) -> Vec<PdfObject> {
    let mut objects: Vec<PdfObject> = Vec::new();
    let mut from = 0;
    while let Some((num, _gen, body_start)) = find_object_header(data, from) {
        from = body_start;
        if objects.iter().any(|object| object.num == num) {
            continue;
        }
        let mut cursor = Cursor {
            data,
            pos: body_start,
        };
        let value = read_value(&mut cursor, 0);
        if !matches!(value, Value::Dict(_)) {
            continue;
        }
        let mut stream_start = None;
        let mut stream_end = None;
        let mut probe = Cursor {
            data,
            pos: cursor.pos,
        };
        skip_whitespace(&mut probe);
        if probe.starts_with(b"stream") {
            let mut start = probe.pos + 6;
            if data.get(start) == Some(&b'\r') {
                start += 1;
            }
            if data.get(start) == Some(&b'\n') {
                start += 1;
            }
            let declared = value
                .dict_get("Length")
                .and_then(Value::as_number)
                .filter(|length| *length >= 0.0)
                .map(|length| start + length as usize);
            let endstream_at = find(data, b"endstream", start);
            let fallback = endstream_at.unwrap_or(data.len());
            stream_start = Some(start);
            stream_end = Some(match declared {
                Some(candidate) if candidate > start && candidate <= fallback => candidate,
                _ => fallback,
            });
        }
        objects.push(PdfObject {
            num,
            dict: value,
            stream_start,
            stream_end,
            decoded: None,
            decode_failed: false,
        });
    }
    objects
}

/// `indexOf` for a byte slice.
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| offset + from)
}

fn resolve<'a>(objects: &'a [PdfObject], value: &'a Value) -> Option<&'a Value> {
    let mut current = value;
    for _ in 0..32 {
        match current {
            Value::Ref(num, _) => {
                let target = objects.iter().find(|object| object.num == *num)?;
                current = &target.dict;
            }
            other => return Some(other),
        }
    }
    None
}

fn filter_names(objects: &[PdfObject], dict: &Value) -> Vec<String> {
    let Some(filter) = dict.dict_get("Filter").and_then(|value| resolve(objects, value)) else {
        return Vec::new();
    };
    match filter {
        Value::Name(name) => vec![name.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| resolve(objects, item).and_then(Value::as_name).map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn trim_trailing_eol(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 {
        let byte = bytes[end - 1];
        if byte == b'\n' || byte == b'\r' {
            end -= 1;
        } else {
            break;
        }
    }
    &bytes[..end]
}

/// Decode every stream once, remembering which ones refused.
///
/// The reference decoded lazily and remembered the result on the object; decoding
/// everything up front is the same answer with no interior mutability, and the cost
/// for a form-sized document is nothing.
fn decode_streams(objects: &mut [PdfObject], data: &[u8]) {
    for index in 0..objects.len() {
        let (Some(start), Some(end)) = (objects[index].stream_start, objects[index].stream_end) else {
            continue;
        };
        let filters = filter_names(objects, &objects[index].dict);
        if filters.iter().any(|name| name != "FlateDecode") {
            objects[index].decode_failed = true;
            continue;
        }
        let slice = &data[start.min(data.len())..end.clamp(start, data.len())];
        if filters.is_empty() {
            objects[index].decoded = Some(slice.to_vec());
            continue;
        }
        let mut decoded = None;
        for candidate in [slice, trim_trailing_eol(slice)] {
            if let Ok(bytes) = miniz_oxide::inflate::decompress_to_vec_zlib(candidate) {
                decoded = Some(bytes);
                break;
            }
            if let Ok(bytes) = miniz_oxide::inflate::decompress_to_vec(candidate) {
                decoded = Some(bytes);
                break;
            }
        }
        match decoded {
            Some(bytes) => objects[index].decoded = Some(bytes),
            None => objects[index].decode_failed = true,
        }
    }
}

fn stream_bytes(objects: &[PdfObject], num: u32) -> Option<&[u8]> {
    objects
        .iter()
        .find(|object| object.num == num)
        .and_then(|object| object.decoded.as_deref())
}

/// The 0x80–0x9F range of cp1252, where it differs from latin-1.
const CP1252_HIGH: [u16; 32] = [
    0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152,
    0x008d, 0x017d, 0x008f, 0x0090, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122,
    0x0161, 0x203a, 0x0153, 0x009d, 0x017e, 0x0178,
];

/// Decode bytes as Windows-1252, which is what a one-byte font means.
#[must_use]
pub fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if (0x80..=0x9f).contains(byte) {
                char::from_u32(u32::from(CP1252_HIGH[usize::from(byte - 0x80)]))
                    .unwrap_or(char::REPLACEMENT_CHARACTER)
            } else {
                char::from(*byte)
            }
        })
        .collect()
}

/// One token of a CMap: the only shapes `parse_to_unicode` needs.
fn cmap_tokens(source: &str) -> Vec<String> {
    let bytes: Vec<char> = source.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let ch = bytes[index];
        if ch == '<' {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end] != '>' {
                end += 1;
            }
            let text: String = bytes[index..=(end.min(bytes.len() - 1))].iter().collect();
            tokens.push(text);
            index = end + 1;
            continue;
        }
        if ch == '[' || ch == ']' {
            tokens.push(ch.to_string());
            index += 1;
            continue;
        }
        if ch.is_ascii_alphabetic() {
            let mut end = index;
            while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                end += 1;
            }
            tokens.push(bytes[index..end].iter().collect());
            index = end;
            continue;
        }
        if ch.is_ascii_digit() || ch == '-' {
            let mut end = index;
            if bytes[end] == '-' {
                end += 1;
            }
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            tokens.push(bytes[index..end].iter().collect());
            index = end;
            continue;
        }
        index += 1;
    }
    tokens
}

/// Hex digits as UTF-16BE text, four at a time.
///
/// A `/ToUnicode` destination is UTF-16BE, so four digits is one code unit: reading
/// two at a time would turn `<0056>` into NUL followed by "V".
fn hex_to_text(hex: &str) -> String {
    let digits: Vec<char> = hex.chars().collect();
    let parse = |slice: &[char]| -> u32 {
        let text: String = slice.iter().collect();
        u32::from_str_radix(&text, 16).unwrap_or(0)
    };
    if digits.len().is_multiple_of(4) {
        let mut units: Vec<u16> = Vec::new();
        let mut index = 0;
        while index + 3 < digits.len() {
            units.push(parse(&digits[index..index + 4]) as u16);
            index += 4;
        }
        return String::from_utf16_lossy(&units);
    }
    digits
        .chunks(2)
        .map(|pair| char::from_u32(parse(pair)).unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// A `/ToUnicode` CMap: character codes in, text out.
#[must_use]
pub fn parse_to_unicode(source: &str) -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    let tokens = cmap_tokens(source);
    let mut index = 0_usize;

    while index < tokens.len() {
        let token = tokens[index].as_str();
        if token == "beginbfchar" {
            index += 1;
            while index < tokens.len() && tokens[index] != "endbfchar" {
                let from = tokens.get(index).cloned().unwrap_or_default();
                let to = tokens.get(index + 1).cloned().unwrap_or_default();
                index += 2;
                if from.starts_with('<') && to.starts_with('<') {
                    let code = u32::from_str_radix(from.trim_matches(['<', '>']), 16).unwrap_or(0);
                    let inner = to.trim_matches(['<', '>']).to_owned();
                    map.insert(code, hex_to_text(&inner));
                }
            }
        } else if token == "beginbfrange" {
            index += 1;
            while index < tokens.len() && tokens[index] != "endbfrange" {
                let low = tokens.get(index).cloned().unwrap_or_default();
                let high = tokens.get(index + 1).cloned().unwrap_or_default();
                let target = tokens.get(index + 2).cloned().unwrap_or_default();
                index += 3;
                if !low.starts_with('<') || !high.starts_with('<') {
                    continue;
                }
                let from = i64::from_str_radix(low.trim_matches(['<', '>']), 16).unwrap_or(0);
                let to = i64::from_str_radix(high.trim_matches(['<', '>']), 16).unwrap_or(0);
                if to < from || to - from > 65535 {
                    continue;
                }
                if target.starts_with('<') {
                    let base = target.trim_matches(['<', '>']);
                    let tail_start = base.len().saturating_sub(4);
                    let tail = i64::from_str_radix(&base[tail_start..], 16).unwrap_or(0);
                    let head = &base[..tail_start];
                    for code in from..=to {
                        let suffix = format!("{:04x}", tail + (code - from));
                        map.insert(code as u32, hex_to_text(&format!("{head}{suffix}")));
                    }
                } else if target == "[" {
                    let mut offset = 0_i64;
                    while index < tokens.len() && tokens[index] != "]" {
                        let item = tokens[index].clone();
                        index += 1;
                        if item.starts_with('<') {
                            let inner = item.trim_matches(['<', '>']).to_owned();
                            map.insert((from + offset) as u32, hex_to_text(&inner));
                        }
                        offset += 1;
                    }
                }
            }
        }
        index += 1;
    }
    map
}

/// How to turn a font's bytes into text.
enum FontDecoder {
    /// A character map, with the width of a character code in bytes.
    CMap(BTreeMap<u32, String>, usize),
    /// One byte per character.
    Cp1252,
    /// Nothing can be read from this font, and the reason was already reported.
    Silent,
}

impl FontDecoder {
    fn decode(&self, bytes: &[u8]) -> String {
        match self {
            Self::CMap(map, step) => {
                let mut text = String::new();
                let mut index = 0;
                while index + step - 1 < bytes.len() {
                    let code = if *step == 2 {
                        (u32::from(bytes[index]) << 8) | u32::from(bytes[index + 1])
                    } else {
                        u32::from(bytes[index])
                    };
                    if let Some(mapped) = map.get(&code) {
                        text.push_str(mapped);
                    }
                    index += step;
                }
                text
            }
            Self::Cp1252 => decode_cp1252(bytes),
            Self::Silent => String::new(),
        }
    }
}

fn build_font_decoder(
    objects: &[PdfObject],
    font_dict: Option<&Value>,
    problems: &mut Vec<String>,
    font_label: &str,
) -> FontDecoder {
    let two_byte = font_dict
        .and_then(|dict| dict.dict_get("Subtype"))
        .and_then(|value| resolve(objects, value))
        == Some(&Value::Name("Type0".to_owned()));

    if let Some(Value::Ref(num, _)) = font_dict.and_then(|dict| dict.dict_get("ToUnicode"))
        && let Some(cmap) = stream_bytes(objects, *num)
    {
        let map = parse_to_unicode(&latin1_to_string(cmap));
        if !map.is_empty() {
            return FontDecoder::CMap(map, if two_byte { 2 } else { 1 });
        }
    }

    if two_byte {
        // A two-byte font with no character map holds glyph ids, and a glyph id is not
        // a character. Saying so is the only honest answer.
        problems.push(format!(
            "A fonte {font_label} não traz mapa de caracteres (/ToUnicode): o texto escrito com \
             ela não foi lido."
        ));
        return FontDecoder::Silent;
    }

    FontDecoder::Cp1252
}

#[derive(Debug, Clone, PartialEq)]
enum Operand {
    Str(Vec<u8>),
    Number(f64),
    Name(String),
    Array(Vec<Operand>),
    Other,
}

fn read_operand(cursor: &mut Cursor<'_>) -> Option<Operand> {
    skip_whitespace(cursor);
    let byte = cursor.peek()?;
    if byte == b'(' {
        return Some(Operand::Str(read_literal_string(cursor)));
    }
    if byte == b'<' {
        if cursor.peek_at(1) == Some(b'<') {
            return Some(Operand::Other);
        }
        return Some(Operand::Str(read_hex_string(cursor)));
    }
    if byte == b'/' {
        return Some(Operand::Name(read_name(cursor)));
    }
    if byte == b'[' {
        cursor.pos += 1;
        let mut items: Vec<Operand> = Vec::new();
        loop {
            skip_whitespace(cursor);
            let Some(next) = cursor.peek() else {
                break;
            };
            if next == b']' {
                cursor.pos += 1;
                break;
            }
            let before = cursor.pos;
            if let Some(item) = read_operand(cursor)
                && !matches!(item, Operand::Other)
            {
                items.push(item);
            }
            if cursor.pos == before {
                cursor.pos += 1;
            }
        }
        return Some(Operand::Array(items));
    }
    let saved = cursor.pos;
    if let Some(number) = read_number_token(cursor) {
        return Some(Operand::Number(number));
    }
    cursor.pos = saved;
    Some(Operand::Other)
}

type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn multiply(left: &Matrix, right: &Matrix) -> Matrix {
    [
        left[0] * right[0] + left[2] * right[1],
        left[1] * right[0] + left[3] * right[1],
        left[0] * right[2] + left[2] * right[3],
        left[1] * right[2] + left[3] * right[3],
        left[0] * right[4] + left[2] * right[5] + left[4],
        left[1] * right[4] + left[3] * right[5] + left[5],
    ]
}

struct TextState {
    font_name: String,
    font_size: f64,
    leading: f64,
    tm: Matrix,
    tlm: Matrix,
}

/// One page's content streams into positioned text items.
///
/// Only the START position of each shown string is kept: rows are built by baseline,
/// so an approximated advance width would add error to the one coordinate that matters
/// and none to the one that does not.
fn render_content(
    data: &[u8],
    fonts: &BTreeMap<String, FontDecoder>,
    items: &mut Vec<PdfTextItem>,
    problems: &mut Vec<String>,
) {
    let mut cursor = Cursor { data, pos: 0 };
    let mut operands: Vec<Operand> = Vec::new();
    let mut ctm_stack: Vec<Matrix> = Vec::new();
    let mut ctm = IDENTITY;
    let mut state = TextState {
        font_name: String::new(),
        font_size: 0.0,
        leading: 0.0,
        tm: IDENTITY,
        tlm: IDENTITY,
    };
    let mut missing_fonts: Vec<String> = Vec::new();

    let show_string = |bytes: &[u8],
                       state: &TextState,
                       ctm: &Matrix,
                       items: &mut Vec<PdfTextItem>,
                       problems: &mut Vec<String>,
                       missing_fonts: &mut Vec<String>| {
        let size = if state.font_size > 0.0 {
            state.font_size
        } else {
            10.0
        };
        let placed = multiply(ctm, &state.tm);
        match fonts.get(&state.font_name) {
            Some(decoder) => {
                let text = decoder.decode(bytes);
                if text.is_empty() {
                    return;
                }
                items.push(PdfTextItem {
                    x: placed[4],
                    y: placed[5],
                    size,
                    text,
                });
            }
            None => {
                if !state.font_name.is_empty() && !missing_fonts.contains(&state.font_name) {
                    missing_fonts.push(state.font_name.clone());
                    problems.push(format!(
                        "O texto usa a fonte {}, que não está declarada na página.",
                        state.font_name
                    ));
                }
                // No declared font: cp1252 is the best reading of the bytes, and the row
                // parser will simply not find the labels if that reading is wrong.
                let text = decode_cp1252(bytes);
                if text.is_empty() {
                    return;
                }
                items.push(PdfTextItem {
                    x: placed[4],
                    y: placed[5],
                    size,
                    text,
                });
            }
        }
    };

    loop {
        skip_whitespace(&mut cursor);
        if cursor.peek().is_none() {
            break;
        }
        let before = cursor.pos;
        let Some(operand) = read_operand(&mut cursor) else {
            break;
        };
        if !matches!(operand, Operand::Other) {
            operands.push(operand);
            if operands.len() > 64 {
                let excess = operands.len() - 64;
                operands.drain(0..excess);
            }
            continue;
        }

        let start = cursor.pos;
        while let Some(byte) = cursor.peek() {
            if is_whitespace(byte) || is_delimiter(byte) {
                break;
            }
            cursor.pos += 1;
        }
        if cursor.pos == before {
            cursor.pos += 1;
            continue;
        }
        let operator = latin1_to_string(&data[start..cursor.pos]);
        let numbers: Vec<f64> = operands
            .iter()
            .filter_map(|item| match item {
                Operand::Number(value) => Some(*value),
                _ => None,
            })
            .collect();
        let arg = |index: usize, fallback: f64| numbers.get(index).copied().unwrap_or(fallback);

        match operator.as_str() {
            "q" => ctm_stack.push(ctm),
            "Q" => ctm = ctm_stack.pop().unwrap_or(IDENTITY),
            "cm" => {
                if numbers.len() >= 6 {
                    let matrix = [
                        arg(0, 1.0),
                        arg(1, 0.0),
                        arg(2, 0.0),
                        arg(3, 1.0),
                        arg(4, 0.0),
                        arg(5, 0.0),
                    ];
                    ctm = multiply(&ctm, &matrix);
                }
            }
            "BT" => {
                state.tm = IDENTITY;
                state.tlm = IDENTITY;
            }
            "Tf" => {
                state.font_name = operands
                    .iter()
                    .find_map(|item| match item {
                        Operand::Name(name) => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                state.font_size = numbers.last().copied().unwrap_or(0.0);
            }
            "TL" => state.leading = arg(0, 0.0),
            "Td" | "TD" => {
                let tx = arg(0, 0.0);
                let ty = arg(1, 0.0);
                if operator == "TD" {
                    state.leading = -ty;
                }
                let matrix = [1.0, 0.0, 0.0, 1.0, tx, ty];
                state.tlm = multiply(&state.tlm, &matrix);
                state.tm = state.tlm;
            }
            "Tm" => {
                if numbers.len() >= 6 {
                    state.tm = [
                        arg(0, 1.0),
                        arg(1, 0.0),
                        arg(2, 0.0),
                        arg(3, 1.0),
                        arg(4, 0.0),
                        arg(5, 0.0),
                    ];
                    state.tlm = state.tm;
                }
            }
            "T*" => {
                let matrix = [1.0, 0.0, 0.0, 1.0, 0.0, -state.leading];
                state.tlm = multiply(&state.tlm, &matrix);
                state.tm = state.tlm;
            }
            "Tj" | "'" | "\"" => {
                if operator != "Tj" {
                    let matrix = [1.0, 0.0, 0.0, 1.0, 0.0, -state.leading];
                    state.tlm = multiply(&state.tlm, &matrix);
                    state.tm = state.tlm;
                }
                for item in &operands {
                    if let Operand::Str(bytes) = item {
                        show_string(bytes, &state, &ctm, items, problems, &mut missing_fonts);
                    }
                }
            }
            "TJ" => {
                if let Some(Operand::Array(array)) = operands
                    .iter()
                    .rev()
                    .find(|item| matches!(item, Operand::Array(_)))
                {
                    for item in array {
                        if let Operand::Str(bytes) = item {
                            show_string(bytes, &state, &ctm, items, problems, &mut missing_fonts);
                        }
                    }
                }
            }
            _ => {}
        }
        operands.clear();
    }
}

/// How far apart two baselines can be and still be the same line of a form.
fn row_tolerance(size: f64) -> f64 {
    (size * 0.45).clamp(2.0, 3.5)
}

/// Positioned items into lines.
///
/// Two items belong to the same line when their baselines are within a fraction of the
/// type size. That fraction is what keeps a label and the value beside it on one line
/// while leaving the next table row — four points lower, in the forms this targets — on
/// its own.
#[must_use]
pub fn items_to_rows(items: &[PdfTextItem]) -> Vec<String> {
    let mut sorted: Vec<&PdfTextItem> = items.iter().collect();
    sorted.sort_by(|left, right| {
        right
            .y
            .partial_cmp(&left.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.x.partial_cmp(&right.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut rows: Vec<(f64, f64, Vec<&PdfTextItem>)> = Vec::new();
    for item in sorted {
        if let Some(current) = rows.last_mut()
            && (current.0 - item.y).abs() <= row_tolerance(current.1)
        {
            current.2.push(item);
            current.1 = current.1.max(item.size);
            continue;
        }
        rows.push((item.y, item.size, vec![item]));
    }

    rows.into_iter()
        .map(|(_, _, members)| {
            let mut ordered = members;
            ordered.sort_by(|left, right| left.x.partial_cmp(&right.x).unwrap_or(std::cmp::Ordering::Equal));
            let joined = ordered
                .iter()
                .map(|item| item.text.as_str())
                .collect::<Vec<&str>>()
                .join(" ");
            collapse_spaces(&joined.replace(['\u{a0}', '\u{202f}'], " "))
        })
        .collect()
}

/// `String.prototype.replace(/\s+/g, ' ')` followed by `trim`, with JavaScript's set of
/// spaces rather than Rust's — they differ on exactly the characters a PDF produces.
pub(crate) fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for ch in text.chars() {
        if is_js_space(ch) {
            in_space = true;
            continue;
        }
        if in_space && !out.is_empty() {
            out.push(' ');
        }
        in_space = false;
        out.push(ch);
    }
    out
}

/// JavaScript's `\s`, which is what the reference used to collapse a row.
fn is_js_space(ch: char) -> bool {
    matches!(
        ch,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// One page's resources and content, before rendering.
struct PageRef {
    resources: Option<Value>,
    contents: Vec<u32>,
}

fn content_objects(objects: &[PdfObject], dict: &Value) -> Vec<u32> {
    // Resolve first: `/Contents` is usually one reference to a stream, but it may be one
    // reference to an ARRAY of streams. When it is not an array, the value to read is the
    // unresolved reference itself — resolving it and then looking for a reference again
    // finds a dictionary and silently produces no content at all.
    let raw = dict.dict_get("Contents");
    let resolved = raw.and_then(|value| resolve(objects, value));
    let refs: Vec<&Value> = match resolved {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => raw.into_iter().collect(),
    };
    refs.into_iter()
        .filter_map(|value| match value {
            Value::Ref(num, _) => Some(*num),
            _ => None,
        })
        .filter(|num| objects.iter().any(|object| object.num == *num))
        .collect()
}

fn collect_pages(objects: &[PdfObject], problems: &mut Vec<String>) -> Vec<PageRef> {
    let mut pages: Vec<PageRef> = Vec::new();
    let mut by_number: Vec<&PdfObject> = objects.iter().collect();
    by_number.sort_by_key(|object| object.num);
    let catalog = by_number
        .iter()
        .find(|object| object.dict.dict_get("Type").and_then(Value::as_name) == Some("Catalog"));

    fn visit(
        objects: &[PdfObject],
        pages: &mut Vec<PageRef>,
        value: Option<&Value>,
        inherited: Option<&Value>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        let Some(value) = value else {
            return;
        };
        let Some(resolved) = resolve(objects, value) else {
            return;
        };
        if resolved.as_dict().is_none() {
            return;
        }
        let resources = resolved.dict_get("Resources").or(inherited).cloned();
        if resolved.dict_get("Type").and_then(Value::as_name) == Some("Page") {
            pages.push(PageRef {
                resources,
                contents: content_objects(objects, resolved),
            });
            return;
        }
        if let Some(Value::Array(kids)) = resolved
            .dict_get("Kids")
            .and_then(|value| resolve(objects, value))
        {
            for kid in kids.clone() {
                visit(objects, pages, Some(&kid), resources.as_ref(), depth + 1);
            }
        }
    }

    if let Some(catalog) = catalog {
        visit(objects, &mut pages, catalog.dict.dict_get("Pages"), None, 0);
    }

    if pages.is_empty() {
        // No usable page tree: fall back to every object that calls itself a page, which
        // is what a damaged or hand-built file usually still has.
        for object in &by_number {
            if object.dict.dict_get("Type").and_then(Value::as_name) != Some("Page") {
                continue;
            }
            pages.push(PageRef {
                resources: object.dict.dict_get("Resources").cloned(),
                contents: content_objects(objects, &object.dict),
            });
        }
        if !pages.is_empty() {
            problems.push(
                "A árvore de páginas do PDF não pôde ser lida; foram usados os objetos marcados \
                 como página."
                    .to_owned(),
            );
        }
    }
    pages
}

fn collect_fonts(
    objects: &[PdfObject],
    resources: Option<&Value>,
    problems: &mut Vec<String>,
) -> BTreeMap<String, FontDecoder> {
    let mut fonts = BTreeMap::new();
    let Some(resources) = resources.and_then(|value| resolve(objects, value)) else {
        return fonts;
    };
    let Some(font_resources) = resources
        .dict_get("Font")
        .and_then(|value| resolve(objects, value))
    else {
        return fonts;
    };
    let Some(entries) = font_resources.as_dict() else {
        return fonts;
    };
    for (name, value) in entries {
        let font_object = resolve(objects, value);
        let decoder = build_font_decoder(objects, font_object, problems, name);
        fonts.insert(name.clone(), decoder);
    }
    fonts
}

/// Read a PDF's text.
#[must_use]
pub fn extract_pdf_text(bytes: &[u8]) -> PdfText {
    let mut problems: Vec<String> = Vec::new();
    if !looks_like_pdf(bytes) {
        return PdfText {
            problems: vec!["O ficheiro não começa por \"%PDF-\": não parece ser um PDF.".to_owned()],
            ..PdfText::default()
        };
    }

    let raw = latin1_to_string(bytes);
    if raw.contains("/Encrypt") {
        return PdfText {
            problems: vec![
                "O PDF está protegido (cifrado). Esta aplicação não abre PDFs com palavra-passe nem \
                 com restrições de cópia: retira a proteção no programa que o emitiu e volta a \
                 tentar. Se só queres guardar o ficheiro no cofre, usa «Guardar documento no cofre»."
                    .to_owned(),
            ],
            ..PdfText::default()
        };
    }
    if raw.contains("/ObjStm") {
        problems.push(
            "O PDF guarda os objetos em fluxos comprimidos (Object Streams), uma forma que este \
             leitor não abre. Se os campos não forem reconhecidos, guarda o documento no cofre e \
             registra a fatura à mão."
                .to_owned(),
        );
    }

    let mut objects = parse_objects(bytes);
    if objects.is_empty() {
        return PdfText {
            problems: vec!["O PDF não tem objetos legíveis: o ficheiro pode estar truncado.".to_owned()],
            ..PdfText::default()
        };
    }
    decode_streams(&mut objects, bytes);

    let pages = collect_pages(&objects, &mut problems);
    if pages.is_empty() {
        problems.push("Não foi encontrada nenhuma página no PDF.".to_owned());
    }

    let mut result: Vec<PdfPageText> = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        let fonts = collect_fonts(&objects, page.resources.as_ref(), &mut problems);
        let mut items: Vec<PdfTextItem> = Vec::new();
        for num in &page.contents {
            let Some(data) = stream_bytes(&objects, *num) else {
                let failed = objects
                    .iter()
                    .find(|object| object.num == *num)
                    .is_some_and(|object| object.decode_failed);
                if failed {
                    problems.push("Um dos fluxos de conteúdo do PDF não pôde ser descomprimido.".to_owned());
                }
                continue;
            };
            render_content(data, &fonts, &mut items, &mut problems);
        }
        let rows: Vec<String> = items_to_rows(&items)
            .into_iter()
            .filter(|row| !row.is_empty())
            .collect();
        result.push(PdfPageText { index, items, rows });
    }

    if !result.is_empty() && result.iter().all(|page| page.rows.is_empty()) {
        problems.push(
            "Não foi encontrado texto no PDF. Se for um documento digitalizado (uma imagem), não há \
             texto para ler: o ficheiro pode ser guardado no cofre e a fatura registada à mão."
                .to_owned(),
        );
    }

    let rows: Vec<String> = result.iter().flat_map(|page| page.rows.clone()).collect();
    let text = rows.join("\n");
    // `[...new Set(problems)]`: the same complaint twice is the same complaint.
    let mut unique: Vec<String> = Vec::new();
    for problem in problems {
        if !unique.contains(&problem) {
            unique.push(problem);
        }
    }
    PdfText {
        page_count: result.len(),
        pages: result,
        rows,
        text,
        problems: unique,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_does_not_claim_to_be_a_pdf_is_refused() {
        let result = extract_pdf_text(b"isto nao e um pdf");
        assert_eq!(result.page_count, 0);
        assert!(result.problems[0].contains("%PDF-"));
        assert!(!looks_like_pdf(b"isto nao e um pdf"));
        assert!(looks_like_pdf(b"%PDF-1.7\n..."));
        assert!(looks_like_pdf(b"  \n%PDF-1.4\n"));
        assert!(!looks_like_pdf(b"%PDFX"));
    }

    #[test]
    fn an_encrypted_file_is_named_as_such_rather_than_read() {
        let result = extract_pdf_text(b"%PDF-1.7\n1 0 obj << /Encrypt 2 0 R >> endobj\n%%EOF");
        assert_eq!(result.page_count, 0);
        assert!(result.problems[0].contains("protegido"));
    }

    #[test]
    fn a_truncated_file_says_so_instead_of_returning_nothing() {
        let result = extract_pdf_text(b"%PDF-1.7\nnada aqui\n%%EOF");
        assert_eq!(result.page_count, 0);
        assert!(result.problems.iter().any(|problem| problem.contains("truncado")));
    }

    #[test]
    fn cp1252_decoding_uses_the_table_not_latin1() {
        // 0xE7 is "ç" in both, but 0x80 is a euro sign only in cp1252.
        assert_eq!(decode_cp1252(&[0x63, 0x61, 0x66, 0xe9]), "café");
        assert_eq!(decode_cp1252(&[0x80]), "€");
        assert_eq!(decode_cp1252(&[0x96]), "–");
        // And a byte outside the table is the same character latin-1 would give.
        assert_eq!(decode_cp1252(&[0xe7]), "ç");
    }

    #[test]
    fn a_unicode_cmap_is_read_four_hex_digits_at_a_time() {
        let source = "1 beginbfchar <0001> <0056> endbfchar";
        let map = parse_to_unicode(source);
        assert_eq!(map.get(&1), Some(&"V".to_owned()));
    }

    #[test]
    fn a_unicode_cmap_range_walks_up_from_its_base() {
        let source = "1 beginbfrange <0010> <0012> <0041> endbfrange";
        let map = parse_to_unicode(source);
        assert_eq!(map.get(&0x10), Some(&"A".to_owned()));
        assert_eq!(map.get(&0x11), Some(&"B".to_owned()));
        assert_eq!(map.get(&0x12), Some(&"C".to_owned()));
    }

    fn item(x: f64, y: f64, size: f64, text: &str) -> PdfTextItem {
        PdfTextItem {
            x,
            y,
            size,
            text: text.to_owned(),
        }
    }

    #[test]
    fn items_on_one_baseline_become_one_row_read_left_to_right() {
        let rows = items_to_rows(&[
            item(120.0, 700.0, 10.0, "100,00"),
            item(30.0, 700.0, 10.0, "Base:"),
            item(30.0, 685.0, 10.0, "IVA:"),
            item(120.0, 685.0, 10.0, "23,00"),
        ]);
        assert_eq!(rows, vec!["Base: 100,00", "IVA: 23,00"]);
    }

    #[test]
    fn a_row_never_keeps_a_stray_space_or_a_narrow_no_break() {
        let rows = items_to_rows(&[
            item(10.0, 100.0, 10.0, "a\u{202f}b"),
            item(60.0, 100.0, 10.0, "c"),
        ]);
        assert_eq!(rows, vec!["a b c"]);
    }

    #[test]
    fn a_baseline_gap_wider_than_the_tolerance_starts_a_new_row() {
        let rows = items_to_rows(&[item(10.0, 100.0, 10.0, "um"), item(10.0, 96.0, 10.0, "dois")]);
        assert_eq!(
            rows.len(),
            2,
            "four points apart is a different row for 10 pt type: {rows:?}"
        );
    }

    #[test]
    fn a_real_pdf_written_by_this_test_reads_back_its_own_text() {
        // A minimal, uncompressed PDF with one page and one string, so the parser's
        // object graph, page tree and content stream are exercised end to end.
        let pdf = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >> endobj\n\
4 0 obj << /Length 44 >> stream\n\
BT /F1 12 Tf 72 700 Td (Fatura 123) Tj ET\n\
endstream endobj\n\
5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n\
trailer << /Root 1 0 R >>\n%%EOF\n";
        let result = extract_pdf_text(pdf);
        assert_eq!(result.page_count, 1);
        assert_eq!(result.rows, vec!["Fatura 123"]);
        assert_eq!(result.text, "Fatura 123");
        assert!(result.problems.is_empty(), "{:?}", result.problems);
    }

    #[test]
    fn a_flate_compressed_content_stream_is_inflated() {
        // The same document, with the content stream deflated.
        let content = b"BT /F1 12 Tf 72 700 Td (Recibo 7) Tj ET";
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(content, 6);
        let mut pdf: Vec<u8> = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        pdf.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >> endobj\n",
        );
        pdf.extend_from_slice(
            format!(
                "4 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
                compressed.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        pdf.extend_from_slice(b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\n");
        pdf.extend_from_slice(b"%%EOF\n");

        let result = extract_pdf_text(&pdf);
        assert_eq!(result.rows, vec!["Recibo 7"]);
        assert!(result.problems.is_empty(), "{:?}", result.problems);
    }
}
