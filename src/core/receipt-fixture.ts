/**
 * The sample documents the PDF and fatura-recibo tests are built on.
 *
 * These are FALSE documents with invented names and numbers. The tests must not
 * depend on anybody's real invoice: personal data never enters this repository, and
 * a fixture that is generated here also lets a test state exactly which layout it
 * is testing — a missing total, a wrong checksum, a two-byte font — instead of
 * hoping a real file happens to contain the case.
 *
 * The builder writes a small but structurally complete PDF: catalog, page tree,
 * one Flate-compressed content stream, a WinAnsi Helvetica font, and an xref
 * table. Nothing here is used at runtime; it exists so `pdf.test.ts` can assert
 * against bytes that a real PDF reader would also accept.
 */

import { deflateSync } from 'node:zlib';

export interface FixtureRow {
  text: string;
  y: number;
  x?: number;
  size?: number;
}

export interface FixtureOptions {
  /** A two-byte font with no `/ToUnicode`, to exercise the refusal path. */
  fontWithoutUnicodeMap?: boolean;
  /** Write the strings as hex rather than literal strings. */
  hexStrings?: boolean;
  /** An empty content stream: a page that carries no text at all. */
  noText?: boolean;
  /** Add an `/Encrypt` entry, so the file must be refused rather than read. */
  encrypted?: boolean;
}

function escapeLiteral(text: string): string {
  let out = '';
  for (const ch of text) {
    if (ch === '(' || ch === ')' || ch === '\\') out += `\\${ch}`;
    else if (ch === '\n') out += '\\n';
    else out += ch;
  }
  return out;
}

function toHexString(text: string): string {
  let out = '';
  for (const ch of text) out += ch.charCodeAt(0).toString(16).padStart(2, '0');
  return out;
}

/**
 * The bytes a PDF would hold for this text.
 *
 * Writing "€" as latin-1 would put 0xAC (¬) in the file, because latin-1 has no
 * euro sign; the character lives at 0x80 in cp1252, which is the encoding the
 * fixture's font declares. Getting this wrong in the fixture would make the test
 * assert the reader's ability to decode a mistake.
 */
function toCp1252(text: string): Buffer {
  const bytes: number[] = [];
  for (const ch of text) {
    const code = ch.charCodeAt(0);
    if (code === 0x20ac) bytes.push(0x80);
    else if (code <= 0xff) bytes.push(code);
    else bytes.push(0x3f); // "?"
  }
  return Buffer.from(bytes);
}

/** The content stream of one page: every row drawn at the position it was given. */
export function contentStream(rows: readonly FixtureRow[], options: FixtureOptions = {}): string {
  const parts: string[] = ['q', 'BT', '/F1 8 Tf', 'ET', 'Q'];
  for (const row of rows) {
    const size = row.size ?? 8;
    const x = row.x ?? 40;
    const literal = options.hexStrings
      ? `<${toHexString(row.text)}>`
      : `(${escapeLiteral(row.text)})`;
    parts.push(
      'BT',
      `1 0 0 1 ${x} ${row.y} Tm`,
      `/F1 ${size} Tf`,
      '0 0 0 rg',
      `${literal}Tj`,
      '0 g',
      'ET',
      '1 0 0 1 0 0 cm',
    );
  }
  return parts.join('\n');
}

/**
 * A one-page PDF around the given content.
 *
 * The object numbers are fixed so the offsets in the xref table can be computed in
 * a single pass; a reader that ignores the xref (this application's, for instance)
 * still sees the same objects.
 */
export function buildPdf(rows: readonly FixtureRow[], options: FixtureOptions = {}): Buffer {
  const stream = deflateSync(toCp1252(contentStream(rows, options)));

  const fontDict = options.fontWithoutUnicodeMap
    ? '<< /Type /Font /Subtype /Type0 /BaseFont /ABCDEF+Embedded /Encoding /Identity-H /DescendantFonts [] >>'
    : '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>';

  const objects: string[] = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>',
    `<< /Length ${stream.length} /Filter /FlateDecode >>\nstream\n${stream.toString('latin1')}\nendstream`,
    fontDict,
  ];
  if (options.encrypted === true) objects.push('<< /Filter /Standard /V 1 /R 2 /O (x) /U (y) /P -1 >>');

  let file = '%PDF-1.5\n%\u00e2\u00e3\u00cf\u00d3\n';
  const offsets: number[] = [];
  for (const [index, body] of objects.entries()) {
    offsets.push(file.length);
    file += `${index + 1} 0 obj\n${body}\nendobj\n`;
  }
  const xrefAt = file.length;
  file += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) file += `${String(offset).padStart(10, '0')} 00000 n \n`;
  file +=
    `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R` +
    (options.encrypted === true ? ' /Encrypt 6 0 R' : '') +
    ` >>\nstartxref\n${xrefAt}\n%%EOF\n`;

  return Buffer.from(file, 'latin1');
}

