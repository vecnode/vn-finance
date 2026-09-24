/**
 * A small PDF reader, large enough for the documents this application imports.
 *
 * The scope is deliberate. This is not a PDF library: it answers one question —
 * "what text does this document say, and roughly where does it say it" — and it
 * says so when it cannot. Everything it cannot do is REPORTED rather than
 * guessed: an encrypted file, an object-stream layout, a font with no character
 * map, a stream that will not inflate. A tax assistant that invents a figure out
 * of a PDF it half-understood is worse than one that refuses.
 *
 * Why write one at all: the application has ZERO runtime dependencies, and a
 * `fatura-recibo` is a form. Reading the form is the feature; pulling in a whole
 * rendering engine (with its own font stack and its own update cadence) to read
 * six labels would be a much larger liability than these lines.
 *
 * What is implemented:
 *   - the object graph (`N 0 obj << … >> stream … endstream`), with xref-free lookup;
 *   - the page tree, with inherited `/Resources`, so a page's fonts are known;
 *   - `FlateDecode` streams through `node:zlib` — no other filter is needed by the
 *     generators this targets, and an unknown filter is reported, not ignored;
 *   - content streams: `Tf`, `TL`, `Td`, `TD`, `Tm`, `T*`, `Tj`, `TJ`, `'`, `"`,
 *     inside `q`/`Q`/`cm`;
 *   - text decoding via `/ToUnicode` (bfchar/bfrange) when present, otherwise
 *     cp1252 for one-byte fonts, and a stated problem for a two-byte font with no
 *     character map (a glyph id is not a character);
 *   - rows: items sharing a baseline become one line, ordered left to right, which
 *     is what turns a positioned form into text a parser can read.
 */

import { inflateRawSync, inflateSync } from 'node:zlib';

// ---------------------------------------------------------------------------
// The value model
// ---------------------------------------------------------------------------

type PdfValue = PdfName | PdfNumber | PdfString | PdfRef | PdfArray | PdfDict | PdfBoolean | null;

interface PdfName {
  kind: 'name';
  value: string;
}
interface PdfNumber {
  kind: 'number';
  value: number;
}
interface PdfString {
  kind: 'string';
  value: string;
}
interface PdfRef {
  kind: 'ref';
  num: number;
  gen: number;
}
interface PdfArray {
  kind: 'array';
  items: PdfValue[];
}
interface PdfDict {
  kind: 'dict';
  entries: Map<string, PdfValue>;
}
interface PdfBoolean {
  kind: 'bool';
  value: boolean;
}

interface Cursor {
  text: string;
  pos: number;
}

const WHITESPACE = new Set(['\u0000', '\t', '\n', '\f', '\r', ' ']);
const DELIMITERS = new Set(['(', ')', '<', '>', '[', ']', '{', '}', '/', '%']);

function skipWhitespace(cursor: Cursor): void {
  while (cursor.pos < cursor.text.length) {
    const ch = cursor.text[cursor.pos] ?? '';
    if (WHITESPACE.has(ch)) {
      cursor.pos += 1;
      continue;
    }
    if (ch === '%') {
      while (cursor.pos < cursor.text.length) {
        const inner = cursor.text[cursor.pos];
        if (inner === '\n' || inner === '\r') break;
        cursor.pos += 1;
      }
      continue;
    }
    return;
  }
}

function readHexString(cursor: Cursor): string {
  cursor.pos += 1; // the '<'
  let digits = '';
  while (cursor.pos < cursor.text.length && (cursor.text[cursor.pos] ?? '') !== '>') {
    const ch = cursor.text[cursor.pos] ?? '';
    if (/[0-9a-fA-F]/.test(ch)) digits += ch;
    cursor.pos += 1;
  }
  cursor.pos += 1; // the '>'
  if (digits.length % 2 === 1) digits += '0';
  let out = '';
  for (let index = 0; index + 1 < digits.length; index += 2) {
    out += String.fromCharCode(Number.parseInt(digits.slice(index, index + 2), 16));
  }
  return out;
}

