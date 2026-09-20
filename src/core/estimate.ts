/**
 * Calculations: invoices, IVA, Segurança Social and the simplified-regime IRS
 * base.
 *
 * The rule here is that every function SHOWS ITS WORK and REFUSES TO GUESS.
 * When a rate is missing from the rule pack, the function returns `null` and a
 * stated limitation rather than a plausible-looking number. A wrong number in a
 * tax tool is worse than a missing one, because the user cannot tell the
 * difference.
 */

import { applyBp, formatEur, splitEvenly, type BasisPoints, type Cents } from './money.ts';
import { quarterRange } from './dates.ts';
import { scopeOf } from './countries.ts';
import type { IsoDate, RulePack, TaxProfile } from './types.ts';

export type InvoiceStatus = 'issued' | 'paid' | 'pending';
export type VatTreatment = 'iva_pt' | 'autoliquidacao_ue' | 'isento_art53' | 'exportacao';

export interface Invoice {
  id: string;
  /** Sequential document number as required by the invoicing rules, e.g. "FR 2026/042". */
  number: string;
  date: IsoDate;
  clientName: string;
  /** Masked or absent in the AI payload; full value stays in the local vault. */
  clientNif: string | null;
  clientCountry: string;
  description: string;
  baseCents: Cents;
  ivaRateBp: BasisPoints;
  vatTreatment: VatTreatment;
  retentionBp: BasisPoints;
  atcud: string | null;
  status: InvoiceStatus;
  paymentProofInVault: boolean;
}

export interface InvoiceTotals {
  baseCents: Cents;
  ivaCents: Cents;
  retentionCents: Cents;
  /** What the client actually owes: base + IVA - retention. */
  netReceivableCents: Cents;
}

/** Whose money each amount is, computed from one invoice. */
export function invoiceTotals(invoice: Invoice): InvoiceTotals {
  const ivaCents = applyBp(invoice.baseCents, invoice.ivaRateBp);
  const retentionCents = applyBp(invoice.baseCents, invoice.retentionBp);
  return {
    baseCents: invoice.baseCents,
    ivaCents,
    retentionCents,
    netReceivableCents: invoice.baseCents + ivaCents - retentionCents,
  };
}

function inRange(invoice: Invoice, start: IsoDate, end: IsoDate): boolean {
  return invoice.date >= start && invoice.date <= end;
}

export interface InvoiceTerms {
  vatTreatment: VatTreatment;
  ivaRateBp: BasisPoints;
  retentionBp: BasisPoints;
}

/**
 * Sensible terms for a new invoice, taken from the rule pack and the profile.
 *
 * This is a small piece of law-adjacent judgement that three surfaces need (the
 * command line, the panel and any importer), so it lives here, tested, rather than
 * being re-derived in each of them. Getting it wrong produces an invoice that
 * contradicts the alert engine — which is exactly what happened when the retention
 * default was written as a literal 25%, and then again when the pack's
 * non-resident rate was mistaken for "a non-resident client withholds from you".
 */
export function defaultInvoiceTerms(
  pack: RulePack,
  profile: TaxProfile | null,
  country: string,
): InvoiceTerms {
  const scope = scopeOf(country);

  if (scope !== 'national') {
    // No Portuguese VAT is charged, and a client outside Portugal does not
    // withhold Portuguese IRS. The pack's non-resident rate applies in the other
    // direction: to income paid *to* a non-resident by a Portuguese payer.
    return {
      vatTreatment: scope === 'eu' ? 'autoliquidacao_ue' : 'exportacao',
      ivaRateBp: 0,
      retentionBp: 0,
    };
  }

  // An art. 53.º exempt taxpayer charges no VAT at all, and says so on the
  // invoice. Retention is independent of the IVA exemption and still applies.
  const exempt = profile?.iva.regime === 'isento_art53';
  return {
    vatTreatment: exempt ? 'isento_art53' : 'iva_pt',
    ivaRateBp: exempt ? 0 : (pack.constants.iva.rates.normal ?? 0),
    retentionBp: pack.constants.irs.withholding.professionalServicesResident ?? 0,
  };
}

export interface PeriodTotals {
  baseCents: Cents;
  ivaLiquidadoCents: Cents;
  retencaoSofridaCents: Cents;
  invoiceCount: number;
}

