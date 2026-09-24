/**
 * The taxpayer profile, and the consistency checks that go with it.
 *
 * These checks encode real constraints of the Portuguese system rather than
 * cosmetic validation. Two examples carried in the code below: the article 53.º
 * exemption demands that the taxpayer carries out no export operations and has
 * national domicile, and a startup contribution exemption is a twelve-month
 * window. Both are things a taxpayer can get wrong silently for a year.
 */

import { isValidPtTaxNumber, normaliseNif } from './nif.ts';
import type { CaeEntry, IvaRegime, IrsRegime, TaxProfile } from './types.ts';

/** The regimes the AT lets a taxpayer declare. Anything else is not a regime. */
export const IVA_REGIMES: readonly IvaRegime[] = ['isento_art53', 'trimestral', 'mensal'];
export const IRS_REGIMES: readonly IrsRegime[] = ['simplificado', 'organizada'];

/** The CAE the interface offers when the user does not name one. */
export const DEFAULT_CAE: CaeEntry = {
  code: '62010',
  description: 'Atividades de programação informática',
  role: 'principal',
};

export interface ProfileProblem {
  level: 'error' | 'warning';
  field: string;
  message: string;
}

export interface ProfileInput {
  nif: string;
  name: string;
  residentPT?: boolean;
  isCompany?: boolean;
  cae?: CaeEntry[];
  startDate?: string;
  trackingStart?: string;
  /** Previous civil year's turnover in national territory, in euro cents. */
  turnoverPreviousYearCents?: number;
  /** The user's estimate for the current civil year, in euro cents. */
  turnoverCurrentYearExpectedCents?: number;
  categoryB?: boolean;
  ivaRegime?: IvaRegime;
  irsRegime?: IrsRegime;
  coefficientBp?: number;
  intraCommunityOperations?: boolean;
  /** RITI art. 30.º n.º 2: intra-community operations above 50 000 EUR in a quarter. */
  intraCommunityOperationsAbove50k?: boolean;
  exports?: boolean;
  hasEmployees?: boolean;
  usesCertifiedInvoicingSoftware?: boolean;
  usesAtWebservice?: boolean;
  startupExemptionActive?: boolean;
}

/**
 * Everything the interface collects when it creates or edits a profile.
 *
 * Unlike `ProfileInput`, which describes a brand-new profile, every field here is
 * optional: the profile being drafted may already exist, and a draft that omits a
 * field must leave the stored value alone rather than reset it to a default. What
 * a profile must have to exist at all is decided by `buildProfile` (NIF and name)
 * and by the interfaces (`init` and the panel also require a declared regime).
 */
export interface ProfileDraft {
  nif?: string;
  name?: string;
  ivaRegime?: IvaRegime;
  irsRegime?: IrsRegime;
  startDate?: string;
  trackingStart?: string;
  turnoverPreviousYearCents?: number | null;
  turnoverCurrentYearExpectedCents?: number | null;
  cae?: CaeEntry[];
  categoryB?: boolean;
  residentPT?: boolean;
  isCompany?: boolean;
  intraCommunityOperations?: boolean;
  intraCommunityOperationsAbove50k?: boolean;
  exports?: boolean;
  startupExemptionActive?: boolean;
}

/**
 * Build one complete profile from a draft and, optionally, the profile it
 * replaces.
 *
 * Every field the draft omits is taken from `current` when there is one, and
 * only then from the documented default. A field explicitly set to `null` clears
 * an optional value — which is how a wrong turnover figure is removed — and is
 * nevertheless a decision the caller made, not something this function guessed.
 *
 * The IVA regime has a default here for the same reason `createDefaultProfile`
 * always had one: a profile built by a test, a fixture or an old import should
 * not be blocked on a declaration it is not making. What the INTERFACES require
 * is a separate matter, and they require it: the profile form and the `init`
 * command both refuse to create a profile without a declared regime, because a
 * guess there would silently change every downstream obligation.
 */
