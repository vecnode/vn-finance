/**
 * Evaluation of the rule pack's declarative conditions.
 *
 * The rule pack is untrusted data that happens to live in a file the user can
 * edit. It is therefore never `eval`'d and never used to build a property path:
 * every readable field is named explicitly in the switch below, so a malformed
 * pack can only ever be wrong, never dangerous.
 */

import type { Condition, ConditionField, ConditionOp, TaxProfile } from './types.ts';

/** The complete vocabulary of fields a rule pack may ask about. */
export const CONDITION_FIELDS: readonly ConditionField[] = [
  'iva.regime',
  'irs.regime',
  'profile.residentPT',
  'profile.isCompany',
  'activity.categoryB',
  'activity.intraCommunityOperations',
  'activity.exports',
  'activity.hasEmployees',
  'activity.usesCertifiedInvoicingSoftware',
  'activity.usesAtWebservice',
  'ss.startupExemptionActive',
];

export const CONDITION_OPS: readonly ConditionOp[] = [
  'eq',
  'ne',
  'in',
  'gt',
  'gte',
  'lt',
  'lte',
  'exists',
];

export function isConditionField(value: string): value is ConditionField {
  return (CONDITION_FIELDS as readonly string[]).includes(value);
}

export function isConditionOp(value: string): value is ConditionOp {
  return (CONDITION_OPS as readonly string[]).includes(value);
}

/** Read one field of the profile. Exhaustive by construction. */
export function readField(profile: TaxProfile, field: ConditionField): unknown {
  switch (field) {
    case 'iva.regime':
      return profile.iva.regime;
    case 'irs.regime':
      return profile.irs.regime;
    case 'profile.residentPT':
      return profile.residentPT;
    case 'profile.isCompany':
      return profile.isCompany;
    case 'activity.categoryB':
      return profile.activity.categoryB;
    case 'activity.intraCommunityOperations':
      return profile.activity.intraCommunityOperations;
    case 'activity.exports':
      return profile.activity.exports;
    case 'activity.hasEmployees':
      return profile.activity.hasEmployees;
    case 'activity.usesCertifiedInvoicingSoftware':
      return profile.activity.usesCertifiedInvoicingSoftware;
    case 'activity.usesAtWebservice':
      return profile.activity.usesAtWebservice;
    case 'ss.startupExemptionActive':
      return profile.ss.startupExemptionActive;
  }
}

function compare(actual: unknown, op: ConditionOp, expected: unknown): boolean {
  switch (op) {
    case 'exists':
      return actual !== undefined && actual !== null;
    case 'eq':
      return actual === expected;
    case 'ne':
      return actual !== expected;
    case 'in':
      return Array.isArray(expected) && expected.includes(actual as string | number | boolean);
    case 'gt':
      return typeof actual === 'number' && typeof expected === 'number' && actual > expected;
    case 'gte':
      return typeof actual === 'number' && typeof expected === 'number' && actual >= expected;
    case 'lt':
      return typeof actual === 'number' && typeof expected === 'number' && actual < expected;
    case 'lte':
      return typeof actual === 'number' && typeof expected === 'number' && actual <= expected;
  }
}

export function testCondition(profile: TaxProfile, condition: Condition): boolean {
  return compare(readField(profile, condition.field), condition.op, condition.value);
}

export function appliesTo(profile: TaxProfile, conditions: readonly Condition[]): boolean {
  return conditions.every((condition) => testCondition(profile, condition));
}

function describeValue(value: unknown): string {
  if (Array.isArray(value)) return value.map((item) => String(item)).join(' | ');
  if (value === undefined) return '(sem valor)';
  return String(value);
}

/**
 * Explain why an obligation does NOT apply, in Portuguese, so the interface can
 * show a reasoned "não aplicável" row instead of silently hiding the obligation.
 */
export function explainFailure(
  profile: TaxProfile,
  conditions: readonly Condition[],
  fieldLabels: Partial<Record<ConditionField, string>> = {},
): string | null {
  const failing = conditions.filter((condition) => !testCondition(profile, condition));
  if (failing.length === 0) return null;
  const parts = failing.map((condition) => {
    const label = fieldLabels[condition.field] ?? condition.field;
    const actual = describeValue(readField(profile, condition.field));
    if (condition.op === 'exists') return `${label} não está definido`;
    if (condition.op === 'eq') return `${label} é "${actual}" (a regra exige "${describeValue(condition.value)}")`;
    if (condition.op === 'ne') return `${label} é "${actual}" (a regra exige algo diferente de "${describeValue(condition.value)}")`;
    if (condition.op === 'in') return `${label} é "${actual}" (a regra exige um de: ${describeValue(condition.value)})`;
    return `${label} é "${actual}" (a regra exige ${condition.op} ${describeValue(condition.value)})`;
  });
  return parts.join('; ');
}