function readLiteralString(cursor: Cursor): string {
  cursor.pos += 1; // the '('
  let depth = 1;
  let out = '';
  while (cursor.pos < cursor.text.length) {
    const ch = cursor.text[cursor.pos] ?? '';
    cursor.pos += 1;
    if (ch === '\\') {
      const escape = cursor.text[cursor.pos] ?? '';
      cursor.pos += 1;
      if (escape === 'n') out += '\n';
      else if (escape === 'r') out += '\r';
      else if (escape === 't') out += '\t';
      else if (escape === 'b') out += '\b';
      else if (escape === 'f') out += '\f';
      else if (escape === '\n') out += '';
      else if (escape === '\r') {
        if ((cursor.text[cursor.pos] ?? '') === '\n') cursor.pos += 1;
      } else if (escape >= '0' && escape <= '7') {
        let octal = escape;
        while (octal.length < 3 && /[0-7]/.test(cursor.text[cursor.pos] ?? '')) {
          octal += cursor.text[cursor.pos] ?? '';
          cursor.pos += 1;
        }
        out += String.fromCharCode(Number.parseInt(octal, 8));
      } else out += escape;
      continue;
    }
    if (ch === '(') {
      depth += 1;
      out += ch;
      continue;
    }
    if (ch === ')') {
      depth -= 1;
      if (depth === 0) break;
      out += ch;
      continue;
    }
    out += ch;
  }
  return out;
}

function readName(cursor: Cursor): string {
  cursor.pos += 1; // the '/'
  let out = '';
  while (cursor.pos < cursor.text.length) {
    const ch = cursor.text[cursor.pos] ?? '';
    if (WHITESPACE.has(ch) || DELIMITERS.has(ch)) break;
    cursor.pos += 1;
    if (ch === '#' && cursor.pos + 1 < cursor.text.length) {
      const hex = cursor.text.slice(cursor.pos, cursor.pos + 2);
      if (/^[0-9a-fA-F]{2}$/.test(hex)) {
        out += String.fromCharCode(Number.parseInt(hex, 16));
        cursor.pos += 2;
        continue;
      }
    }
    out += ch;
  }
  return out;
}

function readNumberToken(cursor: Cursor): number | null {
  const start = cursor.pos;
  while (cursor.pos < cursor.text.length && /[+\-.0-9]/.test(cursor.text[cursor.pos] ?? '')) {
    cursor.pos += 1;
  }
  if (cursor.pos === start) return null;
  const value = Number(cursor.text.slice(start, cursor.pos));
  if (!Number.isFinite(value)) {
    cursor.pos = start;
    return null;
  }
  return value;
}

function readValue(cursor: Cursor, depth = 0): PdfValue {
  skipWhitespace(cursor);
  if (depth > 64 || cursor.pos >= cursor.text.length) return null;
  const ch = cursor.text[cursor.pos] ?? '';

  if (ch === '<') {
    if ((cursor.text[cursor.pos + 1] ?? '') === '<') {
      cursor.pos += 2;
      const entries = new Map<string, PdfValue>();
      for (;;) {
        skipWhitespace(cursor);
        if (cursor.pos >= cursor.text.length) break;
        if ((cursor.text[cursor.pos] ?? '') === '>' && (cursor.text[cursor.pos + 1] ?? '') === '>') {
          cursor.pos += 2;
          break;
        }
        if ((cursor.text[cursor.pos] ?? '') !== '/') {
          // Not a key: consume one token so a malformed dictionary cannot spin forever.
          const before = cursor.pos;
          readValue(cursor, depth + 1);
          if (cursor.pos === before) cursor.pos += 1;
          continue;
        }
        const key = readName(cursor);
        entries.set(key, readValue(cursor, depth + 1));
      }
      return { kind: 'dict', entries };
    }
    return { kind: 'string', value: readHexString(cursor) };
  }

  if (ch === '/') return { kind: 'name', value: readName(cursor) };

  if (ch === '[') {
    cursor.pos += 1;
    const items: PdfValue[] = [];
    for (;;) {
      skipWhitespace(cursor);
      if (cursor.pos >= cursor.text.length) break;
      if ((cursor.text[cursor.pos] ?? '') === ']') {
        cursor.pos += 1;
        break;
      }
      const before = cursor.pos;
      items.push(readValue(cursor, depth + 1));
      if (cursor.pos === before) cursor.pos += 1;
    }
    return { kind: 'array', items };
  }

  if (ch === '(') return { kind: 'string', value: readLiteralString(cursor) };

  if (cursor.text.startsWith('true', cursor.pos)) {
    cursor.pos += 4;
    return { kind: 'bool', value: true };
  }
  if (cursor.text.startsWith('false', cursor.pos)) {
    cursor.pos += 5;
    return { kind: 'bool', value: false };
  }
  if (cursor.text.startsWith('null', cursor.pos)) {
    cursor.pos += 4;
    return null;
  }

  // A number — which may be the first half of an indirect reference ("12 0 R").
  const first = readNumberToken(cursor);
  if (first === null) {
    cursor.pos += 1;
    return null;
  }
  const afterFirst = cursor.pos;
  skipWhitespace(cursor);
  const second = readNumberToken(cursor);
  if (second !== null) {
    skipWhitespace(cursor);
    if ((cursor.text[cursor.pos] ?? '') === 'R') {
      cursor.pos += 1;
      return { kind: 'ref', num: first, gen: second };
    }
  }
  cursor.pos = afterFirst;
  return { kind: 'number', value: first };
}

