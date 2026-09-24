/**
 * The golden test.
 *
 * The obligation engine must reproduce the dates the Autoridade Tributária
 * actually published for 2026 — not dates that follow from a formula that looks
 * about right. These expectations were taken from the Portal das Finanças
 * "Resumo anual — obrigações declarativas / de pagamento em 2026" on 2026-09-20.
 *
 * If a future change to the engine makes one of these fail, the engine is wrong,
 * not the test.
 */

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';

import { buildAgenda, upcoming } from './calendar.ts';
import { appliesTo } from './conditions.ts';
import { isWeekend } from './dates.ts';
import { createDefaultProfile } from './profile.ts';
import { defaultPackPath, loadRulePack, summarisePack } from './rules.ts';
import type { ObligationInstance, ObligationRule, RulePack, TaxProfile } from './types.ts';

function nullConstants(): RulePack['constants'] {
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
      baseMinMultipleIas: null,
      baseMaxMultipleIas: null,
      iasMonthly: null,
      startupExemptionMonths: null,
      startupReductions: [],
      sourceIds: [],
    },
  };
}

function rule(partial: Partial<ObligationRule> & { id: string }): ObligationRule {
  return {
    kind: 'pay',
    authority: 'AT',
    tax: 'IRS',
    title: partial.id,
    appliesWhen: [],
    periodicity: { type: 'annual' },
    deadlineRule: null,
    officialDates: {},
    legalBasis: ['CIRS art. 102.º'],
    sourceIds: ['s1'],
    portalUrl: null,
    documentsToKeep: [],
    penaltyNote: null,
    verification: 'verified',
    verifyNote: null,
    ...partial,
  };
}

function pack(obligations: ObligationRule[]): RulePack {
  return {
    jurisdiction: 'PT',
    year: 2026,
    packVersion: 'test',
    generatedAt: '2026-09-20',
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: 'teste',
    sources: [
      {
        id: 's1',
        tier: 'official',
        authority: 'AT',
        title: 'Agenda Fiscal 2026',
        url: 'https://info.portaldasfinancas.gov.pt/',
        retrievedAt: '2026-09-20',
      },
    ],
    constants: nullConstants(),
    obligations,
    todo: [],
  };
}

const PROFILE: TaxProfile = createDefaultProfile({
  nif: '123456789',
  name: 'Contribuinte de Teste',
  startDate: '2019-04-01',
  // Monthly IVA, so the monthly rules below are actually applicable to this
  // profile. A quarterly taxpayer gets them as "não aplicável", which is what
  // the dedicated not-applicable test checks.
  ivaRegime: 'mensal',
});

function rules(): ObligationRule[] {
  return [
    rule({
      id: 'irs.pagamentosPorConta',
      title: 'IRS — pagamentos por conta',
      periodicity: { type: 'annual' },
      deadlineRule: { kind: 'fixed_date', month: 7, day: 20 },
      officialDates: {
        '2026': [{ month: 7, day: 20 }, { month: 9, day: 21 }, { month: 12, day: 21 }],
      },
    }),
    rule({
      id: 'irs.modelo3',
      title: 'IRS — Modelo 3 e anexos',
      kind: 'declare',
      periodicity: { type: 'annual' },
      deadlineRule: { kind: 'fixed_date', month: 6, day: 30 },
      officialDates: { '2026': [{ month: 6, day: 30 }] },
    }),
    rule({
      id: 'iva.dp.mensal',
      title: 'IVA — declaração periódica (regime mensal)',
      kind: 'declare',
      tax: 'IVA',
      appliesWhen: [{ field: 'iva.regime', op: 'eq', value: 'mensal' }],
      periodicity: { type: 'monthly', lagMonths: 2 },
      deadlineRule: { kind: 'day_of_month', day: 20 },
      officialDates: {
        '2026': [
          { month: 1, day: 20 },
          { month: 2, day: 20 },
          { month: 3, day: 20 },
          { month: 4, day: 20 },
          { month: 5, day: 20 },
          { month: 6, day: 22 },
          { month: 7, day: 20 },
          // August is deliberately absent: the published table has no entry.
          { month: 9, day: 21 },
          { month: 10, day: 20 },
          { month: 11, day: 20 },
          { month: 12, day: 21 },
        ],
      },
    }),
    rule({
      id: 'efatura.comunicacaoFaturas',
      title: 'Comunicação dos elementos das faturas',
      kind: 'communicate',
      tax: 'IVA',
      periodicity: { type: 'monthly', lagMonths: 1 },
      deadlineRule: { kind: 'day_of_month', day: 5 },
      officialDates: {
        '2026': [
          { month: 1, day: 9, note: 'Prazo alargado por despacho.' },
          { month: 2, day: 5 },
          { month: 3, day: 5 },
          { month: 4, day: 8 },
          { month: 5, day: 8 },
          { month: 6, day: 5 },
          { month: 7, day: 6 },
          { month: 8, day: 31, note: 'Prazo de agosto alargado até ao fim do mês.' },
          { month: 9, day: 7 },
          { month: 10, day: 6 },
          { month: 11, day: 5 },
          { month: 12, day: 7 },
        ],
      },
    }),
    rule({
      id: 'ies',
      title: 'IES — Informação Empresarial Simplificada',
      kind: 'declare',
      tax: 'IRS-IRC',
      appliesWhen: [{ field: 'profile.isCompany', op: 'eq', value: true }],
      periodicity: { type: 'annual' },
      deadlineRule: { kind: 'fixed_date', month: 7, day: 15 },
    }),
    rule({
      id: 'guardar.documentos',
      title: 'Conservar faturas e comprovativos',
      kind: 'save',
      authority: 'AT',
      periodicity: { type: 'continuous' },
      deadlineRule: null,
      documentsToKeep: ['Faturas-recibo emitidas', 'Comprovativos de pagamento'],
    }),
  ];
}

