/**
 * The assistant's ONLY job.
 *
 * It is not a conversationalist, an adviser, or a reviewer. It does not answer
 * questions about your affairs and it never sees your invoices, your clients or
 * your turnover. It performs one narrow mechanical task: given the list of
 * rule variables that Portuguese law changes over time, propose their current
 * values, in the pack's exact units, with a source URL for each.
 *
 * Three constraints are enforced in code rather than promised in prose:
 *
 *   1. THE PAYLOAD CONTAINS NO PERSONAL DATA. `buildUpdateMessages` can only be
 *      built from a rule pack. There is no code path that puts a profile, an
 *      invoice or a flag into an update request.
 *   2. THE RESPONSE IS DATA, NOT TEXT. `parseUpdateResponse` requires JSON, keys
 *      every value to a variable that was actually asked about, and rejects
 *      anything else. Prose cannot reach the user through this path.
 *   3. THE ASSISTANT CANNOT MAKE A VALUE TRUE. An applied proposal is recorded
 *      as `ai-proposed` and `unverified`, and stays that way until a human
 *      confirms it against the cited source. The assistant proposes; it never
 *      verifies.
 */

import type { RulePack } from '../core/types.ts';
import type { ChatMessage } from './types.ts';

export type VariableUnit = 'EUR_cents' | 'basis_points' | 'hundredths_of_ias' | 'months';

export interface UpdatableVariable {
  /** Dotted path inside `constants`. */
  path: string;
  label: string;
  unit: VariableUnit;
  currentValue: number | null;
  sourceIds: string[];
  /** Plausibility bounds in the variable's own unit, where the unit alone is too loose. */
  min?: number;
  max?: number;
}

/**
 * The variables that change over time, with the label the model is asked about.
 *
 * `min` and `max` are plausibility bounds, not legal limits. They exist because
 * "an integer number of basis points between 0 and 10 000" would happily accept
 * a VAT rate of 0,23%: syntactically fine, obviously nonsense. A proposal has to
 * survive both the unit check and the magnitude check.
 */
const VARIABLE_LABELS: Record<
  string,
  { label: string; unit: VariableUnit; min?: number; max?: number }
