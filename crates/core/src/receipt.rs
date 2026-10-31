//! Reading a Portuguese `fatura-recibo` out of the text of a PDF.
//!
//! The unit of work is a PAGE OF ROWS — the output of [`crate::pdf`] — because that is
//! what a form looks like once the positions are gone: labels and the value beside them
//! on one line. Everything here is therefore about recognition, and the two rules that
//! shape every function are the ones the rest of the application lives by:
//!
//! 1. **A figure is read or it is absent.** There is no default, no "probably 23%", no
//!    plausible-looking zero. A missing field is reported as missing, and the person
//!    completing the form supplies it.
//! 2. **The document is checked against itself.** A fatura states its base, its VAT
//!    rate, its VAT, its stamp duty and its totals; those have to agree. The checks run
//!    here, in the core, so the panel and the command line report the same
//!    disagreements, and so a disagreement is visible BEFORE anything is written to the
//!    ledger rather than after.
//!
//! The parser is deliberately tolerant about layout and strict about arithmetic.
//! Issuers differ in wording and order, so labels are matched by pattern; but no
//! pattern match is ever allowed to invent a number.
//!
//! ## Why there is no regex engine here
//!
//! The reference wrote these patterns as JavaScript regular expressions. This port
//! writes them as small functions that return where a label matched, because each of
//! them is anchored at the start of a row and asks one question: does the row begin
//! with this label. That is a `starts_with` and a boundary test, and a regex engine
//! (plus its syntax differences, plus the lookarounds it does not support) would be a
//! much larger thing to trust a tax figure to.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::countries::TurnoverScope;
use crate::countries::scope_of;
use crate::estimate::{Invoice, InvoiceStatus, VatTreatment};
use crate::money::{apply_bp, format_eur, parse_eur_to_cents};
use crate::nif::{is_valid_pt_tax_number, normalise_nif};
use crate::pdf::PdfText;
use crate::rules::constant_bp;
use crate::types::{IsoDate, RulePack, TaxProfile};

/// One party to a document: who issued it, and to whom.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptParty {
    /// The name.
    pub name: Option<String>,
    /// The tax number.
    pub nif: Option<String>,
    /// Where they are.
    pub address: Option<String>,
}

/// One line of "what was read", for a panel that must not look more certain than it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptField {
    /// A stable key.
    pub key: String,
    /// What the form calls it.
    pub label: String,
    /// What was read, already formatted for a person.
    pub value: Option<String>,
    /// Why it is missing, when it is.
    pub hint: Option<String>,
}

/// One arithmetic or consistency check the document has to pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptCheck {
    /// A stable key.
    pub id: String,
    /// What it checks, in Portuguese.
    pub label: String,
    /// Whether it passed.
    pub ok: bool,
    /// The arithmetic, so a person can check the check.
    pub detail: String,
}

/// What a document said, with nothing invented.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptDraft {
    /// "Fatura-Recibo", "Fatura", and so on.
    pub kind: Option<String>,
    /// The document number.
    pub number: Option<String>,
    /// The ATCUD code.
    pub atcud: Option<String>,
    /// The issue date.
    pub date: Option<IsoDate>,
    /// Who issued it.
    pub issuer: ReceiptParty,
    /// Who it was issued to.
    pub customer: ReceiptParty,
    /// What was supplied.
    pub description: Option<String>,
    /// "Valor ilíquido": what the VAT and the retention are computed on.
    pub base_cents: Option<i64>,
    /// The VAT charged.
    pub iva_cents: Option<i64>,
    /// The VAT rate, when the document states exactly one.
    pub iva_rate_bp: Option<i64>,
    /// "Imposto do Selo", which the invoice ledger does not represent.
    pub stamp_duty_cents: Option<i64>,
    /// "Retenção na fonte".
    pub retention_cents: Option<i64>,
    /// "TOTAL DO DOCUMENTO".
    pub total_cents: Option<i64>,
    /// "TOTAL A PAGAR".
    pub net_cents: Option<i64>,
    /// Why nothing was withheld, when the document says.
    pub retention_reason: Option<String>,
    /// "Documento emitido a título de: …".
    pub purpose: Option<String>,
    /// The document itself says it was paid.
    pub declares_paid: bool,
    /// Where the customer is, when the document supports a reading.
    pub customer_country_hint: Option<String>,
    /// Every VAT rate the document charges.
    pub iva_rates_bp: Vec<i64>,
    /// How many pages the PDF has.
    pub page_count: usize,
    /// The fields, in the order a form shows them.
    pub fields: Vec<ReceiptField>,
    /// The checks, in the order they were made.
    pub checks: Vec<ReceiptCheck>,
    /// Everything that could not be read or did not add up.
    pub problems: Vec<String>,
}

/// Text with accents removed and runs of whitespace collapsed, plus a map back.
///
/// The map is the whole point: `tail_after` needs to cut the ORIGINAL row after a match
/// found in the normalised one, and cutting at the wrong character eats the first letter
/// of the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalised {
    /// The folded text.
    pub value: String,
    /// For each byte of `value`, its byte index in the original string.
    pub origin: Vec<usize>,
}

/// Fold a row: strip accents, lowercase, collapse whitespace, and remember where each
/// character came from.
#[must_use]
pub fn normalise_with_map(text: &str) -> Normalised {
    let mut value = String::new();
    let mut origin: Vec<usize> = Vec::new();
    let mut pending_space_at: Option<usize> = None;

    for (index, ch) in text.char_indices() {
        // NFD, then drop the combining marks: "á" becomes "a", not "á".
        let stripped: Vec<char> = ch
            .nfd()
            .filter(|inner| !is_combining_mark(*inner))
            .flat_map(char::to_lowercase)
            .collect();
        for folded in stripped {
            if folded.is_whitespace() {
                // The space is remembered with ITS OWN index: a collapsed run must still
                // point at the first character it replaces.
                if !value.is_empty() && pending_space_at.is_none() {
                    pending_space_at = Some(index);
                }
                continue;
            }
            if let Some(at) = pending_space_at.take() {
                value.push(' ');
                origin.push(at);
            }
            let mut buffer = [0_u8; 4];
            let encoded = folded.encode_utf8(&mut buffer);
            for _ in 0..encoded.len() {
                origin.push(index);
            }
            value.push_str(encoded);
        }
    }
    Normalised { value, origin }
}

