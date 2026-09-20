import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { checksumOf, freshness, loadRulePack, summarisePack, validateRulePack } from './rules.ts';

function rawPack(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    jurisdiction: 'PT',
    year: 2026,
    packVersion: '2026.1.0',
    generatedAt: '2026-09-20',
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: 'Não constitui aconselhamento fiscal.',
    sources: [
      {
        id: 'at-decl-2026',
        tier: 'official',
        authority: 'AT',
        title: 'Obrigações declarativas 2026',
        url: 'https://info.portaldasfinancas.gov.pt/',
        retrievedAt: '2026-09-20',
      },
    ],
    constants: {
      iva: {
        rates: { normal: 2300, intermediaire: 1300, reduzida: 600 },
        art53: {
          ceiling: 1_500_000,
          forbidsExportOperations: true,
          requiresNationalDomicile: true,
          annualisationInStartYear: false,
          sourceIds: ['at-decl-2026'],
        },
        periodicRegime: { quarterlyCeiling: null, monthlyAbove: null, sourceIds: [] },
      },
      irs: {
        simplifiedRegime: {
          ceilingForOrganisedAccounting: null,
          coefficients: { servicesProfessionalTable4: 7500, servicesOther: null },
          documentedExpensesDeductionRate: null,
          documentedExpensesDeductionCap: null,
          sourceIds: [],
        },
        withholding: {
          professionalServicesResident: 2500,
          otherCategoryBResident: null,
          nonResident: null,
          sourceIds: [],
        },
      },
      socialSecurity: {
        contributionRate: 2140,
        relevantIncomeShareServices: 7000,
        relevantIncomeShareGoods: null,
        baseMinMultipleIas: 1.5,
        baseMaxMultipleIas: 12,
        iasMonthly: null,
        startupExemptionMonths: 12,
        startupReductions: [],
        sourceIds: [],
      },
    },
    obligations: [
      {
        id: 'iva.dp.mensal',
        kind: 'declare',
        authority: 'AT',
        tax: 'IVA',
        title: 'Declaração periódica de IVA',
        appliesWhen: [{ field: 'iva.regime', op: 'eq', value: 'mensal' }],
        periodicity: { type: 'monthly', lagMonths: 2 },
        deadlineRule: { kind: 'day_of_month', day: 20 },
        officialDates2026: [{ month: 1, day: 20 }],
        legalBasis: ['CIVA art. 41.º'],
        sourceIds: ['at-decl-2026'],
        portalUrl: 'https://www.portaldasfinancas.gov.pt/',
        documentsToKeep: ['Comprovativo da declaração'],
        penaltyNote: 'Coima por falta de entrega.',
        verification: 'verified',
        verifyNote: null,
      },
    ],
    todo: [],
    ...overrides,
  };
}

function errors(problems: Array<{ level: string; path: string; message: string }>): string[] {
  return problems.filter((problem) => problem.level === 'error').map((problem) => problem.path);
}

test('a well-formed pack validates with no errors and keeps its provenance', () => {
  const { pack, problems } = validateRulePack(rawPack());
  assert.deepEqual(errors(problems), []);
  assert.equal(pack.obligations.length, 1);
  assert.equal(pack.obligations[0]?.legalBasis[0], 'CIVA art. 41.º');
  assert.equal(pack.obligations[0]?.verification, 'verified');
});

test('an unknown condition field or operator is an error, not a silent skip', () => {
  const bad = rawPack();
  bad['obligations'] = [
    {
      id: 'x',
      kind: 'declare',
      authority: 'AT',
      tax: 'IVA',
      title: 'x',
      appliesWhen: [{ field: 'iva.turnover', op: 'matches', value: 1 }],
      periodicity: { type: 'annual' },
      deadlineRule: { kind: 'fixed_date', month: 6, day: 30 },
      legalBasis: ['x'],
      sourceIds: ['at-decl-2026'],
      verification: 'verified',
    },
  ];
  const { problems } = validateRulePack(bad);
  assert.ok(errors(problems).some((path) => path.endsWith('.field')));
  assert.ok(errors(problems).some((path) => path.endsWith('.op')));
});

