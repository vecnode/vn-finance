/**
 * Red flags and missing to-dos.
 *
 * EVERY FLAG HERE IS COMPUTED IN CODE. None of them involves a language model,
 * and that is a deliberate design decision rather than an implementation detail:
 *
 *   - "A fatura X não tem retenção na fonte" is arithmetic on a field, not a
 *     judgement about language. Asking a model to find it would make the answer
 *     non-deterministic, unauditable and occasionally absent.
 *   - The consequence is that THE ASSISTANT NEVER RECEIVES THE USER'S DATA. The
 *     only thing ever sent to a model is a list of public-law variable names and
 *     their current values (see `src/ai/update.ts`).
 *
 * Each flag states what was observed, why it matters, and cites the same legal
 * sources the rule pack cites, so a flag and a rule can be checked together.
 */

import { daysBetween, parseIso } from './dates.ts';
import { formatEur, type Cents } from './money.ts';
import { scopeOf } from './countries.ts';
import type { Invoice, VatTreatment } from './estimate.ts';
import type { IsoDate, ObligationInstance, RulePack, TaxProfile } from './types.ts';
import { freshness } from './rules.ts';

export type FlagSeverity = 'urgent' | 'attention' | 'info';

export type FlagCode =
  | 'ART53_NO_TURNOVER_INPUT'
  | 'ART53_EXEMPTION_ACTIVE'
  | 'ART53_EXEMPTION_AVAILABLE'
  | 'ART53_CEILING_EXCEEDED_HISTORY'
  | 'ART53_MARGIN_SHRINKING'
  | 'ART53_IN_TRANSITION_ZONE'
  | 'ART53_CEILING_CROSSED'
  | 'EU_INVOICES_REVERSE_CHARGE'
  | 'NON_EU_CLIENTS_NO_PT_VAT'
  | 'FOREIGN_CLIENTS_NOT_DECLARED'
  | 'PT_IVA_CHARGED_ON_FOREIGN_INVOICE'
  | 'RETENTION_MISSING_ON_RESIDENT'
  | 'RETENTION_CHARGED_ON_NON_RESIDENT'
  | 'MISSING_PAYMENT_PROOF'
  | 'DUPLICATE_INVOICE_NUMBER'
  | 'MISSING_DOCUMENTS_BEFORE_DEADLINE'
  | 'OBLIGATIONS_OVERDUE'
  | 'RULES_NOT_FULLY_VERIFIED'
  | 'RULE_PACK_STALE'
  | 'VALUE_PROPOSED_BY_AI_UNCONFIRMED';

export interface Flag {
  code: FlagCode;
  severity: FlagSeverity;
  title: string;
  detail: string;
  legalBasis: string[];
  sourceIds: string[];
  ruleIds: string[];
  /** Invoice numbers the flag refers to, for the ones that refer to invoices. */
  invoiceNumbers: string[];
}

export interface TurnoverByScope {
  national: Cents;
  eu: Cents;
  nonEu: Cents;
}

/**
 * Turnover split by where the client is, which is what the art. 53.º ceiling
 * actually measures: national territory, not total invoicing.
 */
export function turnoverByScope(invoices: readonly Invoice[], year: number): TurnoverByScope {
  const inYear = (invoice: Invoice): boolean => parseIso(invoice.date).year === year;
  const totals: TurnoverByScope = { national: 0, eu: 0, nonEu: 0 };
  for (const invoice of invoices) {
    if (!inYear(invoice)) continue;
    totals[scopeOf(invoice.clientCountry)] += invoice.baseCents;
  }
  return totals;
}

export interface FlagInput {
  pack: RulePack;
  profile: TaxProfile;
  invoices: readonly Invoice[];
  instances: readonly ObligationInstance[];
  today: IsoDate;
  /** Obligation ids that already have at least one document in the vault. */
  documentedObligationIds?: readonly string[];
  dueSoonDays?: number;
}

const SEVERITY_ORDER: Record<FlagSeverity, number> = { urgent: 0, attention: 1, info: 2 };

function cite(pack: RulePack, ruleId: string): { legalBasis: string[]; sourceIds: string[] } {
  const rule = pack.obligations.find((candidate) => candidate.id === ruleId);
  return { legalBasis: rule?.legalBasis ?? [], sourceIds: rule?.sourceIds ?? [] };
}

function referenceVatTreatment(treatment: VatTreatment): string {
  switch (treatment) {
    case 'iva_pt':
      return 'IVA português';
    case 'autoliquidacao_ue':
      return 'autoliquidação pelo cliente (UE)';
    case 'isento_art53':
      return 'isento pelo art. 53.º';
    case 'exportacao':
      return 'exportação';
  }
}