/// Whether a character is a combining mark, which is what an accent becomes after NFD.
fn is_combining_mark(ch: char) -> bool {
    matches!(u32::from(ch), 0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

/// Where the match ends in the original string.
fn end_of_match(normalised: &Normalised, end: usize) -> usize {
    normalised
        .origin
        .get(end.saturating_sub(1))
        .map_or(0, |index| index + 1)
}

/// Does `value` start with `prefix` as a whole word? Returns where it ends.
fn word_prefix(value: &str, prefix: &str) -> Option<usize> {
    if !value.starts_with(prefix) {
        return None;
    }
    let rest = value.get(prefix.len()..)?;
    match rest.chars().next() {
        Some(next) if next.is_alphanumeric() => None,
        _ => Some(prefix.len()),
    }
}

/// The first of several prefixes that matches, as a whole word.
fn word_prefix_any(value: &str, prefixes: &[&str]) -> Option<usize> {
    prefixes.iter().find_map(|prefix| word_prefix(value, prefix))
}

/// The part of a row that follows a label at its start, or `None`.
fn tail_after(row: &str, label: fn(&str) -> Option<usize>) -> Option<String> {
    let normalised = normalise_with_map(row);
    let end = label(&normalised.value)?;
    let rest = row.get(end_of_match(&normalised, end)..)?;
    let trimmed = rest.trim_start_matches([' ', ':', '.', '-', '–', '—']).trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// A row with its incidental spaces folded exactly as the reader folds them.
fn tidy_row(row: &str) -> String {
    crate::pdf::collapse_spaces(&row.replace(['\u{a0}', '\u{202f}'], " "))
}

/// One Portuguese money amount as it is printed on a document: an optional thousands
/// separator, then exactly two decimals after a comma or a dot.
fn money_tokens(row: &str) -> Vec<&str> {
    let bytes = row.as_bytes();
    let mut tokens: Vec<&str> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        // Thousands groups: up to three digits, then ("." or " ") + three digits, repeated.
        let mut cursor = index;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        let mut grouped = cursor;
        if (1..=3).contains(&(grouped - start)) {
            loop {
                let separator = bytes.get(grouped).copied();
                let three = bytes.get(grouped + 1..grouped + 4);
                let is_separator = matches!(separator, Some(b'.') | Some(b' '));
                let is_three = three.is_some_and(|digits| digits.iter().all(u8::is_ascii_digit));
                if is_separator && is_three {
                    grouped += 4;
                } else {
                    break;
                }
            }
        }
        if bytes.get(grouped) == Some(&b',') || bytes.get(grouped) == Some(&b'.') {
            let decimals = bytes.get(grouped + 1..grouped + 3);
            let is_two = decimals.is_some_and(|digits| digits.iter().all(u8::is_ascii_digit));
            if is_two {
                let end = grouped + 3;
                tokens.push(&row[start..end]);
                index = end;
                continue;
            }
        }
        index = cursor.max(index + 1);
    }
    tokens
}

/// Read one token as Portuguese euros.
fn amount_from(token: &str) -> Option<i64> {
    parse_eur_to_cents(token).ok()
}

/// The last money-looking token of a row.
fn last_amount(row: &str) -> Option<i64> {
    money_tokens(row).last().and_then(|token| amount_from(token))
}

/// The amount on the first row whose label matches at its START.
///
/// The anchor matters: "IVA" also appears inside longer labels ("VALOR IVA", "TAXA
/// IVA"), and a label matched in the middle of a row would read a figure that belongs to
/// something else.
fn amount_for_label(rows: &[String], patterns: &[fn(&str) -> Option<usize>]) -> Option<i64> {
    for pattern in patterns {
        for row in rows {
            let normalised = normalise_with_map(row);
            let Some(end) = pattern(&normalised.value) else {
                continue;
            };
            let rest = row.get(end_of_match(&normalised, end)..).unwrap_or_default();
            if let Some(amount) = last_amount(rest) {
                return Some(amount);
            }
        }
    }
    None
}

/// The first row matching a predicate on its folded text.
fn row_matching(rows: &[String], test: fn(&str) -> bool) -> Option<&String> {
    rows.iter().find(|row| test(&normalise_with_map(row).value))
}

/// A Portuguese date as the document prints it.
#[must_use]
pub fn iso_from_portuguese(date: &str) -> Option<IsoDate> {
    let trimmed = date.trim();
    let mut parts = trimmed.split(['/', '-', '.']);
    let day = parts.next()?.parse::<i64>().ok()?;
    let month = parts.next()?.parse::<i64>().ok()?;
    let year = parts.next()?.parse::<i64>().ok()?;
    // Nothing after the year, or it is not a date at all.
    if parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days_in_month = crate::dates::days_in_month(year, u32::try_from(month).ok()?);
    if day > i64::from(days_in_month) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

/// One labelled block of the document.
#[derive(Debug, Clone, Default)]
struct Section {
    rows: Vec<String>,
}

/// Which section a row starts, if any.
fn section_marker(value: &str) -> Option<&'static str> {
    if value.starts_with("dados do transmitente")
        || value.starts_with("dados do prestador")
        || value.starts_with("dados do vendedor")
    {
        return Some("issuer");
    }
    if value.starts_with("dados do adquirente")
        || value.starts_with("dados do cliente")
        || value.starts_with("dados do comprador")
    {
        return Some("customer");
    }
    if value.starts_with("dados da transmissao")
        || value.starts_with("dados da prestacao")
        || value.starts_with("detalhes da")
    {
        return Some("body");
    }
    if word_prefix(value, "iva").is_some()
        || value.starts_with("resumo de iva")
        || (value.starts_with("taxa") && value.contains("motivo"))
    {
        return Some("vat");
    }
    if word_prefix(value, "irs").is_some() || value.starts_with("retencao na fonte") {
        return Some("irs");
    }
    if value.starts_with("totais do documento") || value.starts_with("resumo do documento") {
        return Some("totals");
    }
    None
}

/// Split the page into the labelled blocks a fatura-recibo is made of.
///
/// "DADOS DO TRANSMITENTE…" and "DADOS DO ADQUIRENTE…" are the two that matter: they
/// are what lets this code say WHOSE invoice it is instead of inferring it from the
/// order in which two tax numbers happen to appear.
fn split_sections(rows: &[String]) -> std::collections::BTreeMap<&'static str, Section> {
    let mut sections: std::collections::BTreeMap<&'static str, Section> = std::collections::BTreeMap::new();
    let mut current: Option<&'static str> = None;
    for row in rows {
        let normalised = normalise_with_map(row).value;
        if let Some(key) = section_marker(&normalised) {
            sections.insert(key, Section::default());
            current = Some(key);
            continue;
        }
        if let Some(key) = current
            && let Some(section) = sections.get_mut(key)
        {
            section.rows.push(row.clone());
        }
    }
    sections
}

/// The name label, if the row starts with one.
fn name_label(value: &str) -> Option<usize> {
    word_prefix_any(
        value,
        &[
            "nome",
            "nome / razao social",
            "nome ou razao social",
            "nome completo",
        ],
    )
}

/// Read one party out of its section.
fn party_from(section: Option<&Section>) -> ReceiptParty {
    let Some(section) = section else {
        return ReceiptParty::default();
    };
    let rows = &section.rows;

    let mut name: Option<String> = None;
    for row in rows {
        if let Some(tail) = tail_after(row, name_label)
            && !tail.starts_with('/')
            && !tail.starts_with('\\')
        {
            name = Some(tail);
            break;
        }
    }
    if name.is_none() {
        // Some layouts put the label on its own line and the value on the next one.
        for (index, row) in rows.iter().enumerate() {
            let folded = normalise_with_map(row).value;
            let bare = folded == "nome" || folded == "nome / razao social";
            if !bare {
                continue;
            }
            if let Some(next) = rows.get(index + 1)
                && nif_in(next).is_none()
            {
                name = Some(next.clone());
                break;
            }
        }
    }

    let nif_row = row_matching(rows, |value| {
        value.contains("identificacao fiscal")
            || word_prefix(value, "nif").is_some()
            || word_prefix(value, "nipc").is_some()
    });
    let nif = nif_row.and_then(|row| nif_in(row));

    let address_row = row_matching(rows, |value| {
        value.contains("domicilio")
            || value.contains("sede")
            || value.contains("morada")
            || value.contains("endereco")
    });
    let address = address_row.and_then(|row| {
        // Everything after the keyword that names the field, and after its colon.
        let folded = normalise_with_map(row).value;
        let keyword = ["sede ou domicilio", "domicilio", "sede", "morada", "endereco"]
            .iter()
            .filter_map(|candidate| folded.find(candidate).map(|at| (at, candidate.len())))
            .min_by_key(|(at, _)| *at)?;
        let start = keyword.0 + keyword.1;
        let rest = &folded[start..];
        let after = rest.trim_start_matches([' ', ':']).len();
        let cut = rest.len() - after;
        // The keyword may appear mid-row, so map back through the origin table.
        let original_from = end_of_match(&normalise_with_map(row), start + cut);
        let tail = row.get(original_from..)?.trim_start_matches([' ', ':']).trim();
        if tail.is_empty() {
            None
        } else {
            Some(tail.to_owned())
        }
    });

    ReceiptParty { name, nif, address }
}

/// The tax number in a row, if it has one.
fn nif_in(row: &str) -> Option<String> {
    let bytes = row.as_bytes();
    let mut index = 0;
    while index + 9 <= bytes.len() {
        let window = &bytes[index..index + 9];
        let all_digits = window.iter().all(u8::is_ascii_digit);
        // `\b(\d{9})\b`: neither neighbour may be a word character.
        let before_ok = index == 0 || !is_word_byte(bytes[index - 1]);
        let after_ok = bytes.get(index + 9).is_none_or(|byte| !is_word_byte(*byte));
        if all_digits && before_ok && after_ok {
            return Some(row[index..index + 9].to_owned());
        }
        index += 1;
    }
    None
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The document kind, from the words it prints.
fn document_kind(row: &str) -> Option<&'static str> {
    let folded = row.to_lowercase();
    if has_fatura_recibo(&folded) {
        return Some("Fatura-Recibo");
    }
    if folded.contains("fatura simplificada") {
        return Some("Fatura Simplificada");
    }
    if folded.contains("nota de credito") || folded.contains("nota de crédito") {
        return Some("Nota de Crédito");
    }
    if folded.contains("nota de debito") || folded.contains("nota de débito") {
        return Some("Nota de Débito");
    }
    if folded.contains("fatura") {
        return Some("Fatura");
    }
    if folded.contains("recibo") {
        return Some("Recibo");
    }
    None
}

/// `fatura[\s-]*recibo`, on a lowercased row.
fn has_fatura_recibo(folded: &str) -> bool {
    let bytes: Vec<char> = folded.chars().collect();
    let needle: Vec<char> = "fatura".chars().collect();
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if bytes[index..index + needle.len()] == needle[..] {
            let mut cursor = index + needle.len();
            while cursor < bytes.len() && (bytes[cursor].is_whitespace() || bytes[cursor] == '-') {
                cursor += 1;
            }
            let tail: Vec<char> = "recibo".chars().collect();
            if cursor + tail.len() <= bytes.len() && bytes[cursor..cursor + tail.len()] == tail[..] {
                return true;
            }
        }
        index += 1;
    }
    false
}

