# vn-finance

A private, local-first assistant for the Portuguese tax obligations of a
self-employed professional — the *recibo verde* world of CIRS category B.

It shows your CAE and CIRS situation, the invoices you issued, what is due across
the year and what documents you must keep. It computes IVA, Segurança Social and
the simplified-regime tax base from a versioned, cited rule pack. It refuses to
guess, and it never files anything for you. There is a command line and a local
browser panel over the same core.

> This application does not replace a certified accountant (*contabilista
> certificado*) and does not submit declarations. It prepares, checks, reminds and
> explains.

## Why it exists

A programmer with an open activity in Portugal has a small number of obligations,
a large number of deadlines that move every year, and no cheap way to see all of it
in one place. Spreadsheets lose the legal basis. Portals show one obligation at a
time. Generic accounting software is built for accountants and for companies, not
for one person who wants to understand their own year.

This project tries to be the third thing: a tool that knows the calendar, shows its
sources, and is honest about what it does not know.

---

## Getting started

### What you need

- **Node 22.18 or newer** (`node --version`). The CLI runs TypeScript directly
  through Node's type stripping: no build step, no bundler, no runtime dependency.
- A browser, for the local panel.
- **Full-disk encryption, if you have it.** The vault is not encrypted at rest yet
  (see `docs/PRIVACY-SECURITY.md` §3), so the operating system's disk encryption is
  the control that protects it today.

### 1. Install and check it works

```bash
git clone <this repository> vn-finance
cd vn-finance
npm install          # only TypeScript and @types/node, for development
npm run verify       # strict typecheck + 111 tests
```

`npm run verify` is the gate. It should end with `# pass 111` and `# fail 0`.

### 2. Start it

```bash
node src/cli.ts          # or: npm start — opens the local panel in your browser
```

Running it with no command is the normal way to start: it opens the panel at
`http://127.0.0.1:7717/?t=<token>` and starts the local server. There is no setup
command to remember and nothing to type at the terminal.

- On a vault with **no profile**, the panel opens its profile form. Fill it in,
  press save, and the profile is written to `~/.vn-finance/profile.json` — the
  same file the command line uses.
- On a vault that **already has a profile**, the panel simply loads it.
- Use `--no-open` if you would rather open the address yourself, and `--port` to
  move it off 7717.

### 3. The profile, and why two fields are compulsory

The application refuses to guess the **NIF** and your declared **IVA regime**:
they are the declaration you made to the AT, and a wrong default would silently
change every obligation the tool shows you.

| Field | What it is | Why it matters |
| --- | --- | --- |
| NIF | Your tax number | Validated with the modulus-11 check digit before it is stored |
| Name | Your name | Used to redact it from anything sent to a model |
| IVA regime | `isento_art53`, `trimestral` or `mensal` | **Required.** It is the enquadramento of your declaração de início de atividade |
| Turnover last year | Turnover in **national territory**, in euros | What the art. 53.º CIVA exemption is measured against. Without it the app says which check it cannot run instead of assuming |
| Turnover this year | Your estimate | Drives the mid-year warning when the art. 53.º ceiling approaches |
| Start date | When the activity opened | Used for the Segurança Social startup period |
| EU / non-EU clients | Who you invoice | Reverse charge, VIES, declaração recapitulativa, export treatment |

Both IVA regimes are plausible for a programmer: `isento_art53` if you stayed under
€15 000 nationally, `trimestral` once you are above it. If you are not sure which
one the AT has on file, check the Portal das Finanças first — the application
deliberately will not decide this for you.

The **Perfil** button in the panel's top bar reopens the same form at any time, and
it also loads and saves profiles as files: *Carregar perfil de ficheiro…* reads a
`profile.json` from another vault or a backup, and *Exportar o perfil atual* writes
the active profile to a file you can carry to another computer.

If you prefer the command line, `init` still exists and does the same thing:

```bash
node src/cli.ts init \
  --nif 123456789 \
  --name "O teu nome" \
  --iva isento_art53 \
  --turnover-ano-anterior 12400 \
  --start-date 2019-04-01 \
  --clientes-ue
```