/** A document whose page has a content stream but no text operators. */
export function imageOnlyPdf(): Buffer {
  const stream = deflateSync(Buffer.from('q 100 0 0 100 40 40 cm /Im0 Do Q', 'latin1'));
  const objects: string[] = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> /Contents 4 0 R >>',
    `<< /Length ${stream.length} /Filter /FlateDecode >>\nstream\n${stream.toString('latin1')}\nendstream`,
  ];
  let file = '%PDF-1.4\n';
  const offsets: number[] = [];
  for (const [index, body] of objects.entries()) {
    offsets.push(file.length);
    file += `${index + 1} 0 obj\n${body}\nendobj\n`;
  }
  const xrefAt = file.length;
  file += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) file += `${String(offset).padStart(10, '0')} 00000 n \n`;
  file += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xrefAt}\n%%EOF\n`;
  return Buffer.from(file, 'latin1');
}

// ---------------------------------------------------------------------------
// A fatura-recibo, invented
// ---------------------------------------------------------------------------

export interface ReceiptFixtureOverrides {
  /** Who issued it. The default is the taxpayer's own number, as on a real one. */
  issuerNif?: string;
  issuerName?: string;
  customerNif?: string;
  customerName?: string;
  base?: string;
  iva?: string;
  rate?: string;
  total?: string;
  retention?: string;
  net?: string;
  /** Drop the "DADOS DO TRANSMITENTE…" header, as layouts without sections do. */
  withoutIssuerSection?: boolean;
}

/**
 * The rows of a Portuguese `fatura-recibo`, one per line of the form.
 *
 * The figures are internally consistent — 1 000,00 at 23% is 1 230,00 — so a test
 * that changes one of them is testing exactly the disagreement it says it is.
 */
export function faturaReciboRows(overrides: ReceiptFixtureOverrides = {}): FixtureRow[] {
  const issuerName = overrides.issuerName ?? 'MARIA EXEMPLO SILVA';
  const issuerNif = overrides.issuerNif ?? '123456789';
  const customerName = overrides.customerName ?? 'Empresa Exemplo, Lda.';
  const customerNif = overrides.customerNif ?? '501234560';
  const base = overrides.base ?? '1.000,00 €';
  const iva = overrides.iva ?? '230,00 €';
  const rate = overrides.rate ?? '23,00%';
  const total = overrides.total ?? '1.230,00 €';
  const retention = overrides.retention ?? '0,00 €';
  const net = overrides.net ?? '1.230,00 €';

  const raw = [
    'ATCUD:ABCD1234-1',
    'Fatura-Recibo <FR TESTE/2026/7>',
    'emitida em 15/03/2026',
    'Original',
    'DADOS DO TRANSMITENTE DE BENS OU DO PRESTADOR DE SERVIÇOS',
    `NOME ${issuerName}`,
    'DOMICÍLIO Rua da Amostra 10, 1000-100 Lisboa',
    `NÚMERO DE IDENTIFICAÇÃO FISCAL (NIF) - ${issuerNif}`,
    'DADOS DO ADQUIRENTE DE BENS OU DE SERVIÇOS',
    `NOME ${customerName}`,
    'SEDE OU DOMICÍLIO Av. Exemplo 5, 4000-000 Porto, PORTUGAL',
    `NÚMERO DE IDENTIFICAÇÃO FISCAL ${customerNif}`,
    'DADOS DA TRANSMISSÃO DE BENS OU DA PRESTAÇÃO DE SERVIÇOS',
    'Documento emitido a título de: Pagamento dos bens ou dos serviços',
    'Data da colocação à disposição dos bens/realização dos serviços: 15/03/2026',
    'VALOR',
    'DESCRIÇÃO QTD DESCONTO TAXA IVA TOTAL C/IMPOSTO',
    'UNITÁRIO',
    'Consultoria de exemplo em engenharia civil',
    'Serviço C/IVA IVA',
    '1 Unidade - 1.230,00 €',
    `Consultoria de exemplo em engenharia civil e gestão de obra ${base} ${rate.replace(',00', '')} %`,
    'IVA',
    'Taxa Motivo de isenção/não sujeição/não tributação Valor Tributável Valor IVA',
    `${rate} - Taxa Normal - Continente ${base} ${iva}`,
    'IRS',
    'Base de incidência Retenção na fonte IRS Rendimento Valor IRS',
    `Sem retenção - Art.101º, n.º1 do CIRS ${retention}`,
    'TOTAIS DO DOCUMENTO',
    `Valor ilíquido ${base}`,
    `IVA ${iva}`,
    'Imposto do Selo 0,00 €',
    `TOTAL DO DOCUMENTO ${total}`,
    `Retenção na fonte IRS ${retention}`,
    `TOTAL A PAGAR ${net}`,
    'Página 1 de 1',
  ];

  const rows = raw
    .filter((line) => !(overrides.withoutIssuerSection === true && line.startsWith('DADOS DO TRANSMITENTE')))
    .map((text, index) => ({ text, y: 800 - index * 11, size: 8 }));
  return rows;
}

export function faturaReciboPdf(overrides: ReceiptFixtureOverrides = {}): Buffer {
  return buildPdf(faturaReciboRows(overrides));
}
