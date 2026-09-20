/**
 * Server tests.
 *
 * A panel that exposes someone's whole financial life on a port has to be tested
 * for the ways that goes wrong, so these cover the guards rather than the happy
 * path alone: the token, the Host allowlist that defeats DNS rebinding, path
 * traversal, the CSP, and the absence of CORS headers.
 */

import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { createDefaultProfile } from '../core/profile.ts';
import { Vault } from '../store/vault.ts';
import { startWebServer, type RunningWebServer } from './server.ts';
import type { DashboardModel } from './report.ts';

const TOKEN = 'teste-token-de-sessao-com-tamanho-suficiente';
const HOST = '127.0.0.1';

interface RawResponse {
  status: number;
  body: string;
  headers: http.IncomingHttpHeaders;
}

function rawRequest(
  port: number,
  path: string,
  options: { method?: string; headers?: Record<string, string>; body?: string } = {},
): Promise<RawResponse> {
  return new Promise((resolve, reject) => {
    const request = http.request(
      {
        host: HOST,
        port,
        path,
        method: options.method ?? 'GET',
        headers: options.headers ?? {},
      },
      (response) => {
        let body = '';
        response.setEncoding('utf8');
        response.on('data', (chunk: string) => {
          body += chunk;
        });
        response.on('end', () =>
          resolve({ status: response.statusCode ?? 0, body, headers: response.headers }),
        );
      },
    );
    request.on('error', reject);
    if (options.body !== undefined) request.write(options.body);
    request.end();
  });
}

async function withServer(run: (server: RunningWebServer, vault: Vault) => Promise<void>): Promise<void> {
  const dir = mkdtempSync(join(tmpdir(), 'vnfin-web-'));
  const vault = new Vault(dir);
  vault.ensure();
  vault.saveProfile(
    createDefaultProfile({
      nif: '245678999',
      name: 'Contribuinte de Teste',
      ivaRegime: 'isento_art53',
      startDate: '2019-04-01',
      turnoverPreviousYearCents: 1_240_000,
    }),
  );

  const server = await startWebServer({ vault, version: 'teste', year: 2026, port: 0, token: TOKEN });
  try {
    await run(server, vault);
  } finally {
    await server.close();
    rmSync(dir, { recursive: true, force: true });
  }
}

function api(server: RunningWebServer, path: string, init: RequestInit = {}): Promise<Response> {
  return fetch(`http://${HOST}:${server.port}${path}`, {
    ...init,
    headers: { 'x-vnfin-token': server.token, ...(init.headers ?? {}) },
  });
}

async function model(response: Response): Promise<DashboardModel> {
  const payload = (await response.json()) as { ok: boolean; model: DashboardModel };
  assert.equal(payload.ok, true);
  return payload.model;
}

test('the API refuses requests without the session token', async () => {
  await withServer(async (server) => {
    const anonymous = await fetch(`http://${HOST}:${server.port}/api/dashboard`);
    assert.equal(anonymous.status, 401);
    const payload = (await anonymous.json()) as { ok: boolean; error: string };
    assert.equal(payload.ok, false);
    assert.match(payload.error, /sessão sem chave/);

    const wrong = await api(server, '/api/dashboard', { headers: { 'x-vnfin-token': 'outra-chave' } });
    assert.equal(wrong.status, 401);

    // A token of a different length must fail cleanly rather than throw inside a
    // timing-safe comparison.
    const short = await api(server, '/api/dashboard', { headers: { 'x-vnfin-token': 'curto' } });
    assert.equal(short.status, 401);
  });
});

test('the panel is served with a strict content security policy and no CORS', async () => {
  await withServer(async (server) => {
    const response = await fetch(`http://${HOST}:${server.port}/app.css`);
    assert.equal(response.status, 200);
    assert.match(response.headers.get('content-type') ?? '', /text\/css/);

    const csp = response.headers.get('content-security-policy') ?? '';
    assert.match(csp, /default-src 'self'/);
    assert.match(csp, /script-src 'self'/);
    assert.doesNotMatch(csp, /unsafe-inline/);
    assert.equal(response.headers.get('x-content-type-options'), 'nosniff');
    assert.equal(response.headers.get('x-frame-options'), 'DENY');
    // No CORS headers at all: a cross-origin page must not be able to read any of this.
    assert.equal(response.headers.get('access-control-allow-origin'), null);
  });
});