export function buildProfile(
  draft: ProfileDraft,
  options: { current?: TaxProfile; coefficientBp?: number } = {},
): TaxProfile {
  const current = options.current;
  const nif = draft.nif ?? current?.nif;
  if (nif === undefined) throw new Error('o NIF é obrigatório para criar o perfil.');
  const name = draft.name ?? current?.name;
  if (name === undefined) throw new Error('o nome é obrigatório para criar o perfil.');
  const ivaRegime = draft.ivaRegime ?? current?.iva.regime ?? 'trimestral';

  // Null deletes an optional figure; undefined means the draft did not mention it.
  const carryOver = (
    drafted: number | null | undefined,
    stored: number | undefined,
  ): number | undefined => (drafted === null ? undefined : (drafted ?? stored));

  const startDate = draft.startDate ?? current?.activity.startDate ?? '1970-01-01';
  const turnoverPreviousYearCents = carryOver(
    draft.turnoverPreviousYearCents,
    current?.activity.turnoverPreviousYearCents,
  );
  const turnoverCurrentYearExpectedCents = carryOver(
    draft.turnoverCurrentYearExpectedCents,
    current?.activity.turnoverCurrentYearExpectedCents,
  );

  const firstActivityDate = current?.ss.firstActivityDate ?? startDate;

  return {
    nif: normaliseNif(nif),
    name,
    residentPT: draft.residentPT ?? current?.residentPT ?? true,
    isCompany: draft.isCompany ?? current?.isCompany ?? false,
    // Only set when someone said so. A profile written without a tracking date is
    // one the panel has not opened yet, and inventing "today" here would silently
    // turn this year's earlier deadlines into history.
    ...(draft.trackingStart === undefined && current?.trackingStart === undefined
      ? {}
      : { trackingStart: draft.trackingStart ?? current?.trackingStart }),
    activity: {
      categoryB: draft.categoryB ?? current?.activity.categoryB ?? true,
      startDate,
      cae:
        draft.cae !== undefined && draft.cae.length > 0
          ? draft.cae
          : current?.activity.cae !== undefined && current.activity.cae.length > 0
            ? current.activity.cae
            : [{ ...DEFAULT_CAE }],
      ...(turnoverPreviousYearCents === undefined ? {} : { turnoverPreviousYearCents }),
      ...(turnoverCurrentYearExpectedCents === undefined ? {} : { turnoverCurrentYearExpectedCents }),
      intraCommunityOperations: draft.intraCommunityOperations ?? current?.activity.intraCommunityOperations ?? false,
      // Deliberately NOT defaulted to false. Absent means "nobody answered", which
      // is a different fact from "no", and the recapitulativa frequency cannot be
      // settled without the answer — see the RITI art. 30.º conditions.
      ...(draft.intraCommunityOperationsAbove50k === undefined &&
      current?.activity.intraCommunityOperationsAbove50k === undefined
        ? {}
        : {
            intraCommunityOperationsAbove50k:
              draft.intraCommunityOperationsAbove50k ?? current?.activity.intraCommunityOperationsAbove50k,
          }),
      exports: draft.exports ?? current?.activity.exports ?? false,
      hasEmployees: current?.activity.hasEmployees ?? false,
      usesCertifiedInvoicingSoftware: current?.activity.usesCertifiedInvoicingSoftware ?? true,
      usesAtWebservice: current?.activity.usesAtWebservice ?? false,
    },
    iva: { regime: ivaRegime },
    irs: {
      regime: draft.irsRegime ?? current?.irs.regime ?? 'simplificado',
      coefficientBp: options.coefficientBp ?? current?.irs.coefficientBp ?? 7500,
    },
    ss: {
      startupExemptionActive: draft.startupExemptionActive ?? current?.ss.startupExemptionActive ?? false,
      firstActivityDate,
    },
  };
}

