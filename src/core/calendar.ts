/**
 * The obligation engine.
 *
 * Turns a rule pack plus a taxpayer profile into the concrete list of things
 * that are due in a given year. Three properties matter more than anything else:
 *
 *   PUBLISHED DATES BEAT DERIVED DATES. If the authority has published a date
 *   for the year, that date is used, even when it disagrees with the general
 *   legal rule (deadlines move for weekends, holidays and despachos de
 *   prorrogação). When the two cannot be reconciled, the instance carries a
 *   `discrepancy` naming both, so a human checks instead of trusting.
 *
 *   DERIVATION KNOWS THE SHAPES REAL DEADLINES HAVE. Some obligations are a day
 *   of the month, some are the last day of a month, some are a window whose end
 *   is the deadline, some are a payment window, and some are triggered by an
 *   event rather than a date. Each shape is derived according to what it
 *   actually means, and the derivation says when it had to assume something.
 *
 *   NOTHING IS INVENTED. When next year's calendar is not published yet, the
 *   derived dates are marked `provisional` and the app says so.
 */

import { appliesTo, explainFailure } from './conditions.ts';
import {
  addMonths,
  compareIso,
  daysBetween,
  daysInMonth,
  formatPtDate,
  formatPtMonthYear,
  isWeekend,
  makeIso,
  monthRange,
  nextBusinessDay,
  parseIso,
  quarterLabel,
  quarterRange,
} from './dates.ts';
import type {
  AgendaOptions,
  ChecklistItem,
  ConditionField,
  DeadlineRule,
  IsoDate,
  ObligationInstance,
  ObligationRule,
  OfficialDate,
  RulePack,
  TaxProfile,
} from './types.ts';

/** Portuguese labels for the condition vocabulary, used in "não aplicável" reasons. */
const FIELD_LABELS: Partial<Record<ConditionField, string>> = {
  'iva.regime': 'regime de IVA',
  'irs.regime': 'regime de IRS',
  'profile.residentPT': 'residência fiscal em Portugal',
  'profile.isCompany': 'ser uma sociedade',
  'activity.categoryB': 'ter atividade aberta na categoria B',
  'activity.intraCommunityOperations': 'efetuar operações intracomunitárias',
  'activity.intraCommunityOperationsAbove50k':
    'ter operações intracomunitárias acima de 50 000 EUR num trimestre',
  'activity.exports': 'efetuar exportações',
  'activity.hasEmployees': 'ter trabalhadores por conta de outrem',
  'activity.usesCertifiedInvoicingSoftware': 'usar software de faturação certificado',
  'activity.usesAtWebservice': 'comunicar faturas pelo webservice da AT',
  'ss.startupExemptionActive': 'estar no período de isenção de início de atividade',
};

interface PeriodSeed {
  periodStart: IsoDate;
  periodEnd: IsoDate;
  periodLabel: string;
  dueYear: number;
  dueMonth: number;
  /** True when the seed came straight from a published date and needs no checking. */
  pinned?: boolean;
  /** An explanation attached to a pinned date, e.g. a merged weekend shift. */
  preNote?: string;
}

// ---------------------------------------------------------------------------
// Published-date clustering
// ---------------------------------------------------------------------------

/** Rough day distance between two month/day pairs inside one year. */
function monthDayDistance(a: OfficialDate, b: OfficialDate): number {
  return (b.month - a.month) * 31 + (b.day - a.day);
}

/**
 * Several published dates for ONE annual obligation can mean two different
 * things: instalments paid months apart (IRS pagamentos por conta is published
 * as three dates), or one deadline plus the business-day shift that follows it
 * (the e-fatura validation publishes 28 February AND 2 March 2026, because
 * 28 February is a Saturday).
 *
 * Dates within a week of each other are therefore treated as a single deadline,
 * keeping the later — the last day you may actually comply. Dates further apart
 * are instalments. Without this, a weekend shift would be shown as a second
 * payment the user does not owe.
 */
