/**
 * Portuguese tax-number handling.
 *
 * The NIF/NIPC is a nine-digit number whose last digit is a modulus-11 check
 * digit. Validating it matters for two reasons: it catches typos before they
 * reach an invoice, and it is what lets the redaction gateway tell a tax number
 * apart from a mobile phone number that also has nine digits.
 */

export function normaliseNif(value: string): string {
  return value.replace(/[\s.\-]/g, '');
}

export function isValidPtTaxNumber(value: string): boolean {
  const digits = normaliseNif(value);
  if (!/^\d{9}$/.test(digits)) return false;
  let sum = 0;
  for (let index = 0; index < 8; index += 1) {
    sum += Number(digits[index]) * (9 - index);
  }
  const remainder = sum % 11;
  const checkDigit = remainder < 2 ? 0 : 11 - remainder;
  return checkDigit === Number(digits[8]);
}

/**
 * `NIF` for a natural person (starts 1-3), `NIPC` for a collective person
 * (starts 5-9). Returns null when the checksum fails.
 */
export function classifyPtTaxNumber(value: string): 'NIF' | 'NIPC' | null {
  const digits = normaliseNif(value);
  if (!isValidPtTaxNumber(digits)) return null;
  const first = digits[0];
  if (first === '1' || first === '2' || first === '3') return 'NIF';
  if (first === '5' || first === '6' || first === '7' || first === '8' || first === '9') return 'NIPC';
  return 'NIF';
}

/** `123456789` -> `123••••89`. Enough to recognise, not enough to leak. */
export function maskNif(value: string): string {
  const digits = normaliseNif(value);
  if (digits.length <= 4) return '••••';
  const head = digits.slice(0, Math.min(3, digits.length - 2));
  const tail = digits.slice(-2);
  return `${head}${'•'.repeat(Math.max(0, digits.length - head.length - tail.length))}${tail}`;
}
