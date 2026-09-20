/**
 * The domain model.
 *
 * Everything the application knows about the taxpayer, about the law, and about
 * what is due. Two design commitments are visible here:
 *
 *   - The LAW IS DATA. Rules, rates, deadlines and their citations live in
 *     versioned rule packs (`src/rules/<jurisdiction>/<year>.json`), never in
 *     `if` statements. When the law changes, a data file changes and every
 *     number keeps its provenance.
 *   - The ENGINE IS DETERMINISTIC. Given a profile and a rule pack it produces
 *     the same obligations every time, with no network access and no model in
 *     the loop.
 */

import type { BasisPoints, Cents } from './money.ts';

/** Calendar date in ISO form, `YYYY-MM-DD`, always interpreted in Europe/Lisbon. */
export type IsoDate = string;

export type IvaRegime = 'isento_art53' | 'mensal' | 'trimestral';
export type IrsRegime = 'simplificado' | 'organizada';
export type ObligationKind = 'declare' | 'pay' | 'communicate' | 'save' | 'review' | 'submit';
export type Authority = 'AT' | 'SS' | 'OTHER';
/**
 * How well established a rule is.
 *
 * `partial` exists because tax reality is not binary: a deadline can be
 * confirmed from the authority's published calendar while the article that
 * establishes it has not been read directly. Saying so is more useful than
 * forcing the rule into "verified" or discarding it.
 */
export type Verification = 'verified' | 'partial' | 'unverified' | 'stale';

/**
 * How confident the engine is that an obligation applies to this taxpayer.
 *
 * `untracked` is the state of a deadline that fell before the user started
 * keeping records here. It is not "late" and must never be nagged about: a tool
 * adopted in September did not miss the previous January.
 */
export type InstanceStatus =
  | 'future'
  | 'due_soon'
  | 'overdue'
  | 'done'
  | 'untracked'
  | 'not_applicable';

// ---------------------------------------------------------------------------
// Taxpayer profile
// ---------------------------------------------------------------------------

export type CaeRole = 'principal' | 'secondary';

export interface CaeEntry {
  code: string;
  description: string;
  role: CaeRole;
}

export interface ActivityProfile {
  /** CIRS category B — independent activity. Without this there is no recibo verde. */
  categoryB: boolean;
  startDate: IsoDate;
  cae: CaeEntry[];
  /**
   * Turnover in national territory in the previous civil year, as declared by the
   * user. NEVER defaulted: the art. 53.º CIVA exemption depends on this figure,
   * and a tool that guesses it cannot be trusted to evaluate the exemption.
   */
  turnoverPreviousYearCents?: Cents;
  /** The user's own estimate for the current civil year, also user-supplied. */
  turnoverCurrentYearExpectedCents?: Cents;
  /** Intra-community supplies of services, triggering reverse charge and recapitulativa. */
  intraCommunityOperations: boolean;
  /** Supplies of services to clients outside the European Union. */
  exports: boolean;
  hasEmployees: boolean;
  usesCertifiedInvoicingSoftware: boolean;
  usesAtWebservice: boolean;
}

export interface TaxProfile {
  nif: string;
  name: string;
  residentPT: boolean;
  isCompany: boolean;
  activity: ActivityProfile;
  /**
   * The date from which this application is expected to have tracked
   * obligations. Everything due before it is history, not a missed deadline.
   */
  trackingStart?: IsoDate;
  iva: {
    regime: IvaRegime;
  };
  irs: {
    regime: IrsRegime;
    /** Simplified-regime coefficient in basis points as applied to gross service income. */
    coefficientBp: BasisPoints;
  };
  ss: {
    startupExemptionActive: boolean;
    firstActivityDate?: IsoDate;
  };
}

// ---------------------------------------------------------------------------
// Conditions — a tiny declarative predicate language, deliberately not code
// ---------------------------------------------------------------------------

export type ConditionField =
  | 'iva.regime'
  | 'irs.regime'
  | 'profile.residentPT'
  | 'profile.isCompany'
  | 'activity.categoryB'
  | 'activity.intraCommunityOperations'
  | 'activity.exports'
  | 'activity.hasEmployees'
  | 'activity.usesCertifiedInvoicingSoftware'
  | 'activity.usesAtWebservice'
  | 'ss.startupExemptionActive';