export function createDefaultProfile(input: ProfileInput): TaxProfile {
  return buildProfile(
    {
      nif: input.nif,
      name: input.name,
      ...(input.ivaRegime === undefined ? {} : { ivaRegime: input.ivaRegime }),
      ...(input.irsRegime === undefined ? {} : { irsRegime: input.irsRegime }),
      ...(input.categoryB === undefined ? {} : { categoryB: input.categoryB }),
      ...(input.residentPT === undefined ? {} : { residentPT: input.residentPT }),
      ...(input.isCompany === undefined ? {} : { isCompany: input.isCompany }),
      ...(input.cae === undefined ? {} : { cae: input.cae }),
      ...(input.startDate === undefined ? {} : { startDate: input.startDate }),
      ...(input.trackingStart === undefined ? {} : { trackingStart: input.trackingStart }),
      ...(input.turnoverPreviousYearCents === undefined
        ? {}
        : { turnoverPreviousYearCents: input.turnoverPreviousYearCents }),
      ...(input.turnoverCurrentYearExpectedCents === undefined
        ? {}
        : { turnoverCurrentYearExpectedCents: input.turnoverCurrentYearExpectedCents }),
      ...(input.intraCommunityOperations === undefined
        ? {}
        : { intraCommunityOperations: input.intraCommunityOperations }),
      ...(input.exports === undefined ? {} : { exports: input.exports }),
      ...(input.intraCommunityOperationsAbove50k === undefined
        ? {}
        : { intraCommunityOperationsAbove50k: input.intraCommunityOperationsAbove50k }),
      ...(input.startupExemptionActive === undefined
        ? {}
        : { startupExemptionActive: input.startupExemptionActive }),
    },
    input.coefficientBp === undefined ? {} : { coefficientBp: input.coefficientBp },
  );
}

/**
 * A profile that arrived as a file, turned back into a profile.
 *
 * An import is not a trusted operation: the file may have been hand-edited, may
 * come from an older version, or may not be a profile at all. Every unknown is a
 * problem for `validateProfile` to report, but a value that is the wrong *type*
 * is refused here, because storing it would make every later calculation
 * nonsense rather than merely wrong.
 */
export function normaliseImportedProfile(value: unknown): TaxProfile {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('o ficheiro não contém um perfil: esperava um objeto JSON.');
  }
  const raw = value as Record<string, unknown>;
  const nif = raw['nif'];
  const name = raw['name'];
  if (typeof nif !== 'string' || typeof name !== 'string') {
    throw new Error('o perfil importado tem de ter "nif" e "name" como texto.');
  }

  const activity = raw['activity'];
  const iva = raw['iva'];
  const irs = raw['irs'];
  const ss = raw['ss'];
  const activityRecord = activity !== null && typeof activity === 'object' ? (activity as Record<string, unknown>) : {};
  const ivaRecord = iva !== null && typeof iva === 'object' ? (iva as Record<string, unknown>) : {};
  const irsRecord = irs !== null && typeof irs === 'object' ? (irs as Record<string, unknown>) : {};
  const ssRecord = ss !== null && typeof ss === 'object' ? (ss as Record<string, unknown>) : {};

  const importedIva = readEnum(ivaRecord['regime'], IVA_REGIMES, 'o regime de IVA');
  const importedIrs = readEnum(irsRecord['regime'], IRS_REGIMES, 'o regime de IRS');

  return buildProfile({
    nif,
    name,
    ...(importedIva === undefined ? {} : { ivaRegime: importedIva }),
    ...(importedIrs === undefined ? {} : { irsRegime: importedIrs }),
    ...readOptionalText(activityRecord['startDate'], 'a data de início de atividade', 'startDate'),
    ...readOptionalText(raw['trackingStart'], 'a data de início do acompanhamento', 'trackingStart'),
    ...readOptionalCents(activityRecord['turnoverPreviousYearCents'], 'turnoverPreviousYearCents'),
    ...readOptionalCents(activityRecord['turnoverCurrentYearExpectedCents'], 'turnoverCurrentYearExpectedCents'),
    ...readCae(activityRecord['cae']),
    ...(typeof activityRecord['intraCommunityOperations'] === 'boolean'
      ? { intraCommunityOperations: activityRecord['intraCommunityOperations'] }
      : {}),
    ...(typeof activityRecord['exports'] === 'boolean' ? { exports: activityRecord['exports'] } : {}),
    ...(typeof activityRecord['intraCommunityOperationsAbove50k'] === 'boolean'
      ? { intraCommunityOperationsAbove50k: activityRecord['intraCommunityOperationsAbove50k'] }
      : {}),
    ...(typeof raw['residentPT'] === 'boolean' ? { residentPT: raw['residentPT'] } : {}),
    ...(typeof raw['isCompany'] === 'boolean' ? { isCompany: raw['isCompany'] } : {}),
    ...(typeof ssRecord['startupExemptionActive'] === 'boolean'
      ? { startupExemptionActive: ssRecord['startupExemptionActive'] }
      : {}),
  }, {
    ...(typeof irsRecord['coefficientBp'] === 'number' ? { coefficientBp: irsRecord['coefficientBp'] } : {}),
  });
}

