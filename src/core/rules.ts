/**
 * Loading and validating rule packs.
 *
 * A rule pack is the only place where Portuguese tax law is written down. It is
 * data, it is versioned, it carries its own citations, and it is validated on
 * load so that a broken pack fails loudly in `vnfin doctor` instead of quietly
 * producing a wrong deadline.
 *
 * The loader is deliberately forgiving about SHAPE and strict about PROVENANCE:
 * a pack that lacks a citation is a warning, a pack with a floating-point rate
 * is a warning, and a pack with a malformed obligation is an error.
 */

import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { isConditionField, isConditionOp } from './conditions.ts';
import { parseIso } from './dates.ts';
import type {
  Authority,
  Condition,
  DeadlineRule,
  LoadedPack,
  ObligationKind,
  ObligationRule,
  OfficialDate,
  PackProblem,
  RulePack,
  RuleSource,
  Verification,
} from './types.ts';

const OBLIGATION_KINDS: readonly ObligationKind[] = [
  'declare',
  'pay',
  'communicate',
  'save',
  'review',
  'submit',
];
const VERIFICATIONS: readonly Verification[] = ['verified', 'partial', 'unverified', 'stale'];
const PERIODICITY_TYPES = [
  'monthly',
  'quarterly',
  'annual',
  'continuous',
  'event_driven',
] as const;
const DEADLINE_KINDS = [
  'day_of_month',
  'fixed_date',
  'range',
  'last_day_of_month',
  'month_of_year',
  'day_range',
  'days_after_event',
  'retention_years',
] as const;

/**
 * The pack may describe an authority in prose ("Seguranca Social"). Normalise it
 * to the closed set the interface uses, and let the caller warn when a value was
 * not recognised rather than silently mis-filing the obligation.
 */
export function normaliseAuthority(value: unknown): Authority {
  const text = String(value ?? '')
    .toLowerCase()
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/g, '');
  if (text === 'at' || text.includes('tribut') || text.includes('financ')) return 'AT';
  if (text === 'ss' || text.includes('social')) return 'SS';
  return 'OTHER';
}

/** Ratios of the IAS are the one place a fractional number is legitimate. */
const NON_INTEGER_ALLOWED = /MultipleIas$/;

export function defaultPackPath(jurisdiction = 'pt', year = 2026): string {
  return fileURLToPath(new URL(`../rules/${jurisdiction}/${year}.json`, import.meta.url));
}

export interface ResolvedPackPath {
  path: string;
  year: number;
  /** False when the requested year was not found and a neighbouring year is used. */
  exact: boolean;
}

/**
 * Find the rule pack for a year, falling back to the most recent year available.
 * The authority publishes next year's calendar only weeks before it starts, so a
 * requested year may legitimately not exist yet; the caller decides how to present
 * that, and never silently pretends the fallback is the requested year.
 */
export function resolvePackPath(year: number, explicit?: string): ResolvedPackPath | null {
  if (explicit !== undefined) return { path: resolveFile(explicit), year, exact: true };

  const exact = defaultPackPath('pt', year);
  if (existsSync(exact)) return { path: exact, year, exact: true };

  const directory = dirname(exact);
  if (!existsSync(directory)) return null;
  const available = readdirSync(directory)
    .map((name) => /^(\d{4})\.json$/.exec(name)?.[1])
    .filter((value): value is string => value !== undefined)
    .map(Number)
    .sort((a, b) => b - a);
  const fallback = available[0];
  if (fallback === undefined) return null;
  return { path: join(directory, `${fallback}.json`), year: fallback, exact: false };
}

function resolveFile(path: string): string {
  return isAbsolute(path) ? path : resolve(path);
}