> = {
  'iva.rates.normal': {
    label: 'Taxa normal de IVA no continente',
    unit: 'basis_points',
    min: 1000,
    max: 3000,
  },
  'iva.rates.intermediaire': {
    label: 'Taxa intermédia de IVA no continente',
    unit: 'basis_points',
    min: 300,
    max: 2000,
  },
  'iva.rates.reduzida': {
    label: 'Taxa reduzida de IVA no continente',
    unit: 'basis_points',
    min: 100,
    max: 1500,
  },
  'iva.art53.ceiling': {
    label:
      'Limite do regime especial de isenção do art. 53.º do CIVA (volume de negócios do ano anterior ' +
      'em território nacional)',
    unit: 'EUR_cents',
    min: 500_000,
    max: 100_000_000,
  },
  'iva.periodicRegime.quarterlyCeiling': {
    label: 'Limite de volume de negócios até ao qual o IVA pode ser declarado trimestralmente',
    unit: 'EUR_cents',
    min: 1_000_000,
    max: 1_000_000_000,
  },
  'iva.periodicRegime.monthlyAbove': {
    label: 'Limite acima do qual o IVA passa a ser declarado mensalmente',
    unit: 'EUR_cents',
    min: 1_000_000,
    max: 1_000_000_000,
  },
  'irs.simplifiedRegime.ceilingForOrganisedAccounting': {
    label: 'Limite de volume de negócios que obriga a contabilidade organizada',
    unit: 'EUR_cents',
    min: 1_000_000,
    max: 1_000_000_000,
  },
  'irs.simplifiedRegime.coefficients.servicesProfessionalTable4': {
    label: 'Coeficiente do regime simplificado para serviços profissionais da Tabela 4',
    unit: 'basis_points',
    min: 1000,
    max: 10_000,
  },
  'irs.simplifiedRegime.coefficients.servicesOther': {
    label: 'Coeficiente do regime simplificado para outras prestações de serviços',
    unit: 'basis_points',
    min: 1000,
    max: 10_000,
  },
  'irs.simplifiedRegime.documentedExpensesDeductionRate': {
    label:
      'Percentagem de referência das despesas documentadas do art. 31.º n.º 13 do CIRS ' +
      '(acresce ao rendimento tributável a diferença positiva face às despesas elegíveis)',
    unit: 'basis_points',
    min: 100,
    max: 5000,
  },
  'irs.simplifiedRegime.documentedExpensesDeductionCap': {
    label: 'Limite monetário da dedução de despesas documentadas, se a lei fixar algum',
    unit: 'EUR_cents',
    max: 10_000_000,
  },
  'irs.withholding.professionalServicesResident': {
    label: 'Taxa de retenção na fonte sobre serviços profissionais pagos a residentes',
    unit: 'basis_points',
    max: 5000,
  },
  'irs.withholding.otherCategoryBResident': {
    label: 'Taxa de retenção na fonte sobre outros rendimentos da categoria B pagos a residentes',
    unit: 'basis_points',
    max: 5000,
  },
  'irs.withholding.nonResident': {
    label: 'Taxa de retenção na fonte sobre rendimentos pagos a não residentes',
    unit: 'basis_points',
    max: 5000,
  },
  'socialSecurity.contributionRate': {
    label: 'Taxa contributiva da Segurança Social dos trabalhadores independentes',
    unit: 'basis_points',
    min: 500,
    max: 5000,
  },
  'socialSecurity.relevantIncomeShareServices': {
    label: 'Percentagem do rendimento de prestações de serviços que constitui rendimento relevante',
    unit: 'basis_points',
    min: 1000,
    max: 10_000,
  },
  'socialSecurity.relevantIncomeShareGoods': {
    label: 'Percentagem do rendimento de venda de bens que constitui rendimento relevante',
    unit: 'basis_points',
    min: 1000,
    max: 10_000,
  },
  'socialSecurity.baseMinMultipleIas': {
    label: 'Base de incidência contributiva mínima, em centésimos do IAS (1,5 × IAS escreve-se 150)',
    unit: 'hundredths_of_ias',
    min: 50,
    max: 1000,
  },
  'socialSecurity.baseMaxMultipleIas': {
    label: 'Base de incidência contributiva máxima, em centésimos do IAS (12 × IAS escreve-se 1200)',
    unit: 'hundredths_of_ias',
    min: 500,
    max: 5000,
  },
  'socialSecurity.iasMonthly': {
    label: 'Valor mensal do IAS (indexante dos apoios sociais)',
    unit: 'EUR_cents',
    min: 30_000,
    max: 200_000,
  },
  'socialSecurity.startupExemptionMonths': {
    label: 'Meses de diferimento contributivo no início de atividade',
    unit: 'months',
    max: 36,
  },
};

function leaves(node: unknown, prefix = ''): Array<{ path: string; value: number | null }> {
  if (node === null) return [{ path: prefix, value: null }];
  if (typeof node === 'number') return [{ path: prefix, value: node }];
  if (Array.isArray(node) || typeof node !== 'object') return [];
  const out: Array<{ path: string; value: number | null }> = [];
  for (const [key, value] of Object.entries(node as Record<string, unknown>)) {
    out.push(...leaves(value, prefix === '' ? key : `${prefix}.${key}`));
  }
  return out;
}

/**
 * The complete set of variables the assistant is allowed to touch. Anything not
 * in `VARIABLE_LABELS` is invisible to it, which is what keeps the request
 * bounded and reviewable.
 */
