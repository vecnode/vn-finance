/**
 * First-run and profile API tests.
 *
 * The application is meant to be usable by running it and filling in one form:
 * there is no `init` step in the way. These tests are about that promise holding
 * on the wire — a panel with an empty vault serves the model and accepts a
 * profile, a second create does not quietly overwrite it, a replacement is
 * deliberate, and a profile that arrives as a file passes the same checks as one
 * typed into the form.
 */

import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { createDefaultProfile } from '../core/profile.ts';
import { Vault } from '../store/vault.ts';
import { startWebServer } from './server.ts';
import type { DashboardModel } from './report.ts';

const TOKEN = 'teste-token-de-sessao-com-tamanho-suficiente';
const HOST = '127.0.0.1';
const NIF = '245678999';

/** A server over a vault with nothing in it: the first run of the application. */
async function withEmptyVault(run: (vault: Vault, api: (path: string, body?: unknown) => Promise<Response>) => Promise<void>): Promise<void> {
  const dir = mkdtempSync(join(tmpdir(), 'vnfin-first-run-'));
  const vault = new Vault(dir);
  const server = await startWebServer({ vault, version: 'teste', year: 2026, port: 0, token: TOKEN });
  const api = (path: string, body?: unknown): Promise<Response> =>
    fetch(`http://${HOST}:${server.port}${path}`, {
      method: body === undefined ? 'GET' : 'POST',
      headers: { 'x-vnfin-token': TOKEN, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
  try {
    await run(vault, api);
  } finally {
    await server.close();
    rmSync(dir, { recursive: true, force: true });
  }
}

async function model(response: Response): Promise<DashboardModel> {
  assert.equal(response.status, 200);
  const payload = (await response.json()) as { ok: boolean; model: DashboardModel };
  assert.equal(payload.ok, true);
  return payload.model;
}

const completeProfile = (overrides: Record<string, unknown> = {}): Record<string, unknown> => ({
  nif: NIF,
  name: 'Contribuinte de Teste',
  ivaRegime: 'isento_art53',
  startDate: '2019-04-01',
  turnoverPreviousYearCents: 1_240_000,
  ...overrides,
});

test('an empty vault serves a model with no profile instead of failing', async () => {
  await withEmptyVault(async (_vault, api) => {
    const dashboard = await model(await api('/api/dashboard'));
    assert.equal(dashboard.profile, null);
    assert.deepEqual(dashboard.agenda, []);
    assert.ok(dashboard.pack.summary.obligations > 10, 'o pacote de regras continua a ser servido');
    assert.ok(
      (dashboard.profileOptions?.ivaRegimes ?? []).length === 3,
      'o formulário precisa das opções de regime que a AT aceita, vindas do modelo',
    );
  });
});

test('the first profile is created from the panel, with no command line step', async () => {
  await withEmptyVault(async (vault, api) => {
    const dashboard = await model(await api('/api/profile', completeProfile()));

    assert.equal(dashboard.profile?.nif, NIF);
    assert.equal(dashboard.profile?.iva.regime, 'isento_art53');
    assert.equal(dashboard.profile?.activity.turnoverPreviousYearCents, 1_240_000);
    assert.equal(dashboard.profile?.trackingStart, dashboard.meta.today);
    assert.ok(dashboard.agenda.length > 0, 'com perfil a agenda passa a existir');
    // And it is in the vault, where the command line would have put it.
    assert.equal(vault.loadProfile()?.name, 'Contribuinte de Teste');
  });
});

test('a profile can be replaced, but only when the replacement says so', async () => {
  await withEmptyVault(async (vault, api) => {
    await model(await api('/api/profile', completeProfile()));

    const replaced = await model(await api('/api/profile', completeProfile({ name: 'Outro Nome', replace: true })));
    assert.equal(replaced.profile?.name, 'Outro Nome');
    assert.equal(vault.loadProfile()?.name, 'Outro Nome');

    // And a replacement that omits the declaration fields is refused rather than
    // inheriting the regime from the profile it is replacing.
    const incomplete = await api('/api/profile', { replace: true, name: 'Sem o resto' });
    assert.equal(incomplete.status, 400);
    assert.match(((await incomplete.json()) as { error: string }).error, /nif/);
    assert.equal(vault.loadProfile()?.name, 'Outro Nome', 'nada foi substituído por um perfil incompleto');
  });
});

test('a profile that the interface sends without a declared IVA regime is refused', async () => {
  await withEmptyVault(async (vault, api) => {
    const response = await api('/api/profile', { nif: NIF, name: 'Sem regime', ivaRegime: 'isento_de_tudo' });
    assert.equal(response.status, 400);
    assert.match(((await response.json()) as { error: string }).error, /regime de IVA/);
    assert.equal(vault.loadProfile(), null, 'um perfil inválido não é gravado');
  });
});

test('a profile exported from another vault can be loaded back in', async () => {
  await withEmptyVault(async (vault, api) => {
    const stored = createDefaultProfile({
      nif: NIF,
      name: 'Contribuinte de Teste',
      ivaRegime: 'trimestral',
      startDate: '2019-04-01',
      turnoverPreviousYearCents: 980_000,
    });
    stored.ss.startupExemptionActive = true;

    const response = await api('/api/profile/import', { profile: JSON.parse(JSON.stringify(stored)) });
    assert.equal(response.status, 200);
    const payload = (await response.json()) as { ok: boolean; model: DashboardModel; warnings: string[] };
    assert.equal(payload.ok, true);
    assert.equal(payload.model.profile?.iva.regime, 'trimestral');
    assert.equal(payload.model.profile?.ss.startupExemptionActive, true);
    assert.equal(vault.loadProfile()?.name, 'Contribuinte de Teste');
    // The import reports what it could not verify instead of hiding it: this
    // profile claims a first-year exemption that started 89 months ago.
    assert.ok(
      payload.warnings.some((warning) => warning.includes('12 meses')),
      `a importação tem de devolver os avisos de coerência (recebido: ${JSON.stringify(payload.warnings)})`,
    );
  });
});

test('an imported file that is not a profile leaves the vault untouched', async () => {
  await withEmptyVault(async (vault, api) => {
    const response = await api('/api/profile/import', { profile: { nome: 'não é um perfil' } });
    assert.equal(response.status, 400);
    assert.match(((await response.json()) as { error: string }).error, /declarar "nif"/);
    assert.equal(vault.loadProfile(), null);
  });
});

test('an imported profile that contradicts itself is refused, with the reason', async () => {
  await withEmptyVault(async (vault, api) => {
    const inconsistent = createDefaultProfile({
      nif: NIF,
      name: 'Contribuinte de Teste',
      ivaRegime: 'isento_art53',
      turnoverPreviousYearCents: 1_240_000,
    });
    // An art. 53.º exemption that also exports cannot exist: the exemption's own
    // conditions exclude export operations.
    inconsistent.activity.exports = true;

    const response = await api('/api/profile/import', { profile: JSON.parse(JSON.stringify(inconsistent)) });
    assert.equal(response.status, 400);
    const payload = (await response.json()) as { error: string };
    assert.match(payload.error, /exportação/);
    assert.equal(vault.loadProfile(), null);
  });
});

test('loading a file over an existing profile needs an explicit confirmation', async () => {
  await withEmptyVault(async (vault, api) => {
    await model(await api('/api/profile', completeProfile()));

    const other = createDefaultProfile({ nif: NIF, name: 'Perfil de outro computador', ivaRegime: 'mensal' });
    const refused = await api('/api/profile/import', { profile: JSON.parse(JSON.stringify(other)) });
    assert.equal(refused.status, 409);
    assert.match(((await refused.json()) as { error: string }).error, /Confirma a substituição/);
    assert.equal(vault.loadProfile()?.name, 'Contribuinte de Teste');

    const confirmed = await api('/api/profile/import', {
      profile: JSON.parse(JSON.stringify(other)),
      replace: true,
    });
    assert.equal(confirmed.status, 200);
    assert.equal(vault.loadProfile()?.name, 'Perfil de outro computador');
    assert.equal(vault.loadProfile()?.iva.regime, 'mensal');
  });
});

test('the profile can be read back out as a file, and only when there is one', async () => {
  await withEmptyVault(async (vault, api) => {
    const missing = await api('/api/profile/export');
    assert.equal(missing.status, 409);

    await model(await api('/api/profile', completeProfile()));
    const response = await api('/api/profile/export');
    assert.equal(response.status, 200);
    const payload = (await response.json()) as { ok: boolean; exportedAt: string; profile: { nif: string } };
    assert.equal(payload.ok, true);
    assert.equal(payload.profile.nif, NIF);
    assert.match(payload.exportedAt, /^\d{4}-\d{2}-\d{2}T/);
    assert.deepEqual(payload.profile, vault.loadProfile(), 'o que sai é exatamente o que está no cofre');
  });
});
