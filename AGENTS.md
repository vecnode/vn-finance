# AGENTS.md — working in this repository

Orientation for anyone (human or model) editing `vn-finance`. Read this first, then
`ARCHITECTURE.md` for the layer map. `README.md` is the user-facing document;
`docs/` holds the domain, privacy and port material.

## What this is

A local-first personal tax assistant for independent workers in Portugal (CIRS
categoria B, *recibos verdes*). One folder of data on one machine, a deterministic
core that computes obligations and estimates, and one interface over it: a desktop
window. It is **not** an accountant and never files anything.

It was a TypeScript program with a browser panel and a command line until the port
recorded in `docs/PORT-TO-RUST.md`. That implementation is in the git history; the
golden fixtures under `crates/core/tests/golden/` were generated from it and are the
reason the Rust core answers the same question the same way.

## The one rule that shapes everything else

**A feature is not finished until the window can do it.**

There is no second surface any more, so this is simpler than it was — but it is still
the rule that decides where code goes. Concretely:

1. put the computation in `crates/core/` (deterministic, no I/O, no clock);
2. carry it on the view model in `crates/store/src/model.rs`;
3. render it in `crates/ui/src/pages/`;
4. test the core, and test that the page draws (`crates/ui/tests/render.rs`).

Never put a number in a page. A rate written as a literal in `crates/ui/` is a bug:
it has happened once (the invoice form defaulted the IRS withholding to 25 %) and it
put a wrong figure beside a correct one.

**The panel is a set of pages**, one per subject, in `Page::ALL`. A new page is a
module, an entry in that list, and a branch in `App::content` — nothing else. New
capability belongs on the page of its subject, not appended to the bottom of the
dashboard.

## Hard invariants — do not break these

- **No network code, at all.** There is no HTTP client in the dependency tree. The
  no-socket promise is checked twice: a source audit over the crates that link into
  the binary, and a lockfile audit for the crates that would add one.
- **No `unsafe`.** `unsafe_code = "forbid"`, plus a source audit.
- **No tax logic in the interface.** `crates/ui/**` may format and lay out; every
  number, threshold and deadline comes from `crates/core/` through the single view
  model.
- **The rule pack is the only place Portuguese tax law is written down.** Rates,
  coefficients and ceilings are read from `rules/pt/2026.json`. A missing value means
  the calculation is *refused*, never defaulted.
- **No guessing personal inputs.** The NIF, the declared IVA regime, turnover and
  eligible expenses are declarations. If they are absent, say which check cannot run
  instead of assuming a value.
- **The interface cannot write.** A page returns an `Action`; `App::perform` carries
  it out through the session, and the model is rebuilt. No widget touches the vault.
- **Append-only records.** Invoices, completions and audit events are appended to
  JSONL and never rewritten. Writes that replace a file go through `store::atomic`
  (unique temporary name, `sync_all`, rename, directory sync).
- **Money is integer cents, rates are integer basis points, dates are ISO
  `YYYY-MM-DD` strings.** Never floats for money.
- **UI strings are European Portuguese, with correct accents.** `README.md` — the
  user-facing document — is in European Portuguese too. Code comments and the
  developer documents (`ARCHITECTURE.md`, this file, `docs/PORT-TO-RUST.md`) are in
  English. A mojibake audit fails the build on a mangled accent.
- **The vault never lives inside this repository.** The store refuses it outright,
  and there is a test for that refusal.

## Repo map

| Path | What lives there |
| --- | --- |
| `crates/core/` | The domain: money, dates, calendar, conditions, rules, profile, estimates, flags, NIF, the PDF reader and the `fatura-recibo` parser |
| `crates/core/tests/golden/` | Parity fixtures generated from the TypeScript implementation, and the tests that assert them |
| `crates/store/` | The vault: locations, atomic writes, append-only ledgers, the document index, the lock, the view model |
| `crates/ui/` | The window: theme, widgets, icons, the ten pages, the profile form, the receipt flow |
| `apps/vnfin/` | The binary |
| `xtask/` | `cargo xtask ci` — the gate |
| `rules/pt/2026.json` | The versioned rule pack: constants, obligations, sources, verification status |
| `design/` | The reviewed mockup the window's structure and colour were ported from |
| `scripts/` | Launchers: `scripts/run.bat` builds the release and opens the window |
| `docs/` | Domain reference, privacy model, the port plan |

## Branches

- **`main`** is what ships. A push to it runs CI; a tag `v*` on it runs the release
  workflow and publishes the three artefacts.
- **`dev`** is where work happens. **A push to `dev` runs no CI** — the `on.push`
  trigger names `main` only — so a work-in-progress commit costs nothing. Opening a
  pull request from `dev` to `main` runs the full three-platform matrix, which is
  where the gate is enforced.
- A release is cut by merging `dev` into `main`, seeing CI green, and tagging.

## Commands

```bash
cargo xtask ci        # the gate: fmt, clippy -D warnings, all tests, three audits
cargo run --release   # build and open the window
cargo build --release # build it without running  ->  target/release/vnfin
cargo test --workspace                                    # the tests alone
cargo run --release --package xtask -- notices            # regenerate THIRD-PARTY-NOTICES.md
```

`cargo xtask ci` is the gate and it is green before anything is committed.

Notes:

- **A new test file is picked up automatically** (unlike the TypeScript suite, which
  listed its files by hand).
- Rust 1.95 or newer; the toolchain file pins it.
- This is developed on Windows; `pwsh` is the shell. The vault defaults to
  `~/.vn-finance` (`%USERPROFILE%\.vn-finance`), and tests use `Env::for_test` so they
  never write to the real home directory.
- **Never edit source with `Get-Content -Raw | Set-Content` or a bare
  `[System.IO.File]::WriteAllText` in this shell.** Windows PowerShell reads UTF-8 as
  the ANSI code page and writes it back double-encoded, turning `exercício` into
  `exercÃ­cio`; a failed expression in that idiom once truncated three files to zero
  bytes. Use the edit tool, which round-trips UTF-8.

## Conventions

- Prefer small pure functions in `crates/core/` over logic in a page. If a page grows
  an `if` about tax, it belongs in the core.
- Comments explain *why*, in English, and the tone is deliberate: state the trade-off,
  including the parts that are not built yet.
- Validation happens in two places: the form catches what a text field makes possible
  (a bad check digit, a date that is not one), and the core catches what only the
  domain knows (an activity that started in the future). Neither is redundant.
- Tests are named as full sentences describing the behaviour, with Portuguese
  assertions about Portuguese data.
- Commits: one concern per commit, imperative summary with a conventional prefix
  (`feat(core):`, `fix(ui):`, `docs:`, `chore:`).

## Before you say it is done

- [ ] `cargo xtask ci` is green.
- [ ] New behaviour is reachable from the window, or the omission is deliberate and
      written down.
- [ ] `README.md` counts and claims still match reality, and `ARCHITECTURE.md` still
      describes the code.
- [ ] `THIRD-PARTY-NOTICES.md` is regenerated if a dependency changed.
- [ ] No personal data, key material or vault content is committed; the vault lives
      outside the repo.
