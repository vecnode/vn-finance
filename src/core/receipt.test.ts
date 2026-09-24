/**
 * Reading a fatura-recibo, and refusing to read one that is not yours.
 *
 * The fixtures are invented documents (see `receipt-fixture.ts`). What is being
 * tested is the boundary between two statements that a tax tool must never
 * confuse: "the document says 1 230,00" and "I have decided it is 1 230,00". The
 * parser only ever makes the first kind of claim, and the checks in these tests
 * are what keep it honest.
 */

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { createDefaultProfile } from './profile.ts';
import { extractPdfText } from './pdf.ts';
import { faturaReciboPdf } from './receipt-fixture.ts';
import { invoiceFromReceipt, parseReceipt, retentionBpFor } from './receipt.ts';
import { loadRulePack, resolvePackPath } from './rules.ts';
import type { RulePack, TaxProfile } from './types.ts';

function pack(): RulePack {
  const resolved = resolvePackPath(2026, undefined);
  assert.ok(resolved !== null, 'o pacote de regras de 2026 tem de existir para estes testes');
  return loadRulePack(resolved.path).pack;
}

const PROFILE: TaxProfile = createDefaultProfile({
  nif: '123456789',
  name: 'Maria Exemplo Silva',
  ivaRegime: 'trimestral',
  startDate: '2019-04-01',
});

function draftOf(overrides: Parameters<typeof faturaReciboPdf>[0] = {}) {
  return parseReceipt(extractPdfText(faturaReciboPdf(overrides)));
}

test('a fatura-recibo is read field by field, figures included', () => {
  const draft = draftOf();

  assert.deepEqual(draft.problems, []);
  assert.equal(draft.kind, 'Fatura-Recibo');
  assert.equal(draft.number, 'FR TESTE/2026/7');
  assert.equal(draft.atcud, 'ABCD1234-1');
  assert.equal(draft.date, '2026-03-15');
  assert.equal(draft.issuer.name, 'MARIA EXEMPLO SILVA');
  assert.equal(draft.issuer.nif, '123456789');
  assert.equal(draft.issuer.address, 'Rua da Amostra 10, 1000-100 Lisboa');
  assert.equal(draft.customer.name, 'Empresa Exemplo, Lda.');
  assert.equal(draft.customer.nif, '501234560');
  assert.equal(draft.customer.address, 'Av. Exemplo 5, 4000-000 Porto, PORTUGAL');
  assert.equal(draft.baseCents, 100_000);
  assert.equal(draft.ivaCents, 23_000);
  assert.equal(draft.ivaRateBp, 2300);
  assert.equal(draft.stampDutyCents, 0);
  assert.equal(draft.retentionCents, 0);
  assert.equal(draft.totalCents, 123_000);
  assert.equal(draft.netCents, 123_000);
  assert.equal(draft.customerCountryHint, 'PT');
  assert.equal(draft.declaresPaid, true, 'um fatura-recibo com "Pagamento" está pago');
  assert.equal(draft.description, 'Consultoria de exemplo em engenharia civil e gestão de obra');
});

test('the document is checked against its own arithmetic, and agrees with itself', () => {
  const draft = draftOf();
  assert.deepEqual(
    draft.checks.map((check) => check.id),
    ['total', 'payable', 'rate', 'nif-prestador', 'nif-adquirente'],
  );
  assert.ok(
    draft.checks.every((check) => check.ok),
    `todas as verificações deviam passar: ${JSON.stringify(draft.checks.filter((c) => !c.ok))}`,
  );
});

test('a document whose totals do not add up is reported, not silently accepted', () => {
  const draft = draftOf({ total: '1.999,00 €' });

  const total = draft.checks.find((check) => check.id === 'total');
  assert.ok(total !== undefined);
  assert.equal(total.ok, false);
  assert.match(draft.problems.join(' '), /Verificação falhada/);
  assert.match(draft.problems.join(' '), /1\.999,00|1999,00/);
});

test('a NIF that fails its check digit is flagged, because a typo must not become a record', () => {
  const draft = draftOf({ customerNif: '501234561' });
  const check = draft.checks.find((candidate) => candidate.id === 'nif-adquirente');
  assert.ok(check !== undefined);
  assert.equal(check.ok, false);
  assert.match(draft.problems.join(' '), /dígito de controlo/);
});

test('a document with no readable base says so, and invents nothing', () => {
  // The label is there but the figure is missing, which is what a corrupted or
  // partly-scanned document looks like.
  const rows = ['TOTAIS DO DOCUMENTO', 'Valor ilíquido', 'IVA 230,00 €'];
  const draft = parseReceipt(rows);

  assert.equal(draft.baseCents, null);
  assert.equal(draft.ivaCents, 23_000);
  assert.match(draft.problems.join(' '), /valor ilíquido/i);
});

test('the VAT rate is derived from the document when it is not printed', () => {
  const rows = ['Valor ilíquido 1.000,00 €', 'IVA 230,00 €', 'TOTAL DO DOCUMENTO 1.230,00 €'];
  const draft = parseReceipt(rows);

  assert.equal(draft.ivaRatesBp.length, 1);
  assert.equal(draft.ivaRatesBp[0], 2300, '1 000,00 com 230,00 de IVA é 23%');
});

test('an amount written with spaces as thousands separators is read', () => {
  const rows = ['Valor ilíquido 12 500,00 €', 'IVA 2 875,00 €', 'TOTAL DO DOCUMENTO 15 375,00 €'];
  const draft = parseReceipt(rows);

  assert.equal(draft.baseCents, 1_250_000);
  assert.equal(draft.ivaCents, 287_500);
  assert.equal(draft.totalCents, 1_537_500);
});