function agenda(year = 2026, profile: TaxProfile = PROFILE): ObligationInstance[] {
  return buildAgenda(pack(rules()), profile, { year, today: '2026-09-20' });
}

function instancesOf(instances: ObligationInstance[], ruleId: string): ObligationInstance[] {
  return instances.filter((instance) => instance.ruleId === ruleId);
}

test('pagamentos por conta: the three published 2026 dates become three separate obligations', () => {
  const found = instancesOf(agenda(), 'irs.pagamentosPorConta');
  assert.deepEqual(
    found.map((instance) => instance.dueDate),
    ['2026-07-20', '2026-09-21', '2026-12-21'],
  );
  assert.ok(found.every((instance) => instance.dueDateSource === 'official'));
  assert.ok(found.every((instance) => instance.periodLabel.includes('rendimentos de 2025')));
});

test('Modelo 3 for 2025 income is due on 30 June 2026, and says which year it reports', () => {
  const [instance] = instancesOf(agenda(), 'irs.modelo3');
  assert.ok(instance);
  assert.equal(instance.dueDate, '2026-06-30');
  assert.equal(instance.periodLabel, 'rendimentos de 2025');
  assert.equal(instance.dueDateSource, 'official');
});

test('when the legal day falls on a weekend, the published date wins and the shift is recorded', () => {
  // 20 June 2026 is a Saturday, so AT publishes Monday 22 June.
  assert.equal(isWeekend('2026-06-20'), true);
  // The April period is due two months later, in June.
  const june = instancesOf(agenda(), 'iva.dp.mensal').find((instance) => instance.dueDate.startsWith('2026-06'));
  assert.ok(june);
  assert.equal(june.dueDate, '2026-06-22');
  assert.equal(june.dueDateSource, 'official');
  assert.equal(june.adjustedForWeekend, true);
  assert.ok(june.discrepancies.some((note) => note.includes('fim de semana')));
});

test('a month with no published deadline falls back to the legal rule AND says so', () => {
  const august = instancesOf(agenda(), 'iva.dp.mensal').find((instance) =>
    instance.dueDate.startsWith('2026-08'),
  );
  assert.ok(august);
  assert.equal(august.dueDate, '2026-08-20');
  assert.equal(august.dueDateSource, 'derived');
  assert.equal(august.provisional, false);
  assert.ok(
    august.discrepancies.some((note) => note.includes('não publica qualquer prazo')),
    'the discrepancy must explain that the authority published nothing for this month',
  );
});

test('a published extension that is neither the rule nor the weekend shift is flagged for a human', () => {
  const august = instancesOf(agenda(), 'efatura.comunicacaoFaturas').find((instance) =>
    instance.dueDate.startsWith('2026-08'),
  );
  assert.ok(august);
  assert.equal(august.dueDate, '2026-08-31');
  assert.equal(august.dueDateSource, 'official');
  assert.ok(august.discrepancies.some((note) => note.includes('alargado')));
});

