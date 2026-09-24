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
import { spawn } from 'node:child_process';
import { existsSync, readFileSync, statSync, unlinkSync, writeFileSync } from 'node:fs';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { extname, isAbsolute, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

import { todayInLisbon } from '../core/dates.ts';
import { estimateIrsSimplifiedBase, type Invoice, type VatTreatment } from '../core/estimate.ts';
import { extractPdfText, looksLikePdf } from '../core/pdf.ts';
import { invoiceFromReceipt, parseReceipt, type ReceiptConfirmation } from '../core/receipt.ts';
import {
  IRS_REGIMES,
  IVA_REGIMES,
  buildProfile,
  normaliseImportedProfile,
  validateProfile,
} from '../core/profile.ts';
import { loadRulePack, resolvePackPath } from '../core/rules.ts';
import type { IrsRegime, IvaRegime, LoadedPack, TaxProfile } from '../core/types.ts';
import {
  MIN_PASSPHRASE_LENGTH,
  apiKeyFilePath,
  decryptKeyFile,
  maskKey,
  resolveApiKey,
  saveApiKey,
  type ResolvedKey,
} from '../ai/keyring.ts';
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
import {
  Vault,
  browseDirectories,
  checkDataDirRisk,
  looksLikeVault,
  sanitiseDocumentName,
  vaultPointerPath,
  writePointerFile,
  type VaultLocationSource,
} from '../store/vault.ts';
import { buildDashboard, type DashboardModel } from './report.ts';

const PUBLIC_DIR = fileURLToPath(new URL('./public/', import.meta.url));
const MAX_BODY_BYTES = 256 * 1024;
/**
 * Document uploads are the one request that is allowed to be large, because a
 * scan of a *guia* is a few megabytes and the alternative — asking someone to
 * type a filesystem path into a web page — is not an interface. The ceiling is
 * still a ceiling: past it the request is refused rather than buffered.
 */
const MAX_UPLOAD_BYTES = 32 * 1024 * 1024;
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
  /** Where the current vault folder was chosen from, for the panel's own account of it. */
  dataDirSource?: VaultLocationSource;
  /**
   * The file that remembers a folder picked in the panel.
   *
   * Passed in rather than derived so tests never write to the real home directory;
   * in normal use it is `~/.vn-finance/vault-location.json`.
   */
  vaultPointerFile?: string;
}

export interface RunningWebServer {
  url: string;
  token: string;
  host: string;
  port: number;
  close(): Promise<void>;
}

interface RequestContext {
  /**
   * The vault, which the panel may REPLACE: choosing a folder is a first-class
   * action, and the process that is serving the panel is the only thing that can
   * act on it. Everything else in the process reads the current one.
   */
  vault: Vault;
  version: string;
  year: number;
  packPath: string | undefined;
  token: string;
  model: string;
  allowedHosts: Set<string>;
  dataDirSource: VaultLocationSource;
  vaultPointerFile: string;
  /**
   * A key the person using the panel supplied in the browser, held for the life
   * of this process only. It is never written to the vault in plaintext and never
   * sent back to the browser; starting the panel again starts with no key, unless
   * the encrypted file was also written.
   */
  runtimeKey: string | null;
}

/** Whether the passphrase that opens a stored key is in this process's environment. */
function passphraseFromEnv(): boolean {
  const value = process.env['VN_FINANCE_PASSPHRASE'];
  return value !== undefined && value !== '';
}

/**
 * The key this request may use, and everything the panel needs to explain it.
 *
 * A key supplied through the panel wins over everything else: it is the most
 * recent, most explicit statement of which credential to use, and it exists only
 * because someone typed it a moment ago.
 */
function resolveContextKey(context: RequestContext): ResolvedKey {
  if (context.runtimeKey !== null) {
    return { key: context.runtimeKey, source: 'session', problems: [] };
  }
  return resolveApiKey({
    dataDir: context.vault.dir,
    ...(passphraseFromEnv() ? { passphrase: process.env['VN_FINANCE_PASSPHRASE'] } : {}),
  });
}