/// The rows of a PDF, tidied.
fn rows_of(pdf: &PdfText) -> Vec<String> {
    pdf.rows
        .iter()
        .map(|row| tidy_row(row))
        .filter(|row| !row.is_empty())
        .collect()
}

/// Everything in a row that is not part of a description: the table's figures.
fn without_figures(text: &str) -> String {
    // A money token, or a bare percentage, cuts the text.
    let mut cut = text.len();
    if let Some(token) = money_tokens(text).first() {
        cut = text.find(token).unwrap_or(text.len());
    }
    if let Some(at) = percentage_at(text)
        && at < cut
    {
        cut = at;
    }
    let head = &text[..cut];
    head.trim_end_matches([' ', '-', '–', '—', '·', '|'])
        .trim()
        .to_owned()
}

/// Where a `12,5 %`-shaped percentage starts, if one does.
fn percentage_at(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    for index in 0..bytes.len() {
        if !bytes[index].is_ascii_digit() {
            continue;
        }
        // A run of up to two digits, optionally followed by , or . and up to two more.
        let mut cursor = index;
        let mut digits = 0;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() && digits < 2 {
            cursor += 1;
            digits += 1;
        }
        if digits == 0 {
            continue;
        }
        if matches!(bytes.get(cursor), Some(b',') | Some(b'.')) {
            let mut decimals = 0;
            let mut inner = cursor + 1;
            while inner < bytes.len() && bytes[inner].is_ascii_digit() && decimals < 2 {
                inner += 1;
                decimals += 1;
            }
            if decimals == 0 {
                continue;
            }
            cursor = inner;
        }
        let mut after = cursor;
        while after < bytes.len() && (bytes[after] == b' ' || bytes[after] == b'\t') {
            after += 1;
        }
        if bytes.get(after) == Some(&b'%') {
            return Some(index);
        }
    }
    None
}

/// The description of what was supplied.
fn description_from(body_rows: &[String]) -> Option<String> {
    for row in body_rows {
        let Some(tail) = tail_after(row, |value| {
            word_prefix_any(
                value,
                &[
                    "descricao",
                    "referencia",
                    "descricao do servico",
                    "descricao da prestacao",
                ],
            )
        }) else {
            continue;
        };
        let folded = normalise_with_map(&tail).value;
        if ["qtd", "unitario", "desconto", "taxa", "total"]
            .iter()
            .any(|word| folded.contains(word))
        {
            continue;
        }
        let text = without_figures(&tail);
        if !text.is_empty() {
            return Some(truncate_chars(&text, 300));
        }
    }

    let header_at = body_rows
        .iter()
        .position(|row| word_prefix(&normalise_with_map(row).value, "descricao").is_some());
    let candidates = header_at.map_or(body_rows, |at| &body_rows[at + 1..]);
    let mut best: Option<String> = None;
    for row in candidates {
        if row.chars().count() < 8 {
            continue;
        }
        if nif_in(row).is_some() {
            continue;
        }
        if contains_iso_shaped_date(row) {
            continue;
        }
        if not_a_description(&normalise_with_map(row).value) {
            continue;
        }
        let text = without_figures(row);
        if text.chars().count() < 4 {
            continue;
        }
        let longer = best
            .as_ref()
            .is_none_or(|current| text.chars().count() > current.chars().count());
        if longer {
            best = Some(text);
        }
    }
    best.map(|text| truncate_chars(&text, 300))
}

