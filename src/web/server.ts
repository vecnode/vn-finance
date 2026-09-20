/**
 * The local panel server.
 *
 * A single-user application that exposes someone's entire financial life on a TCP
 * port deserves more care than "it is only loopback". This server therefore:
 *
 *   1. BINDS TO 127.0.0.1 ONLY, never 0.0.0.0.
 *   2. VALIDATES THE Host HEADER against the loopback names it was started with.
 *      Without this, a page on any website could reach a loopback server through a
 *      DNS name that resolves to 127.0.0.1 (DNS rebinding) — the browser would send
 *      the attacker's hostname, which is exactly what this check rejects.
 *   3. REQUIRES A PER-RUN TOKEN on every /api call, sent as a header. A third-party
 *      page cannot read the token, and cannot set a custom header cross-origin
 *      without a preflight this server never approves. So a malicious page cannot
 *      read the ledger, and cannot record an invoice.
 *   4. SENDS NO CORS HEADERS AT ALL, so cross-origin reads fail in the browser.
 *   5. SERVES ONLY FILES UNDER ./public, resolving every path and refusing anything
 *      that escapes it.
 *   6. IMPOSES A CONTENT SECURITY POLICY with no inline script and no inline style,
 *      which is why all behaviour lives in app.js.
 *
 * It contains no tax logic: every figure comes from `buildDashboard`, which uses
 * the same deterministic core as the command line.
 */

import { randomBytes, timingSafeEqual } from 'node:crypto';
import { existsSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { todayInLisbon } from '../core/dates.ts';
import { type Invoice, type VatTreatment } from '../core/estimate.ts';
import { loadRulePack, resolvePackPath } from '../core/rules.ts';
import type { IvaRegime, LoadedPack, TaxProfile } from '../core/types.ts';
import { resolveApiKey } from '../ai/keyring.ts';
import { DEFAULT_MODEL, DeepSeekClient, scrubCredentials } from '../ai/deepseek.ts';
import { redactForSend } from '../ai/redact.ts';
import {
  applyProposal,
  buildUpdateMessages,
  buildUpdateRequest,
  diffProposal,
  parseUpdateResponse,
  type UpdateProposal,
} from '../ai/update.ts';
import { Vault } from '../store/vault.ts';
import { buildDashboard, type DashboardModel } from './report.ts';

const PUBLIC_DIR = fileURLToPath(new URL('./public/', import.meta.url));
const MAX_BODY_BYTES = 256 * 1024;
const UPDATE_PROPOSAL_PATH = (year: number): string => `rules/proposals-${year}.json`;

const CONTENT_TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

const SECURITY_HEADERS: Record<string, string> = {
  'content-security-policy':
    "default-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; " +
    "img-src 'self' data:; style-src 'self'; script-src 'self'; connect-src 'self'",
  'x-content-type-options': 'nosniff',
  'referrer-policy': 'no-referrer',
  'x-frame-options': 'DENY',
  'cross-origin-opener-policy': 'same-origin',
  'cross-origin-resource-policy': 'same-origin',
  'cache-control': 'no-store',
};

export interface WebServerOptions {
  vault: Vault;
  version: string;
  year?: number;
  packPath?: string;
  host?: string;
  port?: number;
  token?: string;
  model?: string;
}

export interface RunningWebServer {
  url: string;
  token: string;
  host: string;
  port: number;
  close(): Promise<void>;
}

interface RequestContext {
  vault: Vault;
  version: string;
  year: number;
  packPath: string | undefined;
  token: string;
  model: string;
  allowedHosts: Set<string>;
}

class HttpError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'HttpError';
    this.status = status;
  }
}

function loadCurrentPack(context: RequestContext): LoadedPack {
  const resolved = resolvePackPath(context.year, context.packPath);
  if (resolved === null) {
    throw new HttpError(500, 'não há pacote de regras disponível para este ano.');
  }
  return loadRulePack(resolved.path);
}

