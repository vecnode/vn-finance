import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  applyBp,
  assertInteger,
  formatBpAsCoefficient,
  formatBpAsPercent,
  parseEurToCents,
  roundHalfAwayFromZero,
  splitEvenly,
} from './money.ts';

test('money is integer cents: floating point values are rejected loudly', () => {
  assert.throws(() => assertInteger(1.5, 'amount'), /must be an integer/);
  assert.throws(() => assertInteger(Number.NaN, 'amount'), /finite/);
  assert.doesNotThrow(() => assertInteger(0, 'amount'));
  assert.doesNotThrow(() => assertInteger(-100, 'amount'));
});

test('applyBp multiplies by basis points and rounds to whole cents', () => {
  // 48 600,00 EUR at 23% = 11 178,00 EUR
  assert.equal(applyBp(4_860_000, 2300), 1_117_800);
  // 19 400,00 EUR at 70% = 13 580,00 EUR
  assert.equal(applyBp(1_940_000, 7000), 1_358_000);
  // 13 580,00 EUR at 21,4% = 2 906,12 EUR
  assert.equal(applyBp(1_358_000, 2140), 290_612);
  // Half a cent rounds away from zero, the way an accountant expects.
  assert.equal(applyBp(1, 5000), 1);
  assert.equal(applyBp(-1, 5000), -1);
});

test('roundHalfAwayFromZero is symmetric around zero', () => {
  assert.equal(roundHalfAwayFromZero(0.5), 1);
  assert.equal(roundHalfAwayFromZero(-0.5), -1);
  assert.equal(roundHalfAwayFromZero(2.4), 2);
  assert.equal(roundHalfAwayFromZero(-2.6), -3);
});

test('splitEvenly never loses a cent: 2 906,12 EUR in three instalments', () => {
  const parts = splitEvenly(290_612, 3);
  assert.deepEqual(parts, [96_871, 96_871, 96_870]);
  assert.equal(
    parts.reduce((total, part) => total + part, 0),
    290_612,
  );
});

test('splitEvenly handles indivisible and negative amounts exactly', () => {
  for (const [total, parts] of [
    [100, 3],
    [7, 7],
    [0, 4],
    [-290_612, 3],
    [1, 12],
  ] as const) {
    const result = splitEvenly(total, parts);
    assert.equal(result.length, parts);
    assert.equal(
      result.reduce((sum, value) => sum + value, 0),
      total,
      `splitEvenly(${total}, ${parts}) must sum back to the total`,
    );
    const spread = Math.max(...result) - Math.min(...result);
    assert.ok(spread <= 1, 'instalments must differ by at most one cent');
  }
  assert.throws(() => splitEvenly(100, 0), /positive integer/);
});

test('parseEurToCents reads Portuguese and international formats', () => {
  assert.equal(parseEurToCents('1 234,56'), 123_456);
  assert.equal(parseEurToCents('1.234,56'), 123_456);
  assert.equal(parseEurToCents('1234.56'), 123_456);
  assert.equal(parseEurToCents('1,234.56'), 123_456);
  assert.equal(parseEurToCents('1 234,56 €'), 123_456);
  assert.equal(parseEurToCents('0,01'), 1);
  assert.equal(parseEurToCents('-45,50'), -4550);
  assert.throws(() => parseEurToCents(''), /empty amount/);
  assert.throws(() => parseEurToCents('abc'), /cannot parse/);
});

test('rates and coefficients are displayed the Portuguese way', () => {
  assert.equal(formatBpAsPercent(2140), '21,4%');
  assert.equal(formatBpAsPercent(2300), '23%');
  assert.equal(formatBpAsPercent(7000), '70%');
  assert.equal(formatBpAsCoefficient(7500), '0,75');
  assert.equal(formatBpAsCoefficient(3500), '0,35');
});
