/**
 * Reading a Portuguese `fatura-recibo` out of the text of a PDF.
 *
 * The unit of work is a PAGE OF ROWS — the output of `pdf.ts` — because that is
 * what a form looks like once the positions are gone: labels and the value beside
 * them on one line. Everything here is therefore about recognition, and the two
 * rules that shape every function are the ones the rest of the application lives
 * by:
 *
 *   1. A FIGURE IS READ OR IT IS ABSENT. There is no default, no "probably 23%",
 *      no plausible-looking zero. A missing field is reported as missing, and the
 *      person completing the form supplies it.
 *   2. THE DOCUMENT IS CHECKED AGAINST ITSELF. A fatura states its base, its VAT
 *      rate, its VAT, its stamp duty and its totals; those have to agree. The
 *      checks run here, in the core, so the panel and the command line report the
 *      same disagreements, and so a disagreement is visible BEFORE anything is
 *      written to the ledger rather than after.
 *
 * The parser is deliberately tolerant about layout and strict about arithmetic.
 * Issuers differ in wording and order, so labels are matched by pattern; but no
 * pattern match is ever allowed to invent a number.
 */

import { scopeOf } from './countries.ts';
import { applyBp, formatEur, parseEurToCents } from './money.ts';
import { isValidPtTaxNumber, normaliseNif } from './nif.ts';
import type { Invoice, InvoiceStatus, VatTreatment } from './estimate.ts';
import type { IsoDate, RulePack, TaxProfile } from './types.ts';
import type { PdfText } from './pdf.ts';

export interface ReceiptParty {
  name: string | null;
  nif: string | null;
  address: string | null;
}

/** One line of "what was read", for a panel that must not look more certain than it is. */
export interface ReceiptField {
  key: string;
  label: string;
  value: string | null;
  hint: string | null;
}

/** One arithmetic or consistency check the document has to pass. */
export interface ReceiptCheck {
  id: string;
  label: string;
  ok: boolean;
  detail: string;
}

export interface ReceiptDraft {
  kind: string | null;
  number: string | null;
  atcud: string | null;
  date: IsoDate | null;
  issuer: ReceiptParty;
  customer: ReceiptParty;
  description: string | null;
  /** "Valor ilíquido": the base the VAT and the retention are computed on. */
  baseCents: number | null;
  ivaCents: number | null;
  ivaRateBp: number | null;
  /** "Imposto do Selo", which the invoice ledger does not represent. */
  stampDutyCents: number | null;
  retentionCents: number | null;
  /** "TOTAL DO DOCUMENTO". */
  totalCents: number | null;
  /** "TOTAL A PAGAR". */
  netCents: number | null;
  retentionReason: string | null;
  /** "Documento emitido a título de: …". */
  purpose: string | null;
  /** The document itself says it was paid (a recibo, or "Pagamento dos bens"). */
  declaresPaid: boolean;
  /** Where the customer is, when the document supports a reading: never invented. */
  customerCountryHint: string | null;
  /** Every VAT rate the document charges. More than one is a real possibility. */
  ivaRatesBp: number[];
  pageCount: number;
  fields: ReceiptField[];
  checks: ReceiptCheck[];
  problems: string[];
}

// ---------------------------------------------------------------------------
// Rows and labels
// ---------------------------------------------------------------------------

/** Text with accents removed and runs of whitespace collapsed, plus a map back. */
interface Normalised {
  value: string;
  /** For each character of `value`, its index in the original string. */
  origin: number[];
}