function buildModel(context: RequestContext, pendingProposal?: UpdateProposal | null): DashboardModel {
  const loaded = loadCurrentPack(context);
  const key = resolveApiKey({ dataDir: context.vault.dir });
  return buildDashboard({
    vault: context.vault,
    loaded,
    year: context.year,
    // Read the clock per request, not once at startup: a panel left running
    // across midnight must not keep yesterday's date, or every "vence amanhã"
    // becomes wrong.
    today: todayInLisbon(),
    version: context.version,
    apiKeyAvailable: key.key !== null,
    apiKeySource: key.source,
    ...(pendingProposal === undefined ? {} : { pendingProposal }),
  });
}

// ---------------------------------------------------------------------------
// Request helpers
// ---------------------------------------------------------------------------

async function readJsonBody(request: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = chunk as Buffer;
    size += buffer.length;
    if (size > MAX_BODY_BYTES) throw new HttpError(413, 'pedido demasiado grande.');
    chunks.push(buffer);
  }
  if (size === 0) return {};
  try {
    const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8')) as unknown;
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
      throw new HttpError(400, 'o corpo do pedido tem de ser um objeto JSON.');
    }
    return parsed as Record<string, unknown>;
  } catch (cause) {
    if (cause instanceof HttpError) throw cause;
    throw new HttpError(400, 'o corpo do pedido não é JSON válido.');
  }
}

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    ...SECURITY_HEADERS,
    'content-type': 'application/json; charset=utf-8',
    'content-length': Buffer.byteLength(payload),
  });
  response.end(payload);
}

function sendText(response: ServerResponse, status: number, text: string): void {
  response.writeHead(status, {
    ...SECURITY_HEADERS,
    'content-type': 'text/plain; charset=utf-8',
    'content-length': Buffer.byteLength(text),
  });
  response.end(text);
}

function tokensMatch(expected: string, provided: string): boolean {
  const left = Buffer.from(expected);
  const right = Buffer.from(provided);
  if (left.length !== right.length || left.length === 0) return false;
  return timingSafeEqual(left, right);
}

function requireToken(request: IncomingMessage, context: RequestContext): void {
  const header = request.headers['x-vnfin-token'];
  const provided = Array.isArray(header) ? (header[0] ?? '') : (header ?? '');
  if (!tokensMatch(context.token, provided)) {
    throw new HttpError(
      401,
      'sessão sem chave válida. Abre o endereço que o `vnfin web` imprimiu nesta máquina.',
    );
  }
}

// ---------------------------------------------------------------------------
// Input validation
// ---------------------------------------------------------------------------

function requireString(body: Record<string, unknown>, key: string, max = 200): string {
  const value = body[key];
  if (typeof value !== 'string' || value.trim() === '') {
    throw new HttpError(400, `campo "${key}" em falta ou vazio.`);
  }
  if (value.length > max) throw new HttpError(400, `campo "${key}" demasiado longo.`);
  return value.trim();
}

function optionalString(body: Record<string, unknown>, key: string, max = 200): string | null {
  const value = body[key];
  if (value === undefined || value === null || value === '') return null;
  if (typeof value !== 'string') throw new HttpError(400, `campo "${key}" tem de ser texto.`);
  if (value.length > max) throw new HttpError(400, `campo "${key}" demasiado longo.`);
  return value.trim();
}

function requireInt(body: Record<string, unknown>, key: string, min: number, max: number): number {
  const value = body[key];
  if (typeof value !== 'number' || !Number.isInteger(value)) {
    throw new HttpError(400, `campo "${key}" tem de ser um número inteiro.`);
  }
  if (value < min || value > max) {
    throw new HttpError(400, `campo "${key}" está fora do intervalo permitido (${min}–${max}).`);
  }
  return value;
}

function optionalBool(body: Record<string, unknown>, key: string): boolean | undefined {
  const value = body[key];
  if (value === undefined || value === null) return undefined;
  if (typeof value !== 'boolean') throw new HttpError(400, `campo "${key}" tem de ser verdadeiro/falso.`);
  return value;
}