function dictGet(value: PdfValue | null | undefined, key: string): PdfValue | null {
  if (value === null || value === undefined || value.kind !== 'dict') return null;
  return value.entries.get(key) ?? null;
}

function dictName(value: PdfValue | null): string | null {
  return value !== null && value.kind === 'name' ? value.value : null;
}

function dictNumber(value: PdfValue | null): number | null {
  return value !== null && value.kind === 'number' ? value.value : null;
}

// ---------------------------------------------------------------------------
// The file
// ---------------------------------------------------------------------------

interface PdfObject {
  num: number;
  gen: number;
  dict: PdfDict;
  /** The whole file, so a stream can be sliced without copying it up front. */
  rawBytes: Buffer;
  streamStart: number | null;
  streamEnd: number | null;
  decoded: Buffer | null;
  decodeFailed: boolean;
}

export function looksLikePdf(bytes: Uint8Array): boolean {
  const head = Buffer.from(bytes.subarray(0, Math.min(bytes.length, 1024))).toString('latin1');
  return /^\s*%PDF-\d+\.\d+/.test(head);
}

const OBJECT_HEADER = /(\d{1,10})\s+(\d{1,5})\s+obj\b/g;

function parseObjects(raw: string, buffer: Buffer): Map<number, PdfObject> {
  const objects = new Map<number, PdfObject>();
  OBJECT_HEADER.lastIndex = 0;
  let match = OBJECT_HEADER.exec(raw);
  while (match !== null) {
    const num = Number(match[1]);
    const gen = Number(match[2]);
    const bodyStart = match.index + match[0].length;
    const cursor: Cursor = { text: raw, pos: bodyStart };
    const value = readValue(cursor);
    if (value !== null && value.kind === 'dict' && !objects.has(num)) {
      let streamStart: number | null = null;
      let streamEnd: number | null = null;
      const probe: Cursor = { text: raw, pos: cursor.pos };
      skipWhitespace(probe);
      if (raw.startsWith('stream', probe.pos)) {
        let start = probe.pos + 'stream'.length;
        if (raw[start] === '\r') start += 1;
        if (raw[start] === '\n') start += 1;
        const declared = dictNumber(value.entries.get('Length') ?? null);
        const endstreamAt = raw.indexOf('endstream', start);
        const candidate = declared === null ? -1 : start + declared;
        const fallback = endstreamAt === -1 ? raw.length : endstreamAt;
        streamStart = start;
        streamEnd = candidate > start && candidate <= fallback ? candidate : fallback;
      }
      objects.set(num, {
        num,
        gen,
        dict: value,
        rawBytes: buffer,
        streamStart,
        streamEnd,
        decoded: null,
        decodeFailed: false,
      });
    }
    OBJECT_HEADER.lastIndex = bodyStart;
    match = OBJECT_HEADER.exec(raw);
  }
  return objects;
}

function resolve(objects: Map<number, PdfObject>, value: PdfValue | null): PdfValue | null {
  let current = value;
  for (let guard = 0; guard < 32 && current !== null && current.kind === 'ref'; guard += 1) {
    const target = objects.get(current.num);
    if (target === undefined) return null;
    current = target.dict;
  }
  return current;
}

function filterNames(objects: Map<number, PdfObject>, dict: PdfDict): string[] {
  const filter = resolve(objects, dict.entries.get('Filter') ?? null);
  if (filter === null) return [];
  if (filter.kind === 'name') return [filter.value];
  if (filter.kind === 'array') {
    return filter.items
      .map((item) => dictName(resolve(objects, item)) ?? '')
      .filter((name) => name !== '');
  }
  return [];
}

