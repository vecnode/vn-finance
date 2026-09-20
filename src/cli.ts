#!/usr/bin/env node
/**
 * vnfin — assistente fiscal local para atividade independente em Portugal.
 *
 *   node src/cli.ts <comando> [opções]
 *
 * Design notes that are visible from the outside:
 *   - The default data directory is a folder in your home, not a database
 *     somewhere in the cloud. `--data-dir` moves it, and the vault refuses to
 *     live inside a git work tree, so personal data cannot be committed.
 *   - There is no conversational assistant and no connection to any public
 *     body. `update` is the only command that touches the network, it sends
 *     public-law variable names and nothing else, and it changes nothing until
 *     you approve it.
 *   - Alerts are computed in code, deterministically.
 *   - Output is plain text with optional colour; `--json` is machine readable.
 */

import { writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { parseArgs } from 'node:util';

import { buildAgenda, upcoming } from './core/calendar.ts';
import { formatPtDate, parseIso, todayInLisbon } from './core/dates.ts';
import {
  formatBpAsCoefficient,
  formatBpAsPercent,
  formatEur,
  parseEurToCents,
  type Cents,
} from './core/money.ts';
import {
  createDefaultProfile,
  missingProfileInputs,
  validateProfile,
} from './core/profile.ts';
import {
  defaultPackPath,
  freshness,
  loadRulePack,
  resolvePackPath as resolvePackPathCore,
  summarisePack,
} from './core/rules.ts';
import { estimateIrsSimplifiedBase, defaultInvoiceTerms, estimateReserve, invoiceTotals, quarterReport, type Invoice } from './core/estimate.ts';
import { maskNif } from './core/nif.ts';
import { startWebServer } from './web/server.ts';
import {
  computeFlags,
  packHealthFlags,
  summariseFlags,
  type FlagSeverity,
} from './core/flags.ts';
import { DEFAULT_MODEL, DeepSeekClient } from './ai/deepseek.ts';
import { maskKey, resolveApiKey, saveApiKey } from './ai/keyring.ts';
import { redactForSend } from './ai/redact.ts';
import {
  applyProposal,
  buildUpdateMessages,
  buildUpdateRequest,
  diffProposal,
  formatVariables,
  parseUpdateResponse,
  type UpdateProposal,
} from './ai/update.ts';
import type { ObligationInstance } from './core/types.ts';
import { Vault, checkDataDirRisk, resolveDataDir } from './store/vault.ts';

const VERSION = '0.1.0';

// ---------------------------------------------------------------------------
// Terminal helpers
// ---------------------------------------------------------------------------

const useColour = process.stdout.isTTY === true && process.env['NO_COLOR'] === undefined;

function paint(text: string, code: string): string {
  return useColour ? `\u001b[${code}m${text}\u001b[0m` : text;
}
const bold = (text: string): string => paint(text, '1');
const dim = (text: string): string => paint(text, '2');
const red = (text: string): string => paint(text, '31');
const green = (text: string): string => paint(text, '32');
const yellow = (text: string): string => paint(text, '33');
const cyan = (text: string): string => paint(text, '36');

function pad(text: string, width: number): string {
  const visible = text.replace(/\u001b\[\d+m/g, '');
  return text + ' '.repeat(Math.max(0, width - visible.length));
}

function out(line = ''): void {
  process.stdout.write(`${line}\n`);
}

function fail(message: string): never {
  process.stderr.write(`${red('erro')} ${message}\n`);
  process.exit(1);
}

// ---------------------------------------------------------------------------
// Argument access
// ---------------------------------------------------------------------------

type Values = Record<string, string | boolean | Array<string | boolean> | undefined>;

function str(values: Values, name: string): string | undefined {
  const value = values[name];
  if (value === undefined) return undefined;
  if (Array.isArray(value)) return value[value.length - 1]?.toString();
  return value.toString();
}

function bool(values: Values, name: string): boolean {
  const value = values[name];
  if (value === undefined) return false;
  if (Array.isArray(value)) return value.some((item) => item === true || item === 'true');
  return value === true || value === 'true';
}

function num(values: Values, name: string): number | undefined {
  const raw = str(values, name);
  if (raw === undefined) return undefined;
  const parsed = Number(raw);
  if (!Number.isFinite(parsed)) fail(`--${name} tem de ser um número (recebido "${raw}")`);
  return parsed;
}

/** Parse a euro amount given as a command option ("1 200,50") into cents. */
function parseMoneyOption(values: Values, name: string): number | undefined {
  const raw = str(values, name);
  if (raw === undefined) return undefined;
  try {
    return parseEurToCents(raw);
  } catch {
    fail(`--${name} não é um valor em euros válido: "${raw}"`);
  }
}

// ---------------------------------------------------------------------------
// Shared context
// ---------------------------------------------------------------------------

interface Context {
  vault: Vault;
  year: number;
  today: string;
  json: boolean;
}

function resolvePackPath(explicit: string | undefined, requestedYear: number): { path: string; year: number } {
  const resolved = resolvePackPathCore(requestedYear, explicit);
  if (resolved === null) {
    fail(`não há pacote de regras para ${requestedYear} em ${dirname(defaultPackPath('pt', requestedYear))}.`);
  }
  if (!resolved.exact) {
    out(
      yellow(`aviso: não existe pacote de regras para ${requestedYear}; a usar o de ${resolved.year}.`),
    );
  }
  return { path: resolved.path, year: resolved.year };
}

function statusLabel(instance: ObligationInstance): string {
  switch (instance.status) {
    case 'overdue':
      return red('EM ATRASO');
    case 'due_soon':
      return yellow('PRÓXIMO  ');
    case 'done':
      return green('CONCLUÍDO');
    case 'untracked':
      return dim('histórico');
    case 'not_applicable':
      return dim('N/A      ');
    case 'future':
      return dim('futuro   ');
  }
}

const KIND_LABELS: Record<ObligationInstance['kind'], string> = {
  declare: 'declarar',
  pay: 'pagar',
  communicate: 'comunicar',
  save: 'guardar',
  review: 'rever',
  submit: 'entregar',
};

/** Column widths for the agenda table, wide enough that no two columns touch. */
const COLUMNS = { date: 12, kind: 11, title: 54, status: 10 } as const;

function severityLabel(severity: FlagSeverity): string {
  switch (severity) {
    case 'urgent':
      return red('URGENTE  ');
    case 'attention':
      return yellow('ATENÇÃO  ');
    case 'info':
      return cyan('INFO     ');
  }
}

function truncate(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max - 1)}…`;
}

function markers(instance: ObligationInstance): string {
  const flags: string[] = [];
  if (instance.verification !== 'verified') flags.push(yellow('?'));
  if (instance.discrepancies.length > 0) flags.push(yellow('!'));
  if (instance.provisional) flags.push(dim('~'));
  return flags.join('');
}

function renderAgenda(instances: readonly ObligationInstance[]): void {
  out(
    dim(
      pad('DATA', COLUMNS.date) +
        pad('TIPO', COLUMNS.kind) +
        pad('OBRIGAÇÃO', COLUMNS.title) +
        pad('ESTADO', COLUMNS.status) +
        'FONTE',
    ),
  );
  for (const instance of instances) {
    if (instance.status === 'not_applicable') {
      out(
        pad(dim('—'), COLUMNS.date) +
          pad(dim(KIND_LABELS[instance.kind]), COLUMNS.kind) +
          pad(dim(truncate(instance.title, COLUMNS.title - 1)), COLUMNS.title) +
          pad(statusLabel(instance), COLUMNS.status) +
          dim(instance.notApplicableReason ?? ''),
      );
      continue;
    }
    const source =
      instance.dueDateSource === 'official'
        ? green('oficial')
        : instance.provisional
          ? yellow('provisória')
          : dim('calculada');
    out(
      pad(formatPtDate(instance.dueDate), COLUMNS.date) +
        pad(KIND_LABELS[instance.kind], COLUMNS.kind) +
        pad(truncate(instance.title, COLUMNS.title - 1), COLUMNS.title) +
        pad(statusLabel(instance), COLUMNS.status) +
        source +
        (markers(instance) === '' ? '' : ` ${markers(instance)}`),
    );
  }
  out();
  out(dim('  ? regra não verificada   ! divergência a confirmar   ~ data provisória (ano ainda não publicado)'));
  if (instances.some((instance) => instance.notApplicableReason !== null)) {
    out(dim('  N/A obrigação que não se aplica a este perfil (motivo indicado na linha)'));
  }
  if (instances.some((instance) => instance.status === 'untracked')) {
    out(dim('  histórico obrigação anterior ao início do acompanhamento: não é tratada como atraso'));
  }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

function commandInit(values: Values, context: Context): void {
  const nif = str(values, 'nif');
  const name = str(values, 'name');
  const declaredIva = str(values, 'iva');
  if (nif === undefined || name === undefined) {
    fail(
      'init exige --nif e --name. Exemplo:\n' +
        '  vnfin init --nif 123456789 --name "Nome Completo" --iva isento_art53 --turnover-ano-anterior 12400',
    );
  }

  // The IVA regime is an INPUT, never a default. It is a declaration the user
  // made to the AT, and guessing it would silently change every downstream
  // calculation — the art. 53.º watch, the periodic obligations, the invoices.
  const iva = declaredIva;
  if (iva === undefined) {
    fail(
      'init exige o regime de IVA declarado: --iva isento_art53 | trimestral | mensal.\n' +
        '  É o enquadramento da tua declaração de início de atividade; a aplicação não o adivinha.\n' +
        '  Se não tiveres a certeza, vê o regime no Portal das Finanças e volta a correr o init.',
    );
  }
  if (iva !== 'isento_art53' && iva !== 'trimestral' && iva !== 'mensal') {
    fail(`--iva "${iva}" não é válido. Usa isento_art53, trimestral ou mensal.`);
  }

  const { pack } = loadRulePack(resolvePackPath(str(values, 'rules'), context.year).path);
  const profile = createDefaultProfile({
    nif,
    name,
    ivaRegime: iva,
    irsRegime: (str(values, 'irs') as 'simplificado' | 'organizada' | undefined) ?? 'simplificado',
    coefficientBp: pack.constants.irs.simplifiedRegime.coefficients.servicesProfessionalTable4 ?? 7500,
    startDate: str(values, 'start-date'),
    // Obligations due before the first day of use are history, not misses.
    trackingStart: context.today,
    turnoverPreviousYearCents: parseMoneyOption(values, 'turnover-ano-anterior'),
    turnoverCurrentYearExpectedCents: parseMoneyOption(values, 'turnover-ano-corrente'),
    exports: bool(values, 'exports') || bool(values, 'clientes-fora-ue'),
    intraCommunityOperations: bool(values, 'intracomunitarias') || bool(values, 'clientes-ue'),
    startupExemptionActive: bool(values, 'ss-isencao-inicio'),
  });

  if (context.vault.loadProfile() !== null && !bool(values, 'force')) {
    fail(`já existe um perfil em ${context.vault.path('profile.json')}. Usa --force para o substituir.`);
  }

  context.vault.ensure();
  context.vault.saveProfile(profile);
  context.vault.appendAudit({ action: 'profile.created' }, new Date().toISOString());

  out(`${green('perfil gravado')} em ${context.vault.path('profile.json')}`);
  out();
  for (const problem of validateProfile(profile)) {
    const tag = problem.level === 'error' ? red('erro ') : yellow('aviso');
    out(`  ${tag} ${bold(problem.field)}: ${problem.message}`);
  }
  out();
  out(`Próximo passo: ${cyan('vnfin doctor')}, ${cyan('vnfin agenda')} e ${cyan('vnfin flags')}`);
}

function commandDoctor(values: Values, context: Context): void {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const loaded = loadRulePack(packPath.path);
  const summary = summarisePack(loaded);
  const fresh = freshness(loaded.pack, context.today);

  let problems = 0;
  const ok = (label: string, detail: string): void => {
    out(`  ${green('ok  ')} ${pad(label, 26)} ${detail}`);
  };
  const warn = (label: string, detail: string): void => {
    problems += 1;
    out(`  ${yellow('aviso')} ${pad(label, 26)} ${detail}`);
  };
  const bad = (label: string, detail: string): void => {
    problems += 1;
    out(`  ${red('erro ')} ${pad(label, 26)} ${detail}`);
  };

  out(bold('Diagnóstico'));
  out();
  out(`  ${dim('node')}      ${process.version}`);
  out(`  ${dim('cofre')}     ${context.vault.dir}`);
  out(`  ${dim('hoje')}      ${context.today}`);
  out();

  const profile = context.vault.loadProfile();
  if (profile === null) {
    warn('perfil', 'não existe perfil. Corre `vnfin init`.');
  } else {
    const profileProblems = validateProfile(profile);
    const errors = profileProblems.filter((problem) => problem.level === 'error');
    if (errors.length === 0) {
      ok('perfil', `${profile.name} · NIF ${maskNif(profile.nif)} · IVA ${profile.iva.regime}`);
    } else {
      for (const problem of errors) bad(`perfil.${problem.field}`, problem.message);
    }
    for (const problem of profileProblems.filter((item) => item.level === 'warning')) {
      warn(`perfil.${problem.field}`, problem.message);
    }

    // Inputs only the user can supply. Named explicitly rather than left as a
    // validation warning, because "the app cannot check this without your number"
    // is different from "you typed something wrong". Shared with the local panel.
    const missingInputs = missingProfileInputs(profile);
    if (missingInputs.length > 0) {
      out();
      out(bold('Dados que só tu podes fornecer'));
      for (const input of missingInputs) out(`  ${yellow('•')} ${input}`);
      out(dim('  A aplicação não adivinha estes valores: sem eles há verificações que simplesmente não correm.'));
    }
  }

  ok(
    'pacote de regras',
    `pt/${summary.packVersion} · ${summary.obligations} obrigações · ${summary.sources} fontes ` +
      `(${summary.officialSources} oficiais)`,
  );
  out(`       ${dim('checksum')}  ${summary.checksum.slice(0, 16)}…`);
  if (fresh.status === 'current') ok('atualidade', fresh.message);
  else warn('atualidade', fresh.message);

  const notFullyVerified = summary.partial + summary.unverified + summary.stale;
  if (notFullyVerified > 0) {
    warn(
      'verificação',
      `${summary.verified} regras verificadas, ${summary.partial} parcialmente verificadas, ` +
        `${summary.unverified} por verificar, ${summary.stale} desatualizadas. ` +
        'As regras não totalmente verificadas são assinaladas com "?" na agenda.',
    );
  } else {
    ok('verificação', `${summary.verified} regras verificadas contra fonte oficial.`);
  }

  if (summary.eventDriven > 0) {
    out(
      `  ${dim('info ')} ${pad('sem prazo fixo', 26)} ${summary.eventDriven} obrigação(ões) dependentes de evento ` +
        'não entram na agenda por não terem data (ex.: declaração de alterações).',
    );
  }

  if (summary.nullConstants.length > 0) {
    warn(
      'constantes em falta',
      `${summary.nullConstants.length} valores sem fonte (ex.: ${summary.nullConstants.slice(0, 3).join(', ')}). ` +
        'Os cálculos que dependam deles são recusados em vez de estimados.',
    );
  }
  for (const problem of loaded.problems.filter((item) => item.level === 'warning').slice(0, 8)) {
    warn(`regras.${problem.path}`, problem.message);
  }

  // Values the assistant proposed and no human has confirmed yet, plus the
  // pack's own staleness. These are the pack's health, not the user's affairs.
  for (const flag of packHealthFlags(loaded.pack, context.today)) {
    const report = flag.severity === 'urgent' ? bad : warn;
    report(`regras.${flag.code.toLowerCase()}`, `${flag.title} — ${flag.detail.split('\n')[0] ?? ''}`);
  }

  const risk = checkDataDirRisk(context.vault.dir);
  if (risk.level === 'warning' && risk.message !== null) warn('cofre em git', risk.message);

  const key = resolveApiKey({ dataDir: context.vault.dir, explicit: str(values, 'api-key') });
  if (key.key === null) {
    out(`  ${dim('info ')} ${pad('chave DeepSeek', 26)} ausente. O assistente fica indisponível; todo o resto funciona.`);
  } else {
    ok('chave DeepSeek', `${maskKey(key.key)} (origem: ${key.source})`);
  }
  for (const problem of key.problems) warn('chave DeepSeek', problem);

  const stats = context.vault.describe();
  ok('cofre', `${stats.files} ficheiros, ${(stats.bytes / 1024).toFixed(1)} KiB`);

  if (summary.todo.length > 0) {
    out();
    out(bold('Por verificar no pacote de regras'));
    for (const item of summary.todo) out(`  ${yellow('•')} ${item}`);
  }

  out();
  if (problems === 0) out(green('Sem problemas detetados.'));
  else out(yellow(`${problems} ponto(s) a resolver.`));
  if (summary.unverified > 0) {
    out(
      dim(
        'As regras não verificadas não são um erro da aplicação: são sítios onde a lei tem de ser\n' +
          'confirmada na fonte citada antes de confiar no prazo. O comando `vnfin rules` mostra as fontes.',
      ),
    );
  }
}

function commandRules(values: Values, context: Context): void {
  const loaded = loadRulePack(resolvePackPath(str(values, 'rules'), context.year).path);
  const summary = summarisePack(loaded);
  if (context.json) {
    out(JSON.stringify({ summary, sources: loaded.pack.sources, todo: loaded.pack.todo }, null, 2));
    return;
  }
  out(bold(`Pacote de regras pt/${summary.packVersion}`));
  out(dim(`  checksum ${summary.checksum}`));
  out(
    dim(
      `  ${summary.obligations} obrigações · ${summary.verified} verificadas · ` +
        `${summary.partial} parcialmente verificadas · ${summary.unverified} por verificar`,
    ),
  );
  out();
  out(bold('Fontes'));
  for (const source of loaded.pack.sources) {
    out(`  ${source.tier === 'official' ? green('oficial  ') : yellow('secundária')} ${pad(source.id, 28)} ${source.url}`);
  }
  out();
  out(bold('Obrigações'));
  for (const rule of loaded.pack.obligations) {
    const flag = rule.verification === 'verified' ? green('ok ') : yellow('?  ');
    out(
      `  ${flag} ${pad(rule.id, 30)} ${pad(rule.kind, 11)} ${pad(rule.tax, 6)} ${rule.title}` +
        (rule.legalBasis.length > 0 ? dim(`  [${rule.legalBasis.join('; ')}]`) : ''),
    );
  }
}

function commandAgenda(values: Values, context: Context): void {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const { pack } = loadRulePack(packPath.path);
  const profile = context.vault.loadProfile();
  if (profile === null) fail('não existe perfil. Corre `vnfin init` primeiro.');

  const instances = buildAgenda(pack, profile, {
    year: context.year,
    today: context.today,
    dueSoonDays: num(values, 'due-soon') ?? 30,
    completedIds: context.vault.completedIds(),
    ...(profile.trackingStart === undefined ? {} : { trackFrom: profile.trackingStart }),
  });

  if (context.json) {
    out(JSON.stringify(instances, null, 2));
    return;
  }

  const horizon = num(values, 'horizon') ?? 90;
  const window = upcoming(instances, context.today, horizon);

  out(bold(`Agenda fiscal ${context.year}`) + dim(`  ·  hoje ${formatPtDate(context.today)}`));
  out(
    dim(
      `  ${profile.name} · NIF ${maskNif(profile.nif)} · categoria B · IVA ${profile.iva.regime} · ` +
        `IRS ${profile.irs.regime} · regras pt/${pack.packVersion}`,
    ),
  );
  out();

  // A one-line alert summary here, with the detail in `vnfin flags`: the agenda
  // answers "what is due", the alerts answer "what is wrong".
  const flagSummary = summariseFlags(
    computeFlags({
      pack,
      profile,
      invoices: context.vault.loadInvoices(),
      instances,
      today: context.today,
      documentedObligationIds: context.vault.documentedObligationIds(),
      dueSoonDays: num(values, 'due-soon') ?? 30,
    }),
  );
  if (flagSummary.total > 0) {
    out(
      (flagSummary.urgent > 0 ? red(`${flagSummary.urgent} urgente(s)`) : dim('0 urgentes')) +
        dim(' · ') +
        (flagSummary.attention > 0 ? yellow(`${flagSummary.attention} atenção`) : dim('0 atenção')) +
        dim(' · ') +
        dim(`${flagSummary.info} informação`) +
        dim('   — detalhe em `vnfin flags`'),
    );
    out();
  }

  if (window.overdue.length > 0) {
    out(red(bold(`${window.overdue.length} obrigação(ões) em atraso`)));
    for (const instance of window.overdue.slice(0, 10)) {
      out(`  ${red('•')} ${formatPtDate(instance.dueDate)}  ${instance.title}`);
    }
    if (window.overdue.length > 10) {
      out(dim(`  … e mais ${window.overdue.length - 10}.`));
    }
    out();
  }
  out(bold(`Próximos ${horizon} dias`));
  if (window.actionable.length === 0) {
    out(dim('  nada a fazer neste horizonte.'));
  } else {
    for (const instance of window.actionable) {
      const days = Math.round(
        (Date.parse(`${instance.dueDate}T00:00:00Z`) - Date.parse(`${context.today}T00:00:00Z`)) / 86_400_000,
      );
      const when = days === 0 ? red('hoje') : days === 1 ? yellow('amanhã') : dim(`em ${days} dias`);
      out(
        `  ${pad(formatPtDate(instance.dueDate), 12)} ${pad(when, 20)} ` +
          `${pad(truncate(instance.title, 44), 46)} ${cyan(KIND_LABELS[instance.kind])}`,
      );
      if (instance.discrepancies.length > 0) {
        for (const note of instance.discrepancies) out(`      ${yellow('!')} ${dim(note)}`);
      }
      if (instance.verification !== 'verified') {
        out(`      ${yellow('?')} ${dim(`regra não verificada${instance.verifyNote === null ? '' : `: ${instance.verifyNote}`}`)}`);
      }
      if (instance.legalBasis.length > 0) out(`      ${dim(instance.legalBasis.join(' · '))}`);
      if (instance.portalUrl !== null) out(`      ${dim(instance.portalUrl)}`);
    }
  }
  out();

  const showAll = bool(values, 'all');
  out(bold(showAll ? 'Todo o ano' : 'Restante ano'));
  renderAgenda(
    instances.filter((instance) => {
      if (showAll) return true;
      return (
        instance.status !== 'done' &&
        instance.status !== 'not_applicable' &&
        instance.status !== 'untracked'
      );
    }),
  );

  const untracked = instances.filter((instance) => instance.status === 'untracked').length;
  if (untracked > 0 && !showAll) {
    out(
      dim(
        `  ${untracked} obrigação(ões) anteriores a ${profile.trackingStart ?? '—'} ficam como histórico: ` +
          'não são acompanhadas nem contam como atraso. Usa --all para as ver.',
      ),
    );
  }
}

function commandEstimate(values: Values, context: Context): void {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const { pack } = loadRulePack(packPath.path);
  const invoices = context.vault.loadInvoices();
  const asOf = str(values, 'as-of') ?? context.today;

  const reserve = estimateReserve(pack, invoices, asOf);
  const requestedQuarter = num(values, 'quarter');
  if (requestedQuarter !== undefined && (requestedQuarter < 1 || requestedQuarter > 4)) {
    fail('--quarter tem de estar entre 1 e 4');
  }
  const quarters = (requestedQuarter === undefined ? [1, 2, 3, 4] : [requestedQuarter]).map((quarter) =>
    quarterReport(pack, invoices, context.year, quarter),
  );

  if (context.json) {
    out(JSON.stringify({ year: context.year, asOf, quarters, reserve }, null, 2));
    return;
  }

  out(bold(`Estimativas ${context.year}`) + dim(`  ·  apurado a ${formatPtDate(asOf)}`));
  out();
  out(bold('Por trimestre'));
  out(
    dim(
      pad('TRIM.', 8) + pad('FATURADO', 16) + pad('IVA LIQUIDADO', 16) + pad('RETENÇÃO', 16) + 'SEG. SOCIAL',
    ),
  );
  for (const report of quarters) {
    const ss = report.socialSecurity;
    out(
      pad(`${report.quarter}.º`, 8) +
        pad(formatEur(report.totals.baseCents), 16) +
        pad(formatEur(report.totals.ivaLiquidadoCents), 16) +
        pad(formatEur(report.totals.retencaoSofridaCents), 16) +
        formatEur(ss.contributionCents),
    );
    if (ss.contributionCents > 0) {
      out(
        dim(
          `        ${formatEur(ss.relevantIncomeCents)} de rendimento relevante ` +
            `(${formatBpAsPercent(ss.relevantIncomeShareBp)} de ${formatEur(ss.serviceIncomeCents)}) ` +
            `× ${formatBpAsPercent(ss.contributionRateBp)} = ${formatEur(ss.contributionCents)} ` +
            `= ${ss.instalmentsCents.map((value) => formatEur(value)).join(' + ')}`,
        ),
      );
    }
    for (const limitation of ss.limitations) out(`        ${yellow('!')} ${dim(limitation)}`);
  }
  out();
  out(bold('Reserva recomendada para o próximo ano'));
  out(`  ${formatEur(reserve.lowCents)} a ${formatEur(reserve.highCents)}  ${dim(`(${reserve.basis.join('; ')})`)}`);
  for (const limitation of reserve.limitations) out(`  ${yellow('!')} ${dim(limitation)}`);

  // The simplified-regime base. This is the one estimate whose input the ledger
  // cannot supply — eligibility of expenses is a judgement — so it is asked for
  // explicitly rather than assumed to be zero, which would silently be the worst
  // case. The chain is printed so the number can be argued with.
  out();
  out(bold('IRS — rendimento tributável do regime simplificado'));
  const profile = context.vault.loadProfile();
  if (profile === null) {
    out(dim('  sem perfil não há coeficiente a aplicar. Corre `vnfin init`.'));
    return;
  }

  const documented = parseMoneyOption(values, 'despesas');
  if (documented === undefined) {
    out(`  coeficiente aplicado: ${bold(formatBpAsCoefficient(profile.irs.coefficientBp))}`);
    out(
      `  ${yellow('•')} Indica as despesas elegíveis para ver o rendimento tributável: ` +
        cyan('vnfin estimate --despesas 5000'),
    );
    out(dim('  O art. 31.º n.º 13 do CIRS acresce ao rendimento a diferença entre 15% dos'));
    out(dim('  rendimentos de serviços e as despesas que conseguires documentar.'));
    return;
  }

  const irs = estimateIrsSimplifiedBase(pack, profile, invoices, context.year, documented);
  const chain = (label: string, value: number | null, note = ''): void => {
    out(`  ${pad(label, 34)} ${pad(value === null ? '—' : formatEur(value), 16)}${dim(note)}`);
  };
  chain('rendimento de serviços', irs.grossServiceIncomeCents, 'faturado no ano');
  chain(`× coeficiente ${formatBpAsCoefficient(irs.coefficientBp)}`, irs.taxableFromCoefficientCents);
  chain('referência de despesas', irs.documentedExpensesReferenceCents, '15% do rendimento de serviços');
  chain('despesas elegíveis documentadas', irs.eligibleExpensesCents, 'indicadas por ti');
  chain(
    'acréscimo ao rendimento tributável',
    irs.additionToTaxableIncomeCents,
    'diferença positiva entre a referência e o documentado',
  );
  out(`  ${pad('rendimento tributável', 34)} ${bold(pad(irs.taxableIncomeCents === null ? '—' : formatEur(irs.taxableIncomeCents), 16))}`);
  out();
  out(
    dim(
      `  retenções já sofridas no ano: ${formatEur(irs.withholdingCents)} — são creditadas no IRS final, ` +
        'não descontadas aqui.',
    ),
  );
  for (const limitation of irs.limitations) out(`  ${yellow('!')} ${dim(limitation)}`);
  for (const assumption of irs.assumptions) out(`  ${dim(`· ${assumption}`)}`);
}

function commandLedger(values: Values, context: Context): void {
  const action = str(values, 'action') ?? 'list';
  if (action === 'list') {
    const invoices = context.vault.loadInvoices();
    if (context.json) {
      out(JSON.stringify(invoices, null, 2));
      return;
    }
    if (invoices.length === 0) {
      out(dim('Sem faturas registadas. Usa `vnfin ledger add`.'));
      return;
    }
    out(bold(`${invoices.length} fatura(s) registada(s)`));
    out(dim(pad('DATA', 12) + pad('N.º', 16) + pad('CLIENTE', 26) + pad('BASE', 14) + pad('IVA', 12) + 'LÍQUIDO'));
    for (const invoice of invoices) {
      const totals = invoiceTotals(invoice);
      out(
        pad(invoice.date, 12) +
          pad(invoice.number, 16) +
          pad(invoice.clientName.slice(0, 24), 26) +
          pad(formatEur(totals.baseCents), 14) +
          pad(formatEur(totals.ivaCents), 12) +
          formatEur(totals.netReceivableCents) +
          (invoice.paymentProofInVault ? '' : ` ${yellow('sem comprovativo')}`),
      );
    }
    return;
  }

  if (action === 'add') {
    const base = str(values, 'base');
    if (base === undefined) fail('ledger add exige --base. Exemplo: vnfin ledger add --base 1200 --client "ACME, Lda."');
    const baseCents = Math.round(Number(base.replace(',', '.')) * 100);
    if (!Number.isFinite(baseCents)) fail(`--base inválido: ${base}`);

    // Every rate below comes from the rule pack, through one tested function, and
    // never from a literal here: the pack is the only place Portuguese tax law is
    // written down.
    const { pack } = loadRulePack(resolvePackPath(str(values, 'rules'), context.year).path);
    const profile = context.vault.loadProfile();
    const country = (str(values, 'country') ?? 'PT').toUpperCase();
    const terms = defaultInvoiceTerms(pack, profile, country);

    const treatment = (str(values, 'treatment') ?? terms.vatTreatment) as Invoice['vatTreatment'];
    if (!['iva_pt', 'autoliquidacao_ue', 'isento_art53', 'exportacao'].includes(treatment)) {
      fail(`--treatment inválido: ${treatment}. Usa iva_pt, autoliquidacao_ue, isento_art53 ou exportacao.`);
    }

    const requestedIva = num(values, 'iva-rate');
    if (requestedIva !== undefined && treatment !== 'iva_pt' && requestedIva !== 0) {
      fail(
        `--iva-rate ${requestedIva}% não é compatível com o tratamento ${treatment}: ` +
          'só o tratamento iva_pt liquida IVA português.',
      );
    }
    if (treatment === 'iva_pt' && pack.constants.iva.rates.normal === null && requestedIva === undefined) {
      fail('o pacote de regras não tem a taxa normal de IVA; indica --iva-rate explicitamente.');
    }
    const ivaRate = requestedIva ?? terms.ivaRateBp / 100;
    const retention = num(values, 'retention') ?? terms.retentionBp / 100;

    const invoice: Invoice = {
      id: `inv_${Date.now().toString(36)}`,
      number: str(values, 'number') ?? `FR ${context.year}/${String(context.vault.loadInvoices().length + 1).padStart(3, '0')}`,
      date: str(values, 'date') ?? context.today,
      clientName: str(values, 'client') ?? 'Cliente sem nome',
      clientNif: str(values, 'nif') ?? null,
      clientCountry: country,
      description: str(values, 'description') ?? '',
      baseCents,
      ivaRateBp: Math.round(ivaRate * 100),
      vatTreatment: treatment,
      retentionBp: Math.round(retention * 100),
      atcud: str(values, 'atcud') ?? null,
      status: (str(values, 'status') as Invoice['status'] | undefined) ?? 'issued',
      paymentProofInVault: bool(values, 'paid'),
    };

    context.vault.ensure();
    context.vault.appendInvoice(invoice);
    const totals = invoiceTotals(invoice);
    out(`${green('fatura registada')} ${invoice.number}`);
    out(`  base            ${formatEur(totals.baseCents)}`);
    out(`  IVA             ${formatEur(totals.ivaCents)}  ${dim(`(${formatBpAsPercent(invoice.ivaRateBp)}, ${invoice.vatTreatment})`)}`);
    out(`  retenção        ${formatEur(totals.retentionCents)}  ${dim(`(${formatBpAsPercent(invoice.retentionBp)})`)}`);
    out(`  a receber       ${bold(formatEur(totals.netReceivableCents))}`);
    return;
  }

  fail(`ação desconhecida para ledger: ${action}`);
}

function commandVault(values: Values, context: Context): void {
  const action = str(values, 'action') ?? 'list';
  if (action === 'list') {
    const index = context.vault.readJson<Array<{ file: string; sha256: string; addedAt: string }>>(
      'documents/index.json',
      [],
    );
    if (index.length === 0) {
      out(dim('Cofre vazio. Usa `vnfin vault add <ficheiro>`.'));
      return;
    }
    for (const entry of index) out(`  ${entry.addedAt.slice(0, 10)}  ${entry.sha256.slice(0, 12)}  ${entry.file}`);
    return;
  }
  if (action === 'add') {
    const target = str(values, 'file');
    if (target === undefined) fail('vault add exige o caminho do ficheiro.');
    const absolute = resolve(target);
    const kind = str(values, 'kind');
    const obligationId = str(values, 'obligation');
    let added: { file: string; sha256: string };
    try {
      added = context.vault.addDocument(absolute, {
        ...(kind === undefined ? {} : { kind }),
        obligationId: obligationId ?? null,
      });
    } catch (cause) {
      fail((cause as Error).message);
    }
    out(`${green('documento arquivado')} ${added.file}`);
    out(`  sha256 ${added.sha256}`);
    out(dim('  O ficheiro original não foi alterado: foi feita uma cópia com o hash no nome.'));
    return;
  }
  fail(`ação desconhecida para vault: ${action}`);
}

function commandAiKey(values: Values, context: Context): void {
  const action = str(values, 'action') ?? 'status';
  if (action === 'status') {
    const resolved = resolveApiKey({
      dataDir: context.vault.dir,
      passphrase: process.env['VN_FINANCE_PASSPHRASE'],
    });
    if (resolved.key === null) {
      out(dim('Sem chave DeepSeek configurada. O assistente está desligado; o resto da aplicação funciona.'));
      out(dim('Define DEEPSEEK_API_KEY ou usa `vnfin ai-key set`.'));
    } else {
      out(`${green('chave disponível')} ${maskKey(resolved.key)} ${dim(`(origem: ${resolved.source})`)}`);
    }
    for (const problem of resolved.problems) out(`${yellow('aviso')} ${problem}`);
    return;
  }
  if (action === 'set') {
    const key = str(values, 'key') ?? process.env['DEEPSEEK_API_KEY'];
    const passphrase = str(values, 'passphrase') ?? process.env['VN_FINANCE_PASSPHRASE'];
    if (key === undefined || key.trim() === '') fail('falta a chave. Usa --key ou define DEEPSEEK_API_KEY.');
    if (passphrase === undefined || passphrase === '') {
      fail('falta a frase-passe que cifra a chave. Define VN_FINANCE_PASSPHRASE (mínimo 12 caracteres).');
    }
    context.vault.ensure();
    const path = saveApiKey(context.vault.dir, key.trim(), passphrase);
    context.vault.appendAudit({ action: 'apikey.stored' });
    out(`${green('chave cifrada')} em ${path}`);
    out(dim('A chave nunca é gravada na base de dados nem no registo de auditoria.'));
    return;
  }
  fail(`ação desconhecida para ai-key: ${action}`);
}

/**
 * Alerts — red flags and missing to-dos.
 *
 * Everything here is computed by this code from the profile, the ledger and the
 * rule pack. No language model participates in producing an alert: an alert has
 * to be the same on every run and explainable from the data, or it is not worth
 * showing. The consequence is that the assistant never receives your data at all.
 */
function commandFlags(values: Values, context: Context): void {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const { pack } = loadRulePack(packPath.path);
  const profile = context.vault.loadProfile();
  if (profile === null) fail('não existe perfil. Corre `vnfin init` primeiro.');

  const instances = buildAgenda(pack, profile, {
    year: context.year,
    today: context.today,
    completedIds: context.vault.completedIds(),
    ...(profile.trackingStart === undefined ? {} : { trackFrom: profile.trackingStart }),
  });

  const flags = [
    ...computeFlags({
      pack,
      profile,
      invoices: context.vault.loadInvoices(),
      instances,
      today: context.today,
      documentedObligationIds: context.vault.documentedObligationIds(),
      dueSoonDays: num(values, 'due-soon') ?? 30,
    }),
    ...packHealthFlags(pack, context.today),
  ];

  if (context.json) {
    out(JSON.stringify(flags, null, 2));
    return;
  }

  const summary = summariseFlags(flags);
  out(bold('Alertas') + dim(`  ·  hoje ${formatPtDate(context.today)}`));
  out(
    dim(
      `  ${summary.urgent} urgente(s) · ${summary.attention} atenção · ${summary.info} informação` +
        `  ·  regras pt/${pack.packVersion}`,
    ),
  );
  out();

  if (flags.length === 0) {
    out(dim('  Nada a assinalar.'));
    return;
  }

  for (const flag of flags) {
    out(`${severityLabel(flag.severity)} ${bold(flag.title)}`);
    for (const line of flag.detail.split('\n')) out(`    ${line}`);
    if (flag.legalBasis.length > 0) out(`    ${dim(flag.legalBasis.join(' · '))}`);
    if (flag.invoiceNumbers.length > 0 && flag.invoiceNumbers.length <= 6) {
      out(`    ${dim(`faturas: ${flag.invoiceNumbers.join(', ')}`)}`);
    }
    out();
  }

  out(
    dim(
      'Alertas calculados por código a partir do teu perfil, do teu livro de faturas e do pacote de\n' +
        'regras. Nenhum modelo de linguagem participa: o resultado é o mesmo em cada execução.',
    ),
  );
}

/**
 * Update the values that change over time.
 *
 * The assistant is used here for one mechanical job: reading current Portuguese
 * tax figures out of official sources and returning them as structured data in
 * the pack's exact units. It never sees the taxpayer's data — the request is
 * built from the rule pack alone — and it can never make a value true: an
 * applied proposal is recorded as `ai-proposed` and `unverified` until a human
 * confirms it against the cited source.
 *
 * Two explicit stages, and neither is automatic:
 *   `--send`    fetch a proposal and store it in the vault (nothing is changed)
 *   `--approve` apply the pending proposal to the pack
 */
async function commandUpdate(values: Values, context: Context): Promise<void> {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const { pack } = loadRulePack(packPath.path);
  const proposalPath = `rules/proposals-${pack.year}.json`;

  if (bool(values, 'approve')) {
    const proposal = context.vault.readJson<UpdateProposal | null>(proposalPath, null);
    if (proposal === null) {
      fail(
        `não há proposta pendente em ${context.vault.path(proposalPath)}. ` +
          'Corre `vnfin update --send` primeiro.',
      );
    }
    const diffs = diffProposal(proposal).filter((diff) => diff.changed);
    if (diffs.length === 0) {
      out(dim('A proposta pendente não altera nenhum valor do pacote.'));
      return;
    }

    const verifiedByHuman = bool(values, 'verified');
    const { pack: updated, applied } = applyProposal(pack, proposal, {
      at: context.today,
      model: proposal.model,
      verifiedByHuman,
    });
    writeFileSync(packPath.path, `${JSON.stringify(updated, null, 2)}\n`, 'utf8');
    context.vault.appendAudit({
      action: 'rules.applied',
      detail: `${applied} valores propostos por ${proposal.model}${verifiedByHuman ? ' (confirmados)' : ''}`,
    });

    out(`${green('pacote atualizado')} ${packPath.path}`);
    out(`  ${applied} valor(es) · versão ${pack.packVersion} → ${updated.packVersion}`);
    out();
    for (const diff of diffs) {
      out(`  ${pad(diff.path, 58)} ${formatValue(diff.current, diff.unit)} → ${bold(formatValue(diff.proposed, diff.unit))}`);
    }
    out();
    if (verifiedByHuman) {
      out(yellow('Os valores foram marcados como confirmados por ti.'));
      out(dim('  Confirmação humana é uma afirmação tua de que leste a fonte citada.'));
    } else {
      out(yellow('Os valores ficam por confirmar.'));
      out(
        dim(
          '  Cada um é registado como proposto pelo assistente e aparece como «por confirmar» no\n' +
            '  `vnfin doctor` e nos alertas, até uma pessoa o validar na fonte citada.',
        ),
      );
    }
    return;
  }

  const request = buildUpdateRequest(pack);
  const messages = buildUpdateMessages(request);

  out(bold(`Atualização de valores — pt/${pack.year}`) + dim(`  ·  ${request.variables.length} variáveis`));
  out();
  out(dim('  O assistente recebe apenas nomes de variáveis e os valores atuais do pacote de regras,'));
  out(dim('  que são lei pública. Nenhum dado teu — perfil, faturas, clientes, rendimentos — entra'));
  out(dim('  neste pedido, porque não existe código que o possa incluir.'));
  out();

  if (!bool(values, 'send')) {
    out(bold('Variáveis a confirmar'));
    out(dim(formatVariables(request.variables)));
    out();
    out(dim(`  pedido: ${messages[0]?.content.length ?? 0} + ${messages[1]?.content.length ?? 0} caracteres`));
    out(yellow('Nada foi enviado.'));
    out(dim(`  Repete com ${cyan('--send')} para pedir os valores à DeepSeek.`));
    out(dim('  Nada é alterado no pacote sem `--approve`, e mesmo aí fica por confirmar.'));
    return;
  }

  const resolved = resolveApiKey({ dataDir: context.vault.dir, explicit: str(values, 'api-key') });
  if (resolved.key === null) {
    for (const problem of resolved.problems) out(`${yellow('aviso')} ${problem}`);
    fail('não há chave DeepSeek: nada foi enviado. Configura DEEPSEEK_API_KEY ou `vnfin ai-key set`.');
  }

  const client = new DeepSeekClient(resolved.key);
  const model = str(values, 'model') ?? DEFAULT_MODEL;
  const answer = await client.complete(redactForSend(messages), {
    model,
    maxTokens: num(values, 'max-tokens') ?? 2000,
    temperature: 0,
  });
  const parsed = parseUpdateResponse(answer.text, request);

  const proposal: UpdateProposal = {
    year: pack.year,
    asOf: parsed.asOf === '' ? context.today : parsed.asOf,
    model: answer.model,
    proposedAt: context.today,
    values: parsed.values,
    problems: parsed.problems,
  };
  context.vault.ensure();
  context.vault.writeJson(proposalPath, proposal);
  context.vault.appendAudit({
    action: 'rules.proposed',
    detail: `${answer.model} · ${parsed.values.length} variáveis · ${parsed.problems.length} problemas`,
  });

  const diffs = diffProposal(proposal);
  const changed = diffs.filter((diff) => diff.changed);

  out(bold('Proposta recebida') + dim(`  ·  ${answer.model} · referência ${proposal.asOf}`));
  out(
    dim(
      `  ${answer.usage.promptTokens}+${answer.usage.completionTokens} tokens · ` +
        (answer.costCents === null ? 'custo não configurado' : formatEur(answer.costCents as Cents)) +
        `  ·  ${changed.length} valor(es) diferente(s) do pacote`,
    ),
  );
  out();

  if (changed.length === 0) {
    out(dim('  Nenhum valor difere do pacote atual.'));
  } else {
    out(dim(pad('VARIÁVEL', 58) + pad('ATUAL', 14) + pad('PROPOSTO', 14) + 'FONTE INDICADA'));
    for (const diff of changed) {
      out(
        pad(truncate(diff.path, 56), 58) +
          pad(formatValue(diff.current, diff.unit), 14) +
          pad(bold(formatValue(diff.proposed, diff.unit)), 14) +
          (diff.sourceUrl ?? dim('sem fonte indicada')),
      );
    }
  }
  out();

  const unchanged = diffs.filter((diff) => !diff.changed && diff.proposed !== null);
  if (unchanged.length > 0) out(dim(`  ${unchanged.length} valor(es) confirmados sem alteração.`));
  const unanswered = diffs.filter((diff) => diff.proposed === null);
  if (unanswered.length > 0) {
    out(yellow(`  ${unanswered.length} variável(is) sem resposta: ${unanswered.map((d) => d.path).join(', ')}`));
  }
  for (const problem of parsed.problems) out(`  ${yellow('!')} ${dim(problem)}`);
  out();
  out(dim(`  Proposta gravada em ${context.vault.path(proposalPath)} — dentro do cofre local,`));
  out(dim('  que nunca é versionado nem enviado a lado nenhum.'));
  out();
  out(yellow('O pacote de regras não foi alterado.'));
  out(dim(`  Revê o quadro acima e corre ${cyan('vnfin update --approve')} para aplicar.`));
  out(dim('  Acrescenta `--verified` apenas se tiveres confirmado cada valor na fonte citada.'));
}

/**
 * Start the local panel.
 *
 * The server binds to 127.0.0.1 only, validates the Host header, and requires the
 * per-run token printed in the URL. Nothing here reaches the network: the panel is
 * a view over the same vault the command line uses.
 */
async function commandWeb(values: Values, context: Context): Promise<void> {
  const packPath = resolvePackPath(str(values, 'rules'), context.year);
  const running = await startWebServer({
    vault: context.vault,
    version: VERSION,
    year: context.year,
    packPath: packPath.path,
    port: num(values, 'port') ?? 7717,
  });
  context.vault.appendAudit({ action: 'web.started', detail: `porta ${running.port}` });

  out(bold('Painel local') + dim('  ·  só neste computador'));
  out();
  out(`  ${cyan(running.url)}`);
  out();
  out(dim('  Abre este endereço no navegador desta máquina.'));
  out(dim('  O endereço inclui a chave da sessão: sem ela o painel não responde a pedidos.'));
  out(dim('  Não abras este endereço noutro dispositivo nem o partilhes.'));
  out(dim('  O servidor escuta apenas em 127.0.0.1 e recusa pedidos de outros anfitriões.'));
  out();
  out(dim('  Ctrl+C para parar.'));

  await new Promise<void>((resolvePromise) => {
    const stop = (): void => {
      void running.close().then(() => resolvePromise());
    };
    process.once('SIGINT', stop);
    process.once('SIGTERM', stop);
  });
  out(dim('servidor parado.'));
}

function formatValue(value: number | null, unit: string): string {
  if (value === null) return '—';
  switch (unit) {
    case 'EUR_cents':
      return formatEur(value);
    case 'basis_points':
      return `${formatBpAsPercent(value)} (${value})`;
    default:
      return String(value);
  }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

const USAGE = `${bold('vnfin')} — assistente fiscal local (Portugal, CIRS categoria B)  v${VERSION}

${bold('USO')}
  vnfin <comando> [opções]

${bold('COMANDOS')}
  init          Cria o perfil do contribuinte no cofre local
  doctor        Diagnostica o perfil, o pacote de regras, o cofre e a chave da API
  agenda        Mostra o calendário de obrigações e o que vence a seguir
  flags         Alertas, riscos e tarefas em falta (calculados por código)
  web           Abre o painel no navegador, só neste computador (127.0.0.1)
  rules         Lista as regras e as fontes citadas por cada uma
  estimate      Calcula IVA, Segurança Social e reserva a partir do livro de faturas
  ledger        Lista ou registra faturas  (ledger list | ledger add)
  vault         Arquiva documentos com hash  (vault list | vault add <ficheiro>)
  ai-key        Gere a chave DeepSeek  (ai-key status | ai-key set)
  update        Atualiza os valores que mudam com o tempo  (--send, depois --approve)

${bold('OPÇÕES GLOBAIS')}
  --data-dir <dir>    Onde vive o cofre (por omissão ~/.vn-finance)
  --year <ano>        Ano fiscal a considerar (por omissão, o ano corrente)
  --rules <ficheiro>  Pacote de regras alternativo
  --json              Saída legível por máquina
  --help, --version

${bold('OPÇÕES POR COMANDO')}
  web       --port <porta>        Porta local (por omissão 7717)
  estimate  --quarter <1-4>       Só um trimestre
            --despesas <valor>    Despesas elegíveis, para o rendimento tributável do IRS
            --as-of <AAAA-MM-DD>  Data de apuramento
  agenda    --horizon <dias>      Horizonte do plano de ação (por omissão 90)
            --all                 Inclui concluídas, não aplicáveis e histórico

${bold('EXEMPLOS')}
  vnfin init --nif 123456789 --name "Nome Completo" --iva isento_art53 --turnover-ano-anterior 12400
  vnfin agenda --horizon 120
  vnfin flags
  vnfin web --port 7717          # painel local, só neste computador
  vnfin ledger add --base 1200 --client "ACME, Lda." --nif 501234567
  vnfin estimate --quarter 3
  vnfin update --send            # só nomes de variáveis e lei pública; nada teu
  vnfin update --approve         # aplica a proposta; fica por confirmar até validares

${bold('O QUE ESTA APLICAÇÃO NUNCA FAZ')}
  Não fala com o Portal das Finanças, com a Segurança Social nem com qualquer outro
  serviço público. Não faz login em lado nenhum, não sincroniza e não envia telemetria.
  O único pedido de rede é o do comando "update", com dados de lei pública.

${dim('Esta aplicação não substitui um contabilista certificado e não entrega declarações por ti.')}
`;

async function main(): Promise<void> {
  const { values, positionals } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: true,
    // Unknown flags are tolerated, but EVERY option the CLI documents is
    // declared here on purpose: with `strict: false`, an undeclared option is
    // parsed as a boolean and its value silently becomes a positional argument.
    strict: false,
    options: {
      help: { type: 'boolean', short: 'h' },
      version: { type: 'boolean' },
      json: { type: 'boolean' },
      all: { type: 'boolean' },
      force: { type: 'boolean' },
      paid: { type: 'boolean' },
      exports: { type: 'boolean' },
      intracomunitarias: { type: 'boolean' },
      'clientes-ue': { type: 'boolean' },
      'clientes-fora-ue': { type: 'boolean' },
      'ss-isencao-inicio': { type: 'boolean' },
      send: { type: 'boolean' },
      approve: { type: 'boolean' },
      verified: { type: 'boolean' },

      'data-dir': { type: 'string' },
      year: { type: 'string' },
      rules: { type: 'string' },

      nif: { type: 'string' },
      name: { type: 'string' },
      iva: { type: 'string' },
      irs: { type: 'string' },
      'start-date': { type: 'string' },
      'turnover-ano-anterior': { type: 'string' },
      'turnover-ano-corrente': { type: 'string' },

      horizon: { type: 'string' },
      'due-soon': { type: 'string' },
      quarter: { type: 'string' },
      'as-of': { type: 'string' },
      port: { type: 'string' },
      despesas: { type: 'string' },

      base: { type: 'string' },
      client: { type: 'string' },
      country: { type: 'string' },
      treatment: { type: 'string' },
      'iva-rate': { type: 'string' },
      retention: { type: 'string' },
      number: { type: 'string' },
      date: { type: 'string' },
      description: { type: 'string' },
      atcud: { type: 'string' },
      status: { type: 'string' },

      file: { type: 'string' },
      kind: { type: 'string' },
      obligation: { type: 'string' },

      key: { type: 'string' },
      passphrase: { type: 'string' },
      'api-key': { type: 'string' },
      model: { type: 'string' },
      'max-tokens': { type: 'string' },
    },
  });
  const argv = values as Values;

  if (bool(argv, 'version')) {
    out(VERSION);
    return;
  }

  const command = positionals[0];
  if (command === undefined || bool(argv, 'help')) {
    out(USAGE);
    return;
  }

  const year = num(argv, 'year') ?? parseIso(todayInLisbon()).year;
  const context: Context = {
    vault: new Vault(resolveDataDir(str(argv, 'data-dir'))),
    year,
    today: todayInLisbon(),
    json: bool(argv, 'json'),
  };

  switch (command) {
    case 'init':
      commandInit(argv, context);
      return;
    case 'doctor':
      commandDoctor(argv, context);
      return;
    case 'rules':
      commandRules(argv, context);
      return;
    case 'agenda':
      commandAgenda(argv, context);
      return;
    case 'estimate':
      commandEstimate(argv, context);
      return;
    case 'ledger':
      argv['action'] = positionals[1] ?? 'list';
      commandLedger(argv, context);
      return;
    case 'vault':
      argv['action'] = positionals[1] ?? 'list';
      argv['file'] = str(argv, 'file') ?? positionals[2];
      commandVault(argv, context);
      return;
    case 'ai-key':
      argv['action'] = positionals[1] ?? 'status';
      commandAiKey(argv, context);
      return;
    case 'flags':
      commandFlags(argv, context);
      return;
    case 'web':
      await commandWeb(argv, context);
      return;
    case 'update':
      await commandUpdate(argv, context);
      return;
    default:
      fail(`comando desconhecido: "${command}". Usa --help.`);
  }
}

// A user-facing CLI reports a failure in one line, not as a stack trace. The
// stack is still available on demand, because hiding it entirely would make the
// tool harder to debug than it is to use.
try {
  await main();
} catch (cause) {
  const error = cause as Error;
  process.stderr.write(`${red('erro')} ${error.message}\n`);
  if (process.env['VN_FINANCE_DEBUG'] !== undefined) {
    process.stderr.write(`${error.stack ?? ''}\n`);
  }
  process.exit(1);
}
