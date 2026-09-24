/**
 * Profile construction and import tests.
 *
 * The profile is the one place where a wrong default is not merely wrong: it
 * silently changes which obligations apply to someone for a whole year. These
 * tests are about the difference between "the user did not mention this" and
 * "the user removed this", and about an imported file being treated as an
 * untrusted document rather than as data that can be trusted to be shaped right.
 */

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { buildProfile, createDefaultProfile, normaliseImportedProfile, validateProfile } from './profile.ts';

const NIF = '245678999';

function base(extra: Parameters<typeof buildProfile>[0] = {}) {
  return buildProfile({ nif: NIF, name: 'Contribuinte de Teste', ivaRegime: 'isento_art53', ...extra });
}

test('a draft with the two declared fields produces a complete profile', () => {
  const profile = base();

  assert.equal(profile.nif, NIF);
  assert.equal(profile.name, 'Contribuinte de Teste');
  assert.equal(profile.iva.regime, 'isento_art53');
  assert.equal(profile.irs.regime, 'simplificado');
  assert.equal(profile.irs.coefficientBp, 7500);
  assert.equal(profile.activity.categoryB, true);
  assert.equal(profile.activity.cae.length, 1, 'sem CAE indicado fica o principal por omissão');
  // Never invented: the art. 53.º evaluation has to be able to say "I do not know".
  assert.equal(profile.activity.turnoverPreviousYearCents, undefined);
  assert.equal(profile.activity.turnoverCurrentYearExpectedCents, undefined);
  assert.equal(profile.activity.startDate, '1970-01-01', 'a data em falta é visível como tal');
  assert.equal(validateProfile(profile).filter((problem) => problem.level === 'error').length, 0);
});

/*
 * RITI art. 30.º n.º 2 is the exception, so n.º 1 b) is the base rule: "not
 * asked" has to stay distinguishable from "answered no", or an undeclared
 * taxpayer would be pushed onto the monthly series by a default nobody chose.
 */
test('a exceção das operações intracomunitárias não é preenchida por omissão', () => {
  const undecided = base();
  assert.equal(
    undecided.activity.intraCommunityOperationsAbove50k,
    undefined,
    'sem resposta, a exceção fica por declarar — não é o mesmo que "não"',
  );

  const declared = base({ intraCommunityOperationsAbove50k: true });
  assert.equal(declared.activity.intraCommunityOperationsAbove50k, true);

  const denied = base({ intraCommunityOperationsAbove50k: false });
  assert.equal(denied.activity.intraCommunityOperationsAbove50k, false, 'uma negação é uma decisão e é gravada');

  // Editing something else must not quietly answer this question either.
  const edited = buildProfile({ name: 'Outro Nome' }, { current: declared });
  assert.equal(edited.activity.intraCommunityOperationsAbove50k, true, 'a resposta declarada mantém-se');

  const untouched = buildProfile({ name: 'Outro Nome' }, { current: undecided });
  assert.equal(untouched.activity.intraCommunityOperationsAbove50k, undefined, 'e a ausência de resposta também');
});

test('editing a profile keeps what the form did not mention', () => {
  const current = createDefaultProfile({
    nif: NIF,
    name: 'Contribuinte de Teste',
    ivaRegime: 'isento_art53',
    startDate: '2019-04-01',
    trackingStart: '2026-01-15',
    turnoverPreviousYearCents: 1_240_000,
  });

  const edited = buildProfile({ turnoverCurrentYearExpectedCents: 5_000_000 }, { current });

  assert.equal(edited.name, current.name, 'o nome não mencionado mantém-se');
  assert.equal(edited.nif, current.nif);
  assert.equal(edited.iva.regime, 'isento_art53');
  assert.equal(edited.activity.startDate, '2019-04-01');
  assert.equal(edited.trackingStart, '2026-01-15', 'o acompanhamento não recomeça por se editar');
  assert.equal(edited.activity.turnoverPreviousYearCents, 1_240_000);
  assert.equal(edited.activity.turnoverCurrentYearExpectedCents, 5_000_000);
});

test('an explicitly empty turnover is removed, not kept', () => {
  const current = createDefaultProfile({
    nif: NIF,
    name: 'Contribuinte de Teste',
    ivaRegime: 'trimestral',
    turnoverPreviousYearCents: 1_240_000,
  });

  const cleared = buildProfile({ turnoverPreviousYearCents: null }, { current });

  assert.equal(
    cleared.activity.turnoverPreviousYearCents,
    undefined,
    'apagar a caixa tem de remover o valor: é a única forma de corrigir um número errado',
  );
});

test('a profile with no IVA regime at all still gets one, so fixtures do not need to declare it', () => {
  // The interfaces refuse to create a profile without a declared regime; the
  // builder keeps the historical default for tests and older imports.
  assert.equal(createDefaultProfile({ nif: NIF, name: 'Sem regime' }).iva.regime, 'trimestral');
});

test('an imported profile round-trips through the file format', () => {
  const original = createDefaultProfile({
    nif: NIF,
    name: 'Contribuinte de Teste',
    ivaRegime: 'isento_art53',
    startDate: '2019-04-01',
    turnoverPreviousYearCents: 1_240_000,
    intraCommunityOperations: true,
  });
  original.irs.regime = 'organizada';
  original.irs.coefficientBp = 3000;
  original.ss.startupExemptionActive = true;
  original.isCompany = true;
  original.activity.cae = [{ code: '62020', description: 'Consultoria informática', role: 'principal' }];

  const reimported = normaliseImportedProfile(JSON.parse(JSON.stringify(original)));

  assert.deepEqual(reimported, original, 'exportar e carregar tem de devolver o mesmo perfil');
});

test('an imported file that is not a profile is refused, field by field', () => {
  assert.throws(() => normaliseImportedProfile(null), /esperava um objeto JSON/);
  assert.throws(() => normaliseImportedProfile([1, 2]), /esperava um objeto JSON/);
  assert.throws(() => normaliseImportedProfile({ name: 'Sem NIF' }), /"nif" e "name"/);
  assert.throws(
    () => normaliseImportedProfile({ nif: NIF, name: 'X', iva: { regime: 'isento_de_tudo' } }),
    /regime de IVA/,
  );
  assert.throws(
    () => normaliseImportedProfile({ nif: NIF, name: 'X', activity: { startDate: '01/04/2019' } }),
    /AAAA-MM-DD/,
  );
  assert.throws(
    () =>
      normaliseImportedProfile({
        nif: NIF,
        name: 'X',
        activity: { turnoverPreviousYearCents: 'muito' },
      }),
    /cêntimos/,
  );
  assert.throws(() => normaliseImportedProfile({ nif: NIF, name: 'X', activity: { cae: [] } }), /pelo menos um CAE/);
});

test('a NIF that fails the check digit is reported by validation, not silently kept', () => {
  const profile = base({ nif: '123456788' });
  const errors = validateProfile(profile).filter((problem) => problem.level === 'error');
  assert.equal(errors.length, 1);
  assert.equal(errors[0]?.field, 'nif');
});
