/**
 * The two things the panel can now do that only a filesystem could before:
 * choose where the vault lives, and read a `fatura-recibo` PDF into the ledger.
 *
 * Both are writes to somebody's disk, so these tests are as much about the
 * refusals as about the happy path: a vault inside this repository, a file that
 * is not a PDF, an invoice that somebody else issued. Each of those has to fail
 * with an explanation rather than a plausible record.
 */

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { createDefaultProfile } from '../core/profile.ts';
import { faturaReciboPdf } from '../core/receipt-fixture.ts';
import { Vault, readVaultPointer } from '../store/vault.ts';
import { startWebServer, type RunningWebServer } from './server.ts';
import type { DashboardModel } from './report.ts';

const HOST = '127.0.0.1';
const TOKEN = 'teste-token-de-sessao-com-tamanho-suficiente';
const PROFILE_NIF = '123456789';

interface Harness {
  server: RunningWebServer;
  vault: Vault;
  /** A directory that stands in for the user's home: the pointer file lives here. */
  home: string;
}

async function withServer(run: (harness: Harness) => Promise<void>): Promise<void> {
  const root = mkdtempSync(join(tmpdir(), 'vnfin-receipts-'));
  const home = join(root, 'casa');
  const dir = join(root, 'cofre');
  mkdirSync(home, { recursive: true });
  const vault = new Vault(dir);
  vault.ensure();
  vault.saveProfile(
    createDefaultProfile({
      nif: PROFILE_NIF,
      name: 'Maria Exemplo Silva',
      ivaRegime: 'trimestral',
      startDate: '2019-04-01',
    }),
  );

  const server = await startWebServer({
    vault,
    version: 'teste',
    year: 2026,
    port: 0,
    token: TOKEN,
    // The pointer lives where it does in real life — under the default data dir —
    // but under a temporary home, so a test never writes to the real one.
    vaultPointerFile: join(home, '.vn-finance', 'vault-location.json'),
  });
  try {
    await run({ server, vault, home });
  } finally {
    await server.close();
    rmSync(root, { recursive: true, force: true });
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

function postJson(server: RunningWebServer, path: string, body: unknown): Promise<Response> {
  return api(server, path, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

async function uploadReceipt(server: RunningWebServer, name = 'fatura-teste.pdf'): Promise<Record<string, unknown>> {
  const response = await api(server, `/api/receipts/upload?name=${encodeURIComponent(name)}`, {
    method: 'POST',
    headers: { 'content-type': 'application/octet-stream' },
    body: faturaReciboPdf(),
  });
  assert.equal(response.status, 200);
  return (await response.json()) as Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Choosing the vault folder
// ---------------------------------------------------------------------------

test('the folder chooser lists directories, never files', async () => {
  await withServer(async ({ server, home }) => {
    const root = join(home, 'Documentos');
    mkdirSync(join(root, 'subpasta'), { recursive: true });
    mkdirSync(join(root, '.escondida'), { recursive: true });
    writeFileSync(join(root, 'nao-mostrar.txt'), 'x', 'utf8');

    const response = await api(server, `/api/vault/browse?path=${encodeURIComponent(root)}`);
    assert.equal(response.status, 200);
    const listing = (await response.json()) as {
      entries: Array<{ name: string }>;
      roots: unknown[];
      path: string;
    };

    assert.deepEqual(
      listing.entries.map((entry) => entry.name),
      ['subpasta'],
      'só pastas, e nenhuma pasta escondida',
    );
    assert.ok(listing.roots.length > 0, 'o escolhedor tem de propor pontos de partida');
    assert.equal(listing.path, root);
  });
});

test('the folder chooser needs the session token like every other call', async () => {
  await withServer(async ({ server }) => {
    const response = await fetch(`http://${HOST}:${server.port}/api/vault/browse`);
    assert.equal(response.status, 401);
  });
});

test('choosing a folder creates the vault there and remembers it', async () => {
  await withServer(async ({ server, home }) => {
    const target = join(home, 'Desktop', 'vn-finance-cofre');

    const response = await postJson(server, '/api/vault', { path: target });
    assert.equal(response.status, 200);
    const payload = (await response.json()) as { ok: boolean; model: DashboardModel; vault: string };

    assert.equal(payload.vault, target);
    assert.equal(payload.model.meta.dataDir, target);
    assert.equal(payload.model.meta.dataDirSource, 'pointer');
    assert.ok(existsSync(join(target, 'documents')), 'o cofre criado tem de ter a sua estrutura');
    assert.ok(existsSync(join(target, 'ledger')));

    const pointer = readVaultPointer({ USERPROFILE: home, HOME: home });
    assert.equal(pointer?.path, target, 'a pasta escolhida fica lembrada para a próxima vez');
    assert.ok(existsSync(join(home, '.vn-finance', 'vault-location.json')));
  });
});

test('a vault inside the application repository is refused, with the reason', async () => {
  await withServer(async ({ server }) => {
    const inside = join(process.cwd(), 'src');
    const response = await postJson(server, '/api/vault', { path: inside });

    assert.equal(response.status, 400);
    const payload = (await response.json()) as { ok: boolean; error: string };
    assert.equal(payload.ok, false);
    assert.match(payload.error, /repositório da aplicação/);
  });
});

test('a relative folder is refused: the chooser always sends an absolute one', async () => {
  await withServer(async ({ server }) => {
    const response = await postJson(server, '/api/vault', { path: 'cofre-relativo' });
    assert.equal(response.status, 400);
  });
});

// ---------------------------------------------------------------------------
// Importing a fatura-recibo
// ---------------------------------------------------------------------------

test('a fatura-recibo is archived in the vault and read back field by field', async () => {
  await withServer(async ({ server, vault }) => {
    const payload = await uploadReceipt(server, 'FR2026.pdf');
    const added = payload['added'] as { file: string; sha256: string };
    const draft = payload['draft'] as {
      baseCents: number;
      ivaRateBp: number;
      date: string;
      number: string;
      issuer: { nif: string };
      problems: string[];
    };
    const model = payload['model'] as DashboardModel;

    assert.equal(draft.baseCents, 100_000);
    assert.equal(draft.ivaRateBp, 2300);
    assert.equal(draft.date, '2026-03-15');
    assert.equal(draft.issuer.nif, PROFILE_NIF);
    assert.deepEqual(draft.problems, []);

    // A local copy exists, named by its content hash, and the ledger is untouched:
    // reading a document is not the same act as registering an invoice.
    assert.ok(existsSync(vault.path(added.file)), `o PDF devia estar em ${added.file}`);
    assert.equal(payload['issuerMatchesProfile'], true);
    assert.deepEqual(model.invoices, []);
    assert.equal(model.vault.entries.length, 1);
    assert.equal(model.vault.entries[0]?.kind, 'fatura');
    assert.equal(model.vault.entries[0]?.invoiceId, null);
    assert.match(added.file, /^documents\/[0-9a-f]{12}-FR2026\.pdf$/);
  });
});

test('the archived copy is byte-identical to the file that was uploaded', async () => {
  await withServer(async ({ server, vault }) => {
    const pdf = faturaReciboPdf();
    const payload = await uploadReceipt(server);
    const added = payload['added'] as { file: string; sha256: string };

    const stored = readFileSync(vault.path(added.file));
    assert.deepEqual(stored, pdf, 'o cofre guarda o ficheiro tal como foi recebido');
    assert.equal(vault.hashBytes(pdf), added.sha256);
  });
});

test('recording the invoice appends it to the ledger and links the two records', async () => {
  await withServer(async ({ server, vault }) => {
    const uploaded = await uploadReceipt(server);
    const added = uploaded['added'] as { file: string };

    const response = await postJson(server, '/api/receipts/record', { documentFile: added.file });
    assert.equal(response.status, 200);
    const payload = (await response.json()) as {
      invoice: { id: string; number: string; baseCents: number; ivaRateBp: number; status: string };
      divergences: string[];
      warnings: string[];
      model: DashboardModel;
    };

    assert.equal(payload.invoice.number, 'FR TESTE/2026/7');
    assert.equal(payload.invoice.baseCents, 100_000);
    assert.equal(payload.invoice.ivaRateBp, 2300);
    assert.equal(payload.invoice.status, 'paid');

    assert.deepEqual(payload.model.invoices.map((invoice) => invoice.id), [payload.invoice.id]);
    assert.equal(payload.model.vault.entries[0]?.invoiceId, payload.invoice.id);

    // The ledger is append-only JSONL, so the invoice is in the file itself.
    const ledger = readFileSync(vault.path('ledger/invoices.jsonl'), 'utf8').trim().split('\n');
    assert.equal(ledger.length, 1);
    assert.equal((JSON.parse(ledger[0] ?? '{}') as { id: string }).id, payload.invoice.id);
  });
});

test('recording refuses a document that somebody else issued', async () => {
  await withServer(async ({ server }) => {
    // The fixture's issuer is the profile's NIF; here it is a supplier's, and the
    // customer is the taxpayer, which is a purchase and not income.
    const response = await api(server, '/api/receipts/upload?name=recebida.pdf', {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: faturaReciboPdf({ issuerNif: '501234560', customerNif: PROFILE_NIF }),
    });
    const uploaded = (await response.json()) as { added: { file: string }; issuerMatchesProfile: boolean };
    assert.equal(uploaded.issuerMatchesProfile, false);

    const recorded = await postJson(server, '/api/receipts/record', { documentFile: uploaded.added.file });
    assert.equal(recorded.status, 400);
    const payload = (await recorded.json()) as { error: string };
    assert.match(payload.error, /RECEBESTE|livro de despesas/);
  });
});

test('a correction made in the form is registered and written to the audit trail', async () => {
  await withServer(async ({ server, vault }) => {
    const uploaded = await uploadReceipt(server);
    const added = uploaded['added'] as { file: string };

    const response = await postJson(server, '/api/receipts/record', {
      documentFile: added.file,
      baseCents: 110_000,
      number: 'FR TESTE/2026/9',
    });
    assert.equal(response.status, 200);
    const payload = (await response.json()) as {
      invoice: { baseCents: number; number: string };
      divergences: string[];
    };

    assert.equal(payload.invoice.baseCents, 110_000);
    assert.equal(payload.invoice.number, 'FR TESTE/2026/9');
    assert.match(payload.divergences.join(' '), /valor ilíquido/);

    const audit = readFileSync(vault.path('audit/audit.jsonl'), 'utf8');
    assert.match(audit, /receipt\.recorded/);
    assert.match(audit, /corrigido/);
  });
});

test('a file that is not a PDF is refused by the receipt route', async () => {
  await withServer(async ({ server }) => {
    const response = await api(server, '/api/receipts/upload?name=notas.txt', {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: Buffer.from('apenas apontamentos, não é uma fatura'),
    });

    assert.equal(response.status, 400);
    const payload = (await response.json()) as { error: string };
    assert.match(payload.error, /não é um PDF/);
  });
});

test('recording a document that is not in the index is refused', async () => {
  await withServer(async ({ server }) => {
    const response = await postJson(server, '/api/receipts/record', {
      documentFile: 'documents/000000000000-inventado.pdf',
    });
    assert.equal(response.status, 400);
    const payload = (await response.json()) as { error: string };
    assert.match(payload.error, /não está indexado/);
  });
});

test('the same PDF handed over twice is not recorded as two invoices', async () => {
  await withServer(async ({ server }) => {
    const first = await uploadReceipt(server);
    const added = first['added'] as { file: string };
    const recorded = await postJson(server, '/api/receipts/record', { documentFile: added.file });
    assert.equal(recorded.status, 200);

    // A second upload of the same bytes: refused, pointing at the invoice it is.
    const again = await api(server, '/api/receipts/upload?name=copia.pdf', {
      method: 'POST',
      headers: { 'content-type': 'application/octet-stream' },
      body: faturaReciboPdf(),
    });
    assert.equal(again.status, 409);
    const payload = (await again.json()) as { error: string };
    assert.match(payload.error, /já foi registado como fatura/);

    const dashboard = await model(await api(server, '/api/dashboard'));
    assert.equal(dashboard.invoices.length, 1, 'uma fatura por documento, mesmo que o PDF apareça duas vezes');
    assert.equal(dashboard.vault.entries.length, 1, 'o mesmo conteúdo não cria uma segunda entrada no índice');
  });
});

test('a document that is archived but not yet registered is reused, not indexed twice', async () => {
  await withServer(async ({ server }) => {
    await uploadReceipt(server, 'primeiro.pdf');
    const second = await uploadReceipt(server, 'outro-nome.pdf');
    const added = second['added'] as { file: string };

    assert.match(added.file, /primeiro\.pdf$/, 'o nome guardado é o da primeira vez');
    const dashboard = await model(await api(server, '/api/dashboard'));
    assert.equal(dashboard.vault.entries.length, 1);
    assert.deepEqual(dashboard.invoices, []);
  });
});

test('the model tells the panel where the vault is and where that choice came from', async () => {
  await withServer(async ({ server }) => {
    const dashboard = await model(await api(server, '/api/dashboard'));

    assert.equal(dashboard.meta.dataDirSource, 'flag', 'um cofre indicado pela linha de comandos não se pergunta');
    assert.equal(dashboard.meta.vaultInUse, true);
    assert.equal(typeof dashboard.meta.dataDirPointerPath, 'string');
  });
});