export function collectUpdatableVariables(pack: RulePack): UpdatableVariable[] {
  const variables: UpdatableVariable[] = [];
  for (const leaf of leaves(pack.constants)) {
    const meta = VARIABLE_LABELS[leaf.path];
    if (meta === undefined) continue;
    variables.push({
      path: leaf.path,
      label: meta.label,
      unit: meta.unit,
      currentValue: leaf.value,
      sourceIds: sourceIdsFor(pack, leaf.path),
      ...(meta.min === undefined ? {} : { min: meta.min }),
      ...(meta.max === undefined ? {} : { max: meta.max }),
    });
  }
  return variables.sort((a, b) => (a.path < b.path ? -1 : 1));
}

function sourceIdsFor(pack: RulePack, path: string): string[] {
  const section = path.split('.')[0];
  switch (section) {
    case 'iva':
      return pack.constants.iva.art53.sourceIds;
    case 'irs':
      return pack.constants.irs.simplifiedRegime.sourceIds;
    case 'socialSecurity':
      return pack.constants.socialSecurity.sourceIds;
    default:
      return [];
  }
}

export interface UpdateRequest {
  year: number;
  jurisdiction: string;
  variables: UpdatableVariable[];
  sources: Array<{ id: string; authority: string; url: string }>;
}

export function buildUpdateRequest(pack: RulePack): UpdateRequest {
  return {
    year: pack.year,
    jurisdiction: pack.jurisdiction,
    variables: collectUpdatableVariables(pack),
    sources: pack.sources.map((source) => ({
      id: source.id,
      authority: source.authority,
      url: source.url,
    })),
  };
}

export const UPDATE_SYSTEM_PROMPT = [
  'És uma função de extração de dados, não um assistente. Não conversas, não explicas e não dás conselhos.',
  '',
  'A tua única tarefa: para cada variável da lista, indicar o valor em vigor no ano pedido, em Portugal.',
  '',
  'Regras obrigatórias:',
  '1. Responde APENAS com um objeto JSON. Sem texto à volta, sem blocos de código, sem comentários.',
  '2. Usa exatamente as unidades indicadas: dinheiro em cêntimos de euro (inteiro); percentagens e',
  '   coeficientes em pontos base, em que 1% = 100 (23% = 2300; coeficiente 0,75 = 7500); múltiplos do',
  '   IAS em centésimos (1,5 × IAS = 150).',
  '3. Se não conseguires confirmar um valor numa fonte oficial, devolve null nesse valor. Nunca estimes,',
  '   nunca interpolas e nunca repetes o valor atual por defeito.',
  '4. Indica em sourceUrl o endereço exato onde leste o valor. Se não leste nenhum endereço, devolve',
  '   null no valor e null no sourceUrl.',
  '5. Não acrescentes variáveis que não te foram pedidas. Não omitas nenhuma das pedidas.',
  '6. Não dês opinião sobre se o valor é justo, provável ou se vai mudar no futuro.',
  '',
  'Formato exato da resposta:',
  '{"asOf":"AAAA-MM-DD","variables":[{"path":"<caminho pedido>","value":<inteiro ou null>,',
  '"sourceUrl":"<url ou null>","note":"<até 120 caracteres ou null>"}]}',
].join('\n');

export function buildUpdateMessages(request: UpdateRequest): ChatMessage[] {
  const lines = request.variables.map(
    (variable) =>
      `- path: ${variable.path}\n  descrição: ${variable.label}\n  unidade: ${variable.unit}\n` +
      `  valor atual no pacote: ${variable.currentValue === null ? 'null' : variable.currentValue}`,
  );
  return [
    { role: 'system', content: UPDATE_SYSTEM_PROMPT },
    {
      role: 'user',
      content: [
        `Jurisdição: ${request.jurisdiction}`,
        `Ano a que os valores devem respeitar: ${request.year}`,
        '',
        'Variáveis a confirmar:',
        ...lines,
        '',
        'Fontes oficiais já usadas pelo pacote (prefere estas):',
        ...request.sources.map((source) => `- ${source.authority}: ${source.url}`),
      ].join('\n'),
    },
  ];
}

