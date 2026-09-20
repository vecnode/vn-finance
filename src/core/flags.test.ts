import assert from 'node:assert/strict';
import { test } from 'node:test';

import { computeFlags, summariseFlags, turnoverByScope } from './flags.ts';
import { scopeOf } from './countries.ts';
import { formatEur } from './money.ts';
import { createDefaultProfile, type ProfileInput } from './profile.ts';
import type { Invoice } from './estimate.ts';
import type { ObligationInstance, RulePack, TaxProfile } from './types.ts';

function pack(overrides: { art53Ceiling?: number | null } = {}): RulePack {
  const ceiling = overrides.art53Ceiling === undefined ? 1_500_000 : overrides.art53Ceiling;
  return {
    jurisdiction: 'PT',
    year: 2026,
    packVersion: 'test',
    generatedAt: '2026-09-20',
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: 'teste',
    sources: [],
    constants: {
      iva: {
        rates: { normal: 2300, intermediaire: 1300, reduzida: 600 },
        art53: {
          ceiling,
          forbidsExportOperations: true,
          requiresNationalDomicile: true,
          annualisationInStartYear: false,
          sourceIds: ['s1'],
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
    obligations: [
      {
        id: 'irs.retencoesPagamento',
        kind: 'pay',
        authority: 'AT',
        tax: 'IRS',
        title: 'Retenções',
        appliesWhen: [],
        periodicity: { type: 'monthly', lagMonths: 1 },
        deadlineRule: { kind: 'day_of_month', day: 20 },
        officialDates: {},
        legalBasis: ['CIRS art. 101.º'],
        sourceIds: ['s1'],
        portalUrl: null,
        documentsToKeep: [],
        penaltyNote: null,
        verification: 'verified',
        verifyNote: null,
      },
      {
        id: 'iva.recapitulativa',
        kind: 'communicate',
        authority: 'AT',
        tax: 'IVA',
        title: 'Recapitulativa',
        appliesWhen: [],
        periodicity: { type: 'monthly', lagMonths: 1 },
        deadlineRule: { kind: 'day_of_month', day: 20 },
        officialDates: {},
        legalBasis: ['Declaração recapitulativa'],
        sourceIds: ['s1'],
        portalUrl: null,
        documentsToKeep: [],
        penaltyNote: null,
        verification: 'verified',
        verifyNote: null,
      },
    ],
    todo: [],
  };
}

function profile(overrides: Partial<ProfileInput> = {}): TaxProfile {
  return createDefaultProfile({ nif: '123456789', name: 'Teste', startDate: '2019-04-01', ...overrides });
}

function invoice(overrides: Partial<Invoice> = {}): Invoice {
  return {
    id: 'inv',
    number: 'FR 2026/001',
    date: '2026-08-10',
    clientName: 'ACME, Lda.',
    clientNif: '501234560',
    clientCountry: 'PT',
    description: 'Serviços',
    baseCents: 1_000_000,
    ivaRateBp: 2300,
    vatTreatment: 'iva_pt',
    retentionBp: 2300,
    atcud: null,
    status: 'issued',
    paymentProofInVault: true,
    ...overrides,
  };
}

function instance(overrides: Partial<ObligationInstance> = {}): ObligationInstance {
  return {
    id: 'x@2026-11-20',
    ruleId: 'iva.recapitulativa',
    kind: 'communicate',
    authority: 'AT',
    tax: 'IVA',
    title: 'Recapitulativa',
    periodLabel: 'outubro de 2026',
    periodStart: '2026-10-01',
    periodEnd: '2026-10-31',
    dueDate: '2026-10-06',
    dueDateSource: 'official',
    adjustedForWeekend: false,
    provisional: false,
    status: 'due_soon',
    notApplicableReason: null,
    verification: 'verified',
    verifyNote: null,
    legalBasis: [],
    sourceIds: [],
    portalUrl: null,
    documentsToKeep: ['Comprovativo da declaração'],
    penaltyNote: null,
    discrepancies: [],
    checklists: [],
    ...overrides,
  };
}

function run(input: {
  profile?: TaxProfile;
  invoices?: Invoice[];
  instances?: ObligationInstance[];
  pack?: RulePack;
  documented?: string[];
}) {
  return computeFlags({
    pack: input.pack ?? pack(),
    profile: input.profile ?? profile(),
    invoices: input.invoices ?? [],
    instances: input.instances ?? [],
    today: '2026-09-20',
    documentedObligationIds: input.documented ?? [],
  });
}

function codes(flags: ReturnType<typeof run>): string[] {
  return flags.map((flag) => flag.code);
}

test('scope classification splits national, EU and third countries', () => {
  assert.equal(scopeOf('PT'), 'national');
  assert.equal(scopeOf('de'), 'eu');
  assert.equal(scopeOf('FI'), 'eu');
  assert.equal(scopeOf('GB'), 'nonEu');
  assert.equal(scopeOf('US'), 'nonEu');
});

test('turnoverByScope measures national territory separately from total invoicing', () => {
  const invoices = [
    invoice({ id: 'a', baseCents: 1_000_000, clientCountry: 'PT' }),
    invoice({ id: 'b', baseCents: 2_000_000, clientCountry: 'DE' }),
    invoice({ id: 'c', baseCents: 3_000_000, clientCountry: 'US' }),
    invoice({ id: 'd', baseCents: 9_000_000, clientCountry: 'PT', date: '2025-08-10' }),
  ];
  const scopes = turnoverByScope(invoices, 2026);
  assert.deepEqual(scopes, { national: 1_000_000, eu: 2_000_000, nonEu: 3_000_000 });
});

test('without the previous year turnover the app says it cannot check art. 53.º', () => {
  const flags = run({});
  const flag = flags.find((candidate) => candidate.code === 'ART53_NO_TURNOVER_INPUT');
  assert.ok(flag, 'expected the missing-input flag');
  assert.equal(flag.severity, 'attention');
  assert.deepEqual(flag.legalBasis, ['CIVA art. 53.º']);
});

test('an exempt taxpayer with a suitable turnover gets an informational flag with the margin', () => {
  const flags = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 1_240_000 }),
  });
  const flag = flags.find((candidate) => candidate.code === 'ART53_EXEMPTION_ACTIVE');
  assert.ok(flag);
  assert.equal(flag.severity, 'info');
  // Compared against the formatter itself rather than a literal, so the test
  // cannot drift from the currency formatting the CLI actually prints.
  assert.ok(flag.detail.includes(formatEur(260_000)), `expected the margin in ${flag.detail}`);
});