function buildModel(context: RequestContext, pendingProposal?: UpdateProposal | null): DashboardModel {
  const loaded = loadCurrentPack(context);
  const key = resolveContextKey(context);
  const stored = existsSync(apiKeyFilePath(context.vault.dir));
  return buildDashboard({
    vault: context.vault,
    loaded,
    year: context.year,
    // Read the clock per request, not once at startup: a panel left running
    // across midnight must not keep yesterday's date, or every "vence amanhã"
    // becomes wrong.
    today: todayInLisbon(),
    version: context.version,
    dataDirSource: context.dataDirSource,
    apiKeyAvailable: key.key !== null,
    apiKeySource: key.source,
    apiKeyMasked: key.key === null ? null : maskKey(key.key),
    apiKeyProblems: key.problems,
    apiKeyStored: stored,
    // "Locked" is a fact about this process, not about the vault: the same file
    // is openable by a server that was started with the passphrase.
    apiKeyLocked: stored && key.key === null && !passphraseFromEnv(),
    apiKeyPassphraseFromEnv: passphraseFromEnv(),
    ...(pendingProposal === undefined ? {} : { pendingProposal }),
  });
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

/**
 * The body of a document upload, kept as bytes.
 *
 * A file is not JSON, and wrapping one in base64 to fit a JSON envelope would make
 * every upload a third larger for no gain. The cap is enforced while reading, so
 * an oversized body is refused instead of being buffered and then rejected.
 */
async function readBinaryBody(request: IncomingMessage, maxBytes: number): Promise<Buffer> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const chunk of request) {
    const buffer = chunk as Buffer;
    size += buffer.length;
    if (size > maxBytes) throw new HttpError(413, 'ficheiro demasiado grande para o cofre.');
    chunks.push(buffer);
  }
  return Buffer.concat(chunks);
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

/**
 * A passphrase, exactly as it was typed.
 *
 * Every other string off the wire is trimmed, because surrounding whitespace in a
 * name or an id is a typo. A passphrase is not: a leading or trailing space is
 * part of the secret, and trimming it here would make a key saved by the command
 * line impossible to open from the panel.
 */
function secret(body: Record<string, unknown>, key: string, max = 300): string | null {
  const value = body[key];
  if (value === undefined || value === null || value === '') return null;
  if (typeof value !== 'string') throw new HttpError(400, `campo "${key}" tem de ser texto.`);
  if (value.length > max) throw new HttpError(400, `campo "${key}" demasiado longo.`);
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

/** Every API route, with the single method it accepts. */
const API_ROUTES: Record<string, 'GET' | 'POST'> = {
  '/api/dashboard': 'GET',
  '/api/profile': 'POST',
  '/api/profile/import': 'POST',
  '/api/profile/export': 'GET',
  '/api/invoices': 'POST',
  '/api/obligations/complete': 'POST',
  '/api/documents': 'POST',
  '/api/documents/upload': 'POST',
  '/api/vault': 'POST',
  '/api/vault/browse': 'GET',
  '/api/vault/reveal': 'POST',
  '/api/receipts/upload': 'POST',
  '/api/receipts/record': 'POST',
  '/api/estimate/irs': 'POST',
  '/api/ai-key': 'POST',
  '/api/ai-key/unlock': 'POST',
  '/api/update/send': 'POST',
  '/api/update/apply': 'POST',
  '/api/update/discard': 'POST',
};

/** Routes whose body is bytes rather than JSON. */
const BINARY_ROUTES: ReadonlySet<string> = new Set(['/api/documents/upload', '/api/receipts/upload']);

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

/**
 * Build the profile from what the interface sent.
 *
 * Everything here is a value the taxpayer declared to the AT, so a missing one
 * is an error rather than an invitation to guess: the IVA regime in particular
 * changes every downstream obligation, and a wrong default would be a silent
 * lie. `buildProfile` enforces that; this only checks the shapes that come off
 * the wire, where TypeScript has no say.
 */
function profileDraftFromBody(
  body: Record<string, unknown>,
  fallbackCoefficientBp: number,
): ReturnType<typeof buildProfile> {
  const nif = requireString(body, 'nif', 20);
  const name = requireString(body, 'name', 120);
  const declaredIva = requireString(body, 'ivaRegime', 24);
  if (!(IVA_REGIMES as readonly string[]).includes(declaredIva)) {
    throw new HttpError(400, 'regime de IVA inválido: usa isento_art53, trimestral ou mensal.');
  }
  const declaredIrs = optionalString(body, 'irsRegime', 24);
  if (declaredIrs !== null && !(IRS_REGIMES as readonly string[]).includes(declaredIrs)) {
    throw new HttpError(400, 'regime de IRS inválido: usa simplificado ou organizada.');
  }

  const startDate = optionalString(body, 'startDate', 10);
  if (startDate !== null && !/^\d{4}-\d{2}-\d{2}$/.test(startDate)) {
    throw new HttpError(400, 'a data de início de atividade tem de estar em formato AAAA-MM-DD.');
  }

  // Read once each: these validators throw on nonsense, and calling one twice
  // would report the same mistake twice in a worse place.
  const previous = optionalCents(body, 'turnoverPreviousYearCents');
  const expected = optionalCents(body, 'turnoverCurrentYearExpectedCents');
  const intraCommunityOperations = optionalBool(body, 'intraCommunityOperations');
  const exports = optionalBool(body, 'exports');
  const startupExemptionActive = optionalBool(body, 'startupExemptionActive');

  return buildProfile(
    {
      nif,
      name,
      ivaRegime: declaredIva as IvaRegime,
      ...(declaredIrs === null ? {} : { irsRegime: declaredIrs as IrsRegime }),
      ...(startDate === null ? {} : { startDate }),
      ...(previous === undefined ? {} : { turnoverPreviousYearCents: previous }),
      ...(expected === undefined ? {} : { turnoverCurrentYearExpectedCents: expected }),
      ...(intraCommunityOperations === undefined ? {} : { intraCommunityOperations }),
      ...(exports === undefined ? {} : { exports }),
      ...(startupExemptionActive === undefined ? {} : { startupExemptionActive }),
    },
    // The coefficient is law, not a preference: it comes from the rule pack.
    { coefficientBp: fallbackCoefficientBp },
  );
}

/** An optional euro figure in cents: absent means "unmentioned", null means "clear it". */
function optionalCents(body: Record<string, unknown>, key: string): number | null | undefined {
  const value = body[key];
  if (value === undefined) return undefined;
  if (value === null || value === '') return null;
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) {
    throw new HttpError(400, `campo "${key}" tem de ser um valor em cêntimos (inteiro, não negativo).`);
  }
  return value;
}

