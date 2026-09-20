import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  applyProposal,
  buildUpdateMessages,
  buildUpdateRequest,
  collectUpdatableVariables,
  diffProposal,
  parseUpdateResponse,
  type UpdateProposal,
} from './update.ts';
import type { RulePack } from '../core/types.ts';

const SENTINEL = 'CLIENTE-SENTINELA-LDA';

function pack(): RulePack {
  return {
    jurisdiction: 'PT',
    year: 2026,
    packVersion: '2026.1.0',
    generatedAt: '2026-09-20',
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: `Não constitui aconselhamento fiscal. ${SENTINEL}`,
    sources: [
      {
        id: 'at-agenda',
        tier: 'official',
        authority: 'AT',
        title: 'Agenda Fiscal',
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
          sourceIds: ['at-agenda'],
        },
        periodicRegime: { quarterlyCeiling: 65_000_000, monthlyAbove: 65_000_000, sourceIds: [] },
      },
      irs: {
        simplifiedRegime: {
          ceilingForOrganisedAccounting: 20_000_000,
          coefficients: { servicesProfessionalTable4: 7500, servicesOther: 3500 },
          documentedExpensesDeductionRate: 1500,
          documentedExpensesDeductionCap: null,
          sourceIds: [],
        },
        withholding: {
          professionalServicesResident: 2300,
          otherCategoryBResident: 1150,
          nonResident: 2500,
          sourceIds: [],
        },
      },
      socialSecurity: {
        contributionRate: 2140,
        relevantIncomeShareServices: 7000,
        relevantIncomeShareGoods: 2000,
        baseMinMultipleIas: 150,
        baseMaxMultipleIas: 1200,
        iasMonthly: 53_713,
        startupExemptionMonths: 12,
        startupReductions: [],
        sourceIds: [],
      },
    },
    obligations: [],
    todo: [],
  };
}

test('only the labelled, numeric constants are visible to the assistant', () => {
  const variables = collectUpdatableVariables(pack());
  const paths = variables.map((variable) => variable.path);

  assert.ok(paths.includes('socialSecurity.iasMonthly'));
  assert.ok(paths.includes('iva.rates.normal'));
  assert.ok(paths.includes('irs.withholding.nonResident'));
  assert.ok(
    paths.includes('irs.simplifiedRegime.documentedExpensesDeductionCap'),
    'a null value is still a variable the assistant may fill in',
  );
  assert.ok(!paths.includes('iva.art53.forbidsExportOperations'), 'booleans are not the assistant\'s business');
  assert.ok(!paths.includes('socialSecurity.startupReductions'), 'lists are not the assistant\'s business');
  assert.ok(!paths.includes('iva.art53.sourceIds'));

  const ias = variables.find((variable) => variable.path === 'socialSecurity.iasMonthly');
  assert.equal(ias?.unit, 'EUR_cents');
  assert.equal(ias?.currentValue, 53_713);
  const rate = variables.find((variable) => variable.path === 'irs.withholding.nonResident');
  assert.equal(rate?.unit, 'basis_points');
  assert.equal(rate?.currentValue, 2500);
});

test('the update request cannot contain personal data: it is built from the pack alone', () => {
  const request = buildUpdateRequest(pack());
  const text = buildUpdateMessages(request)
    .map((message) => message.content)
    .join('\n');

  assert.ok(text.includes('socialSecurity.iasMonthly'), 'the variables must be described');
  assert.ok(text.includes('https://info.portaldasfinancas.gov.pt/'), 'the official sources are passed along');
  assert.ok(
    !text.includes(SENTINEL),
    'the pack disclaimer is not part of the request — nothing outside the variable list is sent',
  );
  assert.deepEqual(Object.keys(request).sort(), ['jurisdiction', 'sources', 'variables', 'year']);
});

test('a well-formed answer becomes validated values keyed to the variables asked about', () => {
  const request = buildUpdateRequest(pack());
  const answer = JSON.stringify({
    asOf: '2027-01-01',
    variables: [
      { path: 'iva.rates.normal', value: 2300, sourceUrl: 'https://example.gov/', note: null },
      { path: 'socialSecurity.iasMonthly', value: 55_000, sourceUrl: null, note: 'a confirmar' },
    ],
  });
  const parsed = parseUpdateResponse(answer, request);
  assert.equal(parsed.asOf, '2027-01-01');
  assert.equal(parsed.values.length, 2);
  const normal = parsed.values.find((value) => value.path === 'iva.rates.normal');
  assert.equal(normal?.value, 2300);
  assert.equal(normal?.unit, 'basis_points');
  assert.equal(normal?.label.length > 0, true);
  // Every variable that was not answered is reported, not silently absent.
  assert.ok(parsed.problems.some((problem) => problem.includes('não respondeu')));
});