export function clusterPublishedDates(dates: readonly OfficialDate[]): OfficialDate[] {
  const sorted = [...dates].sort((a, b) => a.month - b.month || a.day - b.day);
  const clusters: OfficialDate[] = [];
  for (const date of sorted) {
    const previous = clusters[clusters.length - 1];
    if (previous !== undefined && monthDayDistance(previous, date) <= 7) {
      clusters[clusters.length - 1] = date;
    } else {
      clusters.push(date);
    }
  }
  return clusters;
}

/** The months a rule fell due in, taken from the most recent published year. */
function patternMonths(rule: ObligationRule): number[] {
  const years = Object.keys(rule.officialDates)
    .map(Number)
    .filter((year) => Number.isInteger(year))
    .sort((a, b) => b - a);
  const latest = years[0];
  if (latest === undefined) return [];
  return clusterPublishedDates(rule.officialDates[String(latest)] ?? []).map((date) => date.month);
}

// ---------------------------------------------------------------------------
// Deriving the legal date from the deadline rule
// ---------------------------------------------------------------------------

interface LegalDate {
  day: number | null;
  notes: string[];
}

/**
 * What the rule says about the day, given the month the period falls due in.
 * Returns `null` for the day when the rule genuinely does not fix one.
 */
function resolveLegalDay(deadline: DeadlineRule | null, dueYear: number, dueMonth: number): LegalDate {
  if (deadline === null) return { day: null, notes: [] };
  switch (deadline.kind) {
    case 'day_of_month':
    case 'fixed_date':
      return { day: deadline.day ?? null, notes: [] };
    case 'range': {
      const notes: string[] = [];
      if (
        deadline.monthFrom !== undefined &&
        deadline.dayFrom !== undefined &&
        deadline.monthTo !== undefined &&
        deadline.dayTo !== undefined
      ) {
        notes.push(
          `Janela de cumprimento: de ${formatPtDate(
            makeIso(dueYear, deadline.monthFrom, deadline.dayFrom),
          )} a ${formatPtDate(makeIso(dueYear, deadline.monthTo, deadline.dayTo))}. ` +
            'A data apresentada é o fim da janela.',
        );
      }
      return { day: deadline.dayTo ?? deadline.day ?? null, notes };
    }
    case 'last_day_of_month':
    case 'month_of_year':
      return {
        day: daysInMonth(dueYear, dueMonth),
        notes: ['O pacote de regras não fixa o dia; foi usado o último dia do mês.'],
      };
    case 'day_range':
      return {
        day: deadline.dayTo ?? null,
        notes:
          deadline.dayFrom !== undefined && deadline.dayTo !== undefined
            ? [
                `Janela de pagamento entre os dias ${deadline.dayFrom} e ${deadline.dayTo}. ` +
                  'A data apresentada é o fim da janela.',
              ]
            : [],
      };
    case 'days_after_event':
      return {
        day: null,
        notes: ['Obrigação dependente de evento: não tem prazo fixo no calendário.'],
      };
    case 'retention_years':
      return { day: null, notes: [] };
  }
}

/** The months an annual obligation falls due in, when the year is unpublished. */
function annualDueMonths(rule: ObligationRule): number[] {
  const deadline = rule.deadlineRule;
  const explicit = deadline?.month ?? deadline?.monthTo;
  if (explicit !== undefined) return [explicit];
  const pattern = patternMonths(rule);
  return pattern.length > 0 ? pattern : [6];
}

// ---------------------------------------------------------------------------
// Period enumeration
// ---------------------------------------------------------------------------

function monthSeeds(rule: ObligationRule, targetYear: number): PeriodSeed[] {
  const lag = rule.periodicity.lagMonths ?? 2;
  const seeds: PeriodSeed[] = [];
  for (let month = 1; month <= 12; month += 1) {
    const range = monthRange(targetYear, month);
    const shifted = parseIso(addMonths(range.start, lag));
    seeds.push({
      periodStart: range.start,
      periodEnd: range.end,
      periodLabel: formatPtMonthYear(month, targetYear),
      dueYear: shifted.year,
      dueMonth: shifted.month,
    });
  }
  return seeds;
}

