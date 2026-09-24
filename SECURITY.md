# Security policy

This is a local-first, single-user application. There is no server of ours to
breach, no account system, and no infrastructure belonging to this project. Almost
everything that could go wrong with it is either your own machine's problem or a
bug in the code you are running — which is why the interesting report is a bug.

If you want the threat model rather than the reporting process, it is in
[`docs/PRIVACY-SECURITY.md`](docs/PRIVACY-SECURITY.md): the trust boundaries, what
the loopback panel defends against and why, what is deliberately not protected in
v0, the residual risk, and how to check every claim against the repository.

## Reporting a vulnerability

**Use GitHub's private vulnerability reporting**: the *Security* tab of this
repository → *Report a vulnerability*. That opens an advisory only you and the
maintainer can see. Please do not open a public issue for anything exploitable, and
do not put a working exploit in a public issue.

If private reporting is not available to you, open an issue saying only that you
have a security report and would like a private channel, with no details in it.

What makes a report easy to act on:

- the commit or version you tested (`git rev-parse HEAD` for the code,
  `node src/cli.ts doctor` for the rule-pack version);
- your operating system and Node version;
- what you did, what you expected, and what happened;
- your reading of the impact — in particular **whether anything outside the vault
  became reachable**, and whether the API key or the pseudonym map was involved.

What to expect: an acknowledgement, an honest assessment, and either a fix or a
decision not to fix with the reasoning written down. That assessment may well be
"this is not a vulnerability, and here is why" — for a single-user local tool, that
is a normal outcome rather than a dismissal. This is a single-maintainer project
with no bug bounty and no response-time guarantee. If that matters to you, treat it
as the constraint it is rather than a promise someone forgot to make.

## Supported versions

Pre-1.0. Only the latest commit on `main` is supported: there are no maintenance
branches and no backports.

Two things are versioned separately from the code:

- **The rule pack** (`src/rules/pt/<year>.<major>.<patch>.json`) is data, not code.
  A wrong rate or a wrong deadline is a *correctness* bug, not a security one. The
  pack marks every rule `verified`, `partial` or `unverified` and keeps its own
  list of gaps, so report it as an ordinary issue and cite the source you are
  reading.
- **Your vault** has no version. It is JSON and JSONL, and it is meant to outlive
  the code.

## In scope

Things that would be vulnerabilities here, each next to the control that is
supposed to prevent it:

| Report | The control that is supposed to hold |
| --- | --- |
| A page on another origin can read a response from the panel, or successfully POST to it | No CORS headers, no cookie, and the session token must arrive as a header a browser will not set cross-origin |
| A route reached with a `Host` that is not the loopback name the server started with | The `Host` allowlist, which answers 403 before any route sees the request |
| Reading a file outside `public/` through the static handler, or outside the vault through the document routes — including percent-encoded and symlinked variants | Every path is resolved and required to stay inside its own base directory |
| A crafted document name that escapes the documents directory or overwrites an unrelated file | The name is reduced to its last path segment, sanitised, and prefixed with the SHA-256 of the content itself |
| Script execution in the panel, including through a client name, an invoice description or a file name that came from the vault | Strict CSP with no inline script or style, and a front end that writes text rather than HTML |
| The session token disclosed anywhere beyond the documented moment it sits in the URL | Random per run, stripped from the address bar immediately, worthless once the process exits |
| A request to the DeepSeek API carrying an identifier the redaction gateway should have removed | `src/ai/redact.ts`, the residual scan that fails closed, and the per-request approval step |
| The API key returned to the browser, written to the audit log, or sent anywhere but loopback and the provider you configured | The model carries a mask, the key is never logged, and it is held in memory unless you supply a passphrase to encrypt it |
| A record under `ledger/`, `obligations/` or `audit/` rewritten or silently dropped, or a document whose bytes no longer match its index entry | Append-only JSONL, hash-addressed documents, and a hash check before an archived document is trusted |
| A runtime dependency appearing in `package.json`, or code fetched at install time | Zero runtime dependencies is a security property here, not a style preference |
| The vault guard bypassed, so a data directory inside a git work tree is accepted | `src/store/vault.ts` refuses one outright |

## Out of scope

Stated as decisions rather than as oversights:

- **The vault is not encrypted at rest in v0.** This is the top residual risk, it is
  written down in [`docs/PRIVACY-SECURITY.md`](docs/PRIVACY-SECURITY.md) §3, and it
  is scheduled. Full-disk encryption is the control that matters today. "I read the
  vault's files" is therefore expected behaviour, not a vulnerability.
- **Anything already running as your user.** There is no privilege boundary inside a
  single-user application, and this project does not pretend there is one.
- **Physical access, a stolen unlocked device, malware, or a leaked backup.**
- **The API key placed in an environment variable on a shared machine.**
- **A wrong tax figure, a missing rule, or an unverified rule that turns out to be
  wrong.** That is a real bug and worth reporting — as an ordinary issue, with the
  source — but it is a correctness problem, not a vulnerability. The pack publishes
  its own confidence level, and the application never files anything.
- **The static mockup in `design/`**, which is a design artefact and is not served
  by anything.
- **Social engineering, and the risk that someone trusts this further than they
  should.** It is not an accountant: it prepares and explains, and nothing here
  submits anything to the AT or to a Segurança Social portal.

## Disclosure

Coordinated in the ordinary way. Report privately, allow a reasonable window to
understand the problem and ship a fix, and then either of us can write about it. The
repository is public and `main` is the release, so a fix becomes visible as soon as
it is pushed. There is no embargo process, no security advisory mailing list, and no
CVE process to join.