fn truncate_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Rows that describe the document rather than its content.
fn not_a_description(value: &str) -> bool {
    word_prefix_any(
        value,
        &[
            "valor",
            "descricao",
            "qtd",
            "quantidade",
            "unitario",
            "desconto",
            "taxa",
            "total",
            "data",
            "documento",
            "nome",
            "nif",
            "numero",
            "atcud",
            "iva",
            "irs",
            "base",
            "retencao",
            "imposto",
            "pagina",
            "original",
            "duplicado",
            "servico c/iva",
            "unidade",
        ],
    )
    .is_some()
}

/// Whether a row contains a `dd/mm/yyyy` shaped date.
fn contains_iso_shaped_date(row: &str) -> bool {
    let bytes = row.as_bytes();
    for index in 0..bytes.len().saturating_sub(9) {
        let window = &bytes[index..index + 10];
        let shaped = window[0].is_ascii_digit()
            && window[1].is_ascii_digit()
            && matches!(window[2], b'/' | b'-' | b'.')
            && window[3].is_ascii_digit()
            && window[4].is_ascii_digit()
            && matches!(window[5], b'/' | b'-' | b'.')
            && window[6..10].iter().all(u8::is_ascii_digit);
        if shaped {
            return true;
        }
    }
    false
}

/// Every VAT rate the document charges, as basis points.
fn vat_rates(rows: &[String], base_cents: Option<i64>, iva_cents: Option<i64>) -> Vec<i64> {
    fn add(rates: &mut Vec<i64>, bp: i64) {
        if !rates.contains(&bp) {
            rates.push(bp);
        }
    }

    let mut rates: Vec<i64> = Vec::new();

    // 1. A rate stated at the start of a row, which is what an IVA summary table looks
    //    like: "23,00% - Taxa Normal - Continente   813,01 €   186,99 €".
    for row in rows {
        let folded = normalise_with_map(row).value;
        let Some(end) = percentage_at(&folded).filter(|at| *at == 0) else {
            continue;
        };
        let mentions = ["taxa", "iva", "isenc", "normal", "intermedi", "reduzid"]
            .iter()
            .any(|word| folded.contains(word));
        if !mentions {
            continue;
        }
        if let Some(rate) = rate_from_prefix(&folded[..end]) {
            add(&mut rates, rate);
        }
    }
    if !rates.is_empty() {
        return rates;
    }

    // 2. A rate anywhere in a row that talks about VAT — some issuers print the column
    //    value without repeating the word.
    for row in rows {
        let folded = normalise_with_map(row).value;
        if !folded.contains("taxa") && !folded.contains("iva") {
            continue;
        }
        if let Some(at) = percentage_at(&folded) {
            let start = folded[..at]
                .rfind(|ch: char| !ch.is_ascii_digit() && ch != ',' && ch != '.')
                .map_or(0, |position| position + 1);
            if let Some(rate) = rate_from_prefix(&folded[start..at]) {
                add(&mut rates, rate);
            }
        }
    }
    if !rates.is_empty() {
        return rates;
    }

    // 3. No rate is printed: a document that states its base and its VAT has said its
    //    rate implicitly, and this is arithmetic on the document's own figures.
    if let (Some(base), Some(iva)) = (base_cents, iva_cents)
        && base > 0
    {
        add(&mut rates, ((iva as f64 / base as f64) * 10_000.0).round() as i64);
    }
    rates
}

/// A percentage printed as `23` or `23,5` read as basis points.
fn rate_from_prefix(text: &str) -> Option<i64> {
    let cleaned = text.trim().replace(',', ".");
    let value: f64 = cleaned.parse().ok()?;
    if !(0.0..=100.0).contains(&value) {
        return None;
    }
    Some((value * 100.0).round() as i64)
}

/// The document checked against itself.
///
/// A check that cannot run is NOT emitted: "we could not verify this" and "this is
/// wrong" are different statements, and only the second one belongs in a list of
/// failures.
fn build_checks(
    base_cents: Option<i64>,
    iva_cents: Option<i64>,
    stamp_duty_cents: Option<i64>,
    total_cents: Option<i64>,
    retention_cents: Option<i64>,
    net_cents: Option<i64>,
    iva_rates_bp: &[i64],
    issuer: &ReceiptParty,
    customer: &ReceiptParty,
) -> Vec<ReceiptCheck> {
    let mut checks: Vec<ReceiptCheck> = Vec::new();

    if let (Some(total), Some(iva), Some(base), Some(stamp)) =
        (total_cents, iva_cents, base_cents, stamp_duty_cents)
    {
        let sum = base + iva + stamp;
        checks.push(ReceiptCheck {
            id: "total".to_owned(),
            label: "Base + IVA + imposto do selo = total do documento".to_owned(),
            ok: sum == total,
            detail: format!(
                "{} + {} + {} = {} · documento: {}",
                format_eur(base),
                format_eur(iva),
                format_eur(stamp),
                format_eur(sum),
                format_eur(total)
            ),
        });
    }
    if let (Some(total), Some(net), Some(retention), Some(iva), Some(base)) =
        (total_cents, net_cents, retention_cents, iva_cents, base_cents)
    {
        let gross = base + iva + stamp_duty_cents.unwrap_or(0);
        checks.push(ReceiptCheck {
            id: "payable".to_owned(),
            label: "Total do documento − retenção = total a pagar".to_owned(),
            ok: gross - retention == net,
            detail: format!(
                "{} − {} = {} · documento: {}",
                format_eur(gross),
                format_eur(retention),
                format_eur(gross - retention),
                format_eur(net)
            ),
        });
        let _ = total;
    }
    if let (Some(base), Some(iva)) = (base_cents, iva_cents)
        && iva_rates_bp.len() == 1
    {
        let rate = iva_rates_bp.first().copied().unwrap_or(0);
        checks.push(ReceiptCheck {
            id: "rate".to_owned(),
            label: "Taxa de IVA aplicada à base = IVA do documento".to_owned(),
            ok: apply_bp(base, rate) == iva,
            detail: format!(
                "{} × {}% = {} · documento: {}",
                format_eur(base),
                format_rate(rate),
                format_eur(apply_bp(base, rate)),
                format_eur(iva)
            ),
        });
    }
    for (party, person) in [("prestador", issuer), ("adquirente", customer)] {
        let Some(nif) = &person.nif else {
            continue;
        };
        let valid = is_valid_pt_tax_number(nif);
        checks.push(ReceiptCheck {
            id: format!("nif-{party}"),
            label: format!("Dígito de controlo do NIF do {party}"),
            ok: valid,
            detail: format!(
                "{nif} {}",
                if valid {
                    "é um NIF português válido"
                } else {
                    "não passa o dígito de controlo"
                }
            ),
        });
    }
    checks
}

/// A rate in percent, the way the reference wrote it (`rate / 100`).
fn format_rate(bp: i64) -> String {
    let whole = bp / 100;
    let fraction = (bp % 100).abs();
    if fraction == 0 {
        format!("{whole}")
    } else {
        format!("{whole},{fraction:02}")
    }
}