/**
 * Create the profile — or replace it, when the interface says so explicitly.
 *
 * Creating is refused when a profile already exists, because the interface
 * offering "create" over someone's stored profile is a way to lose it by
 * accident. Replacing is a separate, deliberate flag, and it must carry the
 * declared fields: "replace" means "this is another, complete profile", not
 * "apply only what I happened to send".
 */
function handleProfileCreate(context: RequestContext, body: Record<string, unknown>): DashboardModel {
  const existing = context.vault.loadProfile();
  const replace = optionalBool(body, 'replace') ?? false;
  if (existing !== null && !replace) {
    throw new HttpError(
      409,
      'já existe um perfil neste cofre. Edita-o no painel ou confirma a substituição para o trocar.',
    );
  }

  const loaded = loadCurrentPack(context);
  const profile: TaxProfile = {
    ...profileDraftFromBody(
      body,
      loaded.pack.constants.irs.simplifiedRegime.coefficients.servicesProfessionalTable4 ?? 7500,
    ),
    // The profile is created today, so tracking starts today: deadlines that fell
    // earlier in the year are history, not failures. A replacement does the same,
    // because it is a different profile being written, not a continuation.
    trackingStart: todayInLisbon(),
  };
  const problems = validateProfile(profile).filter((problem) => problem.level === 'error');
  if (problems.length > 0) {
    throw new HttpError(400, problems.map((problem) => `${problem.field}: ${problem.message}`).join(' · '));
  }

  context.vault.ensure();
  context.vault.saveProfile(profile);
  context.vault.appendAudit({ action: existing === null ? 'profile.created' : 'profile.replaced' });
  return buildModel(context);
}

/**
 * Import a profile that arrived as a file.
 *
 * The browser reads the file and posts its contents — the server never reaches
 * out for a path — so an imported file is an untrusted document like any other.
 * It is normalised field by field, the same checks the interface's own form goes
 * through are applied, and the warnings come back rather than being swallowed.
 */