function quarterSeeds(rule: ObligationRule, targetYear: number): PeriodSeed[] {
  const lag = rule.periodicity.lagMonths ?? 2;
  const seeds: PeriodSeed[] = [];
  for (let quarter = 1; quarter <= 4; quarter += 1) {
    const range = quarterRange(targetYear, quarter);
    const shifted = parseIso(addMonths(range.end, lag));
    seeds.push({
      periodStart: range.start,
      periodEnd: range.end,
      periodLabel: quarterLabel(targetYear, quarter),
      dueYear: shifted.year,
      dueMonth: shifted.month,
    });
  }
  return seeds;
}

/**
 * An annual obligation that falls due in `targetYear` reports on the PREVIOUS
 * civil year: the Modelo 3 delivered in June 2026 reports 2025 income.
 *
 * Instalments are handled here: when the published calendar holds several
 * dates months apart, each becomes its own instance, because each one is
 * separately payable and separately late.
 */
function annualSeeds(rule: ObligationRule, targetYear: number): PeriodSeed[] {
  const periodYear = targetYear - 1;
  const periodStart = makeIso(periodYear, 1, 1);
  const periodEnd = makeIso(periodYear, 12, 31);
  const published = rule.officialDates[String(targetYear)] ?? [];
  const clusters = clusterPublishedDates(published);

  if (clusters.length > 1) {
    return clusters.map((entry, index) => ({
      periodStart,
      periodEnd,
      periodLabel:
        entry.periodLabel ??
        (clusters.length === 3
          ? `${index + 1}.º pagamento por conta (rendimentos de ${periodYear})`
          : `prestação ${index + 1} de ${clusters.length} (rendimentos de ${periodYear})`),
      dueYear: targetYear,
      dueMonth: entry.month,
      pinned: true,
    }));
  }

  const cluster = clusters[0];
  if (cluster !== undefined) {
    const merged = published.filter((date) => monthDayDistance(date, cluster) <= 7 && date !== cluster);
    if (merged.length > 0) {
      const mergedText = merged
        .map((date) => formatPtDate(makeIso(targetYear, date.month, date.day)))
        .join(', ');
      return [
        {
          periodStart,
          periodEnd,
          periodLabel: `rendimentos de ${periodYear}`,
          dueYear: targetYear,
          dueMonth: cluster.month,
          pinned: true,
          preNote:
            `Prazo legal de ${mergedText}; a AT admite o cumprimento até ` +
            `${formatPtDate(makeIso(targetYear, cluster.month, cluster.day))}.`,
        },
      ];
    }
  }

  return annualDueMonths(rule).map((month, index, months) => ({
    periodStart,
    periodEnd,
    periodLabel:
      months.length > 1
        ? `${index + 1}.ª prestação (rendimentos de ${periodYear})`
        : `rendimentos de ${periodYear}`,
    dueYear: targetYear,
    dueMonth: month,
  }));
}

function continuousSeed(targetYear: number): PeriodSeed {
  return {
    periodStart: makeIso(targetYear, 1, 1),
    periodEnd: makeIso(targetYear, 12, 31),
    periodLabel: `todo o ano ${targetYear}`,
    dueYear: targetYear,
    dueMonth: 12,
  };
}

function enumeratePeriods(rule: ObligationRule, targetYear: number): PeriodSeed[] {
  switch (rule.periodicity.type) {
    case 'monthly':
      return monthSeeds(rule, targetYear);
    case 'quarterly':
      return quarterSeeds(rule, targetYear);
    case 'annual':
      return annualSeeds(rule, targetYear);
    case 'continuous':
      return [continuousSeed(targetYear)];
    // An event-driven duty has no date. It is deliberately absent from the
    // agenda rather than given an invented one; `vnfin doctor` lists it.
    case 'event_driven':
      return [];
  }
}

// ---------------------------------------------------------------------------
// Resolving one due date
// ---------------------------------------------------------------------------

interface ResolvedDate {
  dueDate: IsoDate;
  source: 'official' | 'derived';
  adjustedForWeekend: boolean;
  discrepancies: string[];
}