test('an obligation that does not apply is shown as not applicable, with the reason', () => {
  const [ies] = instancesOf(agenda(), 'ies');
  assert.ok(ies);
  assert.equal(ies.status, 'not_applicable');
  assert.ok(ies.notApplicableReason?.includes('ser uma sociedade'));
});

test('a company profile makes the same obligation applicable', () => {
  const company = createDefaultProfile({
    nif: '501234560',
    name: 'Sociedade de Teste, Lda.',
    isCompany: true,
    startDate: '2019-04-01',
  });
  const [ies] = instancesOf(agenda(2026, company), 'ies');
  assert.ok(ies);
  assert.equal(ies.status !== 'not_applicable', true);
});

test('a year the authority has not published is marked provisional instead of invented', () => {
  const next = instancesOf(agenda(2027), 'irs.pagamentosPorConta');
  assert.equal(next.length, 1);
  const [instance] = next;
  assert.ok(instance);
  assert.equal(instance.dueDate, '2027-07-20');
  assert.equal(instance.dueDateSource, 'derived');
  assert.equal(instance.provisional, true);
});

test('an annual report covers the previous civil year even in a provisional year', () => {
  const [instance] = instancesOf(agenda(2027), 'irs.modelo3');
  assert.ok(instance);
  assert.equal(instance.periodLabel, 'rendimentos de 2026');
  assert.equal(instance.dueDate, '2027-06-30');
});

test('a standing duty to keep documents is never shown as late', () => {
  const [instance] = instancesOf(agenda(), 'guardar.documentos');
  assert.ok(instance);
  assert.equal(instance.dueDate, '2026-12-31');
  assert.equal(instance.status, 'future');
  assert.equal(instance.checklists.length, 2);
});

test('the engine is deterministic: same inputs, byte-identical output', () => {
  assert.equal(JSON.stringify(agenda()), JSON.stringify(agenda()));
});

test('the upcoming window respects the horizon and the completion list', () => {
  const instances = agenda();
  const window = upcoming(instances, '2026-09-20', 20);
  const dates = window.actionable.map((instance) => instance.dueDate);

  assert.equal(dates[0], '2026-09-21', 'a primeira obrigação acionável é o pagamento por conta de setembro');
  assert.ok(dates.every((date) => date >= '2026-09-20' && date <= '2026-10-10'));
  assert.ok(dates.includes('2026-10-06'), 'a comunicação de faturas de setembro cai dentro do horizonte');
  assert.ok(
    !dates.includes('2026-10-20'),
    'uma data exatamente fora do horizonte não pode aparecer na lista acionável',
  );

  const completed = buildAgenda(pack(rules()), PROFILE, {
    year: 2026,
    today: '2026-09-20',
    completedIds: ['irs.pagamentosPorConta@2026-09-21'],
  });
  const done = instancesOf(completed, 'irs.pagamentosPorConta').find(
    (instance) => instance.dueDate === '2026-09-21',
  );
  assert.equal(done?.status, 'done');
  assert.ok(
    !upcoming(completed, '2026-09-20', 20).actionable.some(
      (instance) => instance.id === 'irs.pagamentosPorConta@2026-09-21',
    ),
    'uma obrigação concluída sai do plano de ação',
  );
});

// ---------------------------------------------------------------------------
// Integration: the real rule pack shipped with the repository.
// ---------------------------------------------------------------------------

const realPackPath = defaultPackPath('pt', 2026);