function handleProfileImport(context: RequestContext, body: Record<string, unknown>): {
  model: DashboardModel;
  warnings: string[];
} {
  const existing = context.vault.loadProfile();
  const replace = optionalBool(body, 'replace') ?? false;
  if (existing !== null && !replace) {
    throw new HttpError(
      409,
      'já existe um perfil neste cofre. Confirma a substituição para carregar este ficheiro por cima.',
    );
  }

  // `buildProfile` fills what the file omits from the profile being replaced,
  // which is right for an edit and wrong here: a file presented as a whole
  // profile must declare what a profile has to declare, or it would inherit a
  // regime from a profile it is meant to replace.
  const incoming = body['profile'];
  const raw = incoming !== null && typeof incoming === 'object' && !Array.isArray(incoming)
    ? (incoming as Record<string, unknown>)
    : null;
  if (raw === null) throw new HttpError(400, 'o ficheiro não contém um perfil.');
  for (const field of ['nif', 'name'] as const) {
    if (typeof raw[field] !== 'string' || (raw[field] as string).trim() === '') {
      throw new HttpError(400, `o perfil importado tem de declarar "${field}".`);
    }
  }
  const importedIva = raw['iva'];
  const importedRegime =
    importedIva !== null && typeof importedIva === 'object'
      ? (importedIva as Record<string, unknown>)['regime']
      : undefined;
  if (typeof importedRegime !== 'string' || !(IVA_REGIMES as readonly string[]).includes(importedRegime)) {
    throw new HttpError(
      400,
      'o perfil importado tem de declarar o regime de IVA (isento_art53, trimestral ou mensal).',
    );
  }

  let profile: TaxProfile;
  try {
    profile = normaliseImportedProfile(incoming);
  } catch (cause) {
    throw new HttpError(400, (cause as Error).message);
  }

  const problems = validateProfile(profile);
  const errors = problems.filter((problem) => problem.level === 'error');
  if (errors.length > 0) {
    throw new HttpError(400, errors.map((problem) => `${problem.field}: ${problem.message}`).join(' · '));
  }

  context.vault.ensure();
  context.vault.saveProfile(profile);
  context.vault.appendAudit({ action: existing === null ? 'profile.imported' : 'profile.replaced' });
  return {
    model: buildModel(context),
    warnings: problems.filter((problem) => problem.level === 'warning').map((problem) => problem.message),
  };
}

