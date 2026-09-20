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
  exports?: boolean;
  hasEmployees?: boolean;
  usesCertifiedInvoicingSoftware?: boolean;
  usesAtWebservice?: boolean;
  startupExemptionActive?: boolean;
}

export function createDefaultProfile(input: ProfileInput): TaxProfile {
  const cae: CaeEntry[] =
    input.cae !== undefined && input.cae.length > 0
      ? input.cae
      : [{ code: '62010', description: 'Atividades de programação informática', role: 'principal' }];

  return {
    nif: normaliseNif(input.nif),
    name: input.name,
    residentPT: input.residentPT ?? true,
    isCompany: input.isCompany ?? false,
    ...(input.trackingStart === undefined ? {} : { trackingStart: input.trackingStart }),
    activity: {
      categoryB: input.categoryB ?? true,
      startDate: input.startDate ?? '1970-01-01',
      cae,
      // Both turnover figures are optional AND never defaulted: the art. 53.º
      // evaluation must be able to say "I do not know" rather than assume.
      ...(input.turnoverPreviousYearCents === undefined
        ? {}
        : { turnoverPreviousYearCents: input.turnoverPreviousYearCents }),
      ...(input.turnoverCurrentYearExpectedCents === undefined
        ? {}
        : { turnoverCurrentYearExpectedCents: input.turnoverCurrentYearExpectedCents }),
      intraCommunityOperations: input.intraCommunityOperations ?? false,
      exports: input.exports ?? false,
      hasEmployees: input.hasEmployees ?? false,
      usesCertifiedInvoicingSoftware: input.usesCertifiedInvoicingSoftware ?? true,
      usesAtWebservice: input.usesAtWebservice ?? false,
    },
    iva: {
      regime: input.ivaRegime ?? 'trimestral',
    },
    irs: {
      regime: input.irsRegime ?? 'simplificado',
      coefficientBp: input.coefficientBp ?? 7500,
    },
    ss: {
      startupExemptionActive: input.startupExemptionActive ?? false,
      firstActivityDate: input.startDate,
    },
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