function resolveDueDate(
  rule: ObligationRule,
  seed: PeriodSeed,
  holidays: readonly IsoDate[],
): ResolvedDate {
  const discrepancies: string[] = [];
  const published = rule.officialDates[String(seed.dueYear)] ?? [];
  const match = published.find((entry) => entry.month === seed.dueMonth) ?? null;
  const clampDay = (day: number): number => Math.min(day, daysInMonth(seed.dueYear, seed.dueMonth));

  if (seed.pinned === true && match !== null) {
    return {
      dueDate: makeIso(seed.dueYear, seed.dueMonth, clampDay(match.day)),
      source: 'official',
      adjustedForWeekend: false,
      discrepancies: [
        ...(match.note === undefined ? [] : [match.note]),
        ...(seed.preNote === undefined ? [] : [seed.preNote]),
      ],
    };
  }

  const legal = resolveLegalDay(rule.deadlineRule, seed.dueYear, seed.dueMonth);
  const lastDayOfMonth = daysInMonth(seed.dueYear, seed.dueMonth);

  if (legal.day === null) {
    if (match !== null) {
      return {
        dueDate: makeIso(seed.dueYear, seed.dueMonth, clampDay(match.day)),
        source: 'official',
        adjustedForWeekend: false,
        discrepancies: match.note === undefined ? [] : [match.note],
      };
    }
    return {
      dueDate: makeIso(seed.dueYear, seed.dueMonth, lastDayOfMonth),
      source: 'derived',
      adjustedForWeekend: false,
      discrepancies: [
        ...legal.notes,
        'Sem dia fixado e sem data publicada: foi usado o último dia do mês como data provisória.',
      ],
    };
  }

  const legalDate = makeIso(seed.dueYear, seed.dueMonth, clampDay(legal.day));
  const businessDate = nextBusinessDay(legalDate, holidays);

  if (match === null) {
    if (published.length > 0) {
      const publishedText = clusterPublishedDates(published)
        .map((entry) => formatPtDate(makeIso(seed.dueYear, entry.month, entry.day)))
        .join(', ');
      discrepancies.push(
        `A Agenda Fiscal de ${seed.dueYear} não publica qualquer prazo desta obrigação para ` +
          `${formatPtMonthYear(seed.dueMonth, seed.dueYear)}. Datas publicadas nesse ano: ${publishedText}. ` +
          'A data apresentada foi calculada pela regra geral e deve ser confirmada na fonte citada.',
      );
    }
    return {
      dueDate: businessDate,
      source: 'derived',
      adjustedForWeekend: businessDate !== legalDate,
      discrepancies: [...legal.notes, ...discrepancies],
    };
  }

  const publishedDate = makeIso(seed.dueYear, seed.dueMonth, clampDay(match.day));
  const isPlainRule = publishedDate === legalDate;
  const isWeekendShift = publishedDate === businessDate;

  if (!isPlainRule && !isWeekendShift) {
    discrepancies.push(
      `Data publicada (${formatPtDate(publishedDate)}) não coincide com a regra geral ` +
        `(${formatPtDate(legalDate)}) nem com o adiamento para o dia útil seguinte ` +
        `(${formatPtDate(businessDate)}). Foi usada a data publicada.`,
    );
  }
  if (isWeekendShift && !isPlainRule && isWeekend(legalDate)) {
    discrepancies.push(
      `A data legal (${formatPtDate(legalDate)}) cai em fim de semana; a AT admite o cumprimento ` +
        `até ${formatPtDate(businessDate)}.`,
    );
  }
  if (match.extension?.reason !== undefined) {
    discrepancies.push(`Prorrogação aplicada: ${match.extension.reason}.`);
  }
  if (match.note !== undefined && !discrepancies.includes(match.note)) {
    discrepancies.push(match.note);
  }
  for (const note of legal.notes) {
    if (!discrepancies.includes(note)) discrepancies.push(note);
  }

  return {
    dueDate: publishedDate,
    source: 'official',
    adjustedForWeekend: isWeekendShift && !isPlainRule,
    discrepancies,
  };
}

function checklistFor(rule: ObligationRule): ChecklistItem[] {
  return rule.documentsToKeep.map((label) => ({ label, done: false }));
}

// ---------------------------------------------------------------------------
// The agenda
// ---------------------------------------------------------------------------

/**
 * Build the agenda for one year. Pure: same inputs, same output, no clock, no
 * network, no model.
 */