/// Read one fatura-recibo out of a PDF's text.
///
/// Nothing here fails: an unreadable document produces a draft whose fields are `None`
/// and whose `problems` say why. The caller decides what to do about that, and the one
/// thing it must not do is fill the gaps with guesses.
#[must_use]
pub fn parse_receipt(pdf: &PdfText) -> ReceiptDraft {
    let rows = rows_of(pdf);
    let mut problems: Vec<String> = pdf.problems.clone();
    let page_count = pdf.page_count.max(1);
    let sections = split_sections(&rows);

    // --- document identity ---------------------------------------------------
    let mut kind: Option<String> = None;
    let mut number: Option<String> = None;
    for row in &rows {
        let Some(found) = document_kind(row) else {
            continue;
        };
        kind = Some(found.to_owned());
        number = bracketed_number(row).or_else(|| labelled_number(row));
        break;
    }

    let atcud = row_matching(&rows, |value| value.contains("atcud")).and_then(|row| atcud_from(row));

    // The issue date: "emitida em 20/09/2026", "Data: 20/09/2026", "Data de emissão".
    let date_row = row_matching(&rows, |value| {
        value.contains("emitida em")
            || value.contains("emitido em")
            || value.contains("data de emissao")
            || word_prefix(value, "data").is_some()
            || value.contains("data da fatura")
            || word_prefix(value, "emitido").is_some()
    })
    .or_else(|| row_matching(&rows, contains_iso_shaped_date));
    let date = date_row
        .and_then(|row| first_date_in(row))
        .and_then(|printed| iso_from_portuguese(&printed));

    // --- parties -------------------------------------------------------------
    let issuer = party_from(sections.get("issuer"));
    let customer = party_from(sections.get("customer"));
    if !sections.contains_key("issuer") {
        problems.push(
            "Não foi encontrada a secção do prestador de serviços (\"DADOS DO TRANSMITENTE…\"): pelo \
             documento sozinho não é possível confirmar que esta fatura é tua."
                .to_owned(),
        );
    }

    // --- description ---------------------------------------------------------
    let body_rows = sections
        .get("body")
        .map(|section| section.rows.clone())
        .unwrap_or_default();
    let description = description_from(&body_rows);

    // --- amounts -------------------------------------------------------------
    let base_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "valor iliquido"),
            |value| word_prefix(value, "total iliquido"),
            |value| word_prefix(value, "base de incidencia"),
            |value| word_prefix(value, "valor sem iva"),
            |value| word_prefix(value, "subtotal"),
            |value| word_prefix(value, "valor tributavel"),
        ],
    );
    let iva_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "iva"),
            |value| word_prefix(value, "total de iva"),
            |value| word_prefix(value, "valor iva"),
        ],
    );
    let stamp_duty_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "imposto do selo"),
            |value| word_prefix(value, "selo"),
        ],
    );
    let total_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "total do documento"),
            |value| word_prefix(value, "total da fatura"),
            |value| {
                let end = word_prefix(value, "total")?;
                // `(?!\s*a\s+(?:pagar|receber))`: "total a pagar" is a different figure.
                let rest = value[end..].trim_start();
                if rest.starts_with("a pagar") || rest.starts_with("a receber") {
                    return None;
                }
                Some(end)
            },
        ],
    );
    let retention_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "retencao na fonte irs"),
            |value| word_prefix(value, "retencao na fonte"),
            |value| word_prefix(value, "retencao irs"),
            |value| word_prefix(value, "valor irs"),
        ],
    );
    let net_cents = amount_for_label(
        &rows,
        &[
            |value| word_prefix(value, "total a pagar"),
            |value| word_prefix(value, "total a receber"),
            |value| word_prefix(value, "liquido a pagar"),
        ],
    );

    // --- VAT rates -----------------------------------------------------------
    let iva_rates_bp = vat_rates(&rows, base_cents, iva_cents);
    let iva_rate_bp = if iva_rates_bp.len() == 1 {
        iva_rates_bp.first().copied()
    } else {
        None
    };

    let retention_reason = row_matching(&rows, |value| {
        value.contains("sem retencao")
            || value.contains("art. 101")
            || value.contains("art 101")
            || value.contains("nao sujeito a retencao")
    })
    .map(|row| without_figures(row))
    .map(|text| truncate_chars(&text, 200))
    .filter(|text| !text.is_empty());

    let purpose = row_matching(&rows, |value| value.contains("documento emitido a titulo de"))
        .and_then(|row| tail_after(row, |value| word_prefix(value, "documento emitido a titulo de")));

    let declares_paid = kind.as_deref().is_some_and(|label| {
        let folded = label.to_lowercase();
        has_fatura_recibo(&folded)
    }) || purpose
        .as_deref()
        .is_some_and(|text| text.to_lowercase().contains("pagamento"));

    // --- the customer's country ---------------------------------------------
    let customer_country_hint = match (&customer.nif, &customer.address) {
        (Some(nif), _) if is_valid_pt_tax_number(nif) => Some("PT".to_owned()),
        (_, Some(address)) if address.to_lowercase().contains("portugal") => Some("PT".to_owned()),
        (_, Some(address)) => {
            let tail = address
                .split([',', ' '])
                .rfind(|part| !part.is_empty())
                .unwrap_or_default();
            let upper = tail.to_uppercase();
            if upper.chars().count() == 2 && upper.chars().all(|ch| ch.is_ascii_uppercase()) {
                Some(upper)
            } else {
                None
            }
        }
        _ => None,
    };

    // --- checks --------------------------------------------------------------
    let checks = build_checks(
        base_cents,
        iva_cents,
        stamp_duty_cents,
        total_cents,
        retention_cents,
        net_cents,
        &iva_rates_bp,
        &issuer,
        &customer,
    );
    for check in &checks {
        if !check.ok {
            problems.push(format!("Verificação falhada — {}: {}", check.label, check.detail));
        }
    }

    let money = |cents: Option<i64>| cents.map(format_eur);
    let fields = vec![
        ReceiptField {
            key: "kind".to_owned(),
            label: "Tipo de documento".to_owned(),
            value: kind.clone(),
            hint: None,
        },
        ReceiptField {
            key: "number".to_owned(),
            label: "Número".to_owned(),
            value: number.clone(),
            hint: number
                .is_none()
                .then(|| "Não encontrado no documento.".to_owned()),
        },
        ReceiptField {
            key: "atcud".to_owned(),
            label: "ATCUD".to_owned(),
            value: atcud.clone(),
            hint: atcud.is_none().then(|| "Não encontrado no documento.".to_owned()),
        },
        ReceiptField {
            key: "date".to_owned(),
            label: "Data de emissão".to_owned(),
            value: date.clone(),
            hint: date.is_none().then(|| "Não encontrada.".to_owned()),
        },
        ReceiptField {
            key: "issuerNif".to_owned(),
            label: "NIF do prestador".to_owned(),
            value: issuer.nif.clone(),
            hint: issuer.nif.is_none().then(|| "Não encontrado.".to_owned()),
        },
        ReceiptField {
            key: "customerNif".to_owned(),
            label: "NIF do adquirente".to_owned(),
            value: customer.nif.clone(),
            hint: customer
                .nif
                .is_none()
                .then(|| "Não encontrado (documento sem NIF do cliente).".to_owned()),
        },
        ReceiptField {
            key: "baseCents".to_owned(),
            label: "Valor ilíquido".to_owned(),
            value: money(base_cents),
            hint: base_cents.is_none().then(|| "Não encontrado.".to_owned()),
        },
        ReceiptField {
            key: "ivaCents".to_owned(),
            label: "IVA".to_owned(),
            value: money(iva_cents),
            hint: iva_cents.is_none().then(|| "Não encontrado.".to_owned()),
        },
        ReceiptField {
            key: "ivaRateBp".to_owned(),
            label: "Taxa de IVA".to_owned(),
            value: iva_rate_bp.map(|bp| format!("{}%", format_rate(bp))),
            hint: iva_rate_bp.is_none().then(|| {
                if iva_rates_bp.len() > 1 {
                    "O documento tem mais do que uma taxa.".to_owned()
                } else {
                    "Não encontrada.".to_owned()
                }
            }),
        },
        ReceiptField {
            key: "totalCents".to_owned(),
            label: "Total do documento".to_owned(),
            value: money(total_cents),
            hint: total_cents.is_none().then(|| "Não encontrado.".to_owned()),
        },
        ReceiptField {
            key: "retentionCents".to_owned(),
            label: "Retenção na fonte".to_owned(),
            value: money(retention_cents),
            hint: retention_cents
                .is_none()
                .then(|| "Não encontrada.".to_owned())
                .or_else(|| retention_reason.clone()),
        },
        ReceiptField {
            key: "netCents".to_owned(),
            label: "Total a pagar".to_owned(),
            value: money(net_cents),
            hint: net_cents.is_none().then(|| "Não encontrado.".to_owned()),
        },
    ];

    if base_cents.is_none() {
        problems.push(
            "Não foi lido o valor ilíquido (\"Valor ilíquido\" ou \"Base de incidência\"). Sem ele não \
             há fatura para registar: escreve-o no formulário a partir do documento."
                .to_owned(),
        );
    }

    let mut unique: Vec<String> = Vec::new();
    for problem in problems {
        if !unique.contains(&problem) {
            unique.push(problem);
        }
    }

    ReceiptDraft {
        kind,
        number,
        atcud,
        date,
        issuer,
        customer,
        description,
        base_cents,
        iva_cents,
        iva_rate_bp,
        stamp_duty_cents,
        retention_cents,
        total_cents,
        net_cents,
        retention_reason,
        purpose,
        declares_paid,
        customer_country_hint,
        iva_rates_bp,
        page_count,
        fields,
        checks,
        problems: unique,
    }
}