function requireDate(body: Record<string, unknown>, key: string): string {
  const value = requireString(body, key, 10);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    throw new HttpError(400, `campo "${key}" tem de estar em formato AAAA-MM-DD.`);
  }
  return value;
}

const VAT_TREATMENTS: readonly VatTreatment[] = [
  'iva_pt',
  'autoliquidacao_ue',
  'isento_art53',
  'exportacao',
];
const INVOICE_STATUSES = ['issued', 'paid', 'pending'] as const;
const IVA_REGIMES: readonly IvaRegime[] = ['isento_art53', 'trimestral', 'mensal'];

/** Every API route, with the single method it accepts. */
const API_ROUTES: Record<string, 'GET' | 'POST'> = {
  '/api/dashboard': 'GET',
  '/api/profile': 'POST',
  '/api/invoices': 'POST',
  '/api/obligations/complete': 'POST',
  '/api/documents': 'POST',
  '/api/update/send': 'POST',
  '/api/update/apply': 'POST',
  '/api/update/discard': 'POST',
};

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async function handleProfile(context: RequestContext, body: Record<string, unknown>): Promise<DashboardModel> {
  const profile = context.vault.loadProfile();
  if (profile === null) {
    throw new HttpError(409, 'não existe perfil neste cofre. Corre `vnfin init` primeiro.');
  }

  const updated: TaxProfile = {
    ...profile,
    activity: { ...profile.activity },
    iva: { ...profile.iva },
  };

  const previous = body['turnoverPreviousYearCents'];
  if (previous !== undefined && previous !== null) {
    if (typeof previous !== 'number' || !Number.isInteger(previous) || previous < 0) {
      throw new HttpError(400, 'o volume de negócios do ano anterior tem de ser um valor em cêntimos.');
    }
    updated.activity.turnoverPreviousYearCents = previous;
  }

  const expected = body['turnoverCurrentYearExpectedCents'];
  if (expected !== undefined && expected !== null) {
    if (typeof expected !== 'number' || !Number.isInteger(expected) || expected < 0) {
      throw new HttpError(400, 'a estimativa para o ano corrente tem de ser um valor em cêntimos.');
    }
    updated.activity.turnoverCurrentYearExpectedCents = expected;
  }

  const regime = body['ivaRegime'];
  if (regime !== undefined && regime !== null) {
    if (typeof regime !== 'string' || !(IVA_REGIMES as readonly string[]).includes(regime)) {
      throw new HttpError(400, 'regime de IVA inválido: usa isento_art53, trimestral ou mensal.');
    }
    updated.iva.regime = regime as IvaRegime;
  }

  const eu = optionalBool(body, 'intraCommunityOperations');
  if (eu !== undefined) updated.activity.intraCommunityOperations = eu;
  const third = optionalBool(body, 'exports');
  if (third !== undefined) updated.activity.exports = third;
  const startup = optionalBool(body, 'startupExemptionActive');
  if (startup !== undefined) updated.ss = { ...updated.ss, startupExemptionActive: startup };

  const startDate = optionalString(body, 'startDate', 10);
  if (startDate !== null) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(startDate)) {
      throw new HttpError(400, 'a data de início de atividade tem de estar em formato AAAA-MM-DD.');
    }
    updated.activity.startDate = startDate;
  }

  context.vault.saveProfile(updated);
  context.vault.appendAudit({ action: 'profile.updated' });
  return buildModel(context);
}

