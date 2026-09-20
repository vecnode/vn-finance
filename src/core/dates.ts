/**
 * Calendar arithmetic for Portuguese tax deadlines.
 *
 * All arithmetic is done in UTC on plain ISO date strings so that a machine in
 * any timezone produces the same deadline. Nothing here reads the clock: the
 * caller always passes `today` explicitly, which is what makes the engine
 * testable and reproducible.
 */

import type { IsoDate } from './types.ts';

const MS_PER_DAY = 86_400_000;

export const PT_MONTHS = [
  'janeiro',
  'fevereiro',
  'março',
  'abril',
  'maio',
  'junho',
  'julho',
  'agosto',
  'setembro',
  'outubro',
  'novembro',
  'dezembro',
] as const;

export interface DateParts {
  year: number;
  month: number;
  day: number;
}

export function parseIso(iso: IsoDate): DateParts {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  if (!match) throw new TypeError(`invalid ISO date: ${JSON.stringify(iso)}`);
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (month < 1 || month > 12 || day < 1 || day > 31) {
    throw new TypeError(`ISO date out of range: ${iso}`);
  }
  return { year, month, day };
}

export function makeIso(year: number, month: number, day: number): IsoDate {
  return `${String(year).padStart(4, '0')}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')}`;
}

export function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

export function addDays(iso: IsoDate, days: number): IsoDate {
  const { year, month, day } = parseIso(iso);
  const shifted = new Date(Date.UTC(year, month - 1, day + days));
  return makeIso(shifted.getUTCFullYear(), shifted.getUTCMonth() + 1, shifted.getUTCDate());
}

/** Add months, clamping the day to the length of the target month. */
export function addMonths(iso: IsoDate, months: number): IsoDate {
  const { year, month, day } = parseIso(iso);
  const zeroBased = month - 1 + months;
  const targetYear = year + Math.floor(zeroBased / 12);
  const targetMonth = ((zeroBased % 12) + 12) % 12 + 1;
  return makeIso(targetYear, targetMonth, Math.min(day, daysInMonth(targetYear, targetMonth)));
}

export function dayOfWeek(iso: IsoDate): number {
  const { year, month, day } = parseIso(iso);
  return new Date(Date.UTC(year, month - 1, day)).getUTCDay();
}

export function isWeekend(iso: IsoDate): boolean {
  const dow = dayOfWeek(iso);
  return dow === 0 || dow === 6;
}

/**
 * Move a date forward to the next business day.
 *
 * AT note (a) on the annual calendar: obligations falling on a weekend or a
 * public holiday may be met on the following business day. Public holidays are
 * NOT guessed here — pass them in from the rule pack, because an app that
 * guesses holidays is an app that misses a deadline.
 */
export function nextBusinessDay(iso: IsoDate, holidays: readonly IsoDate[] = []): IsoDate {
  const holidaySet = new Set(holidays);
  let candidate = iso;
  // 10 iterations is far beyond any real Portuguese holiday run (Christmas).
  for (let guard = 0; guard < 10; guard += 1) {
    if (!isWeekend(candidate) && !holidaySet.has(candidate)) return candidate;
    candidate = addDays(candidate, 1);
  }
  return candidate;
}

/** ISO strings sort lexicographically, so comparison is a string compare. */
export function compareIso(a: IsoDate, b: IsoDate): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Whole days from `from` to `to`; negative when `to` is in the past. */
export function daysBetween(from: IsoDate, to: IsoDate): number {
  const a = parseIso(from);
  const b = parseIso(to);
  const fromMs = Date.UTC(a.year, a.month - 1, a.day);
  const toMs = Date.UTC(b.year, b.month - 1, b.day);
  return Math.round((toMs - fromMs) / MS_PER_DAY);
}

export function quarterRange(year: number, quarter: number): { start: IsoDate; end: IsoDate } {
  if (quarter < 1 || quarter > 4) throw new RangeError(`quarter out of range: ${quarter}`);
  const firstMonth = (quarter - 1) * 3 + 1;
  const lastMonth = firstMonth + 2;
  return {
    start: makeIso(year, firstMonth, 1),
    end: makeIso(year, lastMonth, daysInMonth(year, lastMonth)),
  };
}

export function monthRange(year: number, month: number): { start: IsoDate; end: IsoDate } {
  return { start: makeIso(year, month, 1), end: makeIso(year, month, daysInMonth(year, month)) };
}

export function formatPtDate(iso: IsoDate): string {
  const { year, month, day } = parseIso(iso);
  return `${String(day).padStart(2, '0')}/${String(month).padStart(2, '0')}/${year}`;
}

export function formatPtMonthYear(month: number, year: number): string {
  return `${PT_MONTHS[month - 1]} de ${year}`;
}

export function quarterLabel(year: number, quarter: number): string {
  return `${quarter}.º trimestre ${year}`;
}

/** Today in Europe/Lisbon, as an ISO date. The only clock read in the project. */
export function todayInLisbon(now: Date = new Date()): IsoDate {
  const formatter = new Intl.DateTimeFormat('en-CA', {
    timeZone: 'Europe/Lisbon',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  });
  return formatter.format(now);
}