test('a fractional rate is a warning that names the unit rule', () => {
  const bad = rawPack();
  const constants = bad['constants'] as Record<string, Record<string, unknown>>;
  (constants['socialSecurity'] as Record<string, unknown>)['contributionRate'] = 21.4;
  const { problems } = validateRulePack(bad);
  const warning = problems.find((problem) => problem.message.includes('não é um número inteiro'));
  assert.ok(warning, 'a fractional rate must be flagged');
  assert.ok(warning?.message.includes('pontos base'));
});

test('a multi-valued IAS ratio is the one legitimate non-integer', () => {
  const { problems } = validateRulePack(rawPack());
  assert.equal(problems.filter((problem) => problem.message.includes('não é um número inteiro')).length, 0);
});

test('claiming "verified" while citing nothing is a warning', () => {
  const bad = rawPack();
  const obligations = bad['obligations'] as Array<Record<string, unknown>>;
  if (obligations[0] !== undefined) {
    obligations[0]['sourceIds'] = [];
    obligations[0]['legalBasis'] = [];
  }
  const { problems } = validateRulePack(bad);
  assert.ok(problems.some((problem) => problem.message.includes('sem citar qualquer fonte')));
  assert.ok(problems.some((problem) => problem.message.includes('sem base legal')));
});

test('a reference to a source that does not exist is a warning', () => {
  const bad = rawPack();
  const obligations = bad['obligations'] as Array<Record<string, unknown>>;
  if (obligations[0] !== undefined) obligations[0]['sourceIds'] = ['nao-existe'];
  const { problems } = validateRulePack(bad);
  assert.ok(problems.some((problem) => problem.message.includes('fonte desconhecida')));
});

test('both published-date shapes normalise into one structure', () => {
  const flat = validateRulePack(rawPack()).pack.obligations[0];
  assert.equal(flat?.officialDates['2026']?.[0]?.day, 20);

  const bad = rawPack();
  const obligations = bad['obligations'] as Array<Record<string, unknown>>;
  if (obligations[0] !== undefined) {
    delete obligations[0]['officialDates2026'];
    obligations[0]['officialDates'] = { '2026': [{ month: 3, day: 20, extension: { reason: 'prorrogação' } }] };
  }
  const keyed = validateRulePack(bad).pack.obligations[0];
  assert.equal(keyed?.officialDates['2026']?.[0]?.month, 3);
  assert.equal(keyed?.officialDates['2026']?.[0]?.extension?.reason, 'prorrogação');
});

test('the checksum depends on content, not on key order', () => {
  assert.equal(checksumOf({ a: 1, b: [2, 3] }), checksumOf({ b: [2, 3], a: 1 }));
  assert.notEqual(checksumOf({ a: 1 }), checksumOf({ a: 2 }));
});

test('a malformed pack fails loudly instead of producing wrong deadlines', () => {
  const dir = mkdtempSync(join(tmpdir(), 'vnfin-rules-'));
  const brokenJson = join(dir, 'broken.json');
  writeFileSync(brokenJson, '{ this is not json', 'utf8');
  assert.throws(() => loadRulePack(brokenJson), /não é JSON válido/);

  const wrongJurisdiction = join(dir, 'es.json');
  writeFileSync(wrongJurisdiction, JSON.stringify(rawPack({ jurisdiction: 'ES' })), 'utf8');
  assert.throws(() => loadRulePack(wrongJurisdiction), /falhou a validação/);
});

test('freshness tells the truth about which year the rules cover', () => {
  const { pack } = validateRulePack(rawPack());
  assert.equal(freshness(pack, '2026-09-20').status, 'current');
  assert.equal(freshness(pack, '2027-01-05').status, 'stale');
  assert.match(freshness(pack, '2027-01-05').message, /provisórios|conferidos/);

  const future = validateRulePack(rawPack({ year: 2027 })).pack;
  assert.equal(freshness(future, '2026-09-20').status, 'future');
});

test('the summary counts what is verified and lists what is missing', () => {
  const dir = mkdtempSync(join(tmpdir(), 'vnfin-summary-'));
  const path = join(dir, '2026.json');
  writeFileSync(path, JSON.stringify(rawPack()), 'utf8');
  const summary = summarisePack(loadRulePack(path));
  assert.equal(summary.obligations, 1);
  assert.equal(summary.verified, 1);
  assert.equal(summary.unverified, 0);
  assert.ok(summary.nullConstants.includes('constants.iva.periodicRegime.quarterlyCeiling'));
});