async function handleProfile(context: RequestContext, body: Record<string, unknown>): Promise<DashboardModel> {
  const profile = context.vault.loadProfile();
  if (profile === null) {
    throw new HttpError(409, 'não existe perfil neste cofre. Abre o painel e preenche o formulário de perfil.');
  }

  const updated: TaxProfile = {
    ...profile,
    activity: { ...profile.activity },
    iva: { ...profile.iva },
  };

  // `optionalCents` keeps the three cases apart: absent means "the form did not
  // mention this", null means "remove it" — which is how a wrong turnover figure
  // is corrected — and anything else has to be a whole number of cents.
  const previous = optionalCents(body, 'turnoverPreviousYearCents');
  if (previous !== undefined) {
    if (previous === null) delete updated.activity.turnoverPreviousYearCents;
    else updated.activity.turnoverPreviousYearCents = previous;
  }

  const expected = optionalCents(body, 'turnoverCurrentYearExpectedCents');
  if (expected !== undefined) {
    if (expected === null) delete updated.activity.turnoverCurrentYearExpectedCents;
    else updated.activity.turnoverCurrentYearExpectedCents = expected;
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
    throw new HttpError(409, 'não existe perfil neste cofre. Abre o painel e preenche o formulário de perfil.');
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

/**
 * The same archive, for a file chosen in the browser.
 *
 * The browser hands over the bytes, not a path, because that is what a browser
 * has. Everything after that is the one `addDocumentBytes` rule the command line
 * uses too: hash, name, copy, index, audit.
 */
async function handleDocumentUpload(
  context: RequestContext,
  request: IncomingMessage,
  url: URL,
): Promise<Record<string, unknown>> {
  const name = sanitiseDocumentName(url.searchParams.get('name') ?? 'documento');
  const kind = optionalQuery(url, 'kind', 40);
  const obligationId = optionalQuery(url, 'obligationId', 80);
  const bytes = await readBinaryBody(request, MAX_UPLOAD_BYTES);
  if (bytes.length === 0) throw new HttpError(400, 'o ficheiro recebido está vazio.');

  let added: { file: string; sha256: string };
  try {
    added = context.vault.addDocumentBytes(bytes, name, {
      ...(kind === null ? {} : { kind }),
      obligationId,
    });
  } catch (cause) {
    throw new HttpError(400, (cause as Error).message);
  }
  return { ok: true, model: buildModel(context), added, bytes: bytes.length };
}

/** A query parameter as a bounded string: absent and empty are the same thing. */
function optionalQuery(url: URL, key: string, max: number): string | null {
  const value = url.searchParams.get(key);
  if (value === null || value.trim() === '') return null;
  if (value.length > max) throw new HttpError(400, `parâmetro "${key}" demasiado longo.`);
  return value.trim();
}

// ---------------------------------------------------------------------------
// Where the vault lives
// ---------------------------------------------------------------------------

/**
 * The folders the chooser may offer, one level at a time.
 *
 * Directories only: the picker needs to know what can be entered, and a listing
 * that also returned file names would give any page holding the session token a
 * way to read the shape of the disk. The vault's own path is the only filesystem
 * fact the panel is entitled to, and it is already in the model.
 */
function handleVaultBrowse(url: URL): Record<string, unknown> {
  const requested = optionalQuery(url, 'path', 1024);
  const listing = browseDirectories(requested);
  return {
    ok: true,
    path: listing.path,
    parent: listing.parent,
    exists: listing.exists,
    roots: listing.roots,
    entries: listing.entries,
    risk: listing.risk,
    error: listing.error,
    looksLikeVault: looksLikeVault(listing.path),
  };
}

/**
 * Choose the folder the vault lives in — creating it when it is not there yet.
 *
 * Three things are deliberate here:
 *   - the folder is REMEMBERED, so the next run finds the data instead of opening
 *     an empty default and looking like it lost it;
 *   - a vault inside this application's repository is refused for the reason the
 *     store refuses it everywhere: a `git add -A` would publish it;
 *   - switching away from a vault is recorded in BOTH vaults. The one being left
 *     gets the "closed" line, so its audit trail explains why it stops there.
 */
function handleVaultChoose(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  const requested = requireString(body, 'path', 1024);
  if (!isAbsolute(requested)) {
    throw new HttpError(400, 'indica o caminho completo da pasta (por exemplo C:\\Users\\…\\vn-finance).');
  }
  const target = resolve(requested);

  const risk = checkDataDirRisk(target);
  if (risk.level === 'fatal') throw new HttpError(400, risk.message ?? 'pasta não permitida.');

  if (existsSync(target) && !statSync(target).isDirectory()) {
    throw new HttpError(400, `${target} é um ficheiro, não uma pasta.`);
  }

  const previous = context.vault;
  const vault = new Vault(target);
  vault.ensure();
  const samePlace = previous.dir === vault.dir;
  const previousHadData = looksLikeVault(previous.dir);

  writePointerFile(context.vaultPointerFile, {
    path: target,
    chosenAt: new Date().toISOString(),
    by: 'painel',
  });

  context.vault = vault;
  context.dataDirSource = 'pointer';
  vault.appendAudit({ action: 'vault.opened', detail: target });
  if (!samePlace && previousHadData) {
    previous.appendAudit({ action: 'vault.left', detail: target });
  }

  const notes: string[] = [];
  if (!samePlace && previousHadData) {
    notes.push(
      `O painel passou a mostrar ${target}. O cofre anterior (${previous.dir}) ficou onde estava, ` +
        'com todos os dados: nada foi apagado nem movido.',
    );
  }
  if (risk.level === 'warning' && risk.message !== null) notes.push(risk.message);

  return { ok: true, model: buildModel(context), vault: target, notes };
}

/**
 * Open the vault folder in the machine's file manager.
 *
 * This is the answer to "where does it keep the files?": a path in a header is
 * easy to read and easy to disbelieve, and a folder that opens in Explorer is not.
 * The command is fixed per platform and the path is the vault's own, never a
 * value from the request.
 */
function handleVaultReveal(context: RequestContext): Record<string, unknown> {
  const dir = context.vault.dir;
  const [command, args] =
    process.platform === 'win32'
      ? ['explorer.exe', [dir]]
      : process.platform === 'darwin'
        ? ['open', [dir]]
        : ['xdg-open', [dir]];
  try {
    const child = spawn(command, args, { stdio: 'ignore', detached: true });
    child.on('error', () => {});
    child.unref();
  } catch (cause) {
    throw new HttpError(400, `não foi possível abrir a pasta: ${(cause as Error).message}`);
  }
  return { ok: true, opened: dir };
}

// ---------------------------------------------------------------------------
// Fatura-recibo PDFs
// ---------------------------------------------------------------------------

/**
 * Read a PDF, archive it, and say what was read.
 *
 * The order is archive-then-report on purpose. The file itself is the document
 * the person chose to keep, and keeping it cannot be wrong; what CAN be wrong is
 * the reading of it, which is why nothing is written to the ledger here. The
 * archive happens even when the parse finds nothing, so a scanned receipt still
 * ends up in the vault instead of being thrown away by a failed reading.
 */
async function handleReceiptUpload(
  context: RequestContext,
  request: IncomingMessage,
  url: URL,
): Promise<Record<string, unknown>> {
  const name = sanitiseDocumentName(url.searchParams.get('name') ?? 'fatura-recibo.pdf');
  const bytes = await readBinaryBody(request, MAX_UPLOAD_BYTES);
  if (bytes.length === 0) throw new HttpError(400, 'o ficheiro recebido está vazio.');
  if (!looksLikePdf(bytes)) {
    throw new HttpError(400, 'o ficheiro não é um PDF. Para guardar outro tipo de documento, usa «Guardar documento no cofre».');
  }

  let added: { file: string; sha256: string };
  const sha256 = context.vault.hashBytes(bytes);
  const already = context.vault.findDocumentByHash(sha256);
  if (already !== null && already.invoiceId !== null) {
    // The same PDF, twice, would be two invoices for one piece of income. Content
    // addressing is what makes that answerable, so it is answered here instead of
    // leaving the ledger to be corrected by hand later.
    throw new HttpError(
      409,
      `este PDF já está no cofre e já foi registado como fatura (${already.invoiceId}). ` +
        'Se o documento foi emitido duas vezes, usa "Nova fatura-recibo" e registra a segunda à mão.',
    );
  }
  try {
    // Re-uploading a document that is archived but not yet registered reuses the
    // archive rather than adding a second index entry for the same bytes.
    added =
      already === null
        ? context.vault.addDocumentBytes(bytes, name, { kind: 'fatura' })
        : { file: already.file, sha256: already.sha256 };
  } catch (cause) {
    throw new HttpError(400, (cause as Error).message);
  }

  const pdf = extractPdfText(bytes);
  const draft = parseReceipt(pdf);
  context.vault.appendAudit({
    action: 'receipt.parsed',
    detail: `${name} · ${draft.problems.length} problemas · ${draft.fields.filter((field) => field.value !== null).length} campos lidos`,
  });

  const profile = context.vault.loadProfile();
  const issuerMatchesProfile =
    profile === null || draft.issuer.nif === null
      ? null
      : draft.issuer.nif.replace(/\D/g, '') === profile.nif.replace(/\D/g, '');

  return {
    ok: true,
    model: buildModel(context),
    added,
    draft,
    issuerMatchesProfile,
    bytes: bytes.length,
  };
}

/**
 * Write the confirmed reading into the ledger.
 *
 * The document is re-read HERE, from the copy in the vault, and the fields that
 * arrived from the browser are treated as what they are: the person's
 * CONFIRMATION of the reading, not the source of it. That is what makes the
 * divergences in the audit trail meaningful — "the document said this, and the
 * record says that" is only a fact if both sides were read independently.
 */
function handleReceiptRecord(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  const profile = context.vault.loadProfile();
  if (profile === null) {
    throw new HttpError(409, 'não existe perfil neste cofre. Abre o painel e preenche o formulário de perfil.');
  }

  const documentFile = requireString(body, 'documentFile', 400);
  const document = context.vault.findDocument(documentFile);
  if (document === null) {
    throw new HttpError(400, `o documento "${documentFile}" não está indexado neste cofre.`);
  }

  let bytes: Buffer;
  try {
    bytes = readFileSync(document.absolute);
  } catch {
    throw new HttpError(400, 'o ficheiro do documento já não está no cofre.');
  }
  if (document.sha256 !== '' && context.vault.hashBytes(bytes) !== document.sha256) {
    throw new HttpError(
      409,
      'o ficheiro no cofre já não corresponde ao que foi arquivado (hash diferente). Regista a fatura à mão.',
    );
  }

  const draft = parseReceipt(extractPdfText(bytes));

  const confirmed: ReceiptConfirmation = {};
  const text = (key: string, max: number): string | null => optionalString(body, key, max);
  const cents = (key: string): number | null | undefined => optionalCents(body, key);
  const bp = (key: string): number | null | undefined => {
    const value = optionalCents(body, key);
    if (value === undefined || value === null) return value;
    if (value > 10_000) throw new HttpError(400, `campo "${key}" está fora do intervalo de uma percentagem.`);
    return value;
  };

  const number = text('number', 40);
  if (number !== null) confirmed.number = number;
  const date = text('date', 10);
  if (date !== null) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) {
      throw new HttpError(400, 'a data da fatura tem de estar em formato AAAA-MM-DD.');
    }
    confirmed.date = date;
  }
  const clientName = text('clientName', 120);
  if (clientName !== null) confirmed.clientName = clientName;
  const clientNif = text('clientNif', 20);
  if (clientNif !== null) confirmed.clientNif = clientNif;
  const clientCountry = text('clientCountry', 3);
  if (clientCountry !== null) confirmed.clientCountry = clientCountry.toUpperCase();
  const description = text('description', 300);
  if (description !== null) confirmed.description = description;
  const atcud = text('atcud', 40);
  if (atcud !== null) confirmed.atcud = atcud;

  const baseCents = cents('baseCents');
  if (baseCents !== undefined) confirmed.baseCents = baseCents;
  const ivaRateBp = bp('ivaRateBp');
  if (ivaRateBp !== undefined) confirmed.ivaRateBp = ivaRateBp;
  const retentionBp = bp('retentionBp');
  if (retentionBp !== undefined) confirmed.retentionBp = retentionBp;

  const treatment = text('vatTreatment', 24);
  if (treatment !== null) {
    if (!VAT_TREATMENTS.includes(treatment as VatTreatment)) {
      throw new HttpError(400, 'tratamento de IVA inválido nesta fatura.');
    }
    confirmed.vatTreatment = treatment as VatTreatment;
  }
  const status = text('status', 12);
  if (status !== null) {
    if (!(INVOICE_STATUSES as readonly string[]).includes(status)) {
      throw new HttpError(400, 'estado de fatura inválido.');
    }
    confirmed.status = status as Invoice['status'];
  }

  const loaded = loadCurrentPack(context);
  const invoices = context.vault.loadInvoices();
  const result = invoiceFromReceipt({
    draft,
    pack: loaded.pack,
    profile,
    confirmed,
    id: `inv_${Date.now().toString(36)}_${randomBytes(3).toString('hex')}`,
    documentFile: document.file,
    fallbackNumber: `FR ${context.year}/${String(invoices.length + 1).padStart(3, '0')}`,
  });

  if (result.invoice === null) {
    throw new HttpError(400, result.problems.join(' '));
  }

  // The ledger entry is appended, then the archive entry is pointed at it, then a
  // single audit line records what the document said versus what was registered.
  context.vault.appendInvoice(result.invoice);
  context.vault.linkDocumentToInvoice(document.file, result.invoice.id);
  context.vault.appendAudit({
    action: 'receipt.recorded',
    detail:
      `${result.invoice.number} · ${document.file}` +
      (result.divergences.length === 0 ? '' : ` · corrigido: ${result.divergences.join('; ')}`),
  });

  return {
    ok: true,
    model: buildModel(context),
    invoice: result.invoice,
    warnings: result.warnings,
    divergences: result.divergences,
    draft,
  };
}