function readEnum<T extends string>(value: unknown, allowed: readonly T[], label: string): T | undefined {
  if (value === undefined || value === null) return undefined;
  if (typeof value === 'string' && (allowed as readonly string[]).includes(value)) return value as T;
  throw new Error(`${label} "${String(value)}" não é reconhecido (esperado: ${allowed.join(', ')}).`);
}

function readOptionalText(
  value: unknown,
  label: string,
  field: string,
): Record<string, string> {
  if (value === undefined || value === null || value === '') return {};
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    throw new Error(`${label} não está em formato AAAA-MM-DD.`);
  }
  return { [field]: value };
}

function readOptionalCents(value: unknown, field: string): Record<string, number> {
  if (value === undefined || value === null) return {};
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) {
    throw new Error(`"${field}" tem de ser um valor em cêntimos (inteiro, não negativo).`);
  }
  return { [field]: value };
}

function readCae(value: unknown): { cae?: CaeEntry[] } {
  if (value === undefined || value === null) return {};
  if (!Array.isArray(value) || value.length === 0) {
    throw new Error('"activity.cae" tem de ser uma lista com pelo menos um CAE.');
  }
  return {
    cae: value.map((entry) => {
      if (entry === null || typeof entry !== 'object') {
        throw new Error('cada CAE tem de ser um objeto com "code", "description" e "role".');
      }
      const record = entry as Record<string, unknown>;
      const code = record['code'];
      const description = record['description'];
      const role = record['role'];
      if (typeof code !== 'string' || typeof description !== 'string') {
        throw new Error('cada CAE tem de ter "code" e "description" em texto.');
      }
      if (role !== 'principal' && role !== 'secondary') {
        throw new Error(`o CAE ${code} tem de ter "role" principal ou secondary.`);
      }
      return { code, description, role };
    }),
  };
}

function monthsSince(startDate: string, today = new Date()): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(startDate);
  if (match?.[1] === undefined || match[2] === undefined) return null;
  const start = new Date(Date.UTC(Number(match[1]), Number(match[2]) - 1, 1));
  const now = new Date(Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), 1));
  return (now.getUTCFullYear() - start.getUTCFullYear()) * 12 + (now.getUTCMonth() - start.getUTCMonth());
}

/**
 * The inputs only the taxpayer can supply.
 *
 * Kept separate from `validateProfile` because the two mean different things: a
 * validation problem is "what you typed is wrong", while a missing input is "the
 * application cannot run this check at all without your number". Both the CLI
 * and the local panel print this list, so it lives here rather than in either.
 */