test('being inside the exemption while the previous year broke the ceiling is urgent', () => {
  const flags = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 1_800_000 }),
  });
  const flag = flags.find((candidate) => candidate.code === 'ART53_CEILING_EXCEEDED_HISTORY');
  assert.ok(flag);
  assert.equal(flag.severity, 'urgent');
});

test('an eligible taxpayer on the normal regime is told the exemption may apply, not told to change', () => {
  const flags = run({
    profile: profile({ ivaRegime: 'trimestral', turnoverPreviousYearCents: 900_000 }),
  });
  const flag = flags.find((candidate) => candidate.code === 'ART53_EXEMPTION_AVAILABLE');
  assert.ok(flag);
  assert.match(flag.detail, /contabilista certificado/);
});

test('the transition zone and the crossed ceiling are distinguished, on national turnover only', () => {
  const transition = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 900_000 }),
    invoices: [
      invoice({ id: 'pt', baseCents: 1_600_000, clientCountry: 'PT' }),
      invoice({ id: 'eu', baseCents: 5_000_000, clientCountry: 'DE' }),
    ],
  });
  assert.ok(codes(transition).includes('ART53_IN_TRANSITION_ZONE'));
  assert.ok(!codes(transition).includes('ART53_CEILING_CROSSED'));

  const crossed = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 900_000 }),
    invoices: [invoice({ id: 'pt', baseCents: 1_900_000, clientCountry: 'PT' })],
  });
  assert.ok(codes(crossed).includes('ART53_CEILING_CROSSED'));

  const watch = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 900_000 }),
    invoices: [invoice({ id: 'pt', baseCents: 1_250_000, clientCountry: 'PT' })],
  });
  assert.ok(codes(watch).includes('ART53_MARGIN_SHRINKING'));
});

test('EU and third-country invoicing produce separate, distinct flags', () => {
  const flags = run({
    profile: profile({ intraCommunityOperations: true, exports: true }),
    invoices: [
      invoice({ id: 'eu', clientCountry: 'FI', vatTreatment: 'autoliquidacao_ue', ivaRateBp: 0, retentionBp: 0 }),
      invoice({ id: 'us', clientCountry: 'US', vatTreatment: 'exportacao', ivaRateBp: 0, retentionBp: 0 }),
    ],
  });
  assert.ok(codes(flags).includes('EU_INVOICES_REVERSE_CHARGE'));
  assert.ok(codes(flags).includes('NON_EU_CLIENTS_NO_PT_VAT'));
});

