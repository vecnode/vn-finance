/**
 * The local vault.
 *
 * Everything lives in one directory the user owns: the profile, the invoice
 * ledger, the document index, the obligation completions and the audit log. No
 * server, no sync, no telemetry. Backup is copying a folder; deletion is
 * deleting a folder; and both statements are true only because nothing here
 * writes anywhere else.
 *
 * The ledger is append-only JSONL rather than a mutable table. For a tax
 * record, "what did I record, and when" is as important as "what is the current
 * value", and an append-only file answers both without a database engine.
 */

import { createHash } from 'node:crypto';
import {
  appendFileSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Invoice } from '../core/estimate.ts';
import type { TaxProfile } from '../core/types.ts';

export const LEDGER_INVOICES = 'ledger/invoices.jsonl';
export const LEDGER_EXPENSES = 'ledger/expenses.jsonl';
export const OBLIGATION_COMPLETIONS = 'obligations/completions.jsonl';
export const AUDIT_LOG = 'audit/audit.jsonl';
export const PROFILE_FILE = 'profile.json';
export const DOCUMENTS_DIR = 'documents';

/** Absolute path of the application checkout, used to detect a vault inside it. */
const APP_ROOT = fileURLToPath(new URL('../../', import.meta.url));

/** Walk up looking for a `.git` directory; returns the work-tree root or null. */
export function findGitRoot(start: string): string | null {
  let current = resolve(start);
  for (let guard = 0; guard < 64; guard += 1) {
    if (existsSync(join(current, '.git'))) return current;
    const parent = dirname(current);
    if (parent === current) return null;
    current = parent;
  }
  return null;
}

export interface DataDirRisk {
  level: 'none' | 'warning' | 'fatal';
  gitRoot: string | null;
  message: string | null;
}

/**
 * Personal data must not be committable by accident.
 *
 * A vault inside THIS application's repository is refused outright: it is the
 * one place where a stray `git add -A` would publish someone's entire financial
 * life. A vault inside some *other* repository (a dotfiles repo in the home
 * directory, say) is legitimate but worth a loud warning, because the ignore
 * rules are then someone else's responsibility.
 */
export function checkDataDirRisk(dir: string): DataDirRisk {
  const gitRoot = findGitRoot(dir);
  if (gitRoot === null) return { level: 'none', gitRoot: null, message: null };

  const appGitRoot = findGitRoot(APP_ROOT);
  if (appGitRoot !== null && gitRoot === appGitRoot) {
    return {
      level: 'fatal',
      gitRoot,
      message:
        `o cofre não pode viver dentro do repositório da aplicação (${gitRoot}).\n` +
        `  Caminho indicado: ${resolve(dir)}\n` +
        '  Os teus dados nunca devem estar a um `git add -A` de serem publicados.\n' +
        '  Usa outro --data-dir (por omissão fica em ~/.vn-finance). Se for mesmo isso que queres,\n' +
        '  define VN_FINANCE_ALLOW_IN_REPO=1 e confirma que o .gitignore cobre o cofre.',
    };
  }

  return {
    level: 'warning',
    gitRoot,
    message:
      `o cofre está dentro de um repositório git (${gitRoot}). Confirma que está ignorado: ` +
      'um repositório de dotfiles que inclua esta pasta publica os teus dados.',
  };
}

/**
 * A file name that is safe to join onto the vault directory.
 *
 * A name that arrived from a browser is untrusted input like any other: it is
 * reduced to its last segment (no `..`, no separators, no drive letters), stripped
 * of characters that are not meaningful in a name, and capped so the stored name
 * cannot outgrow the filesystem's limit once the hash prefix is added.
 */