/**
 * The taxable base of the simplified regime, computed from an expense figure the
 * panel asks for.
 *
 * The command line has `estimate --despesas`, and this is the same call with the
 * same core function. Nothing is stored: the figure is a judgement about which
 * expenses are eligible, and the answer belongs to the person who made it, not to
 * a note the application keeps on their behalf.
 */
function handleIrsEstimate(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  const profile = context.vault.loadProfile();
  if (profile === null) {
    throw new HttpError(409, 'não existe perfil neste cofre. Abre o painel e preenche o formulário de perfil.');
  }
  const loaded = loadCurrentPack(context);
  const documented = requireInt(body, 'documentedExpensesCents', 0, 10_000_000_000);
  const estimate = estimateIrsSimplifiedBase(
    loaded.pack,
    profile,
    context.vault.loadInvoices(),
    context.year,
    documented,
  );
  return { ok: true, year: context.year, documentedExpensesCents: documented, estimate };
}

/**
 * Store, forget or delete the assistant's credential, from the panel.
 *
 * Three separate intentions, and none of them is inferred:
 *   - with a key: use it. With a passphrase as well, also encrypt it into the
 *     vault; without one, it lives in this process's memory until the panel is
 *     stopped, and the answer says so rather than pretending it was saved.
 *   - `forget`: stop using the key in this session. Nothing on disk changes.
 *   - `delete`: remove the encrypted file from the vault.
 * The key is never written to the audit log and never sent back to the browser.
 */