/// A document number in brackets, e.g. `Fatura-Recibo <2026/1>`.
fn bracketed_number(row: &str) -> Option<String> {
    let chars: Vec<char> = row.chars().collect();
    for index in 0..chars.len() {
        let open = chars[index];
        if open != '<' && open != '«' {
            continue;
        }
        let mut cursor = index + 1;
        while cursor < chars.len() {
            let ch = chars[cursor];
            if ch == '>' || ch == '»' {
                let inner: String = chars[index + 1..cursor].iter().collect();
                let trimmed = inner.trim();
                if (2..=60).contains(&trimmed.chars().count()) {
                    return Some(trimmed.to_owned());
                }
                break;
            }
            cursor += 1;
        }
    }
    None
}

/// A document number after a `N.º` or `Número` label.
fn labelled_number(row: &str) -> Option<String> {
    let folded = row.to_lowercase();
    let at = ["n.º", "n.º", "nº", "no", "n°", "numero", "número"]
        .iter()
        .filter_map(|label| folded.find(label))
        .min()?;
    let mut rest = &row[at..];
    // Skip the label and any separator after it.
    let label_end = row[at..].char_indices().find_map(|(offset, _ch)| {
        let rest = &row[at + offset..];
        (!rest.starts_with(|inner: char| inner.is_alphanumeric())).then_some(offset)
    });
    rest = match label_end {
        Some(end) => &row[at + end..],
        None => rest,
    };
    let rest = rest.trim_start_matches([' ', ':', '.']).trim();
    let mut value = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '.' | ' ') {
            if value.chars().count() >= 41 {
                break;
            }
            value.push(ch);
        } else {
            break;
        }
    }
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// The ATCUD code.
fn atcud_from(row: &str) -> Option<String> {
    let folded = row.to_lowercase();
    let at = folded.find("atcud")?;
    let rest = row[at + 5..].trim_start_matches([' ', ':']);
    let mut value = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            value.push(ch);
        } else {
            break;
        }
    }
    if value.chars().count() >= 4 {
        Some(value.to_uppercase())
    } else {
        None
    }
}

/// The first `dd/mm/yyyy` shaped date in a row.
fn first_date_in(row: &str) -> Option<String> {
    let chars: Vec<char> = row.chars().collect();
    for index in 0..chars.len() {
        if !chars[index].is_ascii_digit() {
            continue;
        }
        let mut cursor = index;
        let mut digits = 0;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && digits < 2 {
            cursor += 1;
            digits += 1;
        }
        if digits == 0 || cursor >= chars.len() || !matches!(chars[cursor], '/' | '-' | '.') {
            continue;
        }
        let separator = chars[cursor];
        cursor += 1;
        let mut month_digits = 0;
        let month_start = cursor;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && month_digits < 2 {
            cursor += 1;
            month_digits += 1;
        }
        if month_digits == 0 || cursor >= chars.len() || chars[cursor] != separator {
            continue;
        }
        cursor += 1;
        let year_start = cursor;
        let mut year_digits = 0;
        while cursor < chars.len() && chars[cursor].is_ascii_digit() && year_digits < 4 {
            cursor += 1;
            year_digits += 1;
        }
        if year_digits != 4 {
            continue;
        }
        let _ = month_start;
        return Some(chars[index..year_start + 4].iter().collect());
    }
    None
}

/// What the person confirmed or corrected in the panel.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptConfirmation {
    /// The document number.
    pub number: Option<String>,
    /// The issue date.
    pub date: Option<String>,
    /// The client's name.
    pub client_name: Option<String>,
    /// The client's tax number.
    pub client_nif: Option<String>,
    /// The client's country.
    pub client_country: Option<String>,
    /// What was supplied.
    pub description: Option<String>,
    /// The taxable base.
    pub base_cents: Option<i64>,
    /// The VAT rate.
    pub iva_rate_bp: Option<i64>,
    /// The withholding rate.
    pub retention_bp: Option<i64>,
    /// How IVA was treated.
    pub vat_treatment: Option<VatTreatment>,
    /// Where the invoice stands.
    pub status: Option<InvoiceStatus>,
    /// The ATCUD.
    pub atcud: Option<String>,
}

/// What it takes to turn a draft into a ledger record.
pub struct ReceiptInvoiceInput<'a> {
    /// What the document said.
    pub draft: &'a ReceiptDraft,
    /// The rule pack.
    pub pack: &'a RulePack,
    /// The profile, which decides whose invoice this is.
    pub profile: &'a TaxProfile,
    /// What the person confirmed.
    pub confirmed: &'a ReceiptConfirmation,
    /// The identifier for the new record.
    pub id: String,
    /// The name the document was archived under, when it was.
    pub document_file: Option<String>,
    /// Used only when neither the document nor the person supplies a number.
    pub fallback_number: String,
}