async function handleInvoice(context: RequestContext, body: Record<string, unknown>): Promise<DashboardModel> {
  if (context.vault.loadProfile() === null) {
    throw new HttpError(409, 'não existe perfil neste cofre. Corre `vnfin init` primeiro.');
  }

  const country = requireString(body, 'clientCountry', 3).toUpperCase();
  const treatment = requireString(body, 'vatTreatment', 24) as VatTreatment;
  if (!VAT_TREATMENTS.includes(treatment)) {
    throw new HttpError(400, 'tratamento de IVA inválido nesta fatura.');
  }

  const status = optionalString(body, 'status', 12) ?? 'issued';
  if (!(INVOICE_STATUSES as readonly string[]).includes(status)) {
    throw new HttpError(400, 'estado de fatura inválido.');
  }

  const baseCents = requireInt(body, 'baseCents', 1, 10_000_000_000);
  const ivaRateBp = requireInt(body, 'ivaRateBp', 0, 10_000);
  const retentionBp = requireInt(body, 'retentionBp', 0, 10_000);

  // A treatment and a rate that contradict each other describe an invoice that
  // cannot exist: only iva_pt charges Portuguese VAT, and no Portuguese rate is
  // zero (an exempt invoice uses the exemption treatment instead).
  if (treatment === 'iva_pt' && ivaRateBp === 0) {
    throw new HttpError(
      400,
      'uma fatura com tratamento "iva_pt" tem de ter uma taxa de IVA. Se está isenta, o tratamento ' +
        'correto é "isento_art53".',
    );
  }
  if (treatment !== 'iva_pt' && ivaRateBp !== 0) {
    throw new HttpError(
      400,
      `o tratamento "${treatment}" não liquida IVA português, por isso a taxa tem de ser 0 ` +
        `(recebido ${ivaRateBp} pontos base).`,
    );
  }

  const count = context.vault.loadInvoices().length;

  const invoice: Invoice = {
    id: `inv_${Date.now().toString(36)}_${randomBytes(3).toString('hex')}`,
    number: optionalString(body, 'number', 40) ?? `FR ${context.year}/${String(count + 1).padStart(3, '0')}`,
    date: requireDate(body, 'date'),
    clientName: requireString(body, 'clientName', 120),
    clientNif: optionalString(body, 'clientNif', 20),
    clientCountry: country,
    description: optionalString(body, 'description', 300) ?? '',
    baseCents,
    ivaRateBp,
    vatTreatment: treatment,
    retentionBp,
    atcud: optionalString(body, 'atcud', 40),
    status: status as Invoice['status'],
    paymentProofInVault: optionalBool(body, 'paymentProofInVault') ?? false,
  };

  context.vault.appendInvoice(invoice);
  return buildModel(context);
}

async function handleComplete(context: RequestContext, body: Record<string, unknown>): Promise<DashboardModel> {
  const id = requireString(body, 'id', 120);
  const ruleId = requireString(body, 'ruleId', 80);
  const dueDate = requireDate(body, 'dueDate');
  const note = optionalString(body, 'note', 200);
  context.vault.markCompleted(
    { id, ruleId, dueDate, ...(note === null ? {} : { note }) },
    new Date().toISOString(),
  );
  return buildModel(context);
}

async function handleDocument(context: RequestContext, body: Record<string, unknown>): Promise<Record<string, unknown>> {
  const path = requireString(body, 'path', 1024);
  const kind = optionalString(body, 'kind', 40);
  const obligationId = optionalString(body, 'obligationId', 80);
  let added: { file: string; sha256: string };
  try {
    added = context.vault.addDocument(path, {
      ...(kind === null ? {} : { kind }),
      obligationId,
    });
  } catch (cause) {
    // A path that does not exist is a bad request, not an internal failure: the
    // message from the vault already says which file was not found.
    throw new HttpError(400, (cause as Error).message);
  }
  return { ok: true, model: buildModel(context), added };
}

/**
 * The only network call in the whole application, and it is deliberate: a POST,
 * behind the token, with a payload built from the rule pack alone.
 */