export interface ProposedValue {
  path: string;
  value: number | null;
  unit: VariableUnit;
  label: string;
  currentValue: number | null;
  sourceUrl: string | null;
  note: string | null;
}

export interface UpdateProposal {
  year: number;
  asOf: string;
  model: string;
  proposedAt: string;
  values: ProposedValue[];
  problems: string[];
}

/** Plausibility ceilings, so a nonsensical answer is rejected rather than stored. */
const UNIT_BOUNDS: Record<VariableUnit, { min: number; max: number; integer: boolean }> = {
  EUR_cents: { min: 0, max: 10_000_000_000, integer: true },
  basis_points: { min: 0, max: 10_000, integer: true },
  hundredths_of_ias: { min: 0, max: 100_000, integer: true },
  months: { min: 0, max: 120, integer: true },
};

/**
 * Parse the model's answer into validated data. Every deviation is reported; a
 * partially usable answer is kept, but nothing is silently accepted.
 */
export function parseUpdateResponse(
  text: string,
  request: UpdateRequest,
): { values: ProposedValue[]; problems: string[]; asOf: string } {
  const problems: string[] = [];
  const stripped = text
    .trim()
    .replace(/^```(?:json)?/i, '')
    .replace(/```$/, '')
    .trim();

  let parsed: unknown;
  try {
    parsed = JSON.parse(stripped);
  } catch {
    // A fenced or prose-wrapped answer is a failure of the contract, not a
    // partial success: refuse it whole rather than scavenging for JSON.
    return { values: [], problems: ['A resposta não é JSON válido. Nada foi aceite.'], asOf: '' };
  }

  const record = parsed as Record<string, unknown>;
  const asOf = typeof record['asOf'] === 'string' ? record['asOf'] : '';
  const rawValues = record['variables'];
  if (!Array.isArray(rawValues)) {
    return {
      values: [],
      problems: ['A resposta não contém a lista "variables". Nada foi aceite.'],
      asOf,
    };
  }

  const byPath = new Map(request.variables.map((variable) => [variable.path, variable]));
  const values: ProposedValue[] = [];
  const seen = new Set<string>();

  for (const [index, entry] of rawValues.entries()) {
    const at = `variables[${index}]`;
    if (entry === null || typeof entry !== 'object') {
      problems.push(`${at}: não é um objeto.`);
      continue;
    }
    const item = entry as Record<string, unknown>;
    const path = typeof item['path'] === 'string' ? item['path'] : '';
    const expected = byPath.get(path);
    if (expected === undefined) {
      problems.push(`${at}: caminho "${path}" não foi pedido; ignorado.`);
      continue;
    }
    if (seen.has(path)) {
      problems.push(`${at}: caminho "${path}" repetido; mantida a primeira ocorrência.`);
      continue;
    }
    seen.add(path);

    const sourceUrl = typeof item['sourceUrl'] === 'string' ? item['sourceUrl'] : null;
    if (sourceUrl !== null && !/^https?:\/\//.test(sourceUrl)) {
      problems.push(`${at}: sourceUrl não é http(s); tratado como ausente.`);
    }
    const cleanUrl = sourceUrl !== null && /^https?:\/\//.test(sourceUrl) ? sourceUrl : null;

    const note = typeof item['note'] === 'string' ? item['note'].slice(0, 200) : null;
    let value: number | null = null;

    if (item['value'] === null || item['value'] === undefined) {
      problems.push(`${path}: sem valor proposto (null).`);
    } else if (typeof item['value'] !== 'number' || !Number.isFinite(item['value'])) {
      problems.push(`${path}: valor não numérico; ignorado.`);
    } else {
      const bounds = UNIT_BOUNDS[expected.unit];
      const min = expected.min ?? bounds.min;
      const max = expected.max ?? bounds.max;
      const candidate = item['value'];
      if (bounds.integer && !Number.isInteger(candidate)) {
        problems.push(
          `${path}: ${candidate} não é inteiro na unidade ${expected.unit}; ignorado. ` +
            'A unidade é parte do contrato, não uma sugestão.',
        );
      } else if (candidate < min || candidate > max) {
        problems.push(
          `${path}: ${candidate} está fora dos limites plausíveis (${min}–${max}) para ` +
            `${expected.unit}; ignorado.`,
        );
      } else {
        value = candidate;
      }
    }

    values.push({
      path,
      value,
      unit: expected.unit,
      label: expected.label,
      currentValue: expected.currentValue,
      sourceUrl: cleanUrl,
      note,
    });
  }

  for (const variable of request.variables) {
    if (!seen.has(variable.path)) {
      problems.push(`${variable.path}: o assistente não respondeu a esta variável.`);
    }
  }

  if (asOf === '') problems.push('A resposta não indica a data de referência (asOf).');
  return { values, problems, asOf };
}