function handleAiKey(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  if (optionalBool(body, 'forget') === true) {
    context.runtimeKey = null;
    context.vault.appendAudit({ action: 'apikey.forgotten' });
    return { ok: true, model: buildModel(context), forgotten: true, deleted: false };
  }

  if (optionalBool(body, 'delete') === true) {
    const path = apiKeyFilePath(context.vault.dir);
    const existed = existsSync(path);
    if (existed) unlinkSync(path);
    context.runtimeKey = null;
    context.vault.appendAudit({ action: 'apikey.deleted' });
    return { ok: true, model: buildModel(context), forgotten: true, deleted: existed };
  }

  const key = requireString(body, 'key', 300);
  const passphrase = secret(body, 'passphrase');
  if (passphrase !== null && passphrase.length < MIN_PASSPHRASE_LENGTH) {
    throw new HttpError(
      400,
      `a frase-passe que cifra a chave deve ter pelo menos ${MIN_PASSPHRASE_LENGTH} caracteres. ` +
        'Deixa-a vazia para usar a chave só nesta sessão, sem a guardar em disco.',
    );
  }

  let stored = false;
  if (passphrase !== null) {
    context.vault.ensure();
    try {
      saveApiKey(context.vault.dir, key, passphrase);
    } catch (cause) {
      throw new HttpError(400, (cause as Error).message);
    }
    stored = true;
  }

  context.runtimeKey = key;
  context.vault.appendAudit({
    action: 'apikey.stored',
    detail: stored ? 'cifrada no cofre e ativa nesta sessão' : 'apenas na memória desta sessão',
  });
  return { ok: true, model: buildModel(context), stored, sessionOnly: !stored };
}