function trimTrailingEol(bytes: Buffer): Buffer {
  let end = bytes.length;
  while (end > 0) {
    const ch = bytes[end - 1];
    if (ch === 0x0a || ch === 0x0d) end -= 1;
    else break;
  }
  return end === bytes.length ? bytes : bytes.subarray(0, end);
}

/**
 * The decoded bytes of one object's stream, remembered after the first read.
 *
 * Only `FlateDecode` (and no filter at all) is supported. Anything else sets
 * `decodeFailed`, which the caller turns into a stated problem: an unreadable
 * stream must never be mistaken for an empty one.
 */
function streamBytes(objects: Map<number, PdfObject>, object: PdfObject): Buffer | null {
  if (object.streamStart === null || object.streamEnd === null) return null;
  if (object.decoded !== null || object.decodeFailed) return object.decoded;

  const filter = filterNames(objects, object.dict);
  if (filter.some((name) => name !== 'FlateDecode')) {
    object.decodeFailed = true;
    return null;
  }

  const slice = Buffer.from(object.rawBytes.subarray(object.streamStart, Math.max(object.streamStart, object.streamEnd)));
  if (filter.length === 0) {
    object.decoded = slice;
    return object.decoded;
  }

  // The declared length is usually exact; when it is not, the trailing newline
  // before `endstream` is the usual culprit, so it is retried without it.
  for (const candidate of [slice, trimTrailingEol(slice)]) {
    try {
      object.decoded = inflateSync(candidate);
      return object.decoded;
    } catch {
      try {
        object.decoded = inflateRawSync(candidate);
        return object.decoded;
      } catch {
        /* try the next candidate */
      }
    }
  }
  object.decodeFailed = true;
  return null;
}

// ---------------------------------------------------------------------------
// Text decoding
// ---------------------------------------------------------------------------

/** The 0x80–0x9F range of cp1252, where it differs from latin-1. */
const CP1252_HIGH = [
  0x20ac, 0x0081, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039,
  0x0152, 0x008d, 0x017d, 0x008f, 0x0090, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
  0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x009d, 0x017e, 0x0178,
];

export function decodeCp1252(bytes: Uint8Array): string {
  let out = '';
  for (const byte of bytes) {
    if (byte >= 0x80 && byte <= 0x9f) out += String.fromCharCode(CP1252_HIGH[byte - 0x80] ?? byte);
    else out += String.fromCharCode(byte);
  }
  return out;
}

/** A `/ToUnicode` CMap: character codes in, UTF-16 text out. */
export function parseToUnicode(source: string): Map<number, string> {
  const map = new Map<number, string>();
  /**
   * A `/ToUnicode` destination is a UTF-16BE string, so the hex digits are read
   * four at a time: two at a time would turn `<0056>` into NUL followed by "V".
   */
  const hexToText = (hex: string): string => {
    let out = '';
    if (hex.length % 4 === 0) {
      for (let index = 0; index + 3 < hex.length; index += 4) {
        out += String.fromCharCode(Number.parseInt(hex.slice(index, index + 4), 16));
      }
      return out;
    }
    for (let index = 0; index + 1 < hex.length; index += 2) {
      out += String.fromCharCode(Number.parseInt(hex.slice(index, index + 2), 16));
    }
    return out;
  };
  const tokens = source.match(/<[0-9a-fA-F]*>|\[|\]|[A-Za-z]+|-?\d+/g) ?? [];

  let index = 0;
  while (index < tokens.length) {
    const token = tokens[index] ?? '';
    if (token === 'beginbfchar') {
      index += 1;
      while (index < tokens.length && (tokens[index] ?? '') !== 'endbfchar') {
        const from = tokens[index] ?? '';
        const to = tokens[index + 1] ?? '';
        index += 2;
        if (from.startsWith('<') && to.startsWith('<')) {
          map.set(Number.parseInt(from.slice(1, -1), 16), hexToText(to.slice(1, -1)));
        }
      }
    } else if (token === 'beginbfrange') {
      index += 1;
      while (index < tokens.length && (tokens[index] ?? '') !== 'endbfrange') {
        const low = tokens[index] ?? '';
        const high = tokens[index + 1] ?? '';
        const target = tokens[index + 2] ?? '';
        index += 3;
        if (!low.startsWith('<') || !high.startsWith('<')) continue;
        const from = Number.parseInt(low.slice(1, -1), 16);
        const to = Number.parseInt(high.slice(1, -1), 16);
        if (!Number.isFinite(from) || !Number.isFinite(to) || to < from || to - from > 65535) continue;
        if (target.startsWith('<')) {
          const base = target.slice(1, -1);
          const tail = Number.parseInt(base.slice(-4) || '0', 16);
          const head = base.slice(0, -4);
          for (let code = from; code <= to; code += 1) {
            const suffix = (tail + (code - from)).toString(16).padStart(4, '0');
            map.set(code, hexToText(head + suffix));
          }
        } else if (target === '[') {
          let offset = 0;
          while (index < tokens.length && (tokens[index] ?? '') !== ']') {
            const item = tokens[index] ?? '';
            index += 1;
            if (item.startsWith('<')) map.set(from + offset, hexToText(item.slice(1, -1)));
            offset += 1;
          }
        }
      }
    }
    index += 1;
  }
  return map;
}

