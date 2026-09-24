/**
 * The dashboard view model.
 *
 * One function assembles everything the local panel shows, from the same core the
 * CLI uses. It exists so that the browser layer contains no tax logic whatsoever:
 * every number the panel displays is computed here, by the deterministic core, and
 * the front end only decides how to lay it out.
 *
 * The model is deliberately one payload rather than a dozen endpoints. A local
 * application on loopback does not have a network problem to solve, and a single
 * consistent snapshot means the panel can never show an agenda from one moment and
 * flags from another.
 */

import { buildAgenda } from '../core/calendar.ts';
import {
  estimateReserve,
  quarterReport,
  type Invoice,
  type QuarterReport,
  type ReserveEstimate,
} from '../core/estimate.ts';
import {
  computeFlags,
  packHealthFlags,
  summariseFlags,
  type Flag,
  type FlagSummary,
} from '../core/flags.ts';
import { DEFAULT_CAE, missingProfileInputs, validateProfile, type ProfileProblem } from '../core/profile.ts';
import { freshness, summarisePack, type Freshness, type PackSummary } from '../core/rules.ts';
import {
  buildUpdateRequest,
  diffProposal,
  type UpdatableVariable,
  type UpdateProposal,
} from '../ai/update.ts';
import type {
  LoadedPack,
  ObligationInstance,
  ObligationRule,
  PackProblem,
  RuleSource,
  TaxProfile,
} from '../core/types.ts';
import { checkDataDirRisk, looksLikeVault, vaultPointerPath, type Vault, type VaultLocationSource } from '../store/vault.ts';
import { MIN_PASSPHRASE_LENGTH } from '../ai/keyring.ts';

/**
 * What this application never does, in one place, so the command line, the panel
 * and the documentation cannot drift apart on the promise.
 */
export const GUARANTEES: readonly string[] = [
  'Não fala com o Portal das Finanças, com a Segurança Social, com o VIES nem com qualquer outro serviço público.',
  'Não faz login em lado nenhum, não recolhe páginas e não automatiza portais.',
  'Não entrega declarações nem paga nada por ti.',
  'Não sincroniza: os dados vivem numa só pasta, neste computador.',
  'Não envia os teus dados a um modelo: o pedido de atualização de regras é construído a partir do pacote de regras.',
  'Não substitui um contabilista certificado.',
];

export interface DashboardMeta {
  version: string;
  today: string;
  year: number;
  dataDir: string;
  packVersion: string;
  packChecksum: string;
  packPath: string;
  /**
   * The runtime the local server is on, and whether the vault sits inside a git
   * work tree. Both are `vnfin doctor` facts; the panel shows them in the same
   * diagnostic section, because someone who only ever opens the browser must be
   * able to read what the command line would have told them.
   */
  nodeVersion: string;
  dataDirRisk: { level: 'none' | 'warning' | 'fatal'; gitRoot: string | null; message: string | null };
  /**
   * Where the vault folder came from, and whether it holds anything yet.
   *
   * The panel needs both in order to decide whether to ask. A folder somebody
   * chose is not worth asking about again; the default `~/.vn-finance` on an empty
   * vault is exactly the situation where a hidden folder in the home directory
   * becomes somebody's permanent data store without them ever seeing it.
   */
  dataDirSource: VaultLocationSource;
  dataDirPointerPath: string;
  /** The folder already holds a profile, a ledger or archived documents. */
  vaultInUse: boolean;
}

/**
 * The state of the assistant's credential, as the panel may see it.
 *
 * `masked` is the only form of the key that ever reaches the browser: a panel that
 * could read the key back would turn any script-injection bug into credential
 * theft, and there is no feature that needs it.
 */
export interface KeyInfo {
  available: boolean;
  source: string;
  masked: string | null;
  problems: string[];
  /** A key file exists in the vault, whether or not it can be read right now. */
  stored: boolean;
  /** A key file exists but no passphrase is available to open it. */
  locked: boolean;
  /** The passphrase is present in the server's environment, so stored keys open. */
  passphraseFromEnv: boolean;
  minPassphraseLength: number;
}