Add `--force` to overwrite an existing profile.

### 4. Read the diagnosis

```bash
node src/cli.ts doctor
```

This is the most informative command in the tool. It reports:

- whether the profile is internally consistent (an art. 53.º profile that also
  declares exports is an **error**, not a note);
- how much of the rule pack is verified against an official source;
- **the inputs only you can supply**, so you know what is still missing;
- whether the rule pack covers the current year or is provisional;
- how many values the assistant proposed that no human has confirmed yet.

### 5. See what is due

```bash
node src/cli.ts agenda                  # next 90 days, then the rest of the year
node src/cli.ts agenda --horizon 180    # a wider plan
node src/cli.ts agenda --all            # include completed, N/A and history
node src/cli.ts agenda --json           # machine readable
```

Every row carries its **source**: `oficial` (published by the AT), `calculada`
(derived from the general rule because the AT published nothing for that month) or
`provisória` (next year's calendar is not published yet). Three markers appear next
to rows that need your judgement:

| Marker | Meaning |
| --- | --- |
| `?` | The rule is not fully verified — check the cited source before relying on it |
| `!` | A discrepancy: the published date is neither the legal rule nor its weekend shift. Read the explanation |
| `~` | A provisional date: the authority has not published that year yet |

Deadlines that fell before your first day of use are shown as `histórico`, never as
something you failed to do.

### 6. See the red flags

```bash
node src/cli.ts flags
node src/cli.ts flags --json
```

Alerts are computed in code, from your profile, your ledger and the rule pack: the
art. 53.º eligibility and its 15 000 / 18 750 € transition zone, Portuguese VAT
charged to a foreign client, withholding missing on a resident invoice (or charged
to a non-resident one), duplicated invoice numbers, missing payment proofs,
documents missing before a deadline, overdue obligations. **No language model is
involved**, so the result is identical on every run and can be argued with.

### 7. Record invoices

```bash
node src/cli.ts ledger add --base 1200 --client "ACME, Lda." --nif 501234560
node src/cli.ts ledger add --base 4200 --client "Helsinki Labs Oy" --country FI
node src/cli.ts ledger add --base 900 --client "Studio Mira" --retention 0 --paid
node src/cli.ts ledger list
```

Amounts are entered in euros and stored as integer cents. The IVA treatment, the
IVA rate and the withholding rate are **taken from the rule pack**, from the
client's country and from your own IVA regime:

| Client | Treatment | IVA | Retention |
| --- | --- | --- | --- |
| In Portugal, normal regime | `iva_pt` | the pack's normal rate | the pack's resident rate |
| In Portugal, art. 53.º exempt | `isento_art53` | 0% | the pack's resident rate |
| Business in the EU | `autoliquidacao_ue` | 0% | 0% |
| Outside the EU | `exportacao` | 0% | 0% |

A client outside Portugal does not withhold Portuguese IRS — the pack's
non-resident rate applies in the other direction, to income paid *to* a
non-resident by a Portuguese payer. Override any of it with `--treatment`,
`--iva-rate` and `--retention`; the command refuses a combination that contradicts
itself. Also available: `--date`, `--number`, `--atcud`, `--status`, `--paid`.

### 8. See the numbers

```bash
node src/cli.ts estimate                                  # per quarter + reserve
node src/cli.ts estimate --quarter 3                      # one quarter
node src/cli.ts estimate --despesas 5000                  # + the IRS tax base
```

The Segurança Social derivation is shown as a chain you can follow, and the three
monthly instalments always sum **exactly** to the quarterly contribution. The IRS
section stops at the **taxable income**: applying the art. 68.º brackets needs the
year's rate table, and until that is in the rule pack the honest output is a base,
not a tax. Pass `--despesas` with your eligible documented expenses (art. 31.º
n.º 13 CIRS adds back the shortfall against 15% of service income).

### 9. Open the panel again

```bash
node src/cli.ts                     # the panel, with the browser opened for you
node src/cli.ts web --no-open       # same, without opening a browser
node src/cli.ts web --port 7800
```

Open the printed address **in a browser on this machine**. It includes a per-run
session token; the panel will not answer API calls without it. Press Ctrl+C to stop.

### 10. Keep documents

```bash
node src/cli.ts vault add ~/Documents/comprovativo-iva-t3.pdf \
  --kind comprovativo --obligation iva.dp.trimestral
node src/cli.ts vault list
```

The file is copied into the vault under the first 12 characters of its SHA-256 and
indexed; the original is never moved or modified. Re-hashing it later proves it has
not changed.

### 11. Optional — keep the rule values current

The assistant has exactly one job: proposing new values for the rule variables that
change over time (rates, ceilings, the IAS, coefficients), as structured JSON in the
pack's exact units. It never sees your data.

```bash
node src/cli.ts ai-key set --key sk-...          # or export DEEPSEEK_API_KEY
node src/cli.ts update                           # lists the 21 variables; sends NOTHING
node src/cli.ts update --send                    # fetches a proposal, stores it locally
node src/cli.ts update --approve                 # applies it, marked "por confirmar"
node src/cli.ts update --approve --verified      # only if you checked each source
```

`ai-key set` encrypts the key with AES-256-GCM (scrypt-derived, from
`VN_FINANCE_PASSPHRASE`). The stored key is never written to the ledger, the audit
log or a backup of the ledger.

`update` is a dry run unless you pass `--send`, and nothing is written to the pack
without `--approve`. An applied value is recorded as `ai-proposed` and
`unverified`, and stays flagged in `doctor` and in the alerts until a person
confirms it against the cited source. **The assistant proposes; it never verifies.**

### Optional — a global `vnfin` command

```bash
npm link          # then, from anywhere:
vnfin doctor
vnfin agenda
```

`npm unlink -g vn-finance` removes it.

---

## The local panel

Running the application starts the panel: `vnfin web`, or simply `vnfin`. It serves
one HTML file, one stylesheet and one ES module from the same core the CLI uses. It
is also where the profile is created on first use — an empty vault opens the profile
form instead of a screen of zeros — and it loads and saves profiles as files. Five
controls make it safe to leave running:

| Control | Why it exists |
| --- | --- |
| Binds to `127.0.0.1` only | Never `0.0.0.0`. Nothing on your network can reach it |
| `Host` header allowlist | A page on any site can make your browser resolve a name to `127.0.0.1` (DNS rebinding). Such a request carries the attacker's hostname and is refused |
| Per-run session token | Every API call needs the token from the printed URL, sent as a header. A foreign page cannot read it, and cannot set that header without a preflight this server never approves |
| No CORS headers at all | Cross-origin reads fail in the browser by default |
| Content Security Policy | No inline script, no inline style, no external resource — so nothing can be fetched from outside |

The panel contains **no tax logic**: one view model (`src/web/report.ts`) assembles
everything from the deterministic core, and the browser only lays it out. Every
write — recording an invoice, marking an obligation handled, archiving a document,
applying a rule update — goes through the same vault, the same validation and the
same append-only audit log as the CLI.

---

## What it never does

- It does not talk to the Portal das Finanças, Segurança Social Direta, VIES or any
  other public body: no login, no scraping, no browser automation, no webservice.
- It does not file anything, anywhere.
- It does not sync, and it has no server on the network. The panel listens on
  `127.0.0.1` only, and the vault refuses to open a data directory inside this
  repository — personal data is never one `git add -A` away from being published.
- It does not send your data to a model. The assistant's request is built from the
  rule pack alone.

---

## Where the data lives

Default: `~/.vn-finance` (`%USERPROFILE%\.vn-finance` on Windows). Override with
`--data-dir` or `VN_FINANCE_DATA_DIR`.

```
profile.json                  your profile
ledger/invoices.jsonl         append-only invoice ledger
obligations/completions.jsonl obligations you marked as handled
documents/index.json          document index, with SHA-256 hashes
documents/<hash>-<name>       the archived copies
audit/audit.jsonl             append-only audit log
ai/deepseek.key               the encrypted API key, if you set one
rules/proposals-<year>.json   a pending rule update, before you approve it
```

**Backup** is copying that folder. **Restore** is copying it back. **Erasure** is
deleting it — there is no server, no account and no telemetry to clean up
separately.

---

## Troubleshooting

| Message | What to do |
| --- | --- |
| `não existe perfil neste cofre. Corre vnfin init primeiro.` | Run step 2 |
| `init exige o regime de IVA declarado: --iva ...` | Check the regime on the Portal das Finanças, then pass `--iva` |
| `o cofre não pode viver dentro do repositório da aplicação` | Use the default `~/.vn-finance`, or a `--data-dir` outside the checkout. Override deliberately with `VN_FINANCE_ALLOW_IN_REPO=1` only if your `.gitignore` covers it |
| `não há chave DeepSeek: nada foi enviado` | `node src/cli.ts ai-key set`, or export `DEEPSEEK_API_KEY` |
| `Não foi possível decifrar a chave guardada` | The `VN_FINANCE_PASSPHRASE` does not match the one used to store the key |
| `aviso: não existe pacote de regras para 2027; a usar o de 2026` | Expected until the AT publishes next year's calendar. Those dates are marked `provisória` |
| Port already in use | `node src/cli.ts web --port 7800` |
| `perfil.activity.turnoverPreviousYearCents` warning | The art. 53.º check cannot run without it — see step 2, or edit the profile in the panel |

---

## How the rule pack is kept honest

The law lives in `src/rules/pt/<year>.json` as data, never in `if` statements. Each
obligation carries its legal basis, the sources it came from, and a verification
level:

- **verified** — confirmed against an official source;
- **partial** — the deadline is confirmed from the authority's published calendar,
  but the article establishing it was not read in its consolidated text;
- **unverified** — stated by a corroborating secondary source only.

Published dates always beat derived dates, and where the two cannot be reconciled
the obligation carries a discrepancy for a human to read. Next year's calendar does
not exist until the AT publishes it, so those dates are derived and marked
provisional. `vnfin rules` shows every rule with its citation; `vnfin doctor`
reports the counts. The `pt/2026` pack has 19 obligations, 20 sources (17 official),
6 verified and 13 partial — every gap is listed in the pack's own `todo`, and
`docs/research/2026-sources.md` explains what was searched and what was not found.

---

## What is built and what is not

**Built and tested** (`npm run verify`, 111 tests): the rule pack schema with
provenance validation; the obligation engine with published-date precedence and
discrepancy reporting; money and rate discipline in integer cents and basis points;
the IVA, Segurança Social and IRS-base estimates; the alert engine; the local vault
and append-only ledger; the profile builder with its import path; the CLI; the
bounded assistant with unit and magnitude validation; the local panel, its profile
form and its security guards.

**Designed, not built:** the Tauri desktop shell, bank/SAF-T import, an encrypted
backup export, a printable year pack, and the panel's trend views. See
`docs/ROADMAP.md`.

---

## Documentation

| Document | What it covers |
| --- | --- |
| `docs/PROPOSAL.md` | The design: principles, architecture, the rule pack, the engine, the AI boundary, risks, decisions |
| `docs/DOMAIN-PT.md` | The Portuguese domain, and the verification status of every legal fact |
| `docs/PRIVACY-SECURITY.md` | Threat model, the panel's attack surface, key handling, what is not protected yet |
| `docs/ROADMAP.md` | Milestones and exit criteria |
| `docs/research/2026-sources.md` | Research notes: what was verified, what was not, and why |
| `design/dashboard.html` | The design source the panel's stylesheet was lifted from |
| `src/web/` | The panel: server, view model, front end |

Architecture, and the AI boundary in detail:

![Architecture](dsh-resource://diagram/library/vn-finance-architecture)

![AI privacy flow](dsh-resource://diagram/library/vn-finance-ai-privacy-flow)

## Licence

MIT. See `LICENSE`.