/// What came of it.
#[derive(Debug, Clone, Default)]
pub struct ReceiptInvoiceResult {
    /// The invoice, when it can be recorded.
    pub invoice: Option<Invoice>,
    /// Why it cannot.
    pub problems: Vec<String>,
    /// Things worth knowing that are not refusals.
    pub warnings: Vec<String>,
    /// Where what was confirmed differs from what the document says.
    pub divergences: Vec<String>,
}

/// The basis points that reproduce a retention figure exactly.
///
/// The ledger stores a RATE and recomputes the amount, so a rate that does not
/// round-trip would quietly change the document's own arithmetic. The candidate is
/// accepted only when `apply_bp` gives back what the document printed.
#[must_use]
pub fn retention_bp_for(base_cents: i64, retention_cents: i64) -> Option<i64> {
    if base_cents <= 0 {
        return None;
    }
    if retention_cents == 0 {
        return Some(0);
    }
    let candidate = ((retention_cents as f64 / base_cents as f64) * 10_000.0).round() as i64;
    if apply_bp(base_cents, candidate) == retention_cents {
        Some(candidate)
    } else {
        None
    }
}

fn unique(mut items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items.drain(..) {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Build the ledger record from a parsed document plus the person's confirmation.
///
/// The refusal that matters most is the first one: a `fatura-recibo` can be yours or one
/// somebody issued to you, and the only trustworthy way to tell them apart is the tax
/// number in the "prestador" section. Recording a supplier's invoice as your own income
/// would inflate every estimate that follows, so a document whose issuer is not the
/// profile's NIF is REFUSED with an explanation instead.
#[must_use]
pub fn invoice_from_receipt(input: &ReceiptInvoiceInput<'_>) -> ReceiptInvoiceResult {
    let draft = input.draft;
    let confirmed = input.confirmed;
    let mut problems: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut divergences: Vec<String> = Vec::new();

    let profile_nif = normalise_nif(&input.profile.nif);
    let issuer_nif = draft.issuer.nif.as_deref().map(normalise_nif);
    let customer_nif = draft.customer.nif.as_deref().map(normalise_nif);

    if let Some(issuer) = &issuer_nif
        && *issuer != profile_nif
    {
        let as_customer = customer_nif.as_ref() == Some(&profile_nif);
        problems.push(format!(
            "Esta fatura foi emitida pelo NIF {issuer}, e o perfil é o NIF {profile_nif}. {}",
            if as_customer {
                "O adquirente é o NIF do perfil, ou seja: é uma fatura que RECEBESTE, não uma que \
                 emitiste. O livro de despesas ainda não existe nesta aplicação, por isso não pode \
                 ser registada aqui."
            } else {
                "Não é uma fatura tua, por isso não pode entrar no livro de recibos emitidos."
            }
        ));
    }
    if issuer_nif.is_none() && customer_nif.as_ref() == Some(&profile_nif) {
        problems.push(
            "O documento não identifica o prestador, mas o adquirente é o NIF do perfil: parece ser \
             uma fatura que recebeste. O livro de despesas ainda não existe nesta aplicação."
                .to_owned(),
        );
    }
    if issuer_nif.is_none() && customer_nif.is_none() {
        warnings.push(
            "O documento não identifica nenhum NIF. Confirma que é uma fatura emitida por ti antes \
             de a registar."
                .to_owned(),
        );
    }

    let country = confirmed
        .client_country
        .clone()
        .or_else(|| draft.customer_country_hint.clone());
    if country.is_none() {
        problems.push(
            "Não foi possível ler o país do cliente no documento. Escolhe-o no formulário: é o que \
             decide o tratamento de IVA e a retenção."
                .to_owned(),
        );
    }

    let base_cents = confirmed.base_cents.or(draft.base_cents);
    match base_cents {
        Some(cents) if cents > 0 => {}
        _ => problems.push("Falta o valor ilíquido da fatura (em cêntimos, maior do que zero).".to_owned()),
    }

    let scope = country.as_deref().map(scope_of);
    let mut treatment = confirmed.vat_treatment;
    if treatment.is_none()
        && let Some(country) = &country
    {
        let scope = scope_of(country);
        treatment = Some(if scope == TurnoverScope::National {
            let rate = confirmed.iva_rate_bp.or(draft.iva_rate_bp);
            if rate.is_some_and(|rate| rate > 0) {
                VatTreatment::IvaPt
            } else {
                VatTreatment::IsentoArt53
            }
        } else if scope == TurnoverScope::Eu {
            VatTreatment::AutoliquidacaoUe
        } else {
            VatTreatment::Exportacao
        });
    }

    let mut iva_rate_bp = confirmed.iva_rate_bp.or(draft.iva_rate_bp);
    if let Some(treatment) = treatment
        && treatment != VatTreatment::IvaPt
    {
        if iva_rate_bp.is_some_and(|rate| rate != 0) {
            warnings.push(format!(
                "O tratamento \"{}\" não liquida IVA português; a taxa fica em 0% e não em {}%.",
                vat_wire_name(treatment),
                format_rate(iva_rate_bp.unwrap_or(0))
            ));
        }
        iva_rate_bp = Some(0);
    }
    if treatment == Some(VatTreatment::IvaPt) && iva_rate_bp.is_none_or(|rate| rate == 0) {
        problems.push(
            "A fatura liquida IVA português mas não foi lida nenhuma taxa. Escreve a taxa do \
             documento (por exemplo 23) no formulário."
                .to_owned(),
        );
    }

    let mut retention_bp = confirmed.retention_bp;
    if retention_bp.is_none()
        && let (Some(retention), Some(base)) = (draft.retention_cents, base_cents)
        && base > 0
    {
        match retention_bp_for(base, retention) {
            None => warnings.push(format!(
                "A retenção de {} não corresponde a uma percentagem exacta de {}. Confirma a \
                 percentagem a aplicar.",
                format_eur(retention),
                format_eur(base)
            )),
            Some(derived) => retention_bp = Some(derived),
        }
    }
    let retention_bp = retention_bp.unwrap_or(0);

    let pack_retention_bp = constant_bp(
        input.pack,
        &["irs", "withholding", "professionalServicesResident"],
    );
    if retention_bp != 0
        && let Some(pack_rate) = pack_retention_bp
        && retention_bp != pack_rate
        && scope == Some(TurnoverScope::National)
    {
        warnings.push(format!(
            "A retenção lida ({}%) não é a do pacote de regras para serviços de residentes ({}%). \
             Ficou registado o que o documento diz.",
            format_rate(retention_bp),
            format_rate(pack_rate)
        ));
    }

    // --- what the person changed ---------------------------------------------
    let confirmed_number = confirmed.number.clone().or_else(|| draft.number.clone());
    let confirmed_date = confirmed.date.clone().or_else(|| draft.date.clone());
    let confirmed_nif = confirmed
        .client_nif
        .clone()
        .or_else(|| draft.customer.nif.clone());
    let comparisons: [(&str, Option<String>, Option<String>); 5] = [
        ("número", draft.number.clone(), confirmed_number.clone()),
        ("data", draft.date.clone(), confirmed_date.clone()),
        (
            "NIF do cliente",
            draft.customer.nif.clone(),
            confirmed_nif.clone(),
        ),
        (
            "valor ilíquido",
            draft.base_cents.map(format_eur),
            base_cents.map(format_eur),
        ),
        (
            "taxa de IVA",
            draft.iva_rate_bp.map(|bp| format!("{}%", format_rate(bp))),
            iva_rate_bp.map(|bp| format!("{}%", format_rate(bp))),
        ),
    ];
    for (label, read, used) in comparisons {
        if let (Some(read), Some(used)) = (read, used)
            && read != used
        {
            divergences.push(format!(
                "{label}: lido no documento \"{read}\", registado \"{used}\""
            ));
        }
    }

    if let Some(stamp) = draft.stamp_duty_cents
        && stamp != 0
    {
        warnings.push(format!(
            "O documento inclui {} de imposto do selo. O livro de faturas não tem esse campo: a soma \
             do registo fica abaixo do total do documento.",
            format_eur(stamp)
        ));
    }
    if let Some(reason) = &draft.retention_reason
        && retention_bp == 0
    {
        warnings.push(format!("O documento diz: {reason}"));
    }

    let warnings = unique(warnings);
    let Some(base_cents) = base_cents else {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: unique(problems),
            warnings,
            divergences,
        };
    };
    let Some(country) = country else {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: unique(problems),
            warnings,
            divergences,
        };
    };
    let Some(treatment) = treatment else {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: unique(problems),
            warnings,
            divergences,
        };
    };
    if !problems.is_empty() {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: unique(problems),
            warnings,
            divergences,
        };
    }

    let client_name = confirmed
        .client_name
        .clone()
        .or_else(|| draft.customer.name.clone())
        .unwrap_or_default();
    let client_name = client_name.trim().to_owned();
    if client_name.is_empty() {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: vec!["Falta o nome do cliente: não foi lido no documento.".to_owned()],
            warnings,
            divergences,
        };
    }
    if confirmed_date.as_deref().is_none_or(str::is_empty) {
        return ReceiptInvoiceResult {
            invoice: None,
            problems: vec!["Falta a data de emissão da fatura: não foi lida no documento.".to_owned()],
            warnings,
            divergences,
        };
    }

    let invoice = Invoice {
        id: input.id.clone(),
        number: confirmed_number.unwrap_or_else(|| input.fallback_number.clone()),
        date: confirmed_date.unwrap_or_default(),
        client_name,
        client_nif: confirmed_nif,
        client_country: country,
        description: confirmed
            .description
            .clone()
            .or_else(|| draft.description.clone())
            .unwrap_or_default(),
        base_cents,
        iva_rate_bp: iva_rate_bp.unwrap_or(0),
        vat_treatment: treatment,
        retention_bp,
        atcud: confirmed.atcud.clone().or_else(|| draft.atcud.clone()),
        status: confirmed.status.unwrap_or(if draft.declares_paid {
            InvoiceStatus::Paid
        } else {
            InvoiceStatus::Issued
        }),
        // The PDF is archived in the vault as part of the same action, so the document
        // that proves the invoice exists is where the record says it is.
        payment_proof_in_vault: input.document_file.is_some(),
    };

    ReceiptInvoiceResult {
        invoice: Some(invoice),
        problems: Vec::new(),
        warnings,
        divergences,
    }
}