export type ConditionOp = 'eq' | 'ne' | 'in' | 'gt' | 'gte' | 'lt' | 'lte' | 'exists';

export type ConditionValue = string | number | boolean | ReadonlyArray<string | number | boolean>;

export interface Condition {
  field: ConditionField;
  op: ConditionOp;
  value?: ConditionValue;
}

// ---------------------------------------------------------------------------
// Rule pack
// ---------------------------------------------------------------------------

export interface RuleSource {
  id: string;
  tier: 'official' | 'secondary';
  authority: string;
  title: string;
  url: string;
  retrievedAt: string;
}

export type PeriodicityType = 'monthly' | 'quarterly' | 'annual' | 'continuous' | 'event_driven';

export interface Periodicity {
  type: PeriodicityType;
  /** How many months after the end of the period the obligation falls due. */
  lagMonths?: number;
}

/**
 * How a deadline is described when the authority's published calendar does not
 * settle it. The vocabulary is wider than the engine needs because real
 * obligations are messier than "the 20th": a filing window, a deadline that is
 * the last day of a month, a payment window, or a duty triggered by an event.
 */
export interface DeadlineRule {
  kind:
    | 'day_of_month'
    | 'fixed_date'
    /** A window, e.g. Modelo 3: 1 April to 30 June. The deadline is the end. */
    | 'range'
    | 'last_day_of_month'
    | 'month_of_year'
    /** A payment window, e.g. Segurança Social: the 10th to the 20th. */
    | 'day_range'
    | 'days_after_event'
    | 'retention_years';
  /** Day within the month, when the rule fixes one. */
  day?: number;
  /** 1-12: the due month for annual rules, or the window start for `range`. */
  month?: number;
  /** Window start month, for `range` (e.g. Modelo 3: 1 April to 30 June). */
  monthFrom?: number;
  /** Window end month, for `range`. */
  monthTo?: number;
  /** Window end day, for `range`. */
  dayTo?: number;
  /** Window start day, for `day_range`. */
  dayFrom?: number;
  /** Days after a triggering event, for `days_after_event`. */
  days?: number;
  /** Retention period in years for IVA documents, for `retention_years`. */
  iva?: number;
  /** Retention period in years for IRS documents, for `retention_years`. */
  irs?: number;
  note?: string;
}

export interface DateExtension {
  reason: string;
  despacho?: string;
  url?: string;
}

/** A date published by the authority for a specific year, which always wins. */
export interface OfficialDate {
  month: number;
  day: number;
  /** The period the date refers to, when the authority's table makes it explicit. */
  periodLabel?: string;
  extension?: DateExtension;
  note?: string;
}

export interface ObligationRule {
  id: string;
  kind: ObligationKind;
  authority: Authority;
  tax: string;
  title: string;
  /** Empty array means always applicable. */
  appliesWhen: Condition[];
  periodicity: Periodicity;
  deadlineRule: DeadlineRule | null;
  /** Normalised from `officialDates2026` or `officialDates: { "2026": [...] }`. */
  officialDates: Record<string, OfficialDate[]>;
  legalBasis: string[];
  sourceIds: string[];
  portalUrl?: string | null;
  documentsToKeep: string[];
  penaltyNote?: string | null;
  verification: Verification;
  verifyNote?: string | null;
}

export interface IvaConstants {
  rates: {
    normal: BasisPoints | null;
    intermediaire: BasisPoints | null;
    reduzida: BasisPoints | null;
  };
  art53: {
    ceiling: Cents | null;
    forbidsExportOperations: boolean;
    requiresNationalDomicile: boolean;
    annualisationInStartYear: boolean;
    sourceIds: string[];
  };
  periodicRegime: {
    quarterlyCeiling: Cents | null;
    monthlyAbove: Cents | null;
    sourceIds: string[];
  };
}

export interface IrsConstants {
  simplifiedRegime: {
    ceilingForOrganisedAccounting: Cents | null;
    coefficients: {
      servicesProfessionalTable4: BasisPoints | null;
      servicesOther: BasisPoints | null;
    };
    documentedExpensesDeductionRate: BasisPoints | null;
    documentedExpensesDeductionCap: Cents | null;
    sourceIds: string[];
  };
  withholding: {
    professionalServicesResident: BasisPoints | null;
    otherCategoryBResident: BasisPoints | null;
    nonResident: BasisPoints | null;
    sourceIds: string[];
  };
}