interface FontDecoder {
  decode(bytes: Uint8Array): string;
}

function buildFontDecoder(
  objects: Map<number, PdfObject>,
  fontDict: PdfDict | null,
  problems: string[],
  fontLabel: string,
): FontDecoder {
  const twoByte = dictName(resolve(objects, dictGet(fontDict, 'Subtype'))) === 'Type0';
  const toUnicodeRef = dictGet(fontDict, 'ToUnicode');
  const toUnicodeObject =
    toUnicodeRef !== null && toUnicodeRef.kind === 'ref' ? objects.get(toUnicodeRef.num) : undefined;

  if (toUnicodeObject !== undefined) {
    const cmap = streamBytes(objects, toUnicodeObject);
    if (cmap !== null) {
      const map = parseToUnicode(cmap.toString('latin1'));
      if (map.size > 0) {
        return {
          decode: (bytes) => {
            let text = '';
            const step = twoByte ? 2 : 1;
            for (let index = 0; index + step - 1 < bytes.length; index += step) {
              const code =
                step === 2
                  ? ((bytes[index] ?? 0) << 8) | (bytes[index + 1] ?? 0)
                  : (bytes[index] ?? 0);
              text += map.get(code) ?? '';
            }
            return text;
          },
        };
      }
    }
  }

  if (twoByte) {
    // A two-byte font with no character map holds glyph ids, and a glyph id is not
    // a character. Saying so is the only honest answer.
    problems.push(
      `A fonte ${fontLabel} não traz mapa de caracteres (/ToUnicode): o texto escrito com ela não foi lido.`,
    );
    return { decode: () => '' };
  }

  return { decode: decodeCp1252 };
}

// ---------------------------------------------------------------------------
// Content streams
// ---------------------------------------------------------------------------

type Operand =
  | { kind: 'string'; bytes: Buffer }
  | { kind: 'number'; value: number }
  | { kind: 'name'; value: string }
  | { kind: 'array'; items: Operand[] }
  | { kind: 'other' };

function readOperand(cursor: Cursor): Operand | null {
  skipWhitespace(cursor);
  if (cursor.pos >= cursor.text.length) return null;
  const ch = cursor.text[cursor.pos] ?? '';
  if (ch === '(') return { kind: 'string', bytes: Buffer.from(readLiteralString(cursor), 'latin1') };
  if (ch === '<') {
    if ((cursor.text[cursor.pos + 1] ?? '') === '<') return { kind: 'other' };
    return { kind: 'string', bytes: Buffer.from(readHexString(cursor), 'latin1') };
  }
  if (ch === '/') return { kind: 'name', value: readName(cursor) };
  if (ch === '[') {
    cursor.pos += 1;
    const items: Operand[] = [];
    for (;;) {
      skipWhitespace(cursor);
      if (cursor.pos >= cursor.text.length) break;
      if ((cursor.text[cursor.pos] ?? '') === ']') {
        cursor.pos += 1;
        break;
      }
      const before = cursor.pos;
      const item = readOperand(cursor);
      if (item !== null && item.kind !== 'other') items.push(item);
      if (cursor.pos === before) cursor.pos += 1;
    }
    return { kind: 'array', items };
  }
  const saved = cursor.pos;
  const number = readNumberToken(cursor);
  if (number !== null) return { kind: 'number', value: number };
  cursor.pos = saved;
  return { kind: 'other' };
}

type Matrix = readonly [number, number, number, number, number, number];