export interface ValueDiff {
  path: string;
  label: string;
  unit: VariableUnit;
  current: number | null;
  proposed: number | null;
  changed: boolean;
  sourceUrl: string | null;
  note: string | null;
}

export function diffProposal(proposal: UpdateProposal): ValueDiff[] {
  return proposal.values.map((value) => ({
    path: value.path,
    label: value.label,
    unit: value.unit,
    current: value.currentValue,
    proposed: value.value,
    changed: value.value !== null && value.value !== value.currentValue,
    sourceUrl: value.sourceUrl,
    note: value.note,
  }));
}

function setByPath(root: Record<string, unknown>, path: string, value: number): boolean {
  const parts = path.split('.');
  let node: Record<string, unknown> = root;
  for (const part of parts.slice(0, -1)) {
    const next = node[part];
    if (next === null || typeof next !== 'object') return false;
    node = next as Record<string, unknown>;
  }
  const last = parts[parts.length - 1];
  if (last === undefined || !(last in node)) return false;
  node[last] = value;
  return true;
}

function bumpPatch(version: string): string {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (match?.[1] === undefined || match[2] === undefined || match[3] === undefined) {
    return `${version}+local`;
  }
  return `${match[1]}.${match[2]}.${Number(match[3]) + 1}`;
}

/**
 * Apply a reviewed proposal. Never called automatically: the command requires an
 * explicit `--approve`, and the resulting values are marked `unverified` unless
 * the human also asserts `--verified`.
 */
export function applyProposal(
  pack: RulePack,
  proposal: UpdateProposal,
  options: { at: string; model: string; verifiedByHuman: boolean },
): { pack: RulePack; applied: number } {
  const draft = JSON.parse(JSON.stringify(pack)) as RulePack;
  draft.constantProvenance = { ...(draft.constantProvenance ?? {}) };
  let applied = 0;

  for (const value of proposal.values) {
    if (value.value === null) continue;
    if (!setByPath(draft.constants as unknown as Record<string, unknown>, value.path, value.value)) {
      continue;
    }
    draft.constantProvenance[value.path] = {
      source: 'ai-proposed',
      proposedAt: options.at,
      sourceUrl: value.sourceUrl,
      status: options.verifiedByHuman ? 'verified' : 'unverified',
      ...(value.note === null ? {} : { note: value.note }),
    };
    applied += 1;
  }

  draft.packVersion = bumpPatch(pack.packVersion);
  draft.generatedAt = options.at;
  return { pack: draft, applied };
}

/** The pack's variable list, as plain text, for the dry-run preview. */
export function formatVariables(variables: readonly UpdatableVariable[]): string {
  return variables
    .map(
      (variable) =>
        `  ${variable.path.padEnd(62)} ${String(variable.currentValue ?? '—').padStart(10)}  ${variable.unit}`,
    )
    .join('\n');
}