if (existsSync(realPackPath)) {
  test('the shipped pt/2026 pack loads, cites its sources, and matches the published dates', () => {
    const loaded = loadRulePack(realPackPath);
    const summary = summarisePack(loaded);

    assert.ok(summary.obligations >= 10, `esperava pelo menos 10 obrigações, encontrei ${summary.obligations}`);
    assert.ok(summary.sources >= 1, 'o pacote tem de citar pelo menos uma fonte');

    // Provenance is mandatory for anything that can reach this taxpayer's agenda.
    // A rule that is structurally inapplicable to a sole trader (IES, Modelo 22)
    // may lack a citation, but only when the pack declares the gap in its own
    // `todo`. An unsourced rule that WOULD apply is a test failure.
    const alphanumeric = (value: string): string => value.toLowerCase().replace(/[^a-z0-9]/g, '');
    const excused = (id: string): boolean =>
      loaded.pack.todo.some((item) => alphanumeric(item).includes(alphanumeric(id)));

    let unsourced = 0;
    for (const obligation of loaded.pack.obligations) {
      const hasProvenance = obligation.legalBasis.length > 0 && obligation.sourceIds.length > 0;
      if (hasProvenance) continue;
      unsourced += 1;
      assert.ok(
        !appliesTo(PROFILE, obligation.appliesWhen),
        `${obligation.id} aplica-se a este perfil mas não cita a lei nem a fonte`,
      );
      assert.ok(
        excused(obligation.id),
        `${obligation.id} não cita a lei nem a fonte e o pacote não declara a lacuna no todo`,
      );
    }
    assert.ok(
      unsourced < loaded.pack.obligations.length,
      'o pacote não pode estar inteiramente por citar',
    );

    const integerWarnings = loaded.problems.filter((problem) => problem.message.includes('não é um número inteiro'));
    assert.deepEqual(
      integerWarnings,
      [],
      'dinheiro em cêntimos e taxas em pontos base têm de ser inteiros',
    );

    const instances = buildAgenda(loaded.pack, PROFILE, { year: 2026, today: '2026-09-20' });

    const pagamentos = instances.filter((instance) => /pagamentosPorConta/i.test(instance.ruleId));
    assert.ok(pagamentos.length > 0, 'o pacote tem de conter a obrigação dos pagamentos por conta');
    const dates = pagamentos.map((instance) => instance.dueDate);
    for (const expected of ['2026-07-20', '2026-09-21', '2026-12-21']) {
      assert.ok(dates.includes(expected), `falta o pagamento por conta de ${expected} (encontrei ${dates.join(', ')})`);
    }

    const modelo3 = instances.find((instance) => /modelo3/i.test(instance.ruleId));
    assert.ok(modelo3, 'o pacote tem de conter a obrigação do Modelo 3');
    assert.equal(modelo3.dueDate, '2026-06-30');
  });

  /*
   * RITI art. 30.º gives one frequency per taxpayer: n.º 1 a) is monthly for the
   * monthly regime, n.º 1 b) is quarterly for the quarterly regime, and n.º 2
   * turns the quarterly taxpayer monthly again once the intra-community
   * operations pass 50 000 EUR in a quarter. The pack used to schedule the
   * monthly rule on `intraCommunityOperations` alone, so a quarterly taxpayer
   * got both series at once — a combination the law never produces.
   */
  test('as três regras da declaração recapitulativa nunca se aplicam ao mesmo tempo', () => {
    const pack = loadRulePack(realPackPath).pack;
    const rules = [
      'iva.recapitulativa',
      'iva.recapitulativa.trimestral',
      'iva.recapitulativa.mensalPorVolume',
    ].map((id) => {
      const rule = pack.obligations.find((candidate) => candidate.id === id);
      assert.ok(rule, `o pacote tem de declarar ${id}`);
      return rule;
    });

    const profile = (ivaRegime: 'mensal' | 'trimestral', above50k?: boolean): TaxProfile => {
      const built = createDefaultProfile({ nif: '123456789', name: 'Contribuinte de Teste', ivaRegime });
      return {
        ...built,
        activity: {
          ...built.activity,
          intraCommunityOperations: true,
          ...(above50k === undefined ? {} : { intraCommunityOperationsAbove50k: above50k }),
        },
      };
    };

    const scenarios: Array<{ label: string; profile: TaxProfile; expected: string }> = [
      { label: 'regime mensal', profile: profile('mensal'), expected: 'iva.recapitulativa' },
      {
        label: 'regime trimestral, exceção por declarar',
        profile: profile('trimestral'),
        expected: 'iva.recapitulativa.trimestral',
      },
      {
        label: 'regime trimestral, exceção negada',
        profile: profile('trimestral', false),
        expected: 'iva.recapitulativa.trimestral',
      },
      {
        label: 'regime trimestral acima de 50 000 EUR',
        profile: profile('trimestral', true),
        expected: 'iva.recapitulativa.mensalPorVolume',
      },
    ];

    for (const scenario of scenarios) {
      const applicable = rules
        .filter((rule) => appliesTo(scenario.profile, rule.appliesWhen))
        .map((rule) => rule.id);
      assert.deepEqual(
        applicable,
        [scenario.expected],
        `${scenario.label}: exatamente uma frequência pode aplicar-se (obtive ${applicable.join(', ') || 'nenhuma'})`,
      );
    }
  });
}