/** Totals for any date window; the building block for every period report. */
export function periodTotals(invoices: readonly Invoice[], start: IsoDate, end: IsoDate): PeriodTotals {
  let baseCents = 0;
  let ivaLiquidadoCents = 0;
  let retencaoSofridaCents = 0;
  let invoiceCount = 0;
  for (const invoice of invoices) {
    if (!inRange(invoice, start, end)) continue;
    const totals = invoiceTotals(invoice);
    baseCents += totals.baseCents;
    ivaLiquidadoCents += totals.ivaCents;
    retencaoSofridaCents += totals.retentionCents;
    invoiceCount += 1;
  }
  return { baseCents, ivaLiquidadoCents, retencaoSofridaCents, invoiceCount };
}

export interface SsContributionEstimate {
  serviceIncomeCents: Cents;
  relevantIncomeShareBp: BasisPoints;
  relevantIncomeCents: Cents;
  contributionRateBp: BasisPoints;
  contributionCents: Cents;
  /** The contribution is settled in three monthly instalments within the quarter. */
  instalmentsCents: Cents[];
  limitations: string[];
}

/**
 * Segurança Social for one quarter, for a worker whose income is services:
 * relevant income = share of gross service income, contribution = rate applied
 * to it, paid in three monthly instalments.
 */
export function estimateSocialSecurity(
  pack: RulePack,
  serviceIncomeCents: Cents,
  instalments = 3,
): SsContributionEstimate {
  const limitations: string[] = [];
  const shareBp = pack.constants.socialSecurity.relevantIncomeShareServices;
  const rateBp = pack.constants.socialSecurity.contributionRate;

  if (shareBp === null) limitations.push('A percentagem de rendimento relevante não está no pacote de regras.');
  if (rateBp === null) limitations.push('A taxa contributiva não está no pacote de regras.');
  if (pack.constants.socialSecurity.iasMonthly === null) {
    limitations.push(
      'O valor do IAS não está no pacote de regras: não é possível verificar se a base de incidência ' +
        'atinge o mínimo ou o máximo legal.',
    );
  }

  const relevantIncomeCents = shareBp === null ? 0 : applyBp(serviceIncomeCents, shareBp);
  const contributionCents = rateBp === null ? 0 : applyBp(relevantIncomeCents, rateBp);

  return {
    serviceIncomeCents,
    relevantIncomeShareBp: shareBp ?? 0,
    relevantIncomeCents,
    contributionRateBp: rateBp ?? 0,
    contributionCents,
    instalmentsCents: splitEvenly(contributionCents, instalments),
    limitations,
  };
}

export interface QuarterReport {
  year: number;
  quarter: number;
  start: IsoDate;
  end: IsoDate;
  totals: PeriodTotals;
  socialSecurity: SsContributionEstimate;
}

export function quarterReport(
  pack: RulePack,
  invoices: readonly Invoice[],
  year: number,
  quarter: number,
): QuarterReport {
  const range = quarterRange(year, quarter);
  const totals = periodTotals(invoices, range.start, range.end);
  return {
    year,
    quarter,
    start: range.start,
    end: range.end,
    totals,
    socialSecurity: estimateSocialSecurity(pack, totals.baseCents),
  };
}

export interface IrsSimplifiedEstimate {
  grossServiceIncomeCents: Cents;
  coefficientBp: BasisPoints;
  taxableFromCoefficientCents: Cents;
  /** Eligible expenses the taxpayer can document and has communicated to the AT. */
  eligibleExpensesCents: Cents;
  /**
   * 15% of gross service income: the reference the law compares expenses
   * against, per art. 31.º n.º 13 CIRS.
   */
  documentedExpensesReferenceCents: Cents | null;
  /**
   * The positive difference between that reference and the documented expenses.
   * This is ADDED to the taxable income, not subtracted from it.
   */
  additionToTaxableIncomeCents: Cents | null;
  taxableIncomeCents: Cents | null;
  withholdingCents: Cents;
  assumptions: string[];
  limitations: string[];
}

/**
 * The taxable base under the simplified regime.
 *
 * The mechanism is easy to get backwards, which is why this function states it
 * out loud: art. 31.º n.º 13 CIRS does NOT deduct 15% of your expenses. It
 * PRESUMES expenses equal to 15% of your gross service income, and then adds back
 * the positive difference between that reference and the expenses you can
 * actually document. Document less than 15% and you pay on more; document more
 * and nothing is added.
 *
 * Applying the IRS brackets needs the art. 68.º table for the year, so this
 * deliberately stops at taxable income. Without an income figure the honest
 * output is a base, not a tax.
 */
