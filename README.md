# 📊 vn-finance

[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Node.js 22.18 or newer](https://img.shields.io/badge/node-%E2%89%A522.18-339933?logo=node.js&logoColor=white)](https://nodejs.org/)
[![TypeScript 5.9](https://img.shields.io/badge/TypeScript-5.9-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)

A private, local-first assistant for the Portuguese tax obligations of a
self-employed professional, focus on CIRS category B.


### What you need

- **Node 22.18 or newer** (`node --version`). The CLI runs TypeScript directly
  through Node's type stripping: no build step, no bundler, no runtime dependency.
- A browser, for the local panel.
- **Full-disk encryption, if you have it.** The vault is not encrypted at rest yet
  (see `docs/PRIVACY-SECURITY.md` §3), so the operating system's disk encryption is
  the control that protects it today.

### 1. Install and Dev

```bash
git clone <this repository> vn-finance
cd vn-finance
npm install          # only TypeScript and @types/node, for development
npm run verify       # strict typecheck + 164 tests
```

`npm run verify` is the gate. It should end with `# pass 164` and `# fail 0`.

```bash
node src/cli.ts          # or: npm run dev — opens the local panel in your browser
```

```bash
node src/cli.ts doctor
```

```bash
node src/cli.ts agenda                  # next 90 days, then the rest of the year
node src/cli.ts agenda --horizon 180    # a wider plan
node src/cli.ts agenda --all            # include completed, N/A and history
node src/cli.ts agenda --json           # machine readable
```

```bash
node src/cli.ts flags
node src/cli.ts flags --json
```

```bash
node src/cli.ts ledger add --base 1200 --client "ACME, Lda." --nif 501234560
node src/cli.ts ledger add --base 4200 --client "Helsinki Labs Oy" --country FI
node src/cli.ts ledger add --base 900 --client "Studio Mira" --retention 0 --paid
node src/cli.ts ledger list
```

```bash
node src/cli.ts ledger import ~/Downloads/fatura-recibo-2026-014.pdf
node src/cli.ts ledger import fatura.pdf --dry-run   # read it, write nothing
```

```bash
node src/cli.ts estimate                                  # per quarter + reserve
node src/cli.ts estimate --quarter 3                      # one quarter
node src/cli.ts estimate --despesas 5000                  # + the IRS tax base
```

```bash
node src/cli.ts vault add ~/Documents/comprovativo-iva-t3.pdf \
  --kind comprovativo --obligation iva.dp.trimestral
node src/cli.ts vault list
```

## Where the data lives

Default: `~/.vn-finance` (`%USERPROFILE%\.vn-finance` on Windows).

The folder actually in use is decided in this order: `--vault <dir>` or
`--data-dir <dir>` on the command line, then `VN_FINANCE_DATA_DIR`, then the folder
you chose once and the application remembered, then the default above. The
remembered choice lives in `~/.vn-finance/vault-location.json`, never inside a vault:
a file that says where the vault is cannot live in the place you would have to find
first.

Change it from the panel: *Cofre de documentos* → *Mudar de pasta…*. Or from the
command line:

```bash
node src/cli.ts vault where                       # which folder, and why that one
node src/cli.ts vault set "C:\Users\eu\Documents\vn-finance"
```

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

Architecture, and the AI boundary in detail:

![Architecture](dsh-resource://diagram/library/vn-finance-architecture)

![AI privacy flow](dsh-resource://diagram/library/vn-finance-ai-privacy-flow)

## Licence

MIT. See `LICENSE`.