export interface VaultEntry {
  file: string;
  sha256: string;
  addedAt: string;
  kind: string;
  obligationId: string | null;
  /** The ledger invoice this document IS, when it was imported as a fatura-recibo. */
  invoiceId: string | null;
}

export interface UpdateState {
  variables: UpdatableVariable[];
  variableCount: number;
  requestCharacters: number;
  hasKey: boolean;
  keySource: string;
  pending: UpdateProposal | null;
  pendingChanges: number;
}

export interface DashboardPack {
  summary: PackSummary;
  freshness: Freshness;
  problems: PackProblem[];
}

/**
 * The rates an interface needs in order to offer a sensible default when
 * recording an invoice. They come from the rule pack, never from a literal in the
 * front end or in the command line: the retention rate for resident professional
 * services is law, and hardcoding it once already produced a wrong default.
 */
export interface DashboardDefaults {
  ivaNormalBp: number | null;
  ivaIntermediateBp: number | null;
  ivaReducedBp: number | null;
  withholdingResidentBp: number | null;
  withholdingNonResidentBp: number | null;
}

/**
 * The closed vocabularies the profile form offers.
 *
 * They travel in the model for the same reason the tax rates do: a select box is
 * a statement about what the AT accepts, and the front end must not be the place
 * where that list lives. The labels are here too, so a regime cannot be called
 * two different things in two places.
 */
export interface ProfileOptions {
  ivaRegimes: Array<{ value: string; label: string }>;
  irsRegimes: Array<{ value: string; label: string }>;
  defaultCae: { code: string; description: string };
}

export const PROFILE_OPTIONS: ProfileOptions = {
  ivaRegimes: [
    { value: 'isento_art53', label: 'Isenção do art. 53.º do CIVA' },
    { value: 'trimestral', label: 'Regime normal — declaração trimestral' },
    { value: 'mensal', label: 'Regime normal — declaração mensal' },
  ],
  irsRegimes: [
    { value: 'simplificado', label: 'Regime simplificado' },
    { value: 'organizada', label: 'Contabilidade organizada' },
  ],
  defaultCae: { code: DEFAULT_CAE.code, description: DEFAULT_CAE.description },
};

export interface DashboardModel {
  meta: DashboardMeta;
  guarantees: readonly string[];
  defaults: DashboardDefaults;
  profileOptions: ProfileOptions;
  profile: TaxProfile | null;
  profileProblems: ProfileProblem[];
  missingInputs: string[];
  pack: DashboardPack;
  agenda: ObligationInstance[];
  flags: Flag[];
  flagSummary: FlagSummary;
  quarters: QuarterReport[];
  reserve: ReserveEstimate;
  invoices: Invoice[];
  vault: { entries: VaultEntry[]; files: number; bytes: number };
  rules: { sources: RuleSource[]; obligations: ObligationRule[]; todo: string[] };
  update: UpdateState;
  key: KeyInfo;
}

export interface DashboardInput {
  vault: Vault;
  loaded: LoadedPack;
  year: number;
  today: string;
  version: string;
  horizonDays?: number;
  apiKeyAvailable: boolean;
  apiKeySource: string;
  /** Where the vault folder came from, as resolved at startup. */
  dataDirSource?: VaultLocationSource;
  /** The masked key, when there is one. Never the key itself. */
  apiKeyMasked?: string | null;
  /** Why a stored key could not be used, when that is the case. */
  apiKeyProblems?: string[];
  /** A key file exists in the vault. */
  apiKeyStored?: boolean;
  apiKeyLocked?: boolean;
  apiKeyPassphraseFromEnv?: boolean;
  /** Overrides the stored proposal, so a freshly fetched one renders immediately. */
  pendingProposal?: UpdateProposal | null;
}

function readVaultEntries(vault: Vault): VaultEntry[] {
  const raw = vault.readJson<Array<Record<string, unknown>>>('documents/index.json', []);
  return raw.map((entry) => ({
    file: String(entry['file'] ?? ''),
    sha256: String(entry['sha256'] ?? ''),
    addedAt: String(entry['addedAt'] ?? ''),
    kind: String(entry['kind'] ?? 'outro'),
    obligationId: typeof entry['obligationId'] === 'string' ? entry['obligationId'] : null,
    invoiceId: typeof entry['invoiceId'] === 'string' ? entry['invoiceId'] : null,
  }));
}