export function estimateIrsSimplifiedBase(
  pack: RulePack,
  profile: TaxProfile,
  invoices: readonly Invoice[],
  year: number,
  eligibleExpensesCents: Cents,
): IrsSimplifiedEstimate {
  const start: IsoDate = `${year}-01-01`;
  const end: IsoDate = `${year}-12-31`;
  const totals = periodTotals(invoices, start, end);
  const coefficientBp = profile.irs.coefficientBp;
  const rateBp = pack.constants.irs.simplifiedRegime.documentedExpensesDeductionRate;

  const limitations: string[] = [
    'Estimativa para o regime simplificado. Não aplica as taxas do art. 68.º do CIRS: o rendimento ' +
      'tributável apresentado não é o imposto a pagar.',
    'Não aplica deduções à coleta nem tributações autónomas. As retenções sofridas são creditadas ' +
      'no IRS final, não descontadas neste rendimento tributável.',
    'Pressupõe que todas as faturas do ano são rendimento da categoria B e que as despesas indicadas ' +
      'são elegíveis nos termos do art. 31.º do CIRS.',
  ];

  const taxableFromCoefficientCents = applyBp(totals.baseCents, coefficientBp);

  if (rateBp === null) {
    limitations.push(
      'A percentagem de referência das despesas documentadas (art. 31.º n.º 13 do CIRS) não está no ' +
        'pacote de regras: o rendimento tributável não foi calculado.',
    );
    return {
      grossServiceIncomeCents: totals.baseCents,
      coefficientBp,
      taxableFromCoefficientCents,
      eligibleExpensesCents,
      documentedExpensesReferenceCents: null,
      additionToTaxableIncomeCents: null,
      taxableIncomeCents: null,
      withholdingCents: totals.retencaoSofridaCents,
      assumptions: [`Coeficiente aplicado: ${coefficientBp} pontos base, definido no perfil.`],
      limitations,
    };
  }

  const referenceCents = applyBp(totals.baseCents, rateBp);
  const additionToTaxableIncomeCents = Math.max(0, referenceCents - eligibleExpensesCents);
  const taxableIncomeCents = taxableFromCoefficientCents + additionToTaxableIncomeCents;

  return {
    grossServiceIncomeCents: totals.baseCents,
    coefficientBp,
    taxableFromCoefficientCents,
    eligibleExpensesCents,
    documentedExpensesReferenceCents: referenceCents,
    additionToTaxableIncomeCents,
    taxableIncomeCents,
    withholdingCents: totals.retencaoSofridaCents,
    assumptions: [
      `Coeficiente aplicado ao rendimento de serviços: ${coefficientBp} pontos base, definido no perfil.`,
      `Referência de despesas: ${rateBp / 100}% dos rendimentos brutos de serviços = ${formatEur(referenceCents)}.`,
      `Despesas elegíveis documentadas: ${formatEur(eligibleExpensesCents)}.`,
    ],
    limitations,
  };
}

export interface ReserveEstimate {
  lowCents: Cents;
  highCents: Cents;
  basis: string[];
  limitations: string[];
}

/**
 * How much cash to keep aside. Returns a RANGE on purpose: the point of this
 * number is to be a reserve, not a prediction, and a single figure would imply a
 * precision that the available data does not support.
 */
export function estimateReserve(
  pack: RulePack,
  invoices: readonly Invoice[],
  asOf: IsoDate,
  lowBp = 2500,
  highBp = 3500,
): ReserveEstimate {
  const start: IsoDate = `${asOf.slice(0, 4)}-01-01`;
  const totals = periodTotals(invoices, start, asOf);
  const limitations: string[] = [
    `Reserva calculada sobre ${formatEur(totals.baseCents)} de rendimento faturado até ${asOf}.`,
    'Intervalo indicativo: não substitui o cálculo final do IRS nem o apuramento da Segurança Social.',
  ];
  if (pack.constants.irs.simplifiedRegime.documentedExpensesDeductionRate === null) {
    limitations.push(
      'A percentagem de referência das despesas documentadas (art. 31.º n.º 13 do CIRS) é ' +
        'desconhecida, o que alarga o intervalo.',
    );
  }
  return {
    lowCents: applyBp(totals.baseCents, lowBp),
    highCents: applyBp(totals.baseCents, highBp),
    basis: [`${lowBp / 100}% a ${highBp / 100}% do rendimento faturado no ano`],
    limitations,
  };
}