/** Open a key that is already in the vault, with the passphrase typed in the panel. */
function handleAiKeyUnlock(context: RequestContext, body: Record<string, unknown>): Record<string, unknown> {
  const passphrase = secret(body, 'passphrase');
  if (passphrase === null) throw new HttpError(400, 'campo "passphrase" em falta.');
  const path = apiKeyFilePath(context.vault.dir);
  if (!existsSync(path)) {
    throw new HttpError(409, 'não há nenhuma chave guardada no cofre para desbloquear.');
  }
  let key: string;
  try {
    key = decryptKeyFile(path, passphrase);
  } catch {
    // The error from the cipher says nothing useful and can be mistaken for a
    // corrupt file; the only thing the person can act on is that it did not open.
    throw new HttpError(400, 'a chave guardada não abriu com essa frase-passe.');
  }
  context.runtimeKey = key;
  context.vault.appendAudit({ action: 'apikey.unlocked' });
  return { ok: true, model: buildModel(context), unlocked: true };
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
  const key = resolveContextKey(context);
  if (key.key === null) {
    throw new HttpError(
      400,
      'não há chave DeepSeek: nada foi enviado. Guarda uma chave na secção "Diagnóstico" do painel, ' +
        'ou define DEEPSEEK_API_KEY e volta a abrir o painel.',
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
    runtimeKey: null,
    dataDirSource: options.dataDirSource ?? 'flag',
    vaultPointerFile: options.vaultPointerFile ?? vaultPointerPath(),
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

    // The routes whose body is the file itself, dispatched before the JSON reader
    // rather than inside the switch below.
    if (BINARY_ROUTES.has(path)) {
      sendJson(
        response,
        200,
        path === '/api/receipts/upload'
          ? await handleReceiptUpload(context, request, url)
          : await handleDocumentUpload(context, request, url),
      );
      return;
    }

    if (allowed === 'GET') {
      if (path === '/api/profile/export') {
        const profile = context.vault.loadProfile();
        if (profile === null) throw new HttpError(409, 'não existe perfil para exportar.');
        sendJson(response, 200, { ok: true, exportedAt: new Date().toISOString(), profile });
        return;
      }
      if (path === '/api/vault/browse') {
        sendJson(response, 200, handleVaultBrowse(url));
        return;
      }
      sendJson(response, 200, { ok: true, model: buildModel(context) });
      return;
    }

    const body = await readJsonBody(request);

    switch (path) {
      case '/api/profile':
        // The same route does the jobs the panel needs, and what it does is
        // explicit rather than inferred: `replace` writes a whole profile over
        // whatever is stored. Without it, an empty vault CREATES the profile from
        // the declaration fields — which is what makes a first run possible with
        // no command line step at all — and a vault that already has one UPDATES
        // the editable inputs. There, the declaration fields are not re-read,
        // because changing them is a different, deliberate action.
        sendJson(response, 200, {
          ok: true,
          model: (optionalBool(body, 'replace') ?? false) || context.vault.loadProfile() === null
            ? handleProfileCreate(context, body)
            : await handleProfile(context, body),
        });
        return;
      case '/api/profile/import':
        {
          const imported = handleProfileImport(context, body);
          sendJson(response, 200, { ok: true, model: imported.model, warnings: imported.warnings });
        }
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
      case '/api/vault':
        sendJson(response, 200, handleVaultChoose(context, body));
        return;
      case '/api/vault/reveal':
        sendJson(response, 200, handleVaultReveal(context));
        return;
      case '/api/receipts/record':
        sendJson(response, 200, handleReceiptRecord(context, body));
        return;
      case '/api/estimate/irs':
        sendJson(response, 200, handleIrsEstimate(context, body));
        return;
      case '/api/ai-key':
        sendJson(response, 200, handleAiKey(context, body));
        return;
      case '/api/ai-key/unlock':
        sendJson(response, 200, handleAiKeyUnlock(context, body));
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