export function computeFlags(input: FlagInput): Flag[] {
  const { pack, profile, invoices, instances, today } = input;
  const flags: Flag[] = [];
  const year = parseIso(today).year;
  const dueSoonDays = input.dueSoonDays ?? 30;
  const documented = new Set(input.documentedObligationIds ?? []);

  const art53 = pack.constants.iva.art53;
  const ceiling = art53.ceiling;
  const declaredPrevious = profile.activity.turnoverPreviousYearCents;
  const scopes = turnoverByScope(invoices, year);
  const art53Sources = art53.sourceIds;
  const art53Basis = ['CIVA art. 53.º'];

  const push = (flag: Flag): void => {
    flags.push(flag);
  };

  // -------------------------------------------------------------------------
  // art. 53.º CIVA — the exemption is a number to watch all year
  // -------------------------------------------------------------------------

  if (declaredPrevious === undefined) {
    push({
      code: 'ART53_NO_TURNOVER_INPUT',
      severity: 'attention',
      title: 'Falta o volume de negócios do ano anterior',
      detail:
        'O enquadramento no regime especial de isenção do art. 53.º do CIVA depende do volume de ' +
        'negócios em território nacional do ano civil anterior. Enquanto esse valor não for indicado, ' +
        'a aplicação não avalia a isenção nem avisa quando o limite se aproxima. Regista-o com ' +
        '`vnfin init --turnover-ano-anterior <valor>` ou editando o perfil no cofre.',
      legalBasis: art53Basis,
      sourceIds: art53Sources,
      ruleIds: [],
      invoiceNumbers: [],
    });
  } else if (ceiling !== null) {
    const qualifiesHistorically =
      declaredPrevious <= ceiling && !profile.activity.exports && profile.residentPT;
    const exempt = profile.iva.regime === 'isento_art53';
    const marginCents = ceiling - declaredPrevious;

    if (exempt && !qualifiesHistorically) {
      const reasons: string[] = [];
      if (declaredPrevious > ceiling) {
        reasons.push(
          `o volume de negócios do ano anterior (${formatEur(declaredPrevious)}) excedeu o limite`,
        );
      }
      if (profile.activity.exports) reasons.push('o perfil indica operações de exportação');
      if (!profile.residentPT) reasons.push('o perfil não tem domicílio em território nacional');
      push({
        code: 'ART53_CEILING_EXCEEDED_HISTORY',
        severity: 'urgent',
        title: 'O regime do art. 53.º parece não ser aplicável',
        detail:
          `O perfil está enquadrado na isenção do art. 53.º, mas ${reasons.join(' e ')}. ` +
          'Confirma o enquadramento com um contabilista certificado antes de emitir mais faturas sem IVA.',
        legalBasis: art53Basis,
        sourceIds: art53Sources,
        ruleIds: [],
        invoiceNumbers: [],
      });
    } else if (exempt) {
      push({
        code: 'ART53_EXEMPTION_ACTIVE',
        severity: 'info',
        title: 'Isenção do art. 53.º ativa',
        detail:
          `Volume de negócios do ano anterior: ${formatEur(declaredPrevious)}, ` +
          `margem de ${formatEur(marginCents)} até ao limite de ${formatEur(ceiling)}. ` +
          `Faturação de ${formatEur(scopes.national)} em território nacional este ano.`,
        legalBasis: art53Basis,
        sourceIds: art53Sources,
        ruleIds: [],
        invoiceNumbers: [],
      });
    } else if (qualifiesHistorically) {
      push({
        code: 'ART53_EXEMPTION_AVAILABLE',
        severity: 'info',
        title: 'Pode ser elegível para a isenção do art. 53.º',
        detail:
          `O volume de negócios do ano anterior (${formatEur(declaredPrevious)}) ficou ` +
          `${formatEur(marginCents)} abaixo do limite de ${formatEur(ceiling)} e não há ` +
          'operações de exportação no perfil. O enquadramento atual é o regime normal. A mudança para o ' +
          'regime de isenção é uma decisão declarativa: confirma-a com um contabilista certificado.',
        legalBasis: art53Basis,
        sourceIds: art53Sources,
        ruleIds: [],
        invoiceNumbers: [],
      });
    }

    // The current year, watched live against the national-territory figure.
    if (exempt) {
      const transitionCeiling = Math.round(ceiling * 1.25); // 15 000 -> 18 750
      if (scopes.national > transitionCeiling) {
        push({
          code: 'ART53_CEILING_CROSSED',
          severity: 'urgent',
          title: 'Limite do art. 53.º ultrapassado durante o ano',
          detail:
            `Faturação em território nacional este ano: ${formatEur(scopes.national)}, acima de ` +
            `${formatEur(transitionCeiling)}. Acima deste valor o IVA passa a ser liquidado a partir ` +
            'do momento em que foi excedido. Confirma o procedimento com um contabilista certificado.',
          legalBasis: art53Basis,
          sourceIds: art53Sources,
          ruleIds: [],
          invoiceNumbers: [],
        });
      } else if (scopes.national > ceiling) {
        push({
          code: 'ART53_IN_TRANSITION_ZONE',
          severity: 'urgent',
          title: 'Zona de transição do art. 53.º',
          detail:
            `Faturação em território nacional este ano: ${formatEur(scopes.national)}, entre o limite ` +
            `de ${formatEur(ceiling)} e ${formatEur(transitionCeiling)}. Nesta zona é ` +
            'entregue declaração de alterações nos 15 dias úteis seguintes ao fim do ano e o regime normal ' +
            'aplica-se a partir de 1 de janeiro seguinte.',
          legalBasis: art53Basis,
          sourceIds: art53Sources,
          ruleIds: [],
          invoiceNumbers: [],
        });
      } else if (scopes.national > Math.round(ceiling * 0.8)) {
        push({
          code: 'ART53_MARGIN_SHRINKING',
          severity: 'attention',
          title: 'Margem do art. 53.º a diminuir',
          detail:
            `Faturação em território nacional este ano: ${formatEur(scopes.national)}, acima de 80% do ` +
            `limite de ${formatEur(ceiling)}. Faltam ` +
            `${formatEur(ceiling - scopes.national)} para o limite.`,
          legalBasis: art53Basis,
          sourceIds: art53Sources,
          ruleIds: [],
          invoiceNumbers: [],
        });
      }
    }
  }

  // -------------------------------------------------------------------------
  // Work outside Portugal
  // -------------------------------------------------------------------------

  const recap = cite(pack, 'iva.recapitulativa');
  if (scopes.eu > 0) {
    push({
      code: 'EU_INVOICES_REVERSE_CHARGE',
      severity: 'info',
      title: 'Serviços a clientes da União Europeia',
      detail:
        `Faturação a clientes da UE este ano: ${formatEur(scopes.eu)}. Estas operações não levam IVA ` +
        'português (o cliente autoliquida), exigem um número de registo válido no VIES e entram na declaração ' +
        'recapitulativa. Confirma que os recibos indicam "IVA - autoliquidação" e que o VIES está válido.',
      legalBasis: recap.legalBasis,
      sourceIds: recap.sourceIds,
      ruleIds: ['iva.recapitulativa'],
      invoiceNumbers: invoices
        .filter((invoice) => scopeOf(invoice.clientCountry) === 'eu')
        .map((invoice) => invoice.number),
    });
  }

  if (scopes.nonEu > 0) {
    push({
      code: 'NON_EU_CLIENTS_NO_PT_VAT',
      severity: 'info',
      title: 'Serviços a clientes fora da União Europeia',
      detail:
        `Faturação a clientes de países terceiros este ano: ${formatEur(scopes.nonEu)}. ` +
        'Estas prestações não são sujeitas a IVA português e não contam para o limite do art. 53.º, que se ' +
        'mede em território nacional. Guarda o comprovativo de que o cliente está estabelecido fora da UE.',
      legalBasis: ['CIVA art. 6.º (localização das prestações de serviços)'],
      sourceIds: recap.sourceIds,
      ruleIds: [],
      invoiceNumbers: invoices
        .filter((invoice) => scopeOf(invoice.clientCountry) === 'nonEu')
        .map((invoice) => invoice.number),
    });
  }

  const hasForeignInvoices = scopes.eu > 0 || scopes.nonEu > 0;
  if (
    hasForeignInvoices &&
    !profile.activity.intraCommunityOperations &&
    !profile.activity.exports
  ) {
    push({
      code: 'FOREIGN_CLIENTS_NOT_DECLARED',
      severity: 'attention',
      title: 'Faturação ao estrangeiro não declarada no perfil',
      detail:
        'Existem faturas a clientes fora de Portugal mas o perfil não indica operações intracomunitárias ' +
        'nem exportações. O enquadramento no perfil tem de refletir a realidade, porque é ele que determina ' +
        'quais obrigações entram na agenda.',
      legalBasis: [],
      sourceIds: [],
      ruleIds: ['iva.recapitulativa', 'iva.dp.mensal', 'iva.dp.trimestral'],
      invoiceNumbers: [],
    });
  }

  const wrongVat = invoices.filter(
    (invoice) => scopeOf(invoice.clientCountry) !== 'national' && invoice.ivaRateBp > 0,
  );
  if (wrongVat.length > 0) {
    push({
      code: 'PT_IVA_CHARGED_ON_FOREIGN_INVOICE',
      severity: 'urgent',
      title: 'IVA português cobrado em faturas ao estrangeiro',
      detail:
        `${wrongVat.length} fatura(s) a clientes fora de Portugal têm IVA português liquidado ` +
        `(${wrongVat
          .map(
            (invoice) =>
              `${invoice.number}: ${(invoice.ivaRateBp / 100).toFixed(0)}% tratado como ` +
              referenceVatTreatment(invoice.vatTreatment),
          )
          .join(', ')}). ` +
        'Numa prestação de serviços a um cliente da UE o IVA é devido no país do cliente e é o cliente que o ' +
        'autoliquida; a fatura não deve levar IVA português. Se o tratamento estiver errado, a fatura tem de ' +
        'ser corrigida, não apenas o registo.',
      legalBasis: ['CIVA art. 6.º (localização das prestações de serviços)'],
      sourceIds: recap.sourceIds,
      ruleIds: [],
      invoiceNumbers: wrongVat.map((invoice) => invoice.number),
    });
  }

  // -------------------------------------------------------------------------
  // Per-invoice checks that are pure arithmetic
  // -------------------------------------------------------------------------

  const retentionSource = cite(pack, 'irs.retencoesPagamento');
  const missingRetention = invoices.filter(
    (invoice) => scopeOf(invoice.clientCountry) === 'national' && invoice.retentionBp === 0,
  );
  if (missingRetention.length > 0) {
    push({
      code: 'RETENTION_MISSING_ON_RESIDENT',
      severity: 'attention',
      title: 'Faturas a clientes residentes sem retenção na fonte',
      detail:
        `${missingRetention.length} fatura(s) a clientes residentes não têm retenção na fonte ` +
        `(${missingRetention.map((invoice) => invoice.number).join(', ')}). Se o cliente é obrigado a reter, ` +
        'a retenção em falta tem de ser regularizada — o valor é creditado no IRS final e a diferença ' +
        'aparece no acerto. A taxa aplicável está no pacote de regras, verificada no art. 101.º do CIRS.',
      legalBasis: retentionSource.legalBasis,
      sourceIds: retentionSource.sourceIds,
      ruleIds: ['irs.retencoesPagamento'],
      invoiceNumbers: missingRetention.map((invoice) => invoice.number),
    });
  }

  const wrongRetention = invoices.filter(
    (invoice) => scopeOf(invoice.clientCountry) !== 'national' && invoice.retentionBp > 0,
  );
  if (wrongRetention.length > 0) {
    push({
      code: 'RETENTION_CHARGED_ON_NON_RESIDENT',
      severity: 'attention',
      title: 'Retenção na fonte em faturas ao estrangeiro',
      detail:
        `${wrongRetention.length} fatura(s) a clientes não residentes têm retenção na fonte ` +
        `(${wrongRetention.map((invoice) => invoice.number).join(', ')}). Clientes fora de Portugal não fazem ` +
        'retenção de IRS português.',
      legalBasis: retentionSource.legalBasis,
      sourceIds: retentionSource.sourceIds,
      ruleIds: [],
      invoiceNumbers: wrongRetention.map((invoice) => invoice.number),
    });
  }

  const unpaidProof = invoices.filter(
    (invoice) => !invoice.paymentProofInVault && daysBetween(invoice.date, today) > 60,
  );
  if (unpaidProof.length > 0) {
    push({
      code: 'MISSING_PAYMENT_PROOF',
      severity: 'attention',
      title: 'Faturas sem comprovativo de pagamento',
      detail:
        `${unpaidProof.length} fatura(s) emitidas há mais de 60 dias não têm comprovativo de recebimento ` +
        `no cofre (${unpaidProof.map((invoice) => invoice.number).join(', ')}). ` +
        'O comprovativo é o que liga o rendimento declarado ao dinheiro que entrou.',
      legalBasis: [],
      sourceIds: [],
      ruleIds: [],
      invoiceNumbers: unpaidProof.map((invoice) => invoice.number),
    });
  }

  const seenNumbers = new Map<string, number>();
  for (const invoice of invoices) {
    seenNumbers.set(invoice.number, (seenNumbers.get(invoice.number) ?? 0) + 1);
  }
  const duplicates = [...seenNumbers.entries()].filter(([, count]) => count > 1).map(([number]) => number);
  if (duplicates.length > 0) {
    push({
      code: 'DUPLICATE_INVOICE_NUMBER',
      severity: 'urgent',
      title: 'Números de fatura repetidos',
      detail:
        `Há números de fatura usados mais de uma vez (${duplicates.join(', ')}). A numeração sequencial é ` +
        'uma obrigação de faturação e um número repetido é o tipo de erro que faz uma fatura ser recusada.',
      legalBasis: ['CIVA art. 36.º (requisitos de emissão e numeração)'],
      sourceIds: [],
      ruleIds: [],
      invoiceNumbers: duplicates,
    });
  }

  // -------------------------------------------------------------------------
  // Obligations already in the agenda
  // -------------------------------------------------------------------------

  const overdue = instances.filter((instance) => instance.status === 'overdue');
  if (overdue.length > 0) {
    push({
      code: 'OBLIGATIONS_OVERDUE',
      severity: 'urgent',
      title: `${overdue.length} obrigação(ões) em atraso`,
      detail: overdue
        .slice(0, 5)
        .map((instance) => `${instance.dueDate}: ${instance.title}`)
        .join('\n'),
      legalBasis: [],
      sourceIds: [],
      ruleIds: overdue.map((instance) => instance.ruleId),
      invoiceNumbers: [],
    });
  }

  const upcomingWithDocuments = instances.filter(
    (instance) =>
      instance.status !== 'done' &&
      instance.status !== 'untracked' &&
      instance.status !== 'not_applicable' &&
      instance.documentsToKeep.length > 0 &&
      daysBetween(today, instance.dueDate) >= 0 &&
      daysBetween(today, instance.dueDate) <= dueSoonDays &&
      !documented.has(instance.ruleId),
  );
  if (upcomingWithDocuments.length > 0) {
    push({
      code: 'MISSING_DOCUMENTS_BEFORE_DEADLINE',
      severity: 'attention',
      title: 'Documentos em falta para prazos próximos',
      detail: upcomingWithDocuments
        .slice(0, 4)
        .map(
          (instance) =>
            `${instance.dueDate} — ${instance.title}: falta um de ${instance.documentsToKeep.length} ` +
            'documento(s) obrigatório(s) no cofre',
        )
        .join('\n'),
      legalBasis: [],
      sourceIds: [],
      ruleIds: upcomingWithDocuments.map((instance) => instance.ruleId),
      invoiceNumbers: [],
    });
  }

  return flags.sort(
    (a, b) => SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity] || (a.title < b.title ? -1 : 1),
  );
}