/// How the wire names a VAT treatment, which is what the warning quotes.
fn vat_wire_name(treatment: VatTreatment) -> &'static str {
    match treatment {
        VatTreatment::IvaPt => "iva_pt",
        VatTreatment::AutoliquidacaoUe => "autoliquidacao_ue",
        VatTreatment::IsentoArt53 => "isento_art53",
        VatTreatment::Exportacao => "exportacao",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_a_row_removes_accents_and_remembers_where_it_came_from() {
        let folded = normalise_with_map("  Valor Ilíquido:  1 234,56 € ");
        assert_eq!(folded.value, "valor iliquido: 1 234,56 €");
        // The `v` of "Valor" is at byte 2 of the original.
        assert_eq!(folded.origin.first(), Some(&2));
    }

    #[test]
    fn the_tail_after_a_label_starts_after_its_colon() {
        let tail = tail_after("Valor ilíquido: 1 234,56 €", |value| {
            word_prefix(value, "valor iliquido")
        });
        assert_eq!(tail.as_deref(), Some("1 234,56 €"));
    }

    #[test]
    fn a_label_is_only_matched_at_the_start_of_a_row() {
        let tail = tail_after("Taxa de IVA 23%", |value| word_prefix(value, "iva"));
        assert_eq!(
            tail, None,
            "IVA appears in the middle of this row, not at its start"
        );
    }

    #[test]
    fn money_tokens_are_read_the_portuguese_way() {
        assert_eq!(money_tokens("Total: 1 234,56 €"), vec!["1 234,56"]);
        assert_eq!(money_tokens("Base 813,01 € IVA 186,99"), vec!["813,01", "186,99"]);
        assert_eq!(money_tokens("sem valor nenhum"), Vec::<&str>::new());
        // Three decimals: the reference's own pattern takes the first two and leaves the
        // third, so this is parity rather than a preference. No issuer prints three, and
        // changing it would change what a figure means.
        assert_eq!(money_tokens("1,234"), vec!["1,23"]);
    }

    #[test]
    fn a_portuguese_date_becomes_iso_or_nothing() {
        assert_eq!(iso_from_portuguese("20/09/2026").as_deref(), Some("2026-09-20"));
        assert_eq!(iso_from_portuguese("20-9-2026").as_deref(), Some("2026-09-20"));
        assert_eq!(iso_from_portuguese("31/02/2026"), None, "February has no 31st");
        assert_eq!(iso_from_portuguese("2026-09-20"), None);
        assert_eq!(iso_from_portuguese("20/13/2026"), None);
    }

    #[test]
    fn a_tax_number_is_read_only_when_it_stands_alone() {
        assert_eq!(nif_in("NIF: 123456789").as_deref(), Some("123456789"));
        assert_eq!(nif_in("NIF: 1234567890"), None);
        assert_eq!(nif_in("X123456789"), None);
        assert_eq!(nif_in("sem numero"), None);
    }

    #[test]
    fn the_retention_rate_is_only_accepted_when_it_reproduces_the_figure() {
        // 23% of 1 000,00 is 230,00 exactly.
        assert_eq!(retention_bp_for(100_000, 23_000), Some(2300));
        // 100,00 of 1 000,00 is 10%.
        assert_eq!(retention_bp_for(100_000, 10_000), Some(1000));
        // Zero withheld is zero, not "unknown".
        assert_eq!(retention_bp_for(100_000, 0), Some(0));
        // One cent of 300,00 EUR is 0,0033%, which no whole basis point reproduces:
        // the nearest rate gives 0,00 and the check refuses rather than recording a
        // percentage the document never stated.
        assert_eq!(retention_bp_for(30_000, 1), None);
        assert_eq!(retention_bp_for(0, 500), None);
    }
}