test('a prose answer wrapped around JSON is refused whole, not scavenged for data', () => {
  const request = buildUpdateRequest(pack());
  const answer = 'Claro! Aqui está o JSON que pediste:\n{"asOf":"2027-01-01","variables":[]}';
  const parsed = parseUpdateResponse(answer, request);
  assert.deepEqual(parsed.values, []);
  assert.match(parsed.problems[0] ?? '', /não é JSON válido/);
});

test('values in the wrong unit, or out of any plausible range, are rejected', () => {
  const request = buildUpdateRequest(pack());
  const answer = JSON.stringify({
    asOf: '2027-01-01',
    variables: [
      { path: 'iva.rates.normal', value: 23, sourceUrl: null }, // 23 basis points would mean 0,23%
      { path: 'irs.withholding.nonResident', value: 25_000, sourceUrl: null }, // 250%
      { path: 'socialSecurity.iasMonthly', value: 537.13, sourceUrl: null }, // euros, not cents
      { path: 'iva.art53.ceiling', value: -5, sourceUrl: null },
      { path: 'nao.pedido', value: 1, sourceUrl: null },
    ],
  });
  const parsed = parseUpdateResponse(answer, request);
  const accepted = parsed.values.filter((value) => value.value !== null);
  assert.deepEqual(accepted, [], 'none of these are acceptable');
  assert.ok(parsed.problems.some((problem) => problem.includes('não é inteiro')));
  assert.ok(parsed.problems.some((problem) => problem.includes('limites plausíveis')));
  assert.ok(parsed.problems.some((problem) => problem.includes('não foi pedido')));
});

test('a proposal only changes what it says it changes', () => {
  const request = buildUpdateRequest(pack());
  const answer = JSON.stringify({
    asOf: '2027-01-01',
    variables: [
      { path: 'iva.rates.normal', value: 2400, sourceUrl: 'https://example.gov/iva', note: 'OE 2027' },
      { path: 'irs.withholding.nonResident', value: 2500, sourceUrl: null },
    ],
  });
  const parsed = parseUpdateResponse(answer, request);
  const proposal: UpdateProposal = {
    year: 2026,
    asOf: parsed.asOf,
    model: 'deepseek-chat',
    proposedAt: '2026-09-20',
    values: parsed.values,
    problems: parsed.problems,
  };

  const diffs = diffProposal(proposal);
  assert.equal(diffs.find((diff) => diff.path === 'iva.rates.normal')?.changed, true);
  assert.equal(diffs.find((diff) => diff.path === 'irs.withholding.nonResident')?.changed, false);

  const original = pack();
  const { pack: updated, applied } = applyProposal(original, proposal, {
    at: '2026-09-20',
    model: 'deepseek-chat',
    verifiedByHuman: false,
  });

  assert.equal(applied, 2);
  assert.equal(updated.constants.iva.rates.normal, 2400);
  assert.equal(updated.constants.iva.rates.reduzida, 600, 'untouched values stay untouched');
  assert.equal(original.constants.iva.rates.normal, 2300, 'the original pack is not mutated');
  assert.equal(updated.packVersion, '2026.1.1', 'applying a proposal bumps the patch version');
  assert.equal(updated.generatedAt, '2026-09-20');
});

test('an applied proposal is recorded as unverified until a human confirms it', () => {
  const request = buildUpdateRequest(pack());
  const answer = JSON.stringify({
    asOf: '2027-01-01',
    variables: [{ path: 'socialSecurity.contributionRate', value: 2200, sourceUrl: 'https://example.gov/ss' }],
  });
  const parsed = parseUpdateResponse(answer, request);
  const proposal: UpdateProposal = {
    year: 2026,
    asOf: parsed.asOf,
    model: 'deepseek-chat',
    proposedAt: '2026-09-20',
    values: parsed.values,
    problems: parsed.problems,
  };

  const unconfirmed = applyProposal(pack(), proposal, {
    at: '2026-09-20',
    model: 'deepseek-chat',
    verifiedByHuman: false,
  });
  const entry = unconfirmed.pack.constantProvenance?.['socialSecurity.contributionRate'];
  assert.equal(entry?.source, 'ai-proposed');
  assert.equal(entry?.status, 'unverified');
  assert.equal(entry?.sourceUrl, 'https://example.gov/ss');

  const confirmed = applyProposal(pack(), proposal, {
    at: '2026-09-20',
    model: 'deepseek-chat',
    verifiedByHuman: true,
  });
  assert.equal(confirmed.pack.constantProvenance?.['socialSecurity.contributionRate']?.status, 'verified');
});
