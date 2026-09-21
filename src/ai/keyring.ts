/**
 * Local resolution and storage of the DeepSeek API key.
 *
 * Order of precedence: explicit flag, environment variable, encrypted file in
 * the local vault. The key is NEVER written to the database, never logged, and
 * never included in a backup of the ledger.
 *
 * The panel adds one more place a key can live: the memory of the server process
 * it is using (see `KeySource`). A key typed into the browser is never echoed
 * back to the browser — only a mask of it is — and it is gone when the panel
 * stops. Storing it encrypted on disk stays an explicit choice: leaving the
 * passphrase empty means "this session only".
 *
 * The encrypted-file path exists so the tool is usable today with zero native
 * dependencies. The intended production home for the key is the operating
 * system's credential store (Windows Credential Manager / DPAPI, libsecret,
 * macOS Keychain), which the desktop build will use through Tauri's keyring
 * plugin. That is stated here rather than quietly pretended.
 */

import { createCipheriv, createDecipheriv, randomBytes, scryptSync } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const SCRYPT_PARAMS = {
  N: 2 ** 15,
  r: 8,
  p: 1,
  keyLength: 32,
  /**
   * scrypt needs roughly 128 × N × r bytes (32 MiB at these parameters), and
   * Node's default maxmem is exactly 32 MiB, so the derivation fails with
   * "memory limit exceeded" unless the ceiling is raised. Setting it explicitly
   * keeps N at 2^15 rather than weakening the parameters to fit the default.
   */
  maxmem: 64 * 1024 * 1024,
} as const;
const KEY_FILE_VERSION = 1;

/** The one place the minimum passphrase length is written down. */
export const MIN_PASSPHRASE_LENGTH = 12;

/**
 * Where a usable key came from.
 *
 * `session` is the browser panel's half of the file case: a key typed into the
 * panel is held in the server process's memory for the life of that process, so
 * using it does not require the passphrase to be exported into the environment
 * before starting the application. `resolveApiKey` never returns it; only the
 * panel's server does, because only it has a process to hold a key in.
 */
export type KeySource = 'flag' | 'env' | 'file' | 'session' | 'none';

export interface ResolvedKey {
  key: string | null;
  source: KeySource;
  problems: string[];
}

interface KeyFilePayload {
  version: number;
  kdf: 'scrypt';
  salt: string;
  iv: string;
  tag: string;
  ciphertext: string;
}

export function apiKeyFilePath(dataDir: string): string {
  return join(dataDir, 'ai', 'deepseek.key');
}

export function resolveApiKey(options: {
  explicit?: string;
  dataDir: string;
  passphrase?: string;
  env?: NodeJS.ProcessEnv;
}): ResolvedKey {
  const problems: string[] = [];
  const env = options.env ?? process.env;

  if (options.explicit !== undefined && options.explicit.trim() !== '') {
    return { key: options.explicit.trim(), source: 'flag', problems };
  }

  const fromEnv = env['DEEPSEEK_API_KEY'];
  if (fromEnv !== undefined && fromEnv.trim() !== '') {
    return { key: fromEnv.trim(), source: 'env', problems };
  }

  const path = apiKeyFilePath(options.dataDir);
  if (!existsSync(path)) {
    return { key: null, source: 'none', problems };
  }

  const passphrase = options.passphrase ?? env['VN_FINANCE_PASSPHRASE'];
  if (passphrase === undefined || passphrase === '') {
    problems.push(
      'Existe uma chave cifrada no cofre, mas falta a frase-passe. Define VN_FINANCE_PASSPHRASE ' +
        'para a desbloquear; sem ela a chave não pode ser lida.',
    );
    return { key: null, source: 'none', problems };
  }

  try {
    return { key: decryptKeyFile(path, passphrase), source: 'file', problems };
  } catch (cause) {
    problems.push(`Não foi possível decifrar a chave guardada: ${(cause as Error).message}`);
    return { key: null, source: 'none', problems };
  }
}

function deriveKey(passphrase: string, salt: Buffer): Buffer {
  return scryptSync(passphrase, salt, SCRYPT_PARAMS.keyLength, {
    N: SCRYPT_PARAMS.N,
    r: SCRYPT_PARAMS.r,
    p: SCRYPT_PARAMS.p,
    maxmem: SCRYPT_PARAMS.maxmem,
  });
}

export function encryptKeyFile(apiKey: string, passphrase: string): string {
  const salt = randomBytes(16);
  const iv = randomBytes(12);
  const key = deriveKey(passphrase, salt);
  const cipher = createCipheriv('aes-256-gcm', key, iv);
  const ciphertext = Buffer.concat([cipher.update(apiKey, 'utf8'), cipher.final()]);
  const payload: KeyFilePayload = {
    version: KEY_FILE_VERSION,
    kdf: 'scrypt',
    salt: salt.toString('base64'),
    iv: iv.toString('base64'),
    tag: cipher.getAuthTag().toString('base64'),
    ciphertext: ciphertext.toString('base64'),
  };
  return JSON.stringify(payload, null, 2);
}

export function decryptKeyFile(path: string, passphrase: string): string {
  const payload = JSON.parse(readFileSync(path, 'utf8')) as KeyFilePayload;
  if (payload.version !== KEY_FILE_VERSION || payload.kdf !== 'scrypt') {
    throw new Error(`formato de ficheiro de chave não suportado (versão ${payload.version})`);
  }
  const salt = Buffer.from(payload.salt, 'base64');
  const iv = Buffer.from(payload.iv, 'base64');
  const tag = Buffer.from(payload.tag, 'base64');
  const key = deriveKey(passphrase, salt);
  const decipher = createDecipheriv('aes-256-gcm', key, iv);
  decipher.setAuthTag(tag);
  return Buffer.concat([
    decipher.update(Buffer.from(payload.ciphertext, 'base64')),
    decipher.final(),
  ]).toString('utf8');
}

export function saveApiKey(dataDir: string, apiKey: string, passphrase: string): string {
  if (passphrase.length < MIN_PASSPHRASE_LENGTH) {
    throw new Error(`a frase-passe deve ter pelo menos ${MIN_PASSPHRASE_LENGTH} caracteres`);
  }
  const path = apiKeyFilePath(dataDir);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${encryptKeyFile(apiKey, passphrase)}\n`, { encoding: 'utf8', mode: 0o600 });
  return path;
}

/** Never print a key. Show enough to recognise it and nothing more. */
export function maskKey(key: string): string {
  if (key.length <= 8) return '••••';
  return `${key.slice(0, 4)}…${key.slice(-4)}`;
}