export function buildAgenda(
  pack: RulePack,
  profile: TaxProfile,
  options: AgendaOptions,
): ObligationInstance[] {
  const { year, today } = options;
  const dueSoonDays = options.dueSoonDays ?? 30;
  const completed = new Set(options.completedIds ?? []);
  const holidays = options.holidays ?? [];
  const trackFrom = options.trackFrom;
  const instances: ObligationInstance[] = [];

  for (const rule of pack.obligations) {
    if (!appliesTo(profile, rule.appliesWhen)) {
      instances.push({
        id: `${rule.id}@na`,
        ruleId: rule.id,
        kind: rule.kind,
        authority: rule.authority,
        tax: rule.tax,
        title: rule.title,
        periodLabel: '—',
        periodStart: null,
        periodEnd: null,
        dueDate: makeIso(year, 12, 31),
        dueDateSource: 'derived',
        adjustedForWeekend: false,
        provisional: false,
        status: 'not_applicable',
        notApplicableReason: explainFailure(profile, rule.appliesWhen, FIELD_LABELS),
        verification: rule.verification,
        verifyNote: rule.verifyNote ?? null,
        legalBasis: rule.legalBasis,
        sourceIds: rule.sourceIds,
        portalUrl: rule.portalUrl ?? null,
        documentsToKeep: rule.documentsToKeep,
        penaltyNote: rule.penaltyNote ?? null,
        discrepancies: [],
        checklists: [],
      });
      continue;
    }

    for (const seed of enumeratePeriods(rule, year)) {
      const resolved = resolveDueDate(rule, seed, holidays);
      const dueDate = resolved.dueDate;
      const provisional = seed.dueYear !== pack.year;

      let status: ObligationInstance['status'];
      const id = `${rule.id}@${dueDate}`;
      if (completed.has(id)) {
        status = 'done';
      } else {
        const delta = daysBetween(today, dueDate);
        status = delta < 0 ? 'overdue' : delta <= dueSoonDays ? 'due_soon' : 'future';
      }
      // A saving/keeping obligation is never "late": it is a standing duty.
      if (rule.kind === 'save' && status !== 'done') status = 'future';
      // Anything due before the user started tracking is history, not a miss.
      if (trackFrom !== undefined && status !== 'done' && dueDate < trackFrom) {
        status = 'untracked';
      }

      instances.push({
        id,
        ruleId: rule.id,
        kind: rule.kind,
        authority: rule.authority,
        tax: rule.tax,
        title: rule.title,
        periodLabel: seed.periodLabel,
        periodStart: seed.periodStart,
        periodEnd: seed.periodEnd,
        dueDate,
        dueDateSource: resolved.source,
        adjustedForWeekend: resolved.adjustedForWeekend,
        provisional,
        status,
        notApplicableReason: null,
        verification: rule.verification,
        verifyNote: rule.verifyNote ?? null,
        legalBasis: rule.legalBasis,
        sourceIds: rule.sourceIds,
        portalUrl: rule.portalUrl ?? null,
        documentsToKeep: rule.documentsToKeep,
        penaltyNote: rule.penaltyNote ?? null,
        discrepancies: resolved.discrepancies,
        checklists: checklistFor(rule),
      });
    }
  }

  return instances.sort(
    (a, b) =>
      compareIso(a.dueDate, b.dueDate) || (a.ruleId < b.ruleId ? -1 : a.ruleId > b.ruleId ? 1 : 0),
  );
}

export interface DeadlineWindow {
  horizonDays: number;
  actionable: ObligationInstance[];
  overdue: ObligationInstance[];
}

export function upcoming(
  instances: readonly ObligationInstance[],
  today: IsoDate,
  horizonDays: number,
): DeadlineWindow {
  const actionable = instances.filter(
    (instance) =>
      instance.status !== 'not_applicable' &&
      instance.status !== 'done' &&
      instance.status !== 'untracked' &&
      daysBetween(today, instance.dueDate) >= 0 &&
      daysBetween(today, instance.dueDate) <= horizonDays,
  );
  const overdue = instances.filter((instance) => instance.status === 'overdue');
  return { horizonDays, actionable, overdue };
}