export function normaliseWithMap(text: string): Normalised {
  const value: string[] = [];
  const origin: number[] = [];
  let pendingSpaceAt = -1;
  for (let index = 0; index < text.length; index += 1) {
    const stripped = (text[index] ?? '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
    for (const ch of stripped) {
      if (/\s/.test(ch)) {
        // The space is remembered with ITS OWN index: a collapsed run must still
        // point at the first character it replaces, or a label cut would land one
        // character too far and eat the first letter of the value.
        if (value.length > 0 && pendingSpaceAt < 0) pendingSpaceAt = index;
        continue;
      }
      if (pendingSpaceAt >= 0) {
        value.push(' ');
        origin.push(pendingSpaceAt);
        pendingSpaceAt = -1;
      }
      value.push(ch);
      origin.push(index);
    }
  }
  return { value: value.join(''), origin };
}

/** Where the match ends in the original string. */
function endOfMatch(normalised: Normalised, match: RegExpExecArray): number {
  const last = match.index + match[0].length - 1;
  return (normalised.origin[last] ?? -1) + 1;
}

/** The part of a row that follows a label at its start, or null. */
function tailAfter(row: string, pattern: RegExp): string | null {
  const normalised = normaliseWithMap(row);
  const match = pattern.exec(normalised.value);
  if (match === null) return null;
  const tail = row.slice(endOfMatch(normalised, match)).replace(/^[\s:.\-–—]+/, '').trim();
  return tail === '' ? null : tail;
}

function rowsOf(pdf: PdfText | readonly string[]): string[] {
  const rows: readonly string[] = Array.isArray(pdf) ? pdf : (pdf as PdfText).rows;
  return rows.map((row) => row.replace(/[\u00a0\u202f]/g, ' ').replace(/\s+/g, ' ').trim());
}

/** A Portuguese money amount as it is printed on the document. */
const MONEY = /\d{1,3}(?:[ .]\d{3})*,\d{2}|\d+,\d{2}|\d+\.\d{2}/;

function amountFrom(token: string): number | null {
  try {
    return parseEurToCents(token);
  } catch {
    return null;
  }
}

/** The last money-looking token of a row, read as Portuguese euros. */
function lastAmount(row: string): number | null {
  const matches = row.match(new RegExp(MONEY, 'g'));
  if (matches === null || matches.length === 0) return null;
  return amountFrom(matches[matches.length - 1] ?? '');
}

/**
 * The amount on the first row whose label matches at its START.
 *
 * The anchor matters: "IVA" also appears inside longer labels ("VALOR IVA",
 * "TAXA IVA"), and a label matched in the middle of a row would read a figure that
 * belongs to something else.
 */
function amountForLabel(rows: readonly string[], patterns: readonly RegExp[]): number | null {
  for (const pattern of patterns) {
    for (const row of rows) {
      const normalised = normaliseWithMap(row);
      const match = pattern.exec(normalised.value);
      if (match === null || match.index !== 0) continue;
      const amount = lastAmount(row.slice(endOfMatch(normalised, match)));
      if (amount !== null) return amount;
    }
  }
  return null;
}

function rowMatching(rows: readonly string[], pattern: RegExp): string | null {
  for (const row of rows) {
    if (pattern.test(normaliseWithMap(row).value)) return row;
  }
  return null;
}

const NIF_IN_ROW = /\b(\d{9})\b/;

function nifIn(row: string): string | null {
  const match = NIF_IN_ROW.exec(row);
  return match === null ? null : (match[1] ?? null);
}

function isoFromPortuguese(date: string): IsoDate | null {
  const match = /^(\d{1,2})[/\-.](\d{1,2})[/\-.](\d{4})$/.exec(date.trim());
  if (match === null) return null;
  const day = Number(match[1]);
  const month = Number(match[2]);
  const year = Number(match[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) return null;
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  if (day > daysInMonth) return null;
  return `${match[3]}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

interface Section {
  header: string | null;
  rows: string[];
}

const SECTION_MARKERS: ReadonlyArray<{ key: string; pattern: RegExp }> = [
  { key: 'issuer', pattern: /^dados do (?:transmitente|prestador|vendedor)/ },
  { key: 'customer', pattern: /^dados do (?:adquirente|cliente|comprador)/ },
  { key: 'body', pattern: /^dados da (?:transmissao|prestacao)|^detalhes da/ },
  { key: 'vat', pattern: /^iva\b|^resumo de iva|^taxa\b.*motivo/ },
  { key: 'irs', pattern: /^irs\b|^retencao na fonte/ },
  { key: 'totals', pattern: /^totais do documento|^resumo do documento/ },
];

/**
 * Split the page into the labelled blocks a fatura-recibo is made of.
 *
 * "DADOS DO TRANSMITENTE…" and "DADOS DO ADQUIRENTE…" are the two that matter:
 * they are what lets this code say WHOSE invoice it is instead of inferring it
 * from the order in which two tax numbers happen to appear.
 */
function splitSections(rows: readonly string[]): Map<string, Section> {
  const sections = new Map<string, Section>();
  let current: Section | null = null;

  for (const row of rows) {
    const normalised = normaliseWithMap(row).value;
    const marker = SECTION_MARKERS.find((candidate) => candidate.pattern.test(normalised));
    if (marker !== undefined) {
      const section: Section = { header: row, rows: [] };
      sections.set(marker.key, section);
      current = section;
      continue;
    }
    if (current !== null) current.rows.push(row);
  }
  return sections;
}

function partyFrom(section: Section | undefined): ReceiptParty {
  if (section === undefined) return { name: null, nif: null, address: null };
  const rows = section.rows;

  let name: string | null = null;
  for (const row of rows) {
    const tail = tailAfter(row, /^(?:nome|nome\s*\/\s*razao social|nome ou razao social|nome completo)\b/);
    if (tail !== null && !/^[/\\]/.test(tail)) {
      name = tail;
      break;
    }
  }
  if (name === null) {
    // Some layouts put the label on its own line and the value on the next one.
    for (const [index, row] of rows.entries()) {
      if (/^nome(?:\s*\/\s*razao social)?$/.test(normaliseWithMap(row).value)) {
        const next = rows[index + 1];
        if (next !== undefined && nifIn(next) === null) {
          name = next;
          break;
        }
      }
    }
  }

  const nifRow = rowMatching(rows, /identificacao fiscal|^nif\b|^nipc\b/);
  const nif = nifRow === null ? null : nifIn(nifRow);

  const addressRow = rowMatching(rows, /domicilio|sede|morada|endereco/);
  const address =
    addressRow === null
      ? null
      : tailAfter(addressRow, /^(?:[^:]*?(?:sede ou domicilio|domicilio|sede|morada|endereco))\s*:?\s*/);

  return { name, nif, address };
}

// ---------------------------------------------------------------------------
// The parser
// ---------------------------------------------------------------------------

const DOCUMENT_KINDS: ReadonlyArray<{ pattern: RegExp; label: string }> = [
  { pattern: /fatura[\s-]*recibo/i, label: 'Fatura-Recibo' },
  { pattern: /fatura simplificada/i, label: 'Fatura Simplificada' },
  { pattern: /nota de credito/i, label: 'Nota de Crédito' },
  { pattern: /nota de debito/i, label: 'Nota de Débito' },
  { pattern: /fatura/i, label: 'Fatura' },
  { pattern: /recibo/i, label: 'Recibo' },
];

/** Rows that describe the document rather than its content. */
const NOT_A_DESCRIPTION =
  /^(?:valor|descricao|qtd|quantidade|unitario|desconto|taxa|total|data|documento|nome|nif|numero|atcud|iva|irs|base|retencao|imposto|pagina|original|duplicado|servico c\/iva|unidade)\b/;

export interface ParseReceiptOptions {
  /** When the caller counted the pages, used to explain Original + Duplicado. */
  pageCount?: number;
}

/**
 * Read one fatura-recibo out of a PDF's text.
 *
 * Nothing here throws: an unreadable document produces a draft whose fields are
 * null and whose `problems` say why. The caller decides what to do about that,
 * and the one thing it must not do is fill the gaps with guesses.
 */
export function parseReceipt(
  pdf: PdfText | readonly string[],
  options: ParseReceiptOptions = {},
): ReceiptDraft {
  const isArray = Array.isArray(pdf);
  const rows = rowsOf(pdf).filter((row) => row !== '');
  const problems: string[] = isArray ? [] : [...(pdf as PdfText).problems];
  const pageCount = options.pageCount ?? (isArray ? 1 : (pdf as PdfText).pageCount);
  const sections = splitSections(rows);

  // --- document identity ---------------------------------------------------
  let kind: string | null = null;
  let number: string | null = null;
  for (const row of rows) {
    const found = DOCUMENT_KINDS.find((candidate) => candidate.pattern.test(row));
    if (found === undefined) continue;
    kind = found.label;
    const bracketed = /[<«]([^>»]{2,60})[>»]/.exec(row);
    if (bracketed !== null) number = (bracketed[1] ?? '').trim();
    if (number === null) {
      const labelled = /(?:n\.?[ºo°]|numero)\s*[:.]?\s*([A-Za-z0-9][A-Za-z0-9/\-. ]{0,40})/i.exec(row);
      if (labelled !== null) number = (labelled[1] ?? '').trim();
    }
    break;
  }

  const atcudRow = rowMatching(rows, /atcud/);
  const atcudMatch =
    atcudRow === null ? null : /atcud\s*:?\s*([A-Za-z0-9][A-Za-z0-9-]{3,40})/i.exec(atcudRow);
  const atcud = atcudMatch === null ? null : (atcudMatch[1] ?? '').toUpperCase();

  // The issue date: "emitida em 20/09/2026", "Data: 20/09/2026", "Data de emissão".
  let date: IsoDate | null = null;
  const dateRow =
    rowMatching(rows, /emitid[ao] em|data de emissao|^data\b|data da fatura|^emitido\b/) ??
    rowMatching(rows, /\d{2}[/\-.]\d{2}[/\-.]\d{4}/);
  if (dateRow !== null) {
    const match = /(\d{1,2}[/\-.]\d{1,2}[/\-.]\d{4})/.exec(dateRow);
    if (match !== null) date = isoFromPortuguese(match[1] ?? '');
  }

  // --- parties -------------------------------------------------------------
  const issuer = partyFrom(sections.get('issuer'));
  const customer = partyFrom(sections.get('customer'));
  if (sections.get('issuer') === undefined) {
    problems.push(
      'Não foi encontrada a secção do prestador de serviços ("DADOS DO TRANSMITENTE…"): pelo documento ' +
        'sozinho não é possível confirmar que esta fatura é tua.',
    );
  }

  // --- description ---------------------------------------------------------
  const bodyRows = sections.get('body')?.rows ?? [];
  const description = descriptionFrom(bodyRows);

  // --- amounts -------------------------------------------------------------
  const baseCents = amountForLabel(rows, [
    /^valor iliquido\b/,
    /^total iliquido\b/,
    /^base de incidencia\b/,
    /^valor sem iva\b/,
    /^subtotal\b/,
    /^valor tributavel\b/,
  ]);
  const ivaCents = amountForLabel(rows, [/^iva\b/, /^total de iva\b/, /^valor iva\b/]);
  const stampDutyCents = amountForLabel(rows, [/^imposto do selo\b/, /^selo\b/]);
  const totalCents = amountForLabel(rows, [
    /^total do documento\b/,
    /^total da fatura\b/,
    /^total\b(?!\s*a\s+(?:pagar|receber))/,
  ]);
  const retentionCents = amountForLabel(rows, [
    /^retencao na fonte irs\b/,
    /^retencao na fonte\b/,
    /^retencao irs\b/,
    /^valor irs\b/,
  ]);
  const netCents = amountForLabel(rows, [/^total a pagar\b/, /^total a receber\b/, /^liquido a pagar\b/]);

  // --- VAT rates -----------------------------------------------------------
  const ivaRatesBp = vatRates(rows, baseCents, ivaCents);
  const ivaRateBp = ivaRatesBp.length === 1 ? (ivaRatesBp[0] ?? null) : null;

  const retentionReasonRow = rowMatching(rows, /sem retencao|art\.?\s*101|nao sujeito a retencao/);
  const retentionReason =
    retentionReasonRow === null ? null : (withoutFigures(retentionReasonRow).slice(0, 200) || null);

  const purposeRow = rowMatching(rows, /documento emitido a titulo de/);
  const purpose = purposeRow === null ? null : tailAfter(purposeRow, /^documento emitido a titulo de\s*:?\s*/);

  const declaresPaid =
    (kind !== null && /fatura[\s-]*recibo/i.test(kind)) ||
    (purpose !== null && /pagamento/i.test(purpose));

  // --- the customer's country ---------------------------------------------
  let customerCountryHint: string | null = null;
  if (customer.nif !== null && isValidPtTaxNumber(customer.nif)) customerCountryHint = 'PT';
  else if (customer.address !== null && /portugal/i.test(customer.address)) customerCountryHint = 'PT';
  else if (customer.address !== null) {
    const tail = customer.address.split(/[,\s]+/).filter((part) => part !== '').pop() ?? '';
    if (/^[A-Z]{2}$/.test(tail)) customerCountryHint = tail;
  }

  // --- checks --------------------------------------------------------------
  const checks = buildChecks({ baseCents, ivaCents, stampDutyCents, totalCents, retentionCents, netCents, ivaRatesBp, issuer, customer });
  for (const check of checks) {
    if (!check.ok) problems.push(`Verificação falhada — ${check.label}: ${check.detail}`);
  }

  const fields: ReceiptField[] = [
    { key: 'kind', label: 'Tipo de documento', value: kind, hint: null },
    { key: 'number', label: 'Número', value: number, hint: number === null ? 'Não encontrado no documento.' : null },
    { key: 'atcud', label: 'ATCUD', value: atcud, hint: atcud === null ? 'Não encontrado no documento.' : null },
    { key: 'date', label: 'Data de emissão', value: date, hint: date === null ? 'Não encontrada.' : null },
    { key: 'issuerNif', label: 'NIF do prestador', value: issuer.nif, hint: issuer.nif === null ? 'Não encontrado.' : null },
    {
      key: 'customerNif',
      label: 'NIF do adquirente',
      value: customer.nif,
      hint: customer.nif === null ? 'Não encontrado (documento sem NIF do cliente).' : null,
    },
    { key: 'baseCents', label: 'Valor ilíquido', value: money(baseCents), hint: baseCents === null ? 'Não encontrado.' : null },
    { key: 'ivaCents', label: 'IVA', value: money(ivaCents), hint: ivaCents === null ? 'Não encontrado.' : null },
    {
      key: 'ivaRateBp',
      label: 'Taxa de IVA',
      value: ivaRateBp === null ? null : `${ivaRateBp / 100}%`,
      hint: ivaRateBp === null ? (ivaRatesBp.length > 1 ? 'O documento tem mais do que uma taxa.' : 'Não encontrada.') : null,
    },
    { key: 'totalCents', label: 'Total do documento', value: money(totalCents), hint: totalCents === null ? 'Não encontrado.' : null },
    {
      key: 'retentionCents',
      label: 'Retenção na fonte',
      value: money(retentionCents),
      hint: retentionCents === null ? 'Não encontrada.' : (retentionReason ?? null),
    },
    { key: 'netCents', label: 'Total a pagar', value: money(netCents), hint: netCents === null ? 'Não encontrado.' : null },
  ];

  if (baseCents === null) {
    problems.push(
      'Não foi lido o valor ilíquido ("Valor ilíquido" ou "Base de incidência"). Sem ele não há fatura ' +
        'para registar: escreve-o no formulário a partir do documento.',
    );
  }

  return {
    kind,
    number,
    atcud,
    date,
    issuer,
    customer,
    description,
    baseCents,
    ivaCents,
    ivaRateBp,
    stampDutyCents,
    retentionCents,
    totalCents,
    netCents,
    retentionReason,
    purpose,
    declaresPaid,
    customerCountryHint,
    ivaRatesBp,
    pageCount,
    fields,
    checks,
    problems: [...new Set(problems)],
  };
}

function money(cents: number | null): string | null {
  return cents === null ? null : formatEur(cents);
}

/**
 * A free-text fragment with the table's figures cut off.
 *
 * On a fatura, the description of a line sits in the same row as its quantity,
 * unit price and VAT rate. The text is what a person wrote; the rest is a column,
 * and a column is not part of a description.
 */
function withoutFigures(text: string): string {
  const cut = text.search(new RegExp(`${MONEY.source}|\\s\\d{1,2}(?:[.,]\\d{1,2})?\\s*%`));
  const head = cut === -1 ? text : text.slice(0, cut);
  return head.replace(/[\s\-–—·|]+$/, '').trim();
}

/**
 * The description of what was supplied.
 *
 * Two strategies, in order. An issuer that labels the line ("Descrição: …") is
 * believed; failing that, the longest plausible line BELOW the "DESCRIÇÃO" column
 * header wins, because that is where the free text of an invoice table lives —
 * everything above it is a label or a date, and everything with an amount in it is
 * a figure, not a description.
 */
function descriptionFrom(bodyRows: readonly string[]): string | null {
  for (const row of bodyRows) {
    const tail = tailAfter(row, /^(?:descricao|referencia|descricao do servico|descricao da prestacao)\s*:?\s*/);
    if (tail === null) continue;
    if (/qtd|unitario|desconto|taxa|total/.test(normaliseWithMap(tail).value)) continue;
    const text = withoutFigures(tail);
    if (text !== '') return text.slice(0, 300);
  }

  const headerAt = bodyRows.findIndex((row) => /^descricao\b/.test(normaliseWithMap(row).value));
  const candidates = (headerAt >= 0 ? bodyRows.slice(headerAt + 1) : bodyRows)
    .filter((row) => row.length >= 8)
    .filter((row) => nifIn(row) === null)
    .filter((row) => !/\d{2}[/\-.]\d{2}[/\-.]\d{4}/.test(row))
    .filter((row) => !NOT_A_DESCRIPTION.test(normaliseWithMap(row).value))
    .map((row) => withoutFigures(row))
    .filter((row) => row.length >= 4)
    .sort((left, right) => right.length - left.length);
  const best = candidates[0];
  return best === undefined ? null : best.slice(0, 300);
}

/** Every VAT rate the document charges, as basis points. */
function vatRates(rows: readonly string[], baseCents: number | null, ivaCents: number | null): number[] {
  const rates: number[] = [];
  const add = (bp: number): void => {
    if (!rates.includes(bp)) rates.push(bp);
  };

  // 1. A rate stated at the start of a row, which is what an IVA summary table
  //    looks like: "23,00% - Taxa Normal - Continente   813,01 €   186,99 €".
  for (const row of rows) {
    const normalised = normaliseWithMap(row).value;
    const percent = /^(\d{1,2}(?:[.,]\d{1,2})?)\s*%/.exec(normalised);
    if (percent === null) continue;
    if (!/taxa|iva|isenc|normal|intermedi|reduzid/.test(normalised)) continue;
    const value = Number((percent[1] ?? '').replace(',', '.'));
    if (Number.isFinite(value) && value >= 0 && value <= 100) add(Math.round(value * 100));
  }
  if (rates.length > 0) return rates;

  // 2. A rate anywhere in a row that talks about VAT — some issuers print the
  //    column value without repeating the word.
  for (const row of rows) {
    const normalised = normaliseWithMap(row).value;
    if (!/taxa|iva/.test(normalised)) continue;
    const percent = /(\d{1,2}(?:[.,]\d{1,2})?)\s*%/.exec(normalised);
    if (percent === null) continue;
    const value = Number((percent[1] ?? '').replace(',', '.'));
    if (Number.isFinite(value) && value >= 0 && value <= 100) add(Math.round(value * 100));
  }
  if (rates.length > 0) return rates;

  // 3. No rate is printed: a document that states its base and its VAT has said
  //    its rate implicitly, and this is arithmetic on the document's own figures.
  if (baseCents !== null && ivaCents !== null && baseCents > 0) {
    add(Math.round((ivaCents / baseCents) * 10_000));
  }
  return rates;
}

interface CheckInput {
  baseCents: number | null;
  ivaCents: number | null;
  stampDutyCents: number | null;
  totalCents: number | null;
  retentionCents: number | null;
  netCents: number | null;
  ivaRatesBp: readonly number[];
  issuer: ReceiptParty;
  customer: ReceiptParty;
}

/**
 * The document checked against itself.
 *
 * A check that cannot run is NOT emitted: "we could not verify this" and "this is
 * wrong" are different statements, and only the second one belongs in a list of
 * failures.
 */
function buildChecks(input: CheckInput): ReceiptCheck[] {
  const checks: ReceiptCheck[] = [];
  const { baseCents, ivaCents, stampDutyCents, retentionCents, netCents } = input;

  if (input.totalCents !== null && ivaCents !== null && baseCents !== null && stampDutyCents !== null) {
    const sum = baseCents + ivaCents + stampDutyCents;
    checks.push({
      id: 'total',
      label: 'Base + IVA + imposto do selo = total do documento',
      ok: sum === input.totalCents,
      detail: `${formatEur(baseCents)} + ${formatEur(ivaCents)} + ${formatEur(stampDutyCents)} = ${formatEur(sum)} · documento: ${formatEur(input.totalCents)}`,
    });
  }
  if (
    input.totalCents !== null &&
    netCents !== null &&
    retentionCents !== null &&
    ivaCents !== null &&
    baseCents !== null
  ) {
    const gross = baseCents + ivaCents + (stampDutyCents ?? 0);
    checks.push({
      id: 'payable',
      label: 'Total do documento − retenção = total a pagar',
      ok: gross - retentionCents === netCents,
      detail: `${formatEur(gross)} − ${formatEur(retentionCents)} = ${formatEur(gross - retentionCents)} · documento: ${formatEur(netCents)}`,
    });
  }
  if (baseCents !== null && ivaCents !== null && input.ivaRatesBp.length === 1) {
    const rate = input.ivaRatesBp[0] ?? 0;
    checks.push({
      id: 'rate',
      label: 'Taxa de IVA aplicada à base = IVA do documento',
      ok: applyBp(baseCents, rate) === ivaCents,
      detail: `${formatEur(baseCents)} × ${rate / 100}% = ${formatEur(applyBp(baseCents, rate))} · documento: ${formatEur(ivaCents)}`,
    });
  }
  for (const [party, person] of [
    ['prestador', input.issuer],
    ['adquirente', input.customer],
  ] as const) {
    if (person.nif === null) continue;
    const valid = isValidPtTaxNumber(person.nif);
    checks.push({
      id: `nif-${party}`,
      label: `Dígito de controlo do NIF do ${party}`,
      ok: valid,
      detail: `${person.nif} ${valid ? 'é um NIF português válido' : 'não passa o dígito de controlo'}`,
    });
  }
  return checks;
}

// ---------------------------------------------------------------------------
// From the draft to a ledger invoice
// ---------------------------------------------------------------------------

/** What the person confirmed or corrected in the panel. */
export interface ReceiptConfirmation {
  number?: string | null;
  date?: IsoDate | null;
  clientName?: string | null;
  clientNif?: string | null;
  clientCountry?: string | null;
  description?: string | null;
  baseCents?: number | null;
  ivaRateBp?: number | null;
  retentionBp?: number | null;
  vatTreatment?: VatTreatment | null;
  status?: InvoiceStatus | null;
  atcud?: string | null;
}

export interface ReceiptInvoiceInput {
  draft: ReceiptDraft;
  pack: RulePack;
  profile: TaxProfile;
  confirmed: ReceiptConfirmation;
  /** Identifier for the new ledger record; supplied by the caller so it stays a wire concern. */
  id: string;
  /** The name the document was archived under, when it was archived. */
  documentFile?: string | null;
  /** Used only when neither the document nor the person supplies a number. */
  fallbackNumber: string;
}

export interface ReceiptInvoiceResult {
  invoice: Invoice | null;
  problems: string[];
  warnings: string[];
  /** Where what was confirmed differs from what the document says. */
  divergences: string[];
}

function pick<T>(confirmed: T | null | undefined, read: T | null): T | null {
  return confirmed === null || confirmed === undefined ? read : confirmed;
}

/**
 * The basis points that reproduce a retention figure exactly.
 *
 * The ledger stores a RATE and recomputes the amount, so a rate that does not
 * round-trip would quietly change the document's own arithmetic. The candidate is
 * accepted only when `applyBp` gives back what the document printed.
 */
export function retentionBpFor(baseCents: number, retentionCents: number): number | null {
  if (baseCents <= 0) return null;
  if (retentionCents === 0) return 0;
  const candidate = Math.round((retentionCents / baseCents) * 10_000);
  return applyBp(baseCents, candidate) === retentionCents ? candidate : null;
}

/**
 * Build the ledger record from a parsed document plus the person's confirmation.
 *
 * The refusal that matters most is the first one: a `fatura-recibo` can be yours
 * or one somebody issued to you, and the only trustworthy way to tell them apart
 * is the tax number in the "prestador" section. Recording a supplier's invoice as
 * your own income would inflate every estimate that follows, so a document whose
 * issuer is not the profile's NIF is REFUSED with an explanation instead.
 */
export function invoiceFromReceipt(input: ReceiptInvoiceInput): ReceiptInvoiceResult {
  const { draft, pack, profile, confirmed } = input;
  const problems: string[] = [];
  const warnings: string[] = [];
  const divergences: string[] = [];

  const profileNif = normaliseNif(profile.nif);
  const issuerNif = draft.issuer.nif === null ? null : normaliseNif(draft.issuer.nif);
  const customerNif = draft.customer.nif === null ? null : normaliseNif(draft.customer.nif);

  if (issuerNif !== null && issuerNif !== profileNif) {
    const asCustomer = customerNif !== null && customerNif === profileNif;
    problems.push(
      `Esta fatura foi emitida pelo NIF ${issuerNif}, e o perfil é o NIF ${profileNif}. ` +
        (asCustomer
          ? 'O adquirente é o NIF do perfil, ou seja: é uma fatura que RECEBESTE, não uma que emitiste. ' +
            'O livro de despesas ainda não existe nesta aplicação, por isso não pode ser registada aqui.'
          : 'Não é uma fatura tua, por isso não pode entrar no livro de recibos emitidos.'),
    );
  }
  if (issuerNif === null && customerNif === profileNif) {
    problems.push(
      'O documento não identifica o prestador, mas o adquirente é o NIF do perfil: parece ser uma fatura ' +
        'que recebeste. O livro de despesas ainda não existe nesta aplicação.',
    );
  }
  if (issuerNif === null && customerNif === null) {
    warnings.push(
      'O documento não identifica nenhum NIF. Confirma que é uma fatura emitida por ti antes de a registar.',
    );
  }

  const country = pick(confirmed.clientCountry ?? null, draft.customerCountryHint);
  if (country === null) {
    problems.push(
      'Não foi possível ler o país do cliente no documento. Escolhe-o no formulário: é o que decide o ' +
        'tratamento de IVA e a retenção.',
    );
  }

  const baseCents = pick(confirmed.baseCents ?? null, draft.baseCents);
  if (baseCents === null || !Number.isInteger(baseCents) || baseCents <= 0) {
    problems.push('Falta o valor ilíquido da fatura (em cêntimos, maior do que zero).');
  }

  const scope = country === null ? 'national' : scopeOf(country);
  let treatment = confirmed.vatTreatment ?? null;
  if (treatment === null && country !== null) {
    if (scope === 'national') {
      const rate = pick(confirmed.ivaRateBp ?? null, draft.ivaRateBp);
      treatment = rate !== null && rate > 0 ? 'iva_pt' : 'isento_art53';
    } else {
      treatment = scope === 'eu' ? 'autoliquidacao_ue' : 'exportacao';
    }
  }

  let ivaRateBp = pick(confirmed.ivaRateBp ?? null, draft.ivaRateBp);
  if (treatment !== null && treatment !== 'iva_pt') {
    if (ivaRateBp !== null && ivaRateBp !== 0) {
      warnings.push(
        `O tratamento "${treatment}" não liquida IVA português; a taxa fica em 0% e não em ${ivaRateBp / 100}%.`,
      );
    }
    ivaRateBp = 0;
  }
  if (treatment === 'iva_pt' && (ivaRateBp === null || ivaRateBp === 0)) {
    problems.push(
      'A fatura liquida IVA português mas não foi lida nenhuma taxa. Escreve a taxa do documento ' +
        '(por exemplo 23) no formulário.',
    );
  }

  let retentionBp = pick(confirmed.retentionBp ?? null, null);
  if (retentionBp === null && draft.retentionCents !== null && baseCents !== null && baseCents > 0) {
    const derived = retentionBpFor(baseCents, draft.retentionCents);
    if (derived === null) {
      warnings.push(
        `A retenção de ${formatEur(draft.retentionCents)} não corresponde a uma percentagem exacta de ` +
          `${formatEur(baseCents)}. Confirma a percentagem a aplicar.`,
      );
    } else {
      retentionBp = derived;
    }
  }
  if (retentionBp === null) retentionBp = 0;

  const packRetentionBp = pack.constants.irs.withholding.professionalServicesResident;
  if (retentionBp !== 0 && packRetentionBp !== null && retentionBp !== packRetentionBp && scope === 'national') {
    warnings.push(
      `A retenção lida (${retentionBp / 100}%) não é a do pacote de regras para serviços de residentes ` +
        `(${packRetentionBp / 100}%). Ficou registado o que o documento diz.`,
    );
  }

  // --- what the person changed ---------------------------------------------
  const confirmedNumber = pick(confirmed.number ?? null, draft.number);
  const confirmedDate = pick(confirmed.date ?? null, draft.date);
  const confirmedNif = pick(confirmed.clientNif ?? null, draft.customer.nif);
  const comparisons: Array<[string, string | null, string | null]> = [
    ['número', draft.number, confirmedNumber],
    ['data', draft.date, confirmedDate],
    ['NIF do cliente', draft.customer.nif, confirmedNif],
    ['valor ilíquido', money(draft.baseCents), money(baseCents)],
    ['taxa de IVA', draft.ivaRateBp === null ? null : `${draft.ivaRateBp / 100}%`, ivaRateBp === null ? null : `${ivaRateBp / 100}%`],
  ];
  for (const [label, read, used] of comparisons) {
    if (read !== null && used !== null && read !== used) {
      divergences.push(`${label}: lido no documento "${read}", registado "${used}"`);
    }
  }

  if (draft.stampDutyCents !== null && draft.stampDutyCents !== 0) {
    warnings.push(
      `O documento inclui ${formatEur(draft.stampDutyCents)} de imposto do selo. O livro de faturas não ` +
        'tem esse campo: a soma do registo fica abaixo do total do documento.',
    );
  }
  if (draft.retentionReason !== null && retentionBp === 0) {
    warnings.push(`O documento diz: ${draft.retentionReason}`);
  }

  if (problems.length > 0 || baseCents === null || country === null || treatment === null) {
    return { invoice: null, problems: [...new Set(problems)], warnings: [...new Set(warnings)], divergences };
  }

  const clientName = (pick(confirmed.clientName ?? null, draft.customer.name) ?? '').trim();
  if (clientName === '') {
    return {
      invoice: null,
      problems: ['Falta o nome do cliente: não foi lido no documento.'],
      warnings: [...new Set(warnings)],
      divergences,
    };
  }
  if (confirmedDate === null || confirmedDate === '') {
    return {
      invoice: null,
      problems: ['Falta a data de emissão da fatura: não foi lida no documento.'],
      warnings: [...new Set(warnings)],
      divergences,
    };
  }

  const invoice: Invoice = {
    id: input.id,
    number: confirmedNumber ?? input.fallbackNumber,
    date: confirmedDate,
    clientName,
    clientNif: confirmedNif,
    clientCountry: country,
    description: pick(confirmed.description ?? null, draft.description) ?? '',
    baseCents,
    ivaRateBp: ivaRateBp ?? 0,
    vatTreatment: treatment,
    retentionBp,
    atcud: pick(confirmed.atcud ?? null, draft.atcud),
    status: confirmed.status ?? (draft.declaresPaid ? 'paid' : 'issued'),
    // The PDF is archived in the vault as part of the same action, so the document
    // that proves the invoice exists is where the record says it is.
    paymentProofInVault: input.documentFile !== null && input.documentFile !== undefined,
  };

  return { invoice, problems: [], warnings: [...new Set(warnings)], divergences };
}
