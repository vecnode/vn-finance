import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  defaultInvoiceTerms,
  estimateIrsSimplifiedBase,
  estimateReserve,
  estimateSocialSecurity,
  invoiceTotals,
  periodTotals,
  quarterReport,
  type Invoice,
} from './estimate.ts';
import { createDefaultProfile } from './profile.ts';
import type { RulePack } from './types.ts';

function constants(overrides: Partial<RulePack['constants']['irs']['simplifiedRegime']> = {}): RulePack['constants'] {
  return {
    iva: {
      rates: { normal: 2300, intermediaire: 1300, reduzida: 600 },
      art53: {
        ceiling: 1_500_000,
        forbidsExportOperations: true,
        requiresNationalDomicile: true,
        annualisationInStartYear: false,
        sourceIds: ['s1'],
      },
      periodicRegime: { quarterlyCeiling: null, monthlyAbove: null, sourceIds: [] },
    },
    irs: {
      simplifiedRegime: {
        ceilingForOrganisedAccounting: null,
        coefficients: { servicesProfessionalTable4: 7500, servicesOther: null },
        documentedExpensesDeductionRate: 1500,
        documentedExpensesDeductionCap: null,
        sourceIds: ['s1'],
        ...overrides,
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
      relevantIncomeShareGoods: null,
      baseMinMultipleIas: 1.5,
      baseMaxMultipleIas: 12,
      iasMonthly: null,
      startupExemptionMonths: 12,
      startupReductions: [],
      sourceIds: [],
    },
  };
}

function pack(overrides: Partial<RulePack['constants']['irs']['simplifiedRegime']> = {}): RulePack {
  return {
    jurisdiction: 'PT',
    year: 2026,
    packVersion: 'test',
    generatedAt: '2026-09-20',
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: 'teste',
    sources: [],
    constants: constants(overrides),
    obligations: [],
    todo: [],
  };
}

function invoice(overrides: Partial<Invoice> = {}): Invoice {
  return {
    id: 'inv_1',
    number: 'FR 2026/001',
    date: '2026-08-10',
    clientName: 'ACME, Lda.',
    clientNif: '501234560',
    clientCountry: 'PT',
    description: 'Serviços de programação',
    baseCents: 1_000_000,
    ivaRateBp: 2300,
    vatTreatment: 'iva_pt',
    retentionBp: 2500,
    atcud: null,
    status: 'issued',
    paymentProofInVault: false,
    ...overrides,
  };
}

test('invoiceTotals keeps the three amounts separate: base, IVA and retention', () => {
  const totals = invoiceTotals(invoice());
  assert.equal(totals.baseCents, 1_000_000);
  assert.equal(totals.ivaCents, 230_000);
  assert.equal(totals.retentionCents, 250_000);
  // The client withholds the retention, so the cash received is smaller.
  assert.equal(totals.netReceivableCents, 980_000);
});

test('an intra-community invoice carries no Portuguese IVA and no retention', () => {
  const totals = invoiceTotals(
    invoice({ clientCountry: 'FI', vatTreatment: 'autoliquidacao_ue', ivaRateBp: 0, retentionBp: 0 }),
  );
  assert.equal(totals.ivaCents, 0);
  assert.equal(totals.retentionCents, 0);
  assert.equal(totals.netReceivableCents, 1_000_000);
});

test('periodTotals only counts invoices inside the window', () => {
  const invoices = [
    invoice({ id: 'a', date: '2026-09-05', baseCents: 500_000 }),
    invoice({ id: 'b', date: '2026-09-28', baseCents: 300_000 }),
    invoice({ id: 'c', date: '2026-10-01', baseCents: 900_000 }),
  ];
  const totals = periodTotals(invoices, '2026-07-01', '2026-09-30');
  assert.equal(totals.invoiceCount, 2);
  assert.equal(totals.baseCents, 800_000);
  assert.equal(totals.ivaLiquidadoCents, 184_000);
  assert.equal(totals.retencaoSofridaCents, 200_000);
});

test('Segurança Social: 70% of service income, then 21,4% of that, in three instalments', () => {
  const estimate = estimateSocialSecurity(pack(), 1_940_000);
  assert.equal(estimate.relevantIncomeCents, 1_358_000);
  assert.equal(estimate.contributionCents, 290_612);
  assert.deepEqual(estimate.instalmentsCents, [96_871, 96_871, 96_870]);
  assert.equal(
    estimate.instalmentsCents.reduce((total, part) => total + part, 0),
    290_612,
  );
});

test('Segurança Social refuses to guess when a rate is missing from the pack', () => {
  const broken = pack();
  broken.constants.socialSecurity.contributionRate = null;
  const estimate = estimateSocialSecurity(broken, 1_940_000);
  assert.equal(estimate.contributionCents, 0);
  assert.ok(estimate.limitations.some((line) => line.includes('taxa contributiva')));
});

test('quarterReport computes a quarter from the ledger', () => {
  const invoices = [
    invoice({ id: 'q3a', date: '2026-07-15', baseCents: 1_000_000 }),
    invoice({ id: 'q3b', date: '2026-09-15', baseCents: 940_000 }),
    invoice({ id: 'q2', date: '2026-06-30', baseCents: 5_000_000 }),
  ];
  const report = quarterReport(pack(), invoices, 2026, 3);
  assert.equal(report.start, '2026-07-01');
  assert.equal(report.end, '2026-09-30');
  assert.equal(report.totals.baseCents, 1_940_000);
  assert.equal(report.socialSecurity.contributionCents, 290_612);
});

test('the art. 31.º n.º 13 add-back is computed, and the tax is NOT', () => {
  const profile = createDefaultProfile({ nif: '123456789', name: 'Teste', startDate: '2019-04-01' });
  const estimate = estimateIrsSimplifiedBase(
    pack(),
    profile,
    [invoice({ baseCents: 4_860_000 })],
    2026,
    500_000,
  );
  assert.equal(estimate.taxableFromCoefficientCents, 3_645_000); // 48 600 × 0,75
  assert.equal(estimate.documentedExpensesReferenceCents, 729_000); // 15% of 48 600
  // 7 290 reference − 5 000 documented = 2 290 ADDED back to taxable income.
  assert.equal(estimate.additionToTaxableIncomeCents, 229_000);
  assert.equal(estimate.taxableIncomeCents, 3_874_000);
  assert.ok(estimate.limitations.some((line) => line.includes('art. 68.º')));
});

test('documenting more than the 15% reference adds nothing back', () => {
  const profile = createDefaultProfile({ nif: '123456789', name: 'Teste', startDate: '2019-04-01' });
  const estimate = estimateIrsSimplifiedBase(
    pack(),
    profile,
    [invoice({ baseCents: 4_860_000 })],
    2026,
    900_000,
  );
  assert.equal(estimate.additionToTaxableIncomeCents, 0);
  assert.equal(estimate.taxableIncomeCents, 3_645_000);
});

test('without the reference rate in the pack, the taxable income is refused, not guessed', () => {
  const profile = createDefaultProfile({ nif: '123456789', name: 'Teste', startDate: '2019-04-01' });
  const broken = pack();
  broken.constants.irs.simplifiedRegime.documentedExpensesDeductionRate = null;
  const estimate = estimateIrsSimplifiedBase(broken, profile, [invoice()], 2026, 500_000);
  assert.equal(estimate.documentedExpensesReferenceCents, null);
  assert.equal(estimate.additionToTaxableIncomeCents, null);
  assert.equal(estimate.taxableIncomeCents, null);
  assert.ok(estimate.limitations.some((line) => line.includes('referência')));
});

test('the reserve is a range, never a single figure', () => {
  const reserve = estimateReserve(pack(), [invoice({ baseCents: 4_860_000 })], '2026-09-20');
  assert.equal(reserve.lowCents, 1_215_000); // 25%
  assert.equal(reserve.highCents, 1_701_000); // 35%
  assert.ok(reserve.lowCents < reserve.highCents);
  assert.ok(reserve.limitations.length > 0);
});

test('invoice terms come from the pack and the profile, never from a literal', () => {
  const normal = createDefaultProfile({
    nif: '123456789',
    name: 'Teste',
    startDate: '2019-04-01',
    ivaRegime: 'trimestral',
  });
  const exempt = createDefaultProfile({
    nif: '123456789',
    name: 'Teste',
    startDate: '2019-04-01',
    ivaRegime: 'isento_art53',
  });

  // A client in Portugal: the pack's normal rate, and the pack's retention rate
  // for resident professional services. Writing 25% here as a literal was the bug
  // this function exists to prevent.
  assert.deepEqual(defaultInvoiceTerms(pack(), normal, 'PT'), {
    vatTreatment: 'iva_pt',
    ivaRateBp: 2300,
    retentionBp: 2300,
  });
  assert.deepEqual(defaultInvoiceTerms(pack(), normal, 'pt'), {
    vatTreatment: 'iva_pt',
    ivaRateBp: 2300,
    retentionBp: 2300,
  });

  // An art. 53.º exempt taxpayer charges no VAT and says so on the invoice.
  // Retention is independent of the IVA exemption.
  assert.deepEqual(defaultInvoiceTerms(pack(), exempt, 'PT'), {
    vatTreatment: 'isento_art53',
    ivaRateBp: 0,
    retentionBp: 2300,
  });

  // Outside Portugal: no Portuguese VAT, and NO Portuguese withholding. The
  // pack's non-resident rate applies in the other direction — to income paid *to*
  // a non-resident by a Portuguese payer.
  assert.deepEqual(defaultInvoiceTerms(pack(), normal, 'DE'), {
    vatTreatment: 'autoliquidacao_ue',
    ivaRateBp: 0,
    retentionBp: 0,
  });
  assert.deepEqual(defaultInvoiceTerms(pack(), normal, 'US'), {
    vatTreatment: 'exportacao',
    ivaRateBp: 0,
    retentionBp: 0,
  });
});

test('with no rate in the pack the terms are zero rather than invented', () => {
  const broken = pack();
  broken.constants.iva.rates.normal = null;
  broken.constants.irs.withholding.professionalServicesResident = null;
  const normal = createDefaultProfile({
    nif: '123456789',
    name: 'Teste',
    startDate: '2019-04-01',
    ivaRegime: 'trimestral',
  });
  assert.deepEqual(defaultInvoiceTerms(broken, normal, 'PT'), {
    vatTreatment: 'iva_pt',
    ivaRateBp: 0,
    retentionBp: 0,
  });
});