export interface StartupReduction {
  months: number;
  /** Share of the normal contribution still payable, in basis points (5000 = 50%). */
  factorBp: BasisPoints;
}

export interface SocialSecurityConstants {
  contributionRate: BasisPoints | null;
  relevantIncomeShareServices: BasisPoints | null;
  relevantIncomeShareGoods: BasisPoints | null;
  /** Multiples of the IAS; these are ratios, not money, so they may be fractional. */
  baseMinMultipleIas: number | null;
  baseMaxMultipleIas: number | null;
  iasMonthly: Cents | null;
  startupExemptionMonths: number | null;
  startupReductions: StartupReduction[];
  sourceIds: string[];
}

export interface RuleConstants {
  iva: IvaConstants;
  irs: IrsConstants;
  socialSecurity: SocialSecurityConstants;
}

/**
 * Where a single constant came from.
 *
 * This exists because the assistant is allowed to PROPOSE a new value for a
 * variable that changes over time, and is never allowed to make it true. A value
 * the assistant proposed is recorded as `ai-proposed` / `unverified` until a
 * human confirms it against the cited source, and the interface keeps showing it
 * as unconfirmed until then.
 */
export interface ConstantProvenance {
  source: 'human' | 'ai-proposed';
  proposedAt: IsoDate;
  sourceUrl: string | null;
  status: 'verified' | 'unverified';
  note?: string;
}

export interface RulePack {
  jurisdiction: 'PT';
  year: number;
  packVersion: string;
  generatedAt: IsoDate;
  units: { money: 'EUR_cents'; rate: 'basis_points' };
  disclaimer: string;
  sources: RuleSource[];
  constants: RuleConstants;
  /** Keyed by dotted constant path, e.g. `socialSecurity.iasMonthly`. */
  constantProvenance?: Record<string, ConstantProvenance>;
  obligations: ObligationRule[];
  todo: string[];
}

export interface PackProblem {
  level: 'error' | 'warning';
  path: string;
  message: string;
}

export interface LoadedPack {
  pack: RulePack;
  problems: PackProblem[];
  checksum: string;
  /** Resolved relative to the process, for display. */
  path: string;
}

// ---------------------------------------------------------------------------
// Materialised obligations
// ---------------------------------------------------------------------------

export interface ObligationInstance {
  /** Stable identity: `<ruleId>@<dueDate>`. */
  id: string;
  ruleId: string;
  kind: ObligationKind;
  authority: Authority;
  tax: string;
  title: string;
  /** Human label for the period the obligation covers, e.g. "3.º trimestre 2026". */
  periodLabel: string;
  periodStart: IsoDate | null;
  periodEnd: IsoDate | null;
  dueDate: IsoDate;
  /** Whether the date came from the authority's published table or was derived. */
  dueDateSource: 'official' | 'derived';
  adjustedForWeekend: boolean;
  /** True when the authority has not yet published dates for this year. */
  provisional: boolean;
  status: InstanceStatus;
  notApplicableReason: string | null;
  verification: Verification;
  verifyNote: string | null;
  legalBasis: string[];
  sourceIds: string[];
  portalUrl: string | null;
  documentsToKeep: string[];
  penaltyNote: string | null;
  /** Places where derived logic and the published table do not agree. */
  discrepancies: string[];
  /** Only present for obligations that can carry an amount. */
  checklists: ChecklistItem[];
}

export interface ChecklistItem {
  label: string;
  done: boolean;
}

export interface AgendaOptions {
  year: number;
  today: IsoDate;
  /** Days ahead that count as "due soon". */
  dueSoonDays?: number;
  /** Ids of obligations the user has marked as handled. */
  completedIds?: string[];
  /**
   * Only obligations due on or after this date are treated as the user's
   * responsibility. Earlier ones are reported as `untracked`.
   */
  trackFrom?: IsoDate;
  /**
   * Public holidays for the year. Never guessed by the engine: an app that
   * guesses holidays is an app that misses a deadline.
   */
  holidays?: IsoDate[];
}
