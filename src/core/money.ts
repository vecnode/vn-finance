/**
 * Monetary and rate arithmetic.
 *
 * Two rules govern every number in this project:
 *
 *   1. Money is an INTEGER NUMBER OF EURO CENTS. There is no `number` that means
 *      "euros" anywhere in the domain. Floating point money is how accountants
 *      end up with a one-cent difference that nobody can explain.
 *   2. Rates are INTEGER BASIS POINTS. 1% = 100, 21.4% = 2140, a coefficient of
 *      0.75 = 7500. This keeps every percentage exact and every pack file
 *      unambiguous.
 */

/** Integer euro cents. 4860000 === 48 600,00 EUR. */
export type Cents = number;

/** Integer basis points. 1% = 100; 21.4% = 2140; coefficient 0.75 = 7500. */
export type BasisPoints = number;

const EUR_FORMATTER = new Intl.NumberFormat('pt-PT', {
  style: 'currency',
  currency: 'EUR',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

export function assertInteger(value: number, label: string): void {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new TypeError(`${label} must be a finite number, received ${String(value)}`);
  }
  if (!Number.isInteger(value)) {
    throw new TypeError(
      `${label} must be an integer (cents or basis points), received ${value}. ` +
        'Money and rates are never floating point in this codebase.',
    );
  }
}

/** Round half away from zero at the cent. Accountants expect 0.5 to round up. */
export function roundHalfAwayFromZero(value: number): number {
  return value >= 0 ? Math.floor(value + 0.5) : Math.ceil(value - 0.5);
}

/** Apply a basis-point rate to an amount, rounded to whole cents. */
export function applyBp(amount: Cents, bp: BasisPoints): Cents {
  assertInteger(amount, 'amount');
  assertInteger(bp, 'bp');
  return roundHalfAwayFromZero((amount * bp) / 10_000);
}

/**
 * Split an amount into `parts` as evenly as legally possible.
 *
 * The parts always sum EXACTLY to `total`. The remainder cents go to the first
 * parts, which is what an instalment plan does: 2 906,12 EUR in three payments
 * is 968,71 + 968,71 + 968,70, never three times 968,71 with a cent lost.
 */
export function splitEvenly(total: Cents, parts: number): Cents[] {
  assertInteger(total, 'total');
  if (!Number.isInteger(parts) || parts <= 0) {
    throw new TypeError(`parts must be a positive integer, received ${String(parts)}`);
  }
  const base = Math.trunc(total / parts);
  const remainder = total - base * parts;
  const sign = remainder < 0 ? -1 : 1;
  const magnitude = Math.abs(remainder);
  return Array.from({ length: parts }, (_, index) => base + (index < magnitude ? sign : 0));
}

/** Format cents as a pt-PT currency string, e.g. 4860000 -> "48 600,00 €". */
export function formatEur(cents: Cents): string {
  assertInteger(cents, 'cents');
  return EUR_FORMATTER.format(cents / 100).replace(/\u00a0/g, '\u202f');
}

/**
 * Parse a human-entered EUR amount ("1 234,56", "1234.56", "1.234,56 €") into cents.
 * Ambiguity is resolved in favour of the Portuguese convention: comma is the
 * decimal separator, dot and space are thousands separators.
 */
export function parseEurToCents(input: string): Cents {
  const cleaned = input.replace(/[€\s\u00a0\u202f]/g, '').trim();
  if (cleaned === '') throw new TypeError('empty amount');

  const lastComma = cleaned.lastIndexOf(',');
  const lastDot = cleaned.lastIndexOf('.');

  let normalised: string;
  if (lastComma >= 0 && lastDot >= 0) {
    normalised =
      lastComma > lastDot
        ? cleaned.replace(/\./g, '').replace(',', '.') // 1.234,56 -> 1234.56
        : cleaned.replace(/,/g, ''); //                  1,234.56 -> 1234.56
  } else if (lastComma >= 0) {
    normalised = cleaned.replace(',', '.');
  } else {
    normalised = cleaned;
  }

  if (!/^-?\d+(\.\d{1,2})?$/.test(normalised)) {
    throw new TypeError(`cannot parse "${input}" as a EUR amount`);
  }
  return Math.round(Number(normalised) * 100);
}

/** Percentage helper for display only: 2140 -> "21,4%". */
export function formatBpAsPercent(bp: BasisPoints): string {
  assertInteger(bp, 'bp');
  const percent = bp / 100;
  const text = Number.isInteger(percent)
    ? String(percent)
    : percent.toFixed(2).replace(/0$/, '').replace('.', ',');
  return `${text}%`;
}

/** Coefficient helper for display only: 7500 -> "0,75". */
export function formatBpAsCoefficient(bp: BasisPoints): string {
  assertInteger(bp, 'bp');
  return (bp / 10_000).toFixed(4).replace(/0+$/, '').replace(/\.$/, '').replace('.', ',');
}