test('Portuguese IVA charged to a foreign client is urgent and names the invoices', () => {
  const flags = run({
    invoices: [invoice({ id: 'bad', number: 'FR 2026/009', clientCountry: 'DE', ivaRateBp: 2300 })],
  });
  const flag = flags.find((candidate) => candidate.code === 'PT_IVA_CHARGED_ON_FOREIGN_INVOICE');
  assert.ok(flag);
  assert.equal(flag.severity, 'urgent');
  assert.deepEqual(flag.invoiceNumbers, ['FR 2026/009']);
});

test('withholding inconsistencies are caught in both directions', () => {
  const missing = run({ invoices: [invoice({ id: 'a', retentionBp: 0 })] });
  assert.ok(codes(missing).includes('RETENTION_MISSING_ON_RESIDENT'));

  const wrong = run({
    invoices: [invoice({ id: 'b', clientCountry: 'DE', retentionBp: 2300, ivaRateBp: 0 })],
  });
  assert.ok(codes(wrong).includes('RETENTION_CHARGED_ON_NON_RESIDENT'));
});

test('a repeated invoice number is urgent', () => {
  const flags = run({
    invoices: [invoice({ id: 'a' }), invoice({ id: 'b' })],
  });
  const flag = flags.find((candidate) => candidate.code === 'DUPLICATE_INVOICE_NUMBER');
  assert.ok(flag);
  assert.deepEqual(flag.invoiceNumbers, ['FR 2026/001']);
});

test('old invoices without a payment proof are flagged, recent ones are not', () => {
  const flags = run({
    invoices: [
      invoice({ id: 'old', number: 'FR 2026/001', date: '2026-01-10', paymentProofInVault: false }),
      invoice({ id: 'new', number: 'FR 2026/002', date: '2026-09-10', paymentProofInVault: false }),
    ],
  });
  const flag = flags.find((candidate) => candidate.code === 'MISSING_PAYMENT_PROOF');
  assert.ok(flag);
  assert.deepEqual(flag.invoiceNumbers, ['FR 2026/001']);
});

test('documents missing before a deadline are flagged until the vault has one', () => {
  const upcoming = instance();
  const withoutDocuments = run({ instances: [upcoming] });
  assert.ok(codes(withoutDocuments).includes('MISSING_DOCUMENTS_BEFORE_DEADLINE'));

  const withDocuments = run({ instances: [upcoming], documented: ['iva.recapitulativa'] });
  assert.ok(!codes(withDocuments).includes('MISSING_DOCUMENTS_BEFORE_DEADLINE'));
});

test('overdue obligations surface as one urgent flag, and untracked history does not', () => {
  const flags = run({
    instances: [
      instance({ id: 'late', status: 'overdue', dueDate: '2026-07-20' }),
      instance({ id: 'history', status: 'untracked', dueDate: '2026-02-20' }),
    ],
  });
  const flag = flags.find((candidate) => candidate.code === 'OBLIGATIONS_OVERDUE');
  assert.ok(flag);
  assert.equal(flag.severity, 'urgent');
  assert.equal(flag.ruleIds.length, 1, 'only the overdue instance, not the untracked one');
});

test('flags are ordered by severity and the summary counts them', () => {
  const flags = run({
    profile: profile({ ivaRegime: 'isento_art53', turnoverPreviousYearCents: 1_800_000 }),
    invoices: [invoice({ id: 'bad', clientCountry: 'DE', ivaRateBp: 2300 })],
  });
  const severities = flags.map((flag) => flag.severity);
  const urgency = severities.map((severity) => (severity === 'urgent' ? 0 : severity === 'attention' ? 1 : 2));
  assert.deepEqual(urgency, [...urgency].sort((a, b) => a - b), 'urgent flags come first');

  const summary = summariseFlags(flags);
  assert.equal(summary.total, flags.length);
  assert.equal(summary.urgent, severities.filter((severity) => severity === 'urgent').length);
});

test('with no data and no inputs, the only complaint is the missing turnover', () => {
  const flags = run({});
  assert.deepEqual(codes(flags), ['ART53_NO_TURNOVER_INPUT']);
});