/** Deterministic JSON with sorted keys, so a checksum only changes when content does. */
export function canonicalise(value: unknown): string {
  if (value === null || value === undefined) return 'null';
  if (typeof value !== 'object') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalise).join(',')}]`;
  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([, item]) => item !== undefined)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonicalise(item)}`).join(',')}}`;
}

export function checksumOf(value: unknown): string {
  return createHash('sha256').update(canonicalise(value)).digest('hex');
}

function collectNumbers(node: unknown, path: string, out: Array<{ path: string; value: number }>): void {
  if (typeof node === 'number') {
    out.push({ path, value: node });
    return;
  }
  if (Array.isArray(node)) {
    node.forEach((item, index) => collectNumbers(item, `${path}[${index}]`, out));
    return;
  }
  if (node !== null && typeof node === 'object') {
    for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
      collectNumbers(value, path === '' ? key : `${path}.${key}`, out);
    }
  }
}

function collectNulls(node: unknown, path: string, out: string[]): void {
  if (node === null) {
    out.push(path);
    return;
  }
  if (Array.isArray(node)) {
    node.forEach((item, index) => collectNulls(item, `${path}[${index}]`, out));
    return;
  }
  if (typeof node === 'object' && node !== undefined) {
    for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
      collectNulls(value, path === '' ? key : `${path}.${key}`, out);
    }
  }
}

function normaliseOfficialDates(raw: Record<string, unknown>): Record<string, OfficialDate[]> {
  const result: Record<string, OfficialDate[]> = {};

  const absorb = (year: string, list: unknown): void => {
    if (!Array.isArray(list)) return;
    const bucket = result[year] ?? (result[year] = []);
    for (const entry of list) {
      if (entry === null || typeof entry !== 'object') continue;
      const record = entry as Record<string, unknown>;
      const month = Number(record['month']);
      const day = Number(record['day']);
      if (!Number.isInteger(month) || !Number.isInteger(day)) continue;
      const date: OfficialDate = { month, day };
      if (typeof record['periodLabel'] === 'string') date.periodLabel = record['periodLabel'];
      if (typeof record['note'] === 'string') date.note = record['note'];
      if (record['extension'] !== null && typeof record['extension'] === 'object') {
        const extension = record['extension'] as Record<string, unknown>;
        date.extension = {
          reason: typeof extension['reason'] === 'string' ? extension['reason'] : 'prorrogação',
          ...(typeof extension['despacho'] === 'string' ? { despacho: extension['despacho'] } : {}),
          ...(typeof extension['url'] === 'string' ? { url: extension['url'] } : {}),
        };
      }
      bucket.push(date);
    }
  };

  const perYear = raw['officialDates'];
  if (perYear !== null && typeof perYear === 'object') {
    for (const [year, list] of Object.entries(perYear as Record<string, unknown>)) {
      absorb(year, list);
    }
  }
  // Tolerate the flat, year-in-the-key form used by the first-generation packs.
  for (const [key, value] of Object.entries(raw)) {
    const match = /^officialDates(\d{4})$/.exec(key);
    if (match?.[1] !== undefined) absorb(match[1], value);
  }
  for (const list of Object.values(result)) list.sort((a, b) => (a.month - b.month) || (a.day - b.day));
  return result;
}

export function validateRulePack(raw: unknown): { pack: RulePack; problems: PackProblem[] } {
  const problems: PackProblem[] = [];
  const error = (path: string, message: string): void => {
    problems.push({ level: 'error', path, message });
  };
  const warn = (path: string, message: string): void => {
    problems.push({ level: 'warning', path, message });
  };

  if (raw === null || typeof raw !== 'object') {
    throw new TypeError('rule pack is not an object');
  }
  const root = raw as Record<string, unknown>;

  if (root['jurisdiction'] !== 'PT') error('jurisdiction', 'só a jurisdição "PT" é suportada');
  const year = Number(root['year']);
  if (!Number.isInteger(year)) error('year', 'o ano tem de ser um número inteiro');

  const units = root['units'];
  if (units === null || typeof units !== 'object') {
    error('units', 'o bloco units é obrigatório e tem de declarar EUR_cents / basis_points');
  } else {
    const unitRecord = units as Record<string, unknown>;
    if (unitRecord['money'] !== 'EUR_cents') error('units.money', 'tem de ser "EUR_cents"');
    if (unitRecord['rate'] !== 'basis_points') error('units.rate', 'tem de ser "basis_points"');
  }

  const sourcesRaw = root['sources'];
  const sources: RuleSource[] = [];
  if (!Array.isArray(sourcesRaw)) {
    error('sources', 'sources tem de ser uma lista');
  } else {
    sourcesRaw.forEach((entry, index) => {
      if (entry === null || typeof entry !== 'object') {
        error(`sources[${index}]`, 'tem de ser um objeto');
        return;
      }
      const record = entry as Record<string, unknown>;
      const source: RuleSource = {
        id: String(record['id'] ?? ''),
        tier: record['tier'] === 'secondary' ? 'secondary' : 'official',
        authority: String(record['authority'] ?? ''),
        title: String(record['title'] ?? ''),
        url: String(record['url'] ?? ''),
        retrievedAt: String(record['retrievedAt'] ?? ''),
      };
      if (source.id === '') error(`sources[${index}].id`, 'falta o id');
      if (!source.url.startsWith('http')) error(`sources[${index}].url`, 'url ausente ou não-http');
      sources.push(source);
    });
  }
  const sourceIds = new Set(sources.map((source) => source.id));

  const constantsRaw = root['constants'];
  if (constantsRaw === null || typeof constantsRaw !== 'object') {
    error('constants', 'o bloco constants é obrigatório');
  } else {
    const numbers: Array<{ path: string; value: number }> = [];
    collectNumbers(constantsRaw, 'constants', numbers);
    for (const { path, value } of numbers) {
      if (!Number.isInteger(value) && !NON_INTEGER_ALLOWED.test(path)) {
        warn(
          path,
          `${value} não é um número inteiro; dinheiro é em cêntimos de euro e taxas em pontos base. ` +
            'Um valor fracionário aqui é quase sempre um erro.',
        );
      }
    }
  }

  const obligationsRaw = root['obligations'];
  const obligations: ObligationRule[] = [];
  if (!Array.isArray(obligationsRaw)) {
    error('obligations', 'obligations tem de ser uma lista');
  } else {
    const seen = new Set<string>();
    obligationsRaw.forEach((entry, index) => {
      const at = `obligations[${index}]`;
      if (entry === null || typeof entry !== 'object') {
        error(at, 'tem de ser um objeto');
        return;
      }
      const record = entry as Record<string, unknown>;
      const id = String(record['id'] ?? '');
      if (id === '') error(`${at}.id`, 'falta o id');
      else if (seen.has(id)) error(`${at}.id`, `id duplicado "${id}"`);
      else seen.add(id);

      const kind = record['kind'] as ObligationKind;
      if (!OBLIGATION_KINDS.includes(kind)) {
        error(`${at}.kind`, `tipo desconhecido "${String(record['kind'])}"`);
      }
      const authority = normaliseAuthority(record['authority']);
      if (authority === 'OTHER' && String(record['authority'] ?? '') !== 'OTHER') {
        warn(
          `${at}.authority`,
          `autoridade "${String(record['authority'])}" não reconhecida; classificada como OTHER.`,
        );
      }
      const verification = (record['verification'] ?? 'unverified') as Verification;
      if (!VERIFICATIONS.includes(verification)) {
        error(`${at}.verification`, `verificação desconhecida "${String(record['verification'])}"`);
      }

      const appliesRaw = record['appliesWhen'];
      const appliesWhen: Condition[] = [];
      if (appliesRaw !== undefined && appliesRaw !== null) {
        if (!Array.isArray(appliesRaw)) {
          error(`${at}.appliesWhen`, 'tem de ser uma lista de condições');
        } else {
          appliesRaw.forEach((condition, conditionIndex) => {
            const conditionAt = `${at}.appliesWhen[${conditionIndex}]`;
            if (condition === null || typeof condition !== 'object') {
              error(conditionAt, 'tem de ser um objeto');
              return;
            }
            const conditionRecord = condition as Record<string, unknown>;
            const field = String(conditionRecord['field'] ?? '');
            const op = String(conditionRecord['op'] ?? '');
            if (!isConditionField(field)) error(`${conditionAt}.field`, `campo desconhecido "${field}"`);
            if (!isConditionOp(op)) error(`${conditionAt}.op`, `operador desconhecido "${op}"`);
            appliesWhen.push({
              field: field as Condition['field'],
              op: op as Condition['op'],
              value: conditionRecord['value'] as Condition['value'],
            });
          });
        }
      }

      const periodicityRaw = record['periodicity'];
      const periodicity = { type: 'annual' as (typeof PERIODICITY_TYPES)[number], lagMonths: undefined as number | undefined };
      if (periodicityRaw === null || typeof periodicityRaw !== 'object') {
        error(`${at}.periodicity`, 'periodicity é obrigatório');
      } else {
        const periodicityRecord = periodicityRaw as Record<string, unknown>;
        const type = String(periodicityRecord['type'] ?? '');
        if (!(PERIODICITY_TYPES as readonly string[]).includes(type)) {
          error(`${at}.periodicity.type`, `periodicidade desconhecida "${type}"`);
        } else {
          periodicity.type = type as (typeof PERIODICITY_TYPES)[number];
        }
        if (periodicityRecord['lagMonths'] !== undefined && periodicityRecord['lagMonths'] !== null) {
          periodicity.lagMonths = Number(periodicityRecord['lagMonths']);
        }
      }

      const deadlineRaw = record['deadlineRule'];
      let deadlineRule: ObligationRule['deadlineRule'] = null;
      if (deadlineRaw !== null && deadlineRaw !== undefined) {
        if (typeof deadlineRaw !== 'object') {
          error(`${at}.deadlineRule`, 'tem de ser um objeto ou null');
        } else {
          const deadlineRecord = deadlineRaw as Record<string, unknown>;
          const kind = String(deadlineRecord['kind'] ?? '');
          if (!(DEADLINE_KINDS as readonly string[]).includes(kind)) {
            error(`${at}.deadlineRule.kind`, `tipo de prazo desconhecido "${kind}"`);
          } else {
            const numeric: Record<string, number> = {};
            for (const key of [
              'day',
              'month',
              'monthFrom',
              'monthTo',
              'dayTo',
              'dayFrom',
              'days',
              'iva',
              'irs',
            ]) {
              const value = deadlineRecord[key];
              if (value !== undefined && value !== null && Number.isFinite(Number(value))) {
                numeric[key] = Number(value);
              }
            }
            if (Array.isArray(deadlineRecord['months'])) {
              warn(
                `${at}.deadlineRule.months`,
                'a lista "months" não é usada pelo motor: o mês de vencimento resulta de ' +
                  'periodicity.lagMonths. Confirma que as duas descrições coincidem.',
              );
            }
            deadlineRule = {
              kind: kind as DeadlineRule['kind'],
              ...numeric,
              ...(typeof deadlineRecord['note'] === 'string' ? { note: deadlineRecord['note'] } : {}),
            } as DeadlineRule;
          }
        }
      }

      const officialDates = normaliseOfficialDates(record);
      const hasOfficialDates = Object.keys(officialDates).length > 0;
      if (deadlineRule === null && !hasOfficialDates) {
        warn(`${at}`, 'sem deadlineRule e sem datas publicadas: esta obrigação não pode ser agendada');
      }

      const ruleSourceIds = Array.isArray(record['sourceIds'])
        ? record['sourceIds'].map((value) => String(value))
        : [];
      for (const referenced of ruleSourceIds) {
        if (!sourceIds.has(referenced)) {
          warn(`${at}.sourceIds`, `cita uma fonte desconhecida "${referenced}"`);
        }
      }
      const legalBasis = Array.isArray(record['legalBasis'])
        ? record['legalBasis'].map((value) => String(value))
        : [];
      if (legalBasis.length === 0) {
        warn(`${at}.legalBasis`, 'sem base legal registada; todas as regras devem citar a lei');
      }
      if (verification === 'verified' && ruleSourceIds.length === 0) {
        warn(
          `${at}.verification`,
          'declara "verified" sem citar qualquer fonte: uma regra verificada tem de citar a origem.',
        );
      }

      obligations.push({
        id,
        kind: (OBLIGATION_KINDS.includes(kind) ? kind : 'review') satisfies ObligationKind,
        authority,
        tax: String(record['tax'] ?? ''),
        title: String(record['title'] ?? id),
        appliesWhen,
        periodicity,
        deadlineRule,
        officialDates,
        legalBasis,
        sourceIds: ruleSourceIds,
        portalUrl: typeof record['portalUrl'] === 'string' ? record['portalUrl'] : null,
        documentsToKeep: Array.isArray(record['documentsToKeep'])
          ? record['documentsToKeep'].map((value) => String(value))
          : [],
        penaltyNote: typeof record['penaltyNote'] === 'string' ? record['penaltyNote'] : null,
        verification: (VERIFICATIONS.includes(verification) ? verification : 'unverified') satisfies Verification,
        verifyNote: typeof record['verifyNote'] === 'string' ? record['verifyNote'] : null,
      });
    });
  }

  const pack: RulePack = {
    jurisdiction: 'PT',
    year,
    packVersion: String(root['packVersion'] ?? '0.0.0'),
    generatedAt: String(root['generatedAt'] ?? ''),
    units: { money: 'EUR_cents', rate: 'basis_points' },
    disclaimer: String(root['disclaimer'] ?? ''),
    sources,
    constants: (constantsRaw ?? {}) as RulePack['constants'],
    ...(root['constantProvenance'] === undefined || root['constantProvenance'] === null
      ? {}
      : { constantProvenance: root['constantProvenance'] as RulePack['constantProvenance'] }),
    obligations,
    todo: Array.isArray(root['todo']) ? root['todo'].map((value) => String(value)) : [],
  };

  if (pack.disclaimer === '') {
    warn('disclaimer', 'sem advertência registada');
  }
  return { pack, problems };
}

export function loadRulePack(path: string): LoadedPack {
  const text = readFileSync(path, 'utf8');
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (cause) {
    throw new SyntaxError(`${path} não é JSON válido: ${(cause as Error).message}`);
  }
  const { pack, problems } = validateRulePack(raw);
  const errors = problems.filter((problem) => problem.level === 'error');
  if (errors.length > 0) {
    const detail = errors.map((problem) => `  ${problem.path}: ${problem.message}`).join('\n');
    throw new Error(`${path} falhou a validação com ${errors.length} erro(s):\n${detail}`);
  }
  return { pack, problems, checksum: checksumOf(pack), path };
}

export interface PackSummary {
  packVersion: string;
  year: number;
  checksum: string;
  sources: number;
  officialSources: number;
  obligations: number;
  verified: number;
  partial: number;
  unverified: number;
  stale: number;
  /** Obligations with no fixed date, scheduled by an event rather than a calendar. */
  eventDriven: number;
  todo: string[];
  nullConstants: string[];
}

export function summarisePack(loaded: LoadedPack): PackSummary {
  const { pack } = loaded;
  const nulls: string[] = [];
  collectNulls(pack.constants, 'constants', nulls);
  return {
    packVersion: pack.packVersion,
    year: pack.year,
    checksum: loaded.checksum,
    sources: pack.sources.length,
    officialSources: pack.sources.filter((source) => source.tier === 'official').length,
    obligations: pack.obligations.length,
    verified: pack.obligations.filter((rule) => rule.verification === 'verified').length,
    partial: pack.obligations.filter((rule) => rule.verification === 'partial').length,
    unverified: pack.obligations.filter((rule) => rule.verification === 'unverified').length,
    stale: pack.obligations.filter((rule) => rule.verification === 'stale').length,
    eventDriven: pack.obligations.filter((rule) => rule.periodicity.type === 'event_driven').length,
    todo: pack.todo,
    nullConstants: nulls,
  };
}

export interface Freshness {
  status: 'current' | 'stale' | 'future';
  message: string;
}

/**
 * Rule packs are annual artefacts. The authority publishes next year's calendar
 * only a few weeks before it starts, so the app must be honest about the gap
 * rather than pretend to know dates that have not been published.
 */
export function freshness(pack: RulePack, today: string): Freshness {
  const currentYear = parseIso(today).year;
  if (pack.year === currentYear) {
    return { status: 'current', message: `Pacote de regras de ${pack.year} (ano em curso).` };
  }
  if (pack.year < currentYear) {
    return {
      status: 'stale',
      message:
        `O pacote de regras é de ${pack.year} mas o ano em curso é ${currentYear}. ` +
        'As datas publicadas não cobrem este ano: os prazos são provisórios e têm de ser conferidos.',
    };
  }
  return {
    status: 'future',
    message: `Pacote de regras de ${pack.year}, à frente do ano em curso (${currentYear}).`,
  };
}