async function handleUpdateSend(context: RequestContext): Promise<DashboardModel> {
  const key = resolveApiKey({ dataDir: context.vault.dir });
  if (key.key === null) {
    throw new HttpError(
      400,
      'não há chave DeepSeek configurada: nada foi enviado. Corre `vnfin ai-key set`.',
    );
  }

  const loaded = loadCurrentPack(context);
  const request = buildUpdateRequest(loaded.pack);
  let answer;
  try {
    answer = await new DeepSeekClient(key.key).complete(redactForSend(buildUpdateMessages(request)), {
      model: context.model || DEFAULT_MODEL,
      maxTokens: 2000,
      temperature: 0,
    });
  } catch (cause) {
    // The provider's message can echo the credential; scrub it before it reaches
    // the browser or the terminal.
    throw new HttpError(502, scrubCredentials((cause as Error).message));
  }

  const parsed = parseUpdateResponse(answer.text, request);
  const proposal: UpdateProposal = {
    year: loaded.pack.year,
    asOf: parsed.asOf === '' ? todayInLisbon() : parsed.asOf,
    model: answer.model,
    proposedAt: todayInLisbon(),
    values: parsed.values,
    problems: parsed.problems,
  };
  context.vault.ensure();
  context.vault.writeJson(UPDATE_PROPOSAL_PATH(loaded.pack.year), proposal);
  context.vault.appendAudit({
    action: 'rules.proposed',
    detail: `${answer.model} · ${parsed.values.length} variáveis · ${parsed.problems.length} problemas`,
  });

  return buildModel(context, proposal);
}

function handleUpdateApply(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  const verified = optionalBool(body, 'verified') ?? false;
  const loaded = loadCurrentPack(context);
  const proposal = context.vault.readJson<UpdateProposal | null>(
    UPDATE_PROPOSAL_PATH(loaded.pack.year),
    null,
  );
  if (proposal === null) {
    throw new HttpError(409, 'não há proposta pendente. Corre a atualização primeiro.');
  }

  const diffs = diffProposal(proposal).filter((diff) => diff.changed);
  if (diffs.length === 0) {
    throw new HttpError(409, 'a proposta pendente não altera nenhum valor.');
  }

  const { pack: updated, applied } = applyProposal(loaded.pack, proposal, {
    at: todayInLisbon(),
    model: proposal.model,
    verifiedByHuman: verified,
  });
  writeFileSync(loaded.path, `${JSON.stringify(updated, null, 2)}\n`, 'utf8');
  context.vault.appendAudit({
    action: 'rules.applied',
    detail: `${applied} valores de ${proposal.model}${verified ? ' (confirmados)' : ''}`,
  });
  return { ok: true, model: buildModel(context), applied, packPath: loaded.path, verified };
}

function handleUpdateDiscard(context: RequestContext): Record<string, unknown> {
  const loaded = loadCurrentPack(context);
  const path = context.vault.path(UPDATE_PROPOSAL_PATH(loaded.pack.year));
  if (existsSync(path)) unlinkSync(path);
  context.vault.appendAudit({ action: 'rules.discarded' });
  return { ok: true, model: buildModel(context, null) };
}

// ---------------------------------------------------------------------------
// Static files
// ---------------------------------------------------------------------------

function serveStatic(request: IncomingMessage, response: ServerResponse, urlPath: string): void {
  const base = resolve(PUBLIC_DIR);
  let decoded: string;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    // A malformed escape (e.g. "/%") is the client's mistake, not a server fault.
    sendText(response, 400, 'caminho inválido.');
    return;
  }
  const relative = decoded === '/' || decoded === '' ? '/index.html' : decoded;
  const candidate = resolve(base, `.${relative}`);

  if (candidate !== base && !candidate.startsWith(`${base}${sep}`)) {
    sendText(response, 403, 'caminho não permitido.');
    return;
  }
  if (!existsSync(candidate) || !statSync(candidate).isFile()) {
    sendText(response, 404, 'não encontrado.');
    return;
  }

  const type = CONTENT_TYPES[extname(candidate).toLowerCase()] ?? 'application/octet-stream';
  const body = statSync(candidate).size;
  response.writeHead(200, {
    ...SECURITY_HEADERS,
    'content-type': type,
    'content-length': body,
  });
  if (request.method === 'HEAD') {
    response.end();
    return;
  }
  response.end(readFileSync(candidate));
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