test('the index page is served without needing the token in a header', async () => {
  await withServer(async (server) => {
    const response = await fetch(`http://${HOST}:${server.port}/`);
    assert.equal(response.status, 200);
    assert.match(response.headers.get('content-type') ?? '', /text\/html/);
    const html = await response.text();
    assert.match(html, /<html/i);
    // No inline script or style: the CSP forbids both, so a page that needed them
    // would be broken rather than merely non-compliant.
    assert.doesNotMatch(html, /<script(?![^>]*\bsrc=)[^>]*>/i);
    assert.doesNotMatch(html, /<style[\s>]/i);
    assert.match(html, /app\.js/);
  });
});

test('a request arriving through another hostname is refused (DNS rebinding)', async () => {
  await withServer(async (server) => {
    const rebound = await rawRequest(server.port, '/api/dashboard', {
      headers: { host: 'atacante.example', 'x-vnfin-token': server.token },
    });
    assert.equal(rebound.status, 403);
    assert.match(rebound.body, /Host não reconhecido/);

    const legit = await rawRequest(server.port, '/api/dashboard', {
      headers: { host: `${HOST}:${server.port}`, 'x-vnfin-token': server.token },
    });
    assert.equal(legit.status, 200);
  });
});

test('path traversal never leaves the public directory', async () => {
  await withServer(async (server) => {
    for (const attempt of [
      '/../package.json',
      '/..%2fpackage.json',
      '/%2e%2e%2fpackage.json',
      '/../../src/cli.ts',
      '/../src/rules/pt/2026.json',
    ]) {
      const response = await fetch(`http://${HOST}:${server.port}${attempt}`, {
        headers: { 'x-vnfin-token': server.token },
      });
      assert.ok(
        response.status === 403 || response.status === 404,
        `${attempt} devolveu ${response.status}, esperava 403 ou 404`,
      );
      const body = await response.text();
      assert.doesNotMatch(body, /"name": "vn-finance"/, `${attempt} serviu o package.json`);
      assert.doesNotMatch(body, /packVersion/, `${attempt} serviu o pacote de regras`);
    }
  });
});

test('the dashboard model carries everything the panel renders', async () => {
  await withServer(async (server) => {
    const response = await api(server, '/api/dashboard');
    assert.equal(response.status, 200);
    const dashboard = await model(response);

    assert.equal(dashboard.meta.year, 2026);
    assert.match(dashboard.meta.today, /^\d{4}-\d{2}-\d{2}$/);
    assert.equal(dashboard.profile?.nif, '245678999');
    assert.ok(dashboard.pack.summary.obligations > 10, 'o pacote de regras deve estar carregado');
    assert.equal(dashboard.quarters.length, 4);
    assert.ok(dashboard.guarantees.length >= 5);
    assert.ok(dashboard.agenda.length > 0, 'a agenda deve ter obrigações para este perfil');
    assert.ok(dashboard.rules.sources.length > 0);
    assert.ok(dashboard.update.variableCount > 10, 'as variáveis a atualizar devem estar listadas');
    assert.equal(dashboard.update.pending, null, 'sem proposta pendente ao arranque');

    // The art. 53.º watch is the reason turnover is an input, so it must show up.
    assert.ok(dashboard.flags.some((flag) => flag.code.startsWith('ART53')));
  });
});

test('the panel can supply the inputs only the taxpayer has', async () => {
  await withServer(async (server) => {
    const response = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        turnoverPreviousYearCents: 980_000,
        turnoverCurrentYearExpectedCents: 1_100_000,
        ivaRegime: 'trimestral',
        intraCommunityOperations: true,
      }),
    });
    assert.equal(response.status, 200);
    const dashboard = await model(response);
    assert.equal(dashboard.profile?.activity.turnoverPreviousYearCents, 980_000);
    assert.equal(dashboard.profile?.activity.intraCommunityOperations, true);
    assert.equal(dashboard.profile?.iva.regime, 'trimestral');
    assert.ok(
      !dashboard.missingInputs.some((input) => input.includes('ano anterior')),
      'o valor fornecido deixa de estar em falta',
    );

    const bad = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ ivaRegime: 'isento_ao_contrario' }),
    });
    assert.equal(bad.status, 400);

    // An empty box in the form is a decision, not an omission: it removes the
    // figure so the app goes back to saying it cannot run that check. A box the
    // form sends as absent, on the other hand, must leave the value alone.
    const cleared = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ turnoverPreviousYearCents: null }),
    });
    assert.equal(cleared.status, 200);
    const afterClear = await model(cleared);
    assert.equal(afterClear.profile?.activity.turnoverPreviousYearCents, undefined);
    assert.equal(
      afterClear.profile?.activity.turnoverCurrentYearExpectedCents,
      1_100_000,
      'o campo não mencionado mantém o valor guardado',
    );
    assert.ok(afterClear.missingInputs.some((input) => input.includes('ano anterior')));

    const rejected = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ turnoverPreviousYearCents: 12.5 }),
    });
    assert.equal(rejected.status, 400, 'cêntimos fracionados continuam a ser recusados');
  });
});