export function buildDashboard(input: DashboardInput): DashboardModel {
  const { vault, loaded, year, today, version } = input;
  const { pack } = loaded;
  const profile = vault.loadProfile();
  const invoices = vault.loadInvoices();
  const summary = summarisePack(loaded);

  // With no profile there is nothing to schedule, alert or estimate: the panel
  // shows onboarding rather than a screen full of zeros pretending to be data.
  const instances: ObligationInstance[] =
    profile === null
      ? []
      : buildAgenda(pack, profile, {
          year,
          today,
          dueSoonDays: 30,
          completedIds: vault.completedIds(),
          ...(profile.trackingStart === undefined ? {} : { trackFrom: profile.trackingStart }),
        });

  const flags =
    profile === null
      ? packHealthFlags(pack, today)
      : [
          ...computeFlags({
            pack,
            profile,
            invoices,
            instances,
            today,
            documentedObligationIds: vault.documentedObligationIds(),
            dueSoonDays: 30,
          }),
          ...packHealthFlags(pack, today),
        ];

  const pending =
    input.pendingProposal !== undefined
      ? input.pendingProposal
      : vault.readJson<UpdateProposal | null>(`rules/proposals-${year}.json`, null);

  const request = buildUpdateRequest(pack);
  const stats = vault.describe();

  return {
    meta: {
      version,
      today,
      year,
      dataDir: vault.dir,
      packVersion: pack.packVersion,
      packChecksum: loaded.checksum,
      packPath: loaded.path,
      nodeVersion: process.version,
      dataDirRisk: checkDataDirRisk(vault.dir),
      dataDirSource: input.dataDirSource ?? 'flag',
      dataDirPointerPath: vaultPointerPath(),
      vaultInUse: looksLikeVault(vault.dir) || profile !== null || invoices.length > 0,
    },
    guarantees: GUARANTEES,
    defaults: {
      ivaNormalBp: pack.constants.iva.rates.normal,
      ivaIntermediateBp: pack.constants.iva.rates.intermediaire,
      ivaReducedBp: pack.constants.iva.rates.reduzida,
      withholdingResidentBp: pack.constants.irs.withholding.professionalServicesResident,
      withholdingNonResidentBp: pack.constants.irs.withholding.nonResident,
    },
    profile,
    profileOptions: PROFILE_OPTIONS,
    profileProblems: profile === null ? [] : validateProfile(profile),
    missingInputs: profile === null ? [] : missingProfileInputs(profile),
    pack: { summary, freshness: freshness(pack, today), problems: loaded.problems },
    agenda: instances,
    flags,
    flagSummary: summariseFlags(flags),
    quarters: [1, 2, 3, 4].map((quarter) => quarterReport(pack, invoices, year, quarter)),
    reserve: estimateReserve(pack, invoices, today),
    invoices,
    vault: { entries: readVaultEntries(vault), files: stats.files, bytes: stats.bytes },
    rules: { sources: pack.sources, obligations: pack.obligations, todo: pack.todo },
    update: {
      variables: request.variables,
      variableCount: request.variables.length,
      requestCharacters: request.variables.length === 0 ? 0 : JSON.stringify(request).length,
      hasKey: input.apiKeyAvailable,
      keySource: input.apiKeySource,
      pending,
      pendingChanges: pending === null ? 0 : diffProposal(pending).filter((diff) => diff.changed).length,
    },
    key: {
      available: input.apiKeyAvailable,
      source: input.apiKeySource,
      masked: input.apiKeyMasked ?? null,
      problems: input.apiKeyProblems ?? [],
      stored: input.apiKeyStored ?? false,
      locked: input.apiKeyLocked ?? false,
      passphraseFromEnv: input.apiKeyPassphraseFromEnv ?? false,
      minPassphraseLength: MIN_PASSPHRASE_LENGTH,
    },
  };
}
