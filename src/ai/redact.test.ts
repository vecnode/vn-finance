import assert from 'node:assert/strict';
import { test } from 'node:test';

import { classifyPtTaxNumber, isValidPtTaxNumber, maskNif } from '../core/nif.ts';
import { previewRedaction, redactForSend, redactText, residualPii, restorePseudonyms } from './redact.ts';

const VALID_NIF = '123456789';
const VALID_NIPC = '501234560';
const MOBILE = '912345678';

test('the Portuguese tax-number checksum works', () => {
  assert.equal(isValidPtTaxNumber(VALID_NIF), true);
  assert.equal(isValidPtTaxNumber(VALID_NIPC), true);
  assert.equal(isValidPtTaxNumber('123456788'), false);
  assert.equal(classifyPtTaxNumber(VALID_NIF), 'NIF');
  assert.equal(classifyPtTaxNumber(VALID_NIPC), 'NIPC');
  assert.equal(classifyPtTaxNumber(MOBILE), null);
  assert.equal(isValidPtTaxNumber('12345678'), false);
});

test('masking a NIF leaves enough to recognise and not enough to misuse', () => {
  assert.equal(maskNif(VALID_NIF), '123••••89');
  assert.equal(maskNif('1234'), '••••');
});

test('a NIF and a company number are replaced by stable pseudonyms', () => {
  const result = redactText(`O cliente ${VALID_NIPC} e o contribuinte ${VALID_NIF}.`);
  assert.match(result.text, /«NIPC_1»/);
  assert.match(result.text, /«NIF_1»/);
  assert.equal(result.substitutions.length, 2);
  assert.equal(result.substitutions[0]?.original, VALID_NIPC);
});

test('EVERY nine-digit number is removed, even when the checksum fails', () => {
  // The safe direction is over-redaction: an identifier the user cannot audit
  // never leaves the machine, and a number that is not a tax number costs the
  // model nothing to lose.
  const result = redactText('referência 123456788 e telefone 912345678');
  assert.match(result.text, /«IDENTIFICADOR_1»/);
  assert.match(result.text, /«IDENTIFICADOR_2»/);
  assert.doesNotMatch(result.text, /\d{9}/);
});

test('IBANs, e-mails and addresses are redacted', () => {
  // An obviously-placeholder IBAN, so nobody reading the published source has to
  // wonder whether it belongs to someone.
  const result = redactText(
    'Transferir para PT50 0000 0000 0000 0000 0000 0 e enviar para joao.silva@example.pt, Rua das Flores 12, 3.º',
  );
  assert.match(result.text, /«IBAN_1»/);
  assert.match(result.text, /«EMAIL_1»/);
  assert.match(result.text, /«MORADA_1»/);
  assert.doesNotMatch(result.text, /example\.pt/);
  assert.doesNotMatch(result.text, /PT50/);
});

test('amounts and dates survive redaction, because the answer depends on them', () => {
  const result = redactText('Fatura de 12 500,00 € vencida em 2026-11-20.');
  assert.match(result.text, /12 500,00 €/);
  assert.match(result.text, /2026-11-20/);
});

test('the taxpayer name and known clients are replaced', () => {
  const result = redactText('O João Silva prestou serviços à Norte Digital, Lda. em agosto.', {
    selfName: 'João Silva',
    knownParties: [{ name: 'Norte Digital, Lda.', kind: 'CLIENTE' }],
  });
  assert.match(result.text, /«CONTRIBUINTE_1»/);
  assert.match(result.text, /«CLIENTE_1»/);
  assert.doesNotMatch(result.text, /João Silva/);
  assert.doesNotMatch(result.text, /Norte Digital/);
});

test('pseudonyms are stable across the messages of one request', () => {
  const payload = redactForSend([
    { role: 'system', content: 'Contexto.' },
    { role: 'user', content: `Primeira menção ${VALID_NIF}.` },
    { role: 'user', content: `Segunda menção ${VALID_NIF}.` },
  ]);
  const tokens = new Set(payload.substitutions.map((substitution) => substitution.token));
  assert.deepEqual([...tokens], ['NIF_1']);
  assert.match(payload.messages[1]?.content ?? '', /«NIF_1»/);
  assert.match(payload.messages[2]?.content ?? '', /«NIF_1»/);
});

test('the same client keeps the same pseudonym across requests', () => {
  const first = redactForSend([{ role: 'user', content: `Cliente ${VALID_NIPC}` }]);
  const existing = new Map(first.substitutions.map((entry) => [entry.original, entry.token]));
  const second = redactForSend([{ role: 'user', content: `Cliente ${VALID_NIPC} outra vez` }], { existing });
  assert.match(second.messages[0]?.content ?? '', /«NIPC_1»/);
});

test('the scanner detects what redaction is supposed to have removed', () => {
  assert.deepEqual(residualPii(`ficheiro limpo`), []);
  assert.deepEqual(residualPii(`ainda tem ${VALID_NIF}`), ['NIF']);
  assert.deepEqual(residualPii('contacto joao@example.pt'), ['EMAIL']);
});

test('redactForSend returns a payload whose text no longer matches any scanner rule', () => {
  const payload = redactForSend([
    {
      role: 'user',
      content: `Sou o João Silva, NIF ${VALID_NIF}, IBAN PT50 0002 0123 1234 5678 9015 4, joao@example.pt, 912345678.`,
    },
  ]);
  const text = payload.messages[0]?.content ?? '';
  assert.deepEqual(residualPii(text), []);
  assert.ok(payload.charactersSent > 0);
});

test('the preview shows the user exactly what would be hidden', () => {
  const preview = previewRedaction(`NIF ${VALID_NIF} e 4 200,00 €`, {});
  assert.equal(preview.before, `NIF ${VALID_NIF} e 4 200,00 €`);
  assert.match(preview.after, /«NIF_1»/);
  assert.equal(preview.substitutions.length, 1);
});

test('the answer can be read back with the real values restored, locally', () => {
  const payload = redactForSend([{ role: 'user', content: `O cliente ${VALID_NIPC} pagou.` }]);
  const answer = 'O cliente «NIPC_1» pagou 4 200,00 €.';
  assert.equal(restorePseudonyms(answer, payload.substitutions), `O cliente ${VALID_NIPC} pagou 4 200,00 €.`);
});

test('an empty question is still a valid, empty payload', () => {
  const payload = redactForSend([{ role: 'user', content: '' }]);
  assert.deepEqual(payload.substitutions, []);
  assert.equal(payload.charactersSent, 0);
});