test('an invoice added from the panel appears in the ledger with its arithmetic intact', async () => {
  await withServer(async (server) => {
    const response = await api(server, '/api/invoices', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        date: '2026-09-10',
        clientName: 'ACME, Lda.',
        clientNif: '501234560',
        clientCountry: 'PT',
        baseCents: 1_200_00,
        ivaRateBp: 0,
        retentionBp: 2300,
        vatTreatment: 'isento_art53',
      }),
    });
    assert.equal(response.status, 200);
    const dashboard = await model(response);
    assert.equal(dashboard.invoices.length, 1);
    const invoice = dashboard.invoices[0];
    assert.equal(invoice?.baseCents, 120_000);
    assert.equal(invoice?.number, 'FR 2026/001', 'a numeração é atribuída quando não é indicada');
    assert.equal(dashboard.quarters[2]?.totals.baseCents, 120_000, 'a fatura entra no 3.º trimestre');
    // The panel needs the pack's rates to offer a sensible default on the first
    // invoice, before there is any history to learn from.
    assert.equal(dashboard.defaults.ivaNormalBp, 2300);
    assert.equal(dashboard.defaults.withholdingResidentBp, 2300);

    const bad = await api(server, '/api/invoices', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ date: '2026-09-10', clientName: 'X', clientCountry: 'PT', baseCents: 12.5, ivaRateBp: 2300, retentionBp: 0, vatTreatment: 'iva_pt' }),
    });
    assert.equal(bad.status, 400, 'cêntimos fracionados têm de ser recusados');

    const wrongTreatment = await api(server, '/api/invoices', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ date: '2026-09-10', clientName: 'X', clientCountry: 'DE', baseCents: 1000, ivaRateBp: 2300, retentionBp: 0, vatTreatment: 'nao_existe' }),
    });
    assert.equal(wrongTreatment.status, 400);
  });
});

test('an obligation can be marked as handled, and the agenda reflects it', async () => {
  await withServer(async (server) => {
    const before = await model(await api(server, '/api/dashboard'));
    const target = before.agenda.find((instance) => instance.status === 'due_soon' || instance.status === 'future');
    assert.ok(target, 'preciso de uma obrigação por cumprir para este teste');

    const response = await api(server, '/api/obligations/complete', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id: target.id, ruleId: target.ruleId, dueDate: target.dueDate }),
    });
    assert.equal(response.status, 200);
    const after = await model(response);
    const updated = after.agenda.find((instance) => instance.id === target.id);
    assert.equal(updated?.status, 'done');
    // And the completion survives a fresh read, because it is in the vault.
    const reread = await model(await api(server, '/api/dashboard'));
    assert.equal(reread.agenda.find((instance) => instance.id === target.id)?.status, 'done');
  });
});

test('applying a rule update without a pending proposal is refused', async () => {
  await withServer(async (server) => {
    const response = await api(server, '/api/update/apply', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ verified: false }),
    });
    assert.equal(response.status, 409);
    assert.match(((await response.json()) as { error: string }).error, /proposta pendente/);
  });
});

test('asking the assistant to send without a key sends nothing and says so', async () => {
  await withServer(async (server) => {
    const response = await api(server, '/api/update/send', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({}),
    });
    assert.equal(response.status, 400);
    assert.match(((await response.json()) as { error: string }).error, /não há chave DeepSeek/);
  });
});