export function sanitiseDocumentName(fileName: string): string {
  const last = fileName.split(/[\\/]/).pop() ?? '';
  const cleaned = last
    .replace(/[\u0000-\u001f\u007f]/g, '')
    .replace(/[<>:"|?*]/g, '-')
    .replace(/^\.+/, '')
    .replace(/[.\s]+$/, '')
    .trim();
  if (cleaned === '') return 'documento';
  return cleaned.length > 120 ? cleaned.slice(-120) : cleaned;
}

export function resolveDataDir(explicit?: string, env: NodeJS.ProcessEnv = process.env): string {
  const dir = resolveDataDirUnchecked(explicit, env);
  if (env['VN_FINANCE_ALLOW_IN_REPO'] !== '1') {
    const risk = checkDataDirRisk(dir);
    if (risk.level === 'fatal' && risk.message !== null) throw new Error(risk.message);
  }
  return dir;
}

function resolveDataDirUnchecked(explicit: string | undefined, env: NodeJS.ProcessEnv): string {
  if (explicit !== undefined && explicit.trim() !== '') return resolve(explicit);
  const fromEnv = env['VN_FINANCE_DATA_DIR'];
  if (fromEnv !== undefined && fromEnv.trim() !== '') return resolve(fromEnv);
  if (process.platform === 'win32') {
    const home = env['USERPROFILE'] ?? homedir();
    return join(home, '.vn-finance');
  }
  const xdg = env['XDG_DATA_HOME'];
  const base = xdg !== undefined && xdg.trim() !== '' ? xdg : join(homedir(), '.local', 'share');
  return join(base, 'vn-finance');
}

export interface AuditEvent {
  at: string;
  action: string;
  detail?: string;
}

export interface CompletionRecord {
  id: string;
  ruleId: string;
  dueDate: string;
  at: string;
  note?: string;
}

export class Vault {
  readonly dir: string;

  constructor(dir: string) {
    this.dir = resolve(dir);
  }

  path(...parts: string[]): string {
    return join(this.dir, ...parts);
  }

  ensure(): void {
    for (const relative of ['ledger', 'obligations', 'audit', DOCUMENTS_DIR, 'ai', 'exports']) {
      mkdirSync(this.path(relative), { recursive: true });
    }
  }

  exists(relative: string): boolean {
    return existsSync(this.path(relative));
  }

  readJson<T>(relative: string, fallback: T): T {
    const full = this.path(relative);
    if (!existsSync(full)) return fallback;
    return JSON.parse(readFileSync(full, 'utf8')) as T;
  }

  /** Write via a temporary file and rename, so an interrupted write cannot corrupt the vault. */
  writeJson(relative: string, value: unknown): void {
    const full = this.path(relative);
    mkdirSync(dirname(full), { recursive: true });
    const temporary = `${full}.tmp`;
    writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
    renameSync(temporary, full);
  }

  appendJsonl(relative: string, record: unknown): void {
    const full = this.path(relative);
    mkdirSync(dirname(full), { recursive: true });
    appendFileSync(full, `${JSON.stringify(record)}\n`, 'utf8');
  }

  readJsonl<T>(relative: string): T[] {
    const full = this.path(relative);
    if (!existsSync(full)) return [];
    return readFileSync(full, 'utf8')
      .split('\n')
      .filter((line) => line.trim() !== '')
      .map((line, index) => {
        try {
          return JSON.parse(line) as T;
        } catch (cause) {
          throw new Error(`${relative}:${index + 1} está corrompido: ${(cause as Error).message}`);
        }
      });
  }

  loadProfile(): TaxProfile | null {
    return this.readJson<TaxProfile | null>(PROFILE_FILE, null);
  }

  saveProfile(profile: TaxProfile): void {
    this.writeJson(PROFILE_FILE, profile);
  }

  loadInvoices(): Invoice[] {
    return this.readJsonl<Invoice>(LEDGER_INVOICES);
  }

  appendInvoice(invoice: Invoice): void {
    this.appendJsonl(LEDGER_INVOICES, invoice);
    this.appendAudit({ action: 'invoice.recorded', detail: invoice.number });
  }

  completions(): CompletionRecord[] {
    return this.readJsonl<CompletionRecord>(OBLIGATION_COMPLETIONS);
  }

  completedIds(): string[] {
    return this.completions().map((record) => record.id);
  }

  /** Obligation ids that already have at least one document archived in the vault. */
  documentedObligationIds(): string[] {
    const index = this.readJson<Array<{ obligationId?: string | null }>>('documents/index.json', []);
    const ids = new Set<string>();
    for (const entry of index) {
      if (typeof entry.obligationId === 'string' && entry.obligationId !== '') {
        ids.add(entry.obligationId);
      }
    }
    return [...ids];
  }

  /**
   * Archive a file: copy it into the vault under the first 12 characters of its
   * SHA-256, then index it. The copy happens before the index entry, so the index
   * can never point at a file that does not exist, and the original is never moved
   * or modified.
   */
  addDocument(
    absolutePath: string,
    meta: { kind?: string; obligationId?: string | null } = {},
  ): { file: string; sha256: string } {
    if (!existsSync(absolutePath)) {
      throw new Error(`ficheiro não encontrado: ${absolutePath}`);
    }
    this.ensure();
    const sha256 = this.hashFile(absolutePath);
    const baseName = absolutePath.split(/[\\/]/).pop() ?? 'documento';
    const stored = `${sha256.slice(0, 12)}-${baseName}`;
    copyFileSync(absolutePath, this.path(DOCUMENTS_DIR, stored));

    return this.indexDocument(stored, sha256, meta, absolutePath);
  }

  /**
   * The same thing for bytes that arrived over the wire.
   *
   * The local panel runs in a browser, and a browser cannot hand over a path: it
   * can only hand over the file itself. Writing the bytes here rather than through
   * a temporary file keeps one implementation of the archive rule — hash, name,
   * copy, index — instead of two that can drift. There is no `originalPath`,
   * because there is no original on this filesystem to point at.
   */
  addDocumentBytes(
    bytes: Buffer,
    fileName: string,
    meta: { kind?: string; obligationId?: string | null } = {},
  ): { file: string; sha256: string } {
    if (bytes.length === 0) throw new Error('o ficheiro recebido está vazio.');
    this.ensure();
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    const baseName = sanitiseDocumentName(fileName);
    const stored = `${sha256.slice(0, 12)}-${baseName}`;
    writeFileSync(this.path(DOCUMENTS_DIR, stored), bytes);
    return this.indexDocument(stored, sha256, meta, null);
  }

  private indexDocument(
    stored: string,
    sha256: string,
    meta: { kind?: string; obligationId?: string | null },
    originalPath: string | null,
  ): { file: string; sha256: string } {
    const index = this.readJson<Array<Record<string, unknown>>>('documents/index.json', []);
    index.push({
      file: `${DOCUMENTS_DIR}/${stored}`,
      originalPath,
      sha256,
      addedAt: new Date().toISOString(),
      kind: meta.kind ?? 'outro',
      obligationId: meta.obligationId ?? null,
    });
    this.writeJson('documents/index.json', index);
    this.appendAudit({ action: 'document.added', detail: stored });
    return { file: `${DOCUMENTS_DIR}/${stored}`, sha256 };
  }

  markCompleted(record: Omit<CompletionRecord, 'at'>, at: string): void {
    this.appendJsonl(OBLIGATION_COMPLETIONS, { ...record, at });
    this.appendAudit({ action: 'obligation.completed', detail: record.id });
  }

  appendAudit(event: Omit<AuditEvent, 'at'>, at = new Date().toISOString()): void {
    this.appendJsonl(AUDIT_LOG, { ...event, at });
  }

  /** Content hash, so a document in the vault can be proven unchanged. */
  hashFile(absolutePath: string): string {
    return createHash('sha256').update(readFileSync(absolutePath)).digest('hex');
  }

  describe(): { dir: string; files: number; bytes: number } {
    let files = 0;
    let bytes = 0;
    const walk = (directory: string): void => {
      if (!existsSync(directory)) return;
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const child = join(directory, entry.name);
        if (entry.isDirectory()) walk(child);
        else {
          files += 1;
          bytes += statSync(child).size;
        }
      }
    };
    walk(this.dir);
    return { dir: this.dir, files, bytes };
  }
}