export interface FlagSummary {
  urgent: number;
  attention: number;
  info: number;
  total: number;
}

export function summariseFlags(flags: readonly Flag[]): FlagSummary {
  return {
    urgent: flags.filter((flag) => flag.severity === 'urgent').length,
    attention: flags.filter((flag) => flag.severity === 'attention').length,
    info: flags.filter((flag) => flag.severity === 'info').length,
    total: flags.length,
  };
}

/**
 * Flags that belong to the pack's own health rather than to the taxpayer's
 * affairs. Kept separate so `vnfin doctor` can report them without them being
 * mixed into the user's to-do list.
 */
export function packHealthFlags(pack: RulePack, today: IsoDate): Flag[] {
  const flags: Flag[] = [];

  const stale = freshness(pack, today);
  if (stale.status !== 'current') {
    flags.push({
      code: 'RULE_PACK_STALE',
      severity: stale.status === 'stale' ? 'urgent' : 'info',
      title: stale.status === 'stale' ? 'Pacote de regras desatualizado' : 'Pacote de regras de outro ano',
      detail: stale.message,
      legalBasis: [],
      sourceIds: [],
      ruleIds: [],
      invoiceNumbers: [],
    });
  }

  for (const [path, provenance] of Object.entries(pack.constantProvenance ?? {})) {
    if (provenance.status !== 'unverified') continue;
    flags.push({
      code: 'VALUE_PROPOSED_BY_AI_UNCONFIRMED',
      severity: provenance.source === 'ai-proposed' ? 'urgent' : 'attention',
      title: 'Valor por confirmar no pacote de regras',
      detail:
        `${path}: valor ${provenance.source === 'ai-proposed' ? 'proposto pelo assistente' : 'registado'} em ` +
        `${provenance.proposedAt} e ainda não confirmado por uma pessoa.` +
        (provenance.sourceUrl === null ? '' : ` Fonte indicada: ${provenance.sourceUrl}`) +
        ' Enquanto não for confirmado na fonte, os cálculos que dependem dele não devem ser usados.',
      legalBasis: [],
      sourceIds: [],
      ruleIds: [],
      invoiceNumbers: [],
    });
  }

  return flags;
}
