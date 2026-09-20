/**
 * Where a client is, and why it matters.
 *
 * The art. 53.º CIVA ceiling is measured in **national territory** only, while the
 * other two scopes change the VAT treatment, the VIES registration and the
 * declaração recapitulativa. Keeping this in one module means the rule engine, the
 * invoice defaults and the alert engine cannot disagree about what "foreign" means.
 */

export type TurnoverScope = 'national' | 'eu' | 'nonEu';

/** The 27 member states. */
const EU_COUNTRIES = new Set([
  'AT', 'BE', 'BG', 'HR', 'CY', 'CZ', 'DK', 'EE', 'FI', 'FR', 'DE', 'GR', 'HU',
  'IE', 'IT', 'LV', 'LT', 'LU', 'MT', 'NL', 'PL', 'PT', 'RO', 'SK', 'SI', 'ES', 'SE',
]);

export function scopeOf(country: string): TurnoverScope {
  const code = country.trim().toUpperCase();
  if (code === 'PT') return 'national';
  return EU_COUNTRIES.has(code) ? 'eu' : 'nonEu';
}

export function isEuCountry(country: string): boolean {
  return scopeOf(country) === 'eu';
}