test('an invoice issued by somebody else is refused, and the refusal explains why', () => {
  const draft = draftOf({ issuerNif: '501234560', customerNif: '123456789' });
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: {},
    id: 'inv_1',
    fallbackNumber: 'FR 2026/001',
  });

  assert.equal(result.invoice, null);
  assert.match(result.problems.join(' '), /RECEBESTE/);
  assert.match(result.problems.join(' '), /livro de despesas/);
});

test('a consistent fatura-recibo becomes a ledger invoice with the document attached', () => {
  const draft = draftOf();
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: {},
    id: 'inv_2',
    documentFile: 'documents/abcdef123456-FR.pdf',
    fallbackNumber: 'FR 2026/001',
  });

  assert.deepEqual(result.problems, []);
  assert.ok(result.invoice !== null);
  assert.equal(result.invoice.number, 'FR TESTE/2026/7');
  assert.equal(result.invoice.date, '2026-03-15');
  assert.equal(result.invoice.clientName, 'Empresa Exemplo, Lda.');
  assert.equal(result.invoice.clientNif, '501234560');
  assert.equal(result.invoice.clientCountry, 'PT');
  assert.equal(result.invoice.baseCents, 100_000);
  assert.equal(result.invoice.ivaRateBp, 2300);
  assert.equal(result.invoice.vatTreatment, 'iva_pt');
  assert.equal(result.invoice.retentionBp, 0);
  assert.equal(result.invoice.atcud, 'ABCD1234-1');
  assert.equal(result.invoice.status, 'paid');
  assert.equal(result.invoice.paymentProofInVault, true);
});

test('what the person corrects is registered, and the difference is recorded', () => {
  const draft = draftOf();
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: { number: 'FR TESTE/2026/8', baseCents: 110_000 },
    id: 'inv_3',
    fallbackNumber: 'FR 2026/001',
  });

  assert.ok(result.invoice !== null);
  assert.equal(result.invoice.number, 'FR TESTE/2026/8');
  assert.equal(result.invoice.baseCents, 110_000);
  assert.equal(result.divergences.length, 2);
  assert.match(result.divergences.join(' '), /número/);
  assert.match(result.divergences.join(' '), /valor ilíquido/);
});

test('a client whose country cannot be read stops the record instead of assuming Portugal', () => {
  const draft = draftOf({ customerNif: '999999990' });
  draft.customerCountryHint = null;
  draft.customer.address = null;
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: {},
    id: 'inv_4',
    fallbackNumber: 'FR 2026/001',
  });

  assert.equal(result.invoice, null);
  assert.match(result.problems.join(' '), /país do cliente/);
});

test('the retention is stored as the rate that reproduces the document exactly', () => {
  assert.equal(retentionBpFor(100_000, 25_000), 2500, '25% de 1 000,00 é 250,00');
  assert.equal(retentionBpFor(81_301, 0), 0);
  // 11,50 over 100,00 is 11,5%, but nothing in this range of cents rounds to it
  // exactly, so the honest answer is "there is no such rate".
  assert.equal(retentionBpFor(100_00, 11_50), 1150);
  assert.equal(retentionBpFor(0, 100), null, 'sem base não há percentagem');
});

test('a retention figure the document prints becomes a rate in the ledger', () => {
  const rows = [
    'emitida em 15/03/2026',
    'Valor ilíquido 1.000,00 €',
    'TOTAL DO DOCUMENTO 1.230,00 €',
    'Retenção na fonte IRS 250,00 €',
    'TOTAL A PAGAR 980,00 €',
    'Sem retenção - Art.101º, n.º1 do CIRS',
  ];
  const draft = parseReceipt(rows);
  assert.equal(draft.retentionCents, 25_000);

  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: { clientName: 'Cliente Exemplo', clientCountry: 'PT', ivaRateBp: 2300 },
    id: 'inv_5',
    fallbackNumber: 'FR 2026/001',
  });

  assert.ok(result.invoice !== null);
  assert.equal(result.invoice.retentionBp, 2500);
  assert.equal(result.invoice.baseCents, 100_000);
});

test('a stamp duty the ledger cannot represent is a stated warning', () => {
  const rows = [
    'emitida em 15/03/2026',
    'Valor ilíquido 1.000,00 €',
    'IVA 230,00 €',
    'Imposto do Selo 10,00 €',
    'TOTAL DO DOCUMENTO 1.240,00 €',
  ];
  const draft = parseReceipt(rows);
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: { clientName: 'Cliente Exemplo', clientCountry: 'PT', ivaRateBp: 2300 },
    id: 'inv_6',
    fallbackNumber: 'FR 2026/001',
  });

  assert.ok(result.invoice !== null);
  assert.equal(result.invoice.baseCents, 100_000);
  assert.match(result.warnings.join(' '), /imposto do selo/i);
});

test('a document with no issuer section is flagged, because it cannot be attributed', () => {
  const draft = draftOf({ withoutIssuerSection: true });
  assert.match(draft.problems.join(' '), /DADOS DO TRANSMITENTE/);
});

test('a missing customer name stops the record rather than registering a blank client', () => {
  const draft = draftOf();
  draft.customer.name = null;
  const result = invoiceFromReceipt({
    draft,
    pack: pack(),
    profile: PROFILE,
    confirmed: {},
    id: 'inv_7',
    fallbackNumber: 'FR 2026/001',
  });

  assert.equal(result.invoice, null);
  assert.match(result.problems.join(' '), /nome do cliente/);
});