test('every asset the page references is served, with the right content type', async () => {
  await withServer(async (server) => {
    const page = await fetch(`http://${HOST}:${server.port}/`);
    assert.equal(page.status, 200);
    const html = await page.text();

    const references = [...html.matchAll(/(?:href|src)="(\.\/[^"]+)"/g)]
      .map((match) => match[1])
      .filter((value): value is string => value !== undefined);
    assert.ok(references.length > 0, 'a página tem de referenciar os seus próprios recursos');

    for (const reference of new Set(references)) {
      const asset = await fetch(`http://${HOST}:${server.port}${reference.replace('./', '/')}`);
      assert.equal(asset.status, 200, `${reference} não está a ser servido`);
      const type = asset.headers.get('content-type') ?? '';
      if (reference.endsWith('.css')) assert.match(type, /text\/css/, `${reference}: tipo errado`);
      if (reference.endsWith('.js')) assert.match(type, /javascript/, `${reference}: tipo errado`);
      const body = await asset.text();
      assert.ok(body.length > 0, `${reference} está vazio`);
      if (reference.endsWith('.js')) {
        // The panel's behaviour must live in a module the CSP allows, and it must
        // actually talk to the API rather than rendering fixtures.
        assert.match(body, /\/api\/dashboard/);
        assert.match(body, /X-VNFIN-Token|vnfin\.token/i);
      }
    }
  });
});

test('the panel is served as correct Portuguese, with no encoding damage', async () => {
  // A structural test cannot see this: the file parsed, was served, and contained
  // the right API calls — while every accented character in it was mangled. UTF-8
  // decoded as cp1252 and re-encoded leaves exactly this signature, so it is worth
  // an explicit guard in a Portuguese-language product.
  await withServer(async (server) => {
    for (const path of ['/', '/app.css', '/app.js']) {
      const body = await (await fetch(`http://${HOST}:${server.port}${path}`)).text();
      assert.equal(
        (body.match(/\u00c3[\u0080-\u00bf]/g) ?? []).length,
        0,
        `${path} tem mojibake (UTF-8 lido como cp1252)`,
      );
      assert.equal(
        (body.match(/\ufffd/g) ?? []).length,
        0,
        `${path} tem caracteres de substituição: perdeu texto`,
      );
    }

    const html = await (await fetch(`http://${HOST}:${server.port}/`)).text();
    const js = await (await fetch(`http://${HOST}:${server.port}/app.js`)).text();

    // Accented Portuguese must be present, or the file is ASCII-flattened.
    assert.ok(html.includes('Segurança'), 'index.html perdeu os acentos');
    for (const phrase of [
      'Concluído',
      'obrigação',
      'Segurança',
      'Não aplicável',
      'cêntimos',
      'faturação',
      'histórico',
    ]) {
      assert.ok(js.includes(phrase), `app.js perdeu "${phrase}"`);
    }
  });
});

test('an invoice whose treatment and VAT rate contradict each other is refused', async () => {
  await withServer(async (server) => {
    const post = (body: Record<string, unknown>): Promise<Response> =>
      api(server, '/api/invoices', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({
          date: '2026-09-10',
          clientName: 'ACME, Lda.',
          clientCountry: 'PT',
          baseCents: 100_000,
          retentionBp: 2300,
          ...body,
        }),
      });

    // A domestic taxable invoice with no VAT is not a shape that exists.
    assert.equal((await post({ vatTreatment: 'iva_pt', ivaRateBp: 0 })).status, 400);
    // And an exempt invoice must not charge VAT.
    assert.equal((await post({ vatTreatment: 'isento_art53', ivaRateBp: 2300 })).status, 400);
    assert.equal((await post({ vatTreatment: 'autoliquidacao_ue', ivaRateBp: 2300 })).status, 400);
    // The coherent combinations are accepted.
    assert.equal((await post({ vatTreatment: 'iva_pt', ivaRateBp: 2300 })).status, 200);
    assert.equal((await post({ vatTreatment: 'isento_art53', ivaRateBp: 0 })).status, 200);
  });
});

test('unknown routes, wrong methods and oversized bodies are refused', async () => {
  await withServer(async (server) => {
    const unknown = await api(server, '/api/nao-existe');
    assert.equal(unknown.status, 404);

    const options = await api(server, '/api/dashboard', { method: 'OPTIONS' });
    assert.equal(options.status, 405);

    const huge = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ note: 'x'.repeat(300 * 1024) }),
    });
    assert.equal(huge.status, 413);

    const notJson = await api(server, '/api/profile', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: 'isto não é json',
    });
    assert.equal(notJson.status, 400);
  });
});