export function missingProfileInputs(profile: TaxProfile): string[] {
  const missing: string[] = [];
  if (profile.activity.turnoverPreviousYearCents === undefined) {
    missing.push(
      'volume de negócios do ano anterior em território nacional — obrigatório para avaliar o ' +
        'art. 53.º do CIVA e para avisar quando o limite se aproxima',
    );
  }
  if (profile.activity.turnoverCurrentYearExpectedCents === undefined) {
    missing.push(
      'estimativa de volume de negócios para o ano corrente — é o que permite avisar a meio do ano ' +
        'se o limite do art. 53.º está a ser ultrapassado',
    );
  }
  if (profile.activity.startDate === '1970-01-01') {
    missing.push('data de início de atividade');
  }
  if (!profile.activity.intraCommunityOperations && !profile.activity.exports) {
    missing.push(
      'se faturas a clientes fora de Portugal (UE ou países terceiros) — determina a declaração ' +
        'recapitulativa, o VIES e o tratamento de IVA nas faturas',
    );
  }
  return missing;
}

export function validateProfile(profile: TaxProfile): ProfileProblem[] {
  const problems: ProfileProblem[] = [];
  const error = (field: string, message: string): void => {
    problems.push({ level: 'error', field, message });
  };
  const warn = (field: string, message: string): void => {
    problems.push({ level: 'warning', field, message });
  };

  if (!isValidPtTaxNumber(profile.nif)) {
    error('nif', `"${profile.nif}" não é um NIF válido (o dígito de controlo não confere).`);
  }
  if (profile.name.trim() === '') {
    error('name', 'o nome do contribuinte está vazio.');
  }
  if (profile.activity.cae.length === 0) {
    error('activity.cae', 'não há qualquer CAE definido.');
  }
  if (!profile.activity.cae.some((entry) => entry.role === 'principal')) {
    warn('activity.cae', 'não há CAE principal definido; o CAE principal determina a atividade declarada.');
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(profile.activity.startDate)) {
    error('activity.startDate', 'a data de início de atividade não está em formato AAAA-MM-DD.');
  }
  if (!profile.activity.categoryB) {
    warn(
      'activity.categoryB',
      'sem atividade aberta na categoria B não é possível emitir recibos verdes: confirma o início de atividade.',
    );
  }

  // The art. 53.º evaluation is impossible without this figure, and a tool that
  // guesses it is a tool that cannot be trusted to say whether the exemption
  // applies. Better to name the missing input than to assume a value.
  if (profile.activity.turnoverPreviousYearCents === undefined) {
    warn(
      'activity.turnoverPreviousYearCents',
      'falta o volume de negócios do ano anterior em território nacional. Sem esse valor a aplicação ' +
        'não pode avaliar a isenção do art. 53.º do CIVA nem avisar quando o limite se aproxima.',
    );
  }

  // Art. 53.º CIVA requires national domicile and no export operations.
  if (profile.iva.regime === 'isento_art53') {
    if (!profile.residentPT) {
      error(
        'iva.regime',
        'o regime especial de isenção do art. 53.º do CIVA exige domicílio ou sede em território nacional.',
      );
    }
    if (profile.activity.exports) {
      error(
        'iva.regime',
        'o regime especial de isenção do art. 53.º do CIVA não é aplicável a quem pratique operações de ' +
          'exportação ou atividades conexas. O perfil indica exportações.',
      );
    }
  }

  if (profile.irs.regime === 'simplificado' && profile.irs.coefficientBp <= 0) {
    error('irs.coefficientBp', 'o coeficiente do regime simplificado tem de ser positivo.');
  }

  if (profile.ss.startupExemptionActive) {
    const months = monthsSince(profile.activity.startDate);
    if (months !== null && months > 12) {
      warn(
        'ss.startupExemptionActive',
        `a isenção de contribuições do primeiro ano é uma janela de 12 meses e a atividade começou há ` +
          `${months} meses. Confirma o enquadramento na Segurança Social Social Direta.`,
      );
    }
  }

  if (profile.activity.intraCommunityOperations && profile.iva.regime !== 'mensal' && profile.iva.regime !== 'trimestral') {
    warn(
      'activity.intraCommunityOperations',
      'há operações intracomunitárias: confirma o registo no VIES e a obrigação de declaração recapitulativa.',
    );
  }

  return problems;
}
