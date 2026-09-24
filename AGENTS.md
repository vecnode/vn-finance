# AGENTS.md — working in this repository

Orientation for anyone (human or model) editing `vn-finance`. Read this first, then
`ARCHITECTURE.md` for the layer map. `README.md` is the user-facing document;
`docs/` holds the domain, privacy and roadmap material.

## What this is

A local-first personal tax assistant for independent workers in Portugal (CIRS
categoria B, *recibos verdes*). One folder of data on one machine, a deterministic
core that computes obligations and estimates, and two front ends over it: a browser
panel and a command line. It is **not** an accountant and never files anything.

## The one rule that shapes everything else

**The browser panel is the primary interface.** The person who uses this opens
`vnfin` and stays in the browser; the CLI must remain an equivalent interface, not a
prerequisite. So a feature is not finished when a command works — it is finished
when the panel can do it too, through the same core function, the same vault and the
same validation.

Concretely, when you add capability:

1. put the computation in `src/core/` (deterministic, testable, no I/O);
2. expose it on the model in `src/web/report.ts` or as a route in `src/web/server.ts`;
3. render it in `src/web/public/app.js` (and `index.html` for navigation);
4. wire the equivalent command in `src/cli.ts` (or say in the PR why the CLI does
   not need it);
5. test both the core and the wire, and update the docs.

Never tell a browser-only user to run a terminal command. If a step can only be done
on the command line today, that is a bug to close, not a note to add.

**The panel is a set of pages**, one per subject, routed by the URL hash
(`#painel`, `#agenda`, `#cofre`, …). The single list is `PAGES` at the top of
`app.js`; the sidebar in `index.html` and the narrow-screen tab strip are both
driven from it, so a new page is an entry in `PAGES`, a `data-page` link in the
sidebar, a branch in `render()`'s dispatch, and nothing else. New capability
belongs on the page of its subject — not appended to the bottom of the dashboard,
which is how the panel became one long scroll in the first place.

## Hard invariants — do not break these

- **No tax logic in the web layer.** `src/web/**` and `src/web/public/app.js` may
  format and lay out; every number, threshold and deadline comes from `src/core/`
  through the single view model. A rate written as a literal in the front end is a
  bug (this already happened once and produced a wrong default).
- **The rule pack is the only place Portuguese tax law is written down.** Routes,
  commands and forms read rates, coefficients and ceilings from the pack. A missing
  value in the pack means the calculation is *refused*, never defaulted.
- **No guessing personal inputs.** The NIF, the declared IVA regime, turnover and
  eligible expenses are declarations. If they are absent, say which check cannot run
  instead of assuming a value.
- **One network call in the whole application**, the rule-pack update, and it is
  built from the rule pack alone. No profile, invoice, client or income data has a
  code path into it. Alerts and estimates never involve a model.
- **Local first.** The server binds to `127.0.0.1`, validates the `Host` header,
  requires the per-run token, sends no CORS headers, and serves the panel under a
  strict CSP with no inline script or style.
- **Append-only records.** Invoices, completions and audit events are appended to
  JSONL and never rewritten. Writes that replace a file go through
  `Vault.writeJson` (temporary file + rename).
- **Money is integer cents, rates are integer basis points, dates are ISO
  `YYYY-MM-DD` strings.** Never floats for money.
- **UI strings are European Portuguese, with correct accents.** Code comments and
  documentation are English. `src/web/server.test.ts` has a mojibake guard — a
  mangled accent fails the suite.
- **Zero runtime dependencies.** Only `typescript` and `@types/node` as dev
  dependencies. Do not add a framework, a bundler or a UI library.

## Repo map

| Path | What lives there |
| --- | --- |
| `src/core/` | The domain: money, dates, calendar, obligations, rules, profile, estimates, flags, NIF, the PDF reader and the `fatura-recibo` parser |
| `src/core/*.test.ts` | Unit tests for the domain, run by `npm test` |
| `src/core/receipt-fixture.ts` | The invented PDFs the reader and parser tests parse; no real invoice is ever committed |
| `src/store/vault.ts` | The vault: profile, append-only ledger and audit, document index, data-dir risk, the remembered vault location, the folder chooser's directory listing |
| `src/ai/` | The bounded assistant: DeepSeek client, keyring, redaction, the update proposal flow |
| `src/cli.ts` | The command line over the same core (`init`, `doctor`, `agenda`, `flags`, `rules`, `estimate`, `ledger`, `ledger import`, `vault`, `vault where`, `vault set`, `ai-key`, `update`, `web`) |
| `src/web/report.ts` | `buildDashboard` — the single view model the panel renders |
| `src/web/server.ts` | The local HTTP server, its routes and its security guards |
| `src/web/public/` | The panel: `index.html`, `app.css`, `app.js` (no build step) |
| `src/rules/pt/2026.json` | The versioned rule pack: constants, obligations, sources, verification status |
| `design/` | The reviewed panel mockup: the reference for structure and colour. The panel has deliberately diverged from it on type size — see the header of `app.css` |
| `docs/` | Domain reference, privacy model, proposal, roadmap, research notes |

## Commands

```bash
npm run verify        # typecheck + tests. This is the gate; it must be green.
npm run typecheck     # tsc --noEmit
npm test              # node --test, the files listed in package.json
npm run dev           # start the panel (same as: node src/cli.ts)
npm run doctor        # diagnose profile, pack, vault and API key
npm run agenda        # what is due
```

Notes:

- **A new test file must be added to the `test` script in `package.json`** by hand;
  it lists files explicitly rather than globbing. Forgetting this means the tests
  silently never run.
- Runtime is Node ≥ 22.18 with native TypeScript execution: imports carry the `.ts`
  extension, there is no build step for either the CLI or the panel.
- The panel's JavaScript is served as-is and is not bundled: a syntax error breaks
  the whole panel. `node --check src/web/public/app.js` is a fast sanity check.
- This is developed on Windows; `pwsh` is the shell. The vault defaults to
  `~/.vn-finance` (`%USERPROFILE%\.vn-finance`), and a vault must never live inside
  this repository — the store refuses that outright. The folder actually in use is
  remembered in `~/.vn-finance/vault-location.json`; precedence is
  `--vault`/`--data-dir` → `VN_FINANCE_DATA_DIR` → that pointer → the default. Tests
  pass `vaultPointerFile` so they never write to the real home directory.

## Conventions

- ESM only, `import type` for types, `.ts` extensions in specifiers.
- Prefer small pure functions in `src/core/` over logic in handlers. If a handler
  grows an `if` about tax, it belongs in the core.
- Comments explain *why*, in English, and the codebase's tone is deliberate: state
  the trade-off, including the parts that are not built yet.
- Validation happens on the wire (server) **and** in the domain (core). The server
  checks shapes TypeScript cannot see; the core checks meaning.
- Tests are `node:test` + `node:assert/strict`, named as full sentences describing
  the behaviour, with Portuguese assertions about Portuguese data.
- Commits: one concern per commit, imperative summary with a conventional prefix
  (`feat(web):`, `fix(core):`, `docs:`, `chore:`).

## Before you say it is done

- [ ] `npm run verify` is green (typecheck + all tests).
- [ ] New behaviour is reachable from the panel, or the omission is deliberate and
      written down.
- [ ] Any new test file is in the `package.json` test list.
- [ ] `README.md` counts and claims still match reality (test counts, panel
      features), and `ARCHITECTURE.md` still describes the code.
- [ ] No personal data, key material or vault content is committed; the vault lives
      outside the repo.