function multiply(left: Matrix, right: Matrix): Matrix {
  return [
    left[0] * right[0] + left[2] * right[1],
    left[1] * right[0] + left[3] * right[1],
    left[0] * right[2] + left[2] * right[3],
    left[1] * right[2] + left[3] * right[3],
    left[0] * right[4] + left[2] * right[5] + left[4],
    left[1] * right[4] + left[3] * right[5] + left[5],
  ];
}

export interface PdfTextItem {
  x: number;
  y: number;
  size: number;
  text: string;
}

interface TextState {
  fontName: string;
  fontSize: number;
  leading: number;
  tm: Matrix;
  tlm: Matrix;
}

/**
 * One page's content streams into positioned text items.
 *
 * Only the START position of each shown string is kept: rows are built by
 * baseline, so an approximated advance width would add error to the one
 * coordinate that matters and none to the one that does not.
 */
function renderContent(
  content: Buffer,
  fonts: Map<string, FontDecoder>,
  items: PdfTextItem[],
  problems: string[],
): void {
  const cursor: Cursor = { text: content.toString('latin1'), pos: 0 };
  const operands: Operand[] = [];
  const ctmStack: Matrix[] = [];
  let ctm: Matrix = [1, 0, 0, 1, 0, 0];
  const state: TextState = {
    fontName: '',
    fontSize: 0,
    leading: 0,
    tm: [1, 0, 0, 1, 0, 0],
    tlm: [1, 0, 0, 1, 0, 0],
  };
  const missingFonts = new Set<string>();

  const numbers = (): number[] =>
    operands
      .filter((item): item is { kind: 'number'; value: number } => item.kind === 'number')
      .map((item) => item.value);

  const showString = (bytes: Buffer): void => {
    const decoder = fonts.get(state.fontName);
    if (decoder === undefined) {
      if (state.fontName !== '' && !missingFonts.has(state.fontName)) {
        missingFonts.add(state.fontName);
        problems.push(`O texto usa a fonte ${state.fontName}, que não está declarada na página.`);
      }
      // No declared font: cp1252 is the best reading of the bytes, and the row
      // parser will simply not find the labels if that reading is wrong.
      const text = decodeCp1252(bytes);
      if (text === '') return;
      const placed = multiply(ctm, state.tm);
      items.push({ x: placed[4], y: placed[5], size: state.fontSize > 0 ? state.fontSize : 10, text });
      return;
    }
    const text = decoder.decode(bytes);
    if (text === '') return;
    const placed = multiply(ctm, state.tm);
    items.push({ x: placed[4], y: placed[5], size: state.fontSize > 0 ? state.fontSize : 10, text });
  };

  for (;;) {
    skipWhitespace(cursor);
    if (cursor.pos >= cursor.text.length) break;
    const before = cursor.pos;
    const operand = readOperand(cursor);
    if (operand === null) break;
    if (operand.kind !== 'other') {
      operands.push(operand);
      if (operands.length > 64) operands.splice(0, operands.length - 64);
      continue;
    }

    const start = cursor.pos;
    while (cursor.pos < cursor.text.length) {
      const ch = cursor.text[cursor.pos] ?? '';
      if (WHITESPACE.has(ch) || DELIMITERS.has(ch)) break;
      cursor.pos += 1;
    }
    if (cursor.pos === before) {
      cursor.pos += 1;
      continue;
    }
    const operator = cursor.text.slice(start, cursor.pos);
    const args = numbers();

    switch (operator) {
      case 'q':
        ctmStack.push(ctm);
        break;
      case 'Q':
        ctm = ctmStack.pop() ?? [1, 0, 0, 1, 0, 0];
        break;
      case 'cm':
        if (args.length >= 6) {
          ctm = multiply(ctm, [
            args[0] ?? 1,
            args[1] ?? 0,
            args[2] ?? 0,
            args[3] ?? 1,
            args[4] ?? 0,
            args[5] ?? 0,
          ]);
        }
        break;
      case 'BT':
        state.tm = [1, 0, 0, 1, 0, 0];
        state.tlm = [1, 0, 0, 1, 0, 0];
        break;
      case 'Tf': {
        const name = operands.find((item) => item.kind === 'name');
        state.fontName = name !== undefined && name.kind === 'name' ? name.value : '';
        state.fontSize = args[args.length - 1] ?? 0;
        break;
      }
      case 'TL':
        state.leading = args[0] ?? 0;
        break;
      case 'Td':
      case 'TD': {
        const tx = args[0] ?? 0;
        const ty = args[1] ?? 0;
        if (operator === 'TD') state.leading = -ty;
        state.tlm = multiply(state.tlm, [1, 0, 0, 1, tx, ty]);
        state.tm = state.tlm;
        break;
      }
      case 'Tm':
        if (args.length >= 6) {
          state.tm = [
            args[0] ?? 1,
            args[1] ?? 0,
            args[2] ?? 0,
            args[3] ?? 1,
            args[4] ?? 0,
            args[5] ?? 0,
          ];
          state.tlm = state.tm;
        }
        break;
      case 'T*':
        state.tlm = multiply(state.tlm, [1, 0, 0, 1, 0, -state.leading]);
        state.tm = state.tlm;
        break;
      case 'Tj':
      case "'":
      case '"': {
        if (operator !== 'Tj') {
          state.tlm = multiply(state.tlm, [1, 0, 0, 1, 0, -state.leading]);
          state.tm = state.tlm;
        }
        for (const item of operands) {
          if (item.kind === 'string') showString(item.bytes);
        }
        break;
      }
      case 'TJ': {
        const array = [...operands].reverse().find((item) => item.kind === 'array');
        if (array !== undefined && array.kind === 'array') {
          for (const item of array.items) {
            if (item.kind === 'string') showString(item.bytes);
          }
        }
        break;
      }
      default:
        break;
    }
    operands.length = 0;
  }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/** How far apart two baselines can be and still be the same line of a form. */
function rowTolerance(size: number): number {
  return Math.max(2, Math.min(3.5, size * 0.45));
}

/**
 * Positioned items into lines.
 *
 * Two items belong to the same line when their baselines are within a fraction of
 * the type size. That fraction is what keeps a label and the value beside it on
 * one line while leaving the next table row — four points lower, in the forms this
 * targets — on its own.
 */
export function itemsToRows(items: readonly PdfTextItem[]): string[] {
  const sorted = [...items].sort((left, right) => right.y - left.y || left.x - right.x);
  const rows: { y: number; size: number; items: PdfTextItem[] }[] = [];
  for (const item of sorted) {
    const current = rows[rows.length - 1];
    if (current !== undefined && Math.abs(current.y - item.y) <= rowTolerance(current.size)) {
      current.items.push(item);
      current.size = Math.max(current.size, item.size);
      continue;
    }
    rows.push({ y: item.y, size: item.size, items: [item] });
  }
  return rows.map((row) => {
    const ordered = [...row.items].sort((left, right) => left.x - right.x);
    return ordered
      .map((item) => item.text)
      .join(' ')
      .replace(/[\u00a0\u202f]/g, ' ')
      .replace(/\s+/g, ' ')
      .trim();
  });
}

// ---------------------------------------------------------------------------
// Pages, fonts and the public entry point
// ---------------------------------------------------------------------------

interface PdfPageRef {
  resources: PdfValue | null;
  contents: PdfObject[];
}

function contentObjects(objects: Map<number, PdfObject>, dict: PdfDict): PdfObject[] {
  const contents = resolve(objects, dict.entries.get('Contents') ?? null);
  const refs: PdfValue[] =
    contents !== null && contents.kind === 'array'
      ? contents.items
      : [dict.entries.get('Contents') ?? null];
  const found: PdfObject[] = [];
  for (const ref of refs) {
    if (ref !== null && ref.kind === 'ref') {
      const object = objects.get(ref.num);
      if (object !== undefined) found.push(object);
    }
  }
  return found;
}

function collectPages(objects: Map<number, PdfObject>, problems: string[]): PdfPageRef[] {
  const pages: PdfPageRef[] = [];
  const byNumber = [...objects.values()].sort((left, right) => left.num - right.num);
  const catalog = byNumber.find((object) => dictName(object.dict.entries.get('Type') ?? null) === 'Catalog');

  const visit = (value: PdfValue | null, inherited: PdfValue | null, depth: number): void => {
    if (depth > 64 || value === null) return;
    const resolved = resolve(objects, value);
    if (resolved === null || resolved.kind !== 'dict') return;
    const resources = resolved.entries.get('Resources') ?? inherited;
    if (dictName(resolved.entries.get('Type') ?? null) === 'Page') {
      pages.push({ resources, contents: contentObjects(objects, resolved) });
      return;
    }
    const kids = resolve(objects, resolved.entries.get('Kids') ?? null);
    if (kids !== null && kids.kind === 'array') {
      for (const kid of kids.items) visit(kid, resources, depth + 1);
    }
  };

  if (catalog !== undefined) visit(catalog.dict.entries.get('Pages') ?? null, null, 0);

  if (pages.length === 0) {
    // No usable page tree: fall back to every object that calls itself a page,
    // which is what a damaged or hand-built file usually still has.
    for (const object of byNumber) {
      if (dictName(object.dict.entries.get('Type') ?? null) !== 'Page') continue;
      pages.push({
        resources: object.dict.entries.get('Resources') ?? null,
        contents: contentObjects(objects, object.dict),
      });
    }
    if (pages.length > 0) {
      problems.push(
        'A árvore de páginas do PDF não pôde ser lida; foram usados os objetos marcados como página.',
      );
    }
  }
  return pages;
}

function collectFonts(
  objects: Map<number, PdfObject>,
  resources: PdfValue | null,
  problems: string[],
): Map<string, FontDecoder> {
  const fonts = new Map<string, FontDecoder>();
  const resolved = resolve(objects, resources);
  if (resolved === null || resolved.kind !== 'dict') return fonts;
  const fontResources = resolve(objects, resolved.entries.get('Font') ?? null);
  if (fontResources === null || fontResources.kind !== 'dict') return fonts;

  for (const [name, value] of fontResources.entries) {
    const fontObject = resolve(objects, value);
    const dict = fontObject !== null && fontObject.kind === 'dict' ? fontObject : null;
    fonts.set(name, buildFontDecoder(objects, dict, problems, name));
  }
  return fonts;
}

export interface PdfPageText {
  index: number;
  items: PdfTextItem[];
  rows: string[];
}

export interface PdfText {
  pageCount: number;
  pages: PdfPageText[];
  /** Every page's rows, in document order. */
  rows: string[];
  text: string;
  problems: string[];
}

export function extractPdfText(bytes: Uint8Array): PdfText {
  const empty = (problems: string[]): PdfText => ({
    pageCount: 0,
    pages: [],
    rows: [],
    text: '',
    problems,
  });

  const problems: string[] = [];
  const buffer = Buffer.from(bytes);
  if (!looksLikePdf(buffer)) {
    return empty(['O ficheiro não começa por "%PDF-": não parece ser um PDF.']);
  }

  const raw = buffer.toString('latin1');
  if (/\/Encrypt\b/.test(raw)) {
    return empty([
      'O PDF está protegido (cifrado). Esta aplicação não abre PDFs com palavra-passe nem com ' +
        'restrições de cópia: retira a proteção no programa que o emitiu e volta a tentar. Se só ' +
        'queres guardar o ficheiro no cofre, usa «Guardar documento no cofre».',
    ]);
  }
  if (/\/ObjStm\b/.test(raw)) {
    problems.push(
      'O PDF guarda os objetos em fluxos comprimidos (Object Streams), uma forma que este leitor não ' +
        'abre. Se os campos não forem reconhecidos, guarda o documento no cofre e registra a fatura à mão.',
    );
  }

  const objects = parseObjects(raw, buffer);
  if (objects.size === 0) {
    return empty(['O PDF não tem objetos legíveis: o ficheiro pode estar truncado.']);
  }

  const pages = collectPages(objects, problems);
  if (pages.length === 0) {
    problems.push('Não foi encontrada nenhuma página no PDF.');
  }

  const result: PdfPageText[] = [];
  for (const [index, page] of pages.entries()) {
    const fonts = collectFonts(objects, page.resources, problems);
    const items: PdfTextItem[] = [];
    for (const contentObject of page.contents) {
      const data = streamBytes(objects, contentObject);
      if (data === null) {
        if (contentObject.decodeFailed) {
          problems.push('Um dos fluxos de conteúdo do PDF não pôde ser descomprimido.');
        }
        continue;
      }
      renderContent(data, fonts, items, problems);
    }
    result.push({ index, items, rows: itemsToRows(items).filter((row) => row !== '') });
  }

  if (result.length > 0 && result.every((page) => page.rows.length === 0)) {
    problems.push(
      'Não foi encontrado texto no PDF. Se for um documento digitalizado (uma imagem), não há texto ' +
        'para ler: o ficheiro pode ser guardado no cofre e a fatura registada à mão.',
    );
  }

  const rows = result.flatMap((page) => page.rows);
  return {
    pageCount: result.length,
    pages: result,
    rows,
    text: rows.join('\n'),
    problems: [...new Set(problems)],
  };
}
