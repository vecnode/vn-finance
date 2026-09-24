/**
 * The PDF reader.
 *
 * The reader is the part of the import that is hardest to trust, so these tests
 * cover both halves of its promise: it finds the text of a form, and it SAYS SO
 * when it cannot. The second half matters more — a reader that returns an empty
 * string for a scanned page, or garbage for a font it does not understand, would
 * turn "we could not read this" into "this document is empty".
 */

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { decodeCp1252, extractPdfText, itemsToRows, looksLikePdf, parseToUnicode } from './pdf.ts';
import { buildPdf, imageOnlyPdf } from './receipt-fixture.ts';

test('a PDF is recognised by its header, and anything else is not', () => {
  assert.equal(looksLikePdf(buildPdf([{ text: 'olá', y: 700 }])), true);
  assert.equal(looksLikePdf(Buffer.from('<!DOCTYPE html><p>não é um pdf</p>')), false);
  assert.equal(looksLikePdf(Buffer.alloc(0)), false);
});

test('the text of a one-page form is read, row by row', () => {
  const pdf = extractPdfText(
    buildPdf([
      { text: 'Fatura-Recibo <FR TESTE/2026/7>', y: 789 },
      { text: 'emitida em 15/03/2026', y: 778 },
      { text: 'Valor ilíquido 1.000,00 €', y: 300 },
    ]),
  );

  assert.deepEqual(pdf.problems, [], 'um PDF simples não deve produzir problemas');
  assert.equal(pdf.pageCount, 1);
  assert.deepEqual(pdf.rows, [
    'Fatura-Recibo <FR TESTE/2026/7>',
    'emitida em 15/03/2026',
    'Valor ilíquido 1.000,00 €',
  ]);
});

test('a label and the value beside it land on the same row, in reading order', () => {
  const rows = itemsToRows([
    { x: 300, y: 300, size: 8, text: '1.000,00 €' },
    { x: 40, y: 300, size: 8, text: 'Valor ilíquido' },
    { x: 40, y: 280, size: 8, text: 'TOTAL A PAGAR' },
  ]);

  assert.deepEqual(rows, ['Valor ilíquido 1.000,00 €', 'TOTAL A PAGAR']);
});

test('rows four points apart stay apart, which is what keeps a table readable', () => {
  const rows = itemsToRows([
    { x: 40, y: 500, size: 7, text: 'Consultoria' },
    { x: 40, y: 496, size: 7, text: 'Serviço' },
  ]);

  assert.equal(rows.length, 2, 'duas linhas da tabela não podem ser fundidas numa só');
});

test('Portuguese accents survive the round trip through cp1252', () => {
  const pdf = extractPdfText(buildPdf([{ text: 'SEDE OU DOMICÍLIO, Avenida da República, 2.º', y: 700 }]));
  assert.deepEqual(pdf.rows, ['SEDE OU DOMICÍLIO, Avenida da República, 2.º']);

  assert.equal(decodeCp1252(Buffer.from([0xe7, 0xe3, 0xf5])), 'çãõ');
  // 0x80 is the euro sign in cp1252 and a control character in latin-1.
  assert.equal(decodeCp1252(Buffer.from([0x80])), '€');
});

test('escaped parentheses and octal codes are decoded, not printed raw', () => {
  const pdf = extractPdfText(
    buildPdf([
      { text: 'Fatura-Recibo (FR 2026/7)', y: 700 },
      { text: 'CÂMARA MUNICIPAL', y: 690 },
    ]),
  );

  assert.deepEqual(pdf.rows, ['Fatura-Recibo (FR 2026/7)', 'CÂMARA MUNICIPAL']);
});

test('hex strings are decoded as well as literal strings', () => {
  const pdf = extractPdfText(buildPdf([{ text: 'Fatura n.º 7', y: 700 }], { hexStrings: true }));
  assert.deepEqual(pdf.rows, ['Fatura n.º 7']);
});

test('a file that is not a PDF says so instead of returning empty text', () => {
  const pdf = extractPdfText(Buffer.from('%!PS-Adobe-3.0\nshowpage\n'));
  assert.equal(pdf.text, '');
  assert.equal(pdf.pageCount, 0);
  assert.match(pdf.problems.join(' '), /não parece ser um PDF/);
});

test('an encrypted PDF is refused with an explanation, not read as empty', () => {
  const pdf = extractPdfText(buildPdf([{ text: 'Fatura-Recibo', y: 700 }], { encrypted: true }));
  assert.equal(pdf.text, '');
  assert.match(pdf.problems.join(' '), /cifrado|protegido/);
});

test('a two-byte font with no character map is reported rather than guessed', () => {
  const pdf = extractPdfText(
    buildPdf([{ text: 'Fatura-Recibo', y: 700 }], { fontWithoutUnicodeMap: true }),
  );

  assert.equal(pdf.text.replace(/\s/g, ''), '', 'não há caracteres a inventar a partir de glyph ids');
  assert.match(pdf.problems.join(' '), /ToUnicode/);
});

test('a page that is an image is reported as having no text to read', () => {
  const pdf = extractPdfText(imageOnlyPdf());
  assert.equal(pdf.pageCount, 1);
  assert.deepEqual(pdf.rows, []);
  assert.match(pdf.problems.join(' '), /Não foi encontrado texto/);
});

test('an object stream layout is announced as unsupported instead of silently empty', () => {
  const base = buildPdf([{ text: 'Fatura-Recibo', y: 700 }]);
  const patched = Buffer.concat([base, Buffer.from('\n6 0 obj\n<< /Type /ObjStm /N 1 /First 10 >>\nendobj\n')]);
  const pdf = extractPdfText(patched);
  assert.match(pdf.problems.join(' '), /Object Streams/);
  // The text is still read: warning about a layout must not throw away what works.
  assert.deepEqual(pdf.rows, ['Fatura-Recibo']);
});

test('a ToUnicode CMap maps codes to characters, including ranges', () => {
  const cmap = parseToUnicode(`
    beginbfchar
    <0041> <0056>
    endbfchar
    beginbfrange
    <0042> <0044> <0061>
    endbfrange
  `);

  assert.equal(cmap.get(0x41), 'V');
  assert.equal(cmap.get(0x42), 'a');
  assert.equal(cmap.get(0x43), 'b');
  assert.equal(cmap.get(0x44), 'c');
});

test('a truncated file is reported instead of throwing', () => {
  const pdf = extractPdfText(Buffer.from('%PDF-1.5\n1 0 obj\n<< /Type /Catalog'));
  assert.equal(pdf.text, '');
  assert.ok(pdf.problems.length > 0, 'um ficheiro truncado tem de dizer o que se passa');
});