export async function startWebServer(options: WebServerOptions): Promise<RunningWebServer> {
  const host = options.host ?? '127.0.0.1';
  const port = options.port ?? 7717;
  const token = options.token ?? randomBytes(32).toString('hex');
  const today = todayInLisbon();

  const server = createServer((request, response) => {
    void handleRequest(request, response).catch((cause: unknown) => {
      const error = cause instanceof HttpError ? cause : new HttpError(500, (cause as Error).message);
      if (!response.headersSent) sendJson(response, error.status, { ok: false, error: error.message });
      else response.end();
    });
  });

  const context: RequestContext = {
    vault: options.vault,
    version: options.version,
    year: options.year ?? Number(today.slice(0, 4)),
    packPath: options.packPath,
    token,
    model: options.model ?? DEFAULT_MODEL,
    allowedHosts: new Set(),
  };

  async function handleRequest(request: IncomingMessage, response: ServerResponse): Promise<void> {
    // The Host allowlist is what defeats DNS rebinding: a request that arrived
    // through an attacker's hostname carries that hostname here.
    const hostHeader = request.headers.host ?? '';
    if (!context.allowedHosts.has(hostHeader)) {
      throw new HttpError(403, `Host não reconhecido: ${hostHeader || '(vazio)'}`);
    }

    const url = new URL(request.url ?? '/', `http://${hostHeader}`);
    const path = url.pathname;
    const method = request.method ?? 'GET';

    if (!path.startsWith('/api/')) {
      if (method !== 'GET' && method !== 'HEAD') {
        throw new HttpError(405, 'método não permitido.');
      }
      serveStatic(request, response, path);
      return;
    }

    if (method === 'OPTIONS') throw new HttpError(405, 'preflight não é permitido.');

    // The route table is consulted BEFORE the method, so an unknown path is a 404
    // (no such resource) rather than a 405 (wrong verb for an existing resource).
    const allowed = API_ROUTES[path];
    if (allowed === undefined) throw new HttpError(404, `rota desconhecida: ${path}`);
    if (method !== allowed) throw new HttpError(405, `esta rota só aceita ${allowed}.`);

    requireToken(request, context);

    if (allowed === 'GET') {
      sendJson(response, 200, { ok: true, model: buildModel(context) });
      return;
    }

    const body = await readJsonBody(request);

    switch (path) {
      case '/api/profile':
        sendJson(response, 200, { ok: true, model: await handleProfile(context, body) });
        return;
      case '/api/invoices':
        sendJson(response, 200, { ok: true, model: await handleInvoice(context, body) });
        return;
      case '/api/obligations/complete':
        sendJson(response, 200, { ok: true, model: await handleComplete(context, body) });
        return;
      case '/api/documents':
        sendJson(response, 200, await handleDocument(context, body));
        return;
      case '/api/update/send':
        sendJson(response, 200, { ok: true, model: await handleUpdateSend(context) });
        return;
      case '/api/update/apply':
        sendJson(response, 200, handleUpdateApply(context, body));
        return;
      case '/api/update/discard':
        sendJson(response, 200, handleUpdateDiscard(context));
        return;
      default:
        throw new HttpError(404, `rota desconhecida: ${path}`);
    }
  }

  await new Promise<void>((resolvePromise, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => resolvePromise());
  });

  const address = server.address();
  if (address === null || typeof address === 'string') {
    server.close();
    throw new Error('não foi possível determinar a porta do servidor.');
  }
  const boundPort = address.port;
  for (const name of ['127.0.0.1', 'localhost', '[::1]']) {
    context.allowedHosts.add(`${name}:${boundPort}`);
  }

  return {
    url: `http://127.0.0.1:${boundPort}/?t=${token}`,
    token,
    host,
    port: boundPort,
    close: () =>
      new Promise<void>((resolvePromise) => {
        server.close(() => resolvePromise());
      }),
  };
}

export { PUBLIC_DIR };
