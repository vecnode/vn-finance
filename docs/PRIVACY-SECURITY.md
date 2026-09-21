# Privacy and security

A tool that holds one person's complete professional financial life — every
client, every invoice, every document — has a different risk profile from a
generic SaaS product. There is no server to breach, and that is the point. But
"local" is not automatically "safe", and this document says exactly what is
protected, what is not, and what the user has to bring.

---

## 1. What is being protected

| Asset | Why it matters |
| --- | --- |
| The invoice ledger | Names every client, every amount, the whole commercial relationship |
| Archived documents (PDF invoices, guides, contracts) | Often more identifying than the ledger: addresses, IBANs, signatures |
| The taxpayer profile | NIF, name, CAE, regime, dates |
| The DeepSeek API key | A billable credential; leakage costs money and, if the vault is nearby, context |
| The pseudonym map | The mapping from `CLIENTE_1` back to a real client is itself identifying |

## 2. Trust boundaries

![AI privacy flow](dsh-resource://diagram/library/vn-finance-ai-privacy-flow)

Two boundaries exist, and only two:

**Boundary A — the machine.** Everything inside the vault is trusted. Files are
readable by anything running as the user; there is no privilege separation inside
a single-user application, and pretending otherwise would be theatre.

**Boundary B — the network.** Exactly one function in the codebase crosses it:
`DeepSeekClient.complete()`. `grep -rn "await fetch(" src/` returns one hit, and
that one is worth a reviewer's minute. There is no telemetry, no update check, no
crash reporter, no font CDN, no analytics.

### The local panel: a port on your machine

`vnfin web` exposes the whole vault over HTTP on loopback. "It is only loopback" is
not by itself a security argument, so the panel is built against the specific ways a
local server goes wrong:

| Attack | Why it would work without a control | What stops it |
| --- | --- | --- |
| Another process running as you | Anything with your file permissions can read the vault directly | Nothing, and nothing can: there is no privilege boundary inside a single-user application. Full-disk encryption is the control that matters here. |
| A website you happen to visit | Browsers will send a request to `127.0.0.1` quite happily | It cannot read the answer (no CORS headers, so the response is opaque to it) and cannot write one (the token must arrive as a header, which the browser will not let it set without a preflight this server refuses). |
| DNS rebinding | An attacker's hostname resolves to `127.0.0.1`, so the browser sends *that* hostname in the `Host` header | The server validates `Host` against the loopback names it was started with and answers 403 otherwise. The request never reaches a route. |
| A leaked or guessed session token | The token is in the URL, so it can reach browser history, a screenshot or a shared screen | It is random per run, the page strips it from the address bar immediately, and it is worthless once the process exits. |
| Path traversal | `GET /../package.json`-style requests | Every path is resolved and must stay inside `public/`; anything else is 403. Tested, including percent-encoded variants. |
| Injected script | The panel renders values that came from the vault, including client names | A strict CSP allows no inline script, no inline style and no external resource, so an injected `<script>` cannot run, and the front end writes text rather than HTML. |
| Cross-site request forgery | A form on another page could POST to loopback | The token header cannot be set cross-origin, and with no CORS there is no preflight to approve. There is no cookie, so there is no session to ride. |
| A hostile file upload | The panel accepts bytes from the browser, and a file name is attacker-controlled input | The body cap (32 MiB) is enforced while reading, so an oversized upload is refused rather than buffered; the name is reduced to its last path segment and sanitised before it is joined to the vault directory; the stored name is the file's own SHA-256 plus that name, so content cannot masquerade as another document. |
| A stolen API key through the panel | The key travels from the browser to the local server | It crosses loopback only, behind the session token, is never returned to the browser (the model carries a mask), is never written to the audit log, and is held in process memory unless the user supplies a passphrase to encrypt it into the vault. |

What the panel explicitly does **not** do: it never binds to `0.0.0.0`, it opens no
port on your network, it sends no CORS headers, it sets no cookie, and it fetches
nothing from outside. These guards, and the writes the panel performs, are covered by
tests rather than the happy path alone (`src/web/server.test.ts`,
`src/web/profile-api.test.ts`).

The residual risk is the one stated in the next section: the vault itself is not
encrypted at rest, so the panel changes the *reachability* of your data, not its
protection on disk.

## 3. What is not protected yet (stated plainly)

**The vault is not encrypted at rest in v0.** The files under the data directory
are plain JSON and JSONL; their confidentiality rests on the operating system's
own controls — user account isolation and, where enabled, full-disk encryption
(BitLocker/FileVault/LUKS). Anyone who can read files as your user can read the
vault, and anyone who walks off with an unencrypted disk can read all of it.

This is a deliberate v0 trade-off: the core is being built and reviewed, and a
half-implemented key-management scheme would be worse than an honest absence.
M6 in the roadmap adds OS-level encryption at rest with auto-lock. Until then,
**enable full-disk encryption on the machine that holds the vault** — that
recommendation is the actual security control today.

Two things v0 does not have, and does not claim: the API key file is encrypted
(§6), and documents are hash-addressed so tampering is detectable (§7).

## 4. Third parties and the dependency tree

**Zero runtime dependencies.** The application ships no third-party code at
runtime: no npm packages, no transitive tree, so an upstream compromise cannot
reach the vault. TypeScript and `@types/node` are development-only. This is a
security property, not a stylistic preference, and it is enforced by
`package.json` having no `dependencies` block at all.

The only third party that ever sees anything is the AI provider, and only when the
user approves a specific request (§5).

## 5. The AI boundary

Full rationale is in `docs/PROPOSAL.md` §8. The security-relevant facts, in order
of strength:

**1. The assistant never receives your data — by construction, not by filtering.**
The only request the application can build is a rule-update request, and
`buildUpdateMessages` takes a rule pack and nothing else. There is no code path
that places a profile, an invoice, a client name, a turnover figure or an alert
into a model request. The payload is a list of public-law variable names, their
current values from the pack, and the official URLs the pack already cites. You can
read every byte of it with `vnfin update`, no flag needed, before anything is sent.

**2. Redaction is still in the path, as defence in depth.**
`DeepSeekClient.complete()` accepts only a `RedactedPayload`, whose single producer
is `redactForSend()`. Sending raw text is a compile error, not a code-review
question. That gateway is now the second line rather than the first: even a future
change that tried to put user data into a request would have to defeat the payload
type and the residual re-scan as well.

**3. The assistant cannot make anything true.**
An applied proposal is recorded with `source: 'ai-proposed'` and
`status: 'unverified'`, surfaced by `vnfin doctor` and by an alert, until a person
confirms it against the cited source. The application has no path by which a model
output becomes a verified rule value.

**4. The answer must be data, and only data.**
`parseUpdateResponse` requires JSON, refuses a fenced or prose-wrapped answer whole,
rejects values whose unit or magnitude is implausible, and ignores any variable that
was not asked about. A model cannot reach you with prose through this path, and
cannot add a variable to the pack.

**5. Every send is explicit and audited.**
`update` is a dry run unless `--send` is present, and `--approve` is a separate,
later decision. Every proposal and every application is written to the append-only
audit log. There is no "trust this app" switch that disables any of it.

**6. Model output is never a source.**
The prompt asks for a source URL, and it is stored as a *lead* for a human, not as
provenance. Nothing the assistant returns is recorded as `verified` without a person
asserting it.

### What is never sent, and what is never contacted

- **Never sent:** profile, NIF, invoices, clients, amounts, turnover, documents,
  alerts, obligations, completions.
- **Never contacted:** the Portal das Finanças, Segurança Social Direta, VIES or any
  other public body. No login, no scraping, no browser automation, no webservice,
  no read-only integration. The rule pack cites where an obligation is fulfilled as
  a reference for you; the application does not go there.
- **Never synced:** there is no sync of any kind. One machine, one folder.

## 6. The API key

| Aspect | v0 behaviour |
| --- | --- |
| Sources, in order | `--key` flag → `DEEPSEEK_API_KEY` environment variable → key held by the running panel (session) → encrypted file in the vault |
| Typed in the panel | Sent to the local server over loopback with the session token; kept in the server process's memory, and written to the vault only if a passphrase is supplied with it |
| Back to the browser | Never. Only a mask (`sk-a…f9c2`) and the source are part of the view model |
| Encrypted file | AES-256-GCM; key derived with scrypt (N=2¹⁵, r=8, p=1, 32-byte key) from `VN_FINANCE_PASSPHRASE`, random 16-byte salt and 12-byte IV per file |
| Minimum passphrase | 12 characters, enforced (also for the panel's form) |
| Never written to | The ledger, the audit log, the profile, any backup of the ledger |
| Displayed as | `sk-a…f9c2`, never in full |
| Target (M6) | OS credential store: Windows Credential Manager/DPAPI, libsecret, macOS Keychain, via Tauri's keyring plugin |

The environment variable is the most convenient path and the least private one: on
a shared machine, environment variables leak through process listings and shell
history. The encrypted file is the better default; the credential store is the
right answer once the desktop shell exists. This ordering is stated in the code
and here rather than glossed over.

The panel adds one case that the command line cannot have, because only a long-lived
process can hold a secret without writing it down: a key typed into the browser can
live in the server's memory for the life of that run — nothing on disk, nothing in
the audit log longer than the fact that a key was set — and it is gone when the panel
stops. It is not a substitute for the encrypted file, and the panel says which of the
two it did.

## 7. Integrity of records

- **Append-only ledger.** Invoices, completions and audit events are appended to
  JSONL files, never rewritten. "What did I record, and when" survives, which
  matters when a figure is questioned a year later.
- **Hash-addressed documents.** `vnfin vault add` copies the file into the vault
  under the first 12 characters of its SHA-256, and the original is never moved or
  modified. Re-hashing the file proves it has not changed since it was archived. A
  document uploaded from the panel goes through the same one archive function, on the
  bytes the browser sent instead of a path.
- **Atomic writes.** Non-append writes go to a temporary file and are renamed, so
  an interrupted write cannot leave a half-written profile.
- **Copy before index.** A document is written into the vault before the index
  entry that references it, so the index can never point at a file that is not
  there.

## 8. Threats and residual risk

| Threat | Mitigation | Residual risk |
| --- | --- | --- |
| Malware or another process running as the user reads the vault | None in v0; OS account isolation only | **High** until M6 encryption; full-disk encryption is the current control |
| Device theft or loss | Full-disk encryption (user-supplied) | Medium — depends on the user enabling it |
| A dependency is compromised | Zero runtime dependencies | Very low |
| An identifier leaks to the AI provider | Redaction + residual scan + per-request approval + audit | Low — depends on the patterns being right; the residual scan makes a missed pattern fail closed |
| The API key leaks | Encrypted at rest, masked in output, never logged, held in memory only for the life of a panel run unless saved | Low; rises if the key is put in an environment variable on a shared machine |
| The wrong obligation is trusted | Verification flags, discrepancy reporting, provisional marking, golden test | Low, and the failure is visible rather than silent |
| A backup leaks | Backups are the vault; same controls apply | Medium until encrypted export (M3) |
| The tool is trusted too much | Disclaimers in pack, CLI and UI; nothing is filed | Medium — a social risk, mitigated by design honesty rather than technology |

The risk that is *not* on this list, deliberately: there is no risk of a server-side
breach, because there is no server. That is the main reason this project is built
local-first.

## 9. Data protection notes (RGPD)

- The application processes the **user's own** personal data, on the user's own
  device, for the user's own tax obligations. There is no controller-processor
  relationship with this project: no data is transmitted to its authors, and there
  is no infrastructure to transmit it to.
- The only transfer to a third party is the DeepSeek request, initiated by the
  user for a specific question, with identifiers removed before it leaves. The
  user is the one who decides whether that transfer happens.
- Declarations to the AT and Segurança Social remain the user's own acts, done on
  the official portals. The application prepares and explains; it does not submit.
- **Erasure** is deleting one folder. **Portability** is the folder itself, in
  JSON and JSONL, with no proprietary format. **Access** is reading the folder.
  These are properties of the architecture rather than features that have to be
  built.

## 10. How to check these claims

The claims in this document are all checkable against the repository:

```bash
grep -rn "await fetch(" src/    # exactly one hit, in src/ai/deepseek.ts
grep -n "dependencies" package.json   # no runtime dependencies
npm run verify                  # typecheck + 91 tests, including the panel guards
node src/cli.ts update          # lists the rule variables; sends nothing
node src/cli.ts vault list      # what is in the vault, with hashes
node src/cli.ts doctor          # what the tool knows and what is missing
node src/cli.ts web             # and then check the response headers on 127.0.0.1
```

A security document that cannot be checked is a marketing document.
