# Security policy

This is a local-first, single-user application. There is no server of ours to
breach, no account system, and no infrastructure belonging to this project. Almost
everything that could go wrong with it is either your own machine's problem or a
bug in the code you are running — which is why the interesting report is a bug.

If you want the threat model rather than the reporting process, it is in
[`docs/PRIVACY-SECURITY.md`](docs/PRIVACY-SECURITY.md): the trust boundaries, what
is deliberately not protected, the residual risk, and how to check every claim
against the repository.

## The one promise, and how to check it

**The application has no network code.** There is no HTTP client, no TLS stack and
no async runtime in the dependency tree. Two checks enforce it, both in
`cargo xtask ci`:

- a scan of every source file in the crates that link into the binary, for HTTP
  APIs and socket types;
- a scan of `Cargo.lock` for the crates that would add one (`reqwest`, `hyper`,
  `rustls`, `native-tls`, `openssl`, `ureq`, `curl`, `isahc`, `surf`, `awc`).

Neither check can prove the absence of a network *syscall* — a crate could reach one
through `libc`. What they do guarantee is that adding one would be a visible,
deliberate change to this repository rather than an accident of a dependency.

## Reporting a vulnerability

**Use GitHub's private vulnerability reporting**: the *Security* tab of this
repository → *Report a vulnerability*. That opens an advisory only you and the
maintainer can see. Please do not open a public issue for anything exploitable, and
do not put a working exploit in a public issue.

If private reporting is not available to you, open an issue saying only that you
have a security report and would like a private channel, with no details in it.

What makes a report easy to act on:

- the version you tested (`vnfin` shows it on the Diagnóstico page, and
  `git rev-parse HEAD` for the commit), and the rule-pack version it reported;
- your operating system;
- what you did, what you expected, and what happened;
- your reading of the impact — in particular **whether anything outside the vault
  became reachable**.

What to expect: an acknowledgement, an honest assessment, and either a fix or a
decision not to fix with the reasoning written down. That assessment may well be
"this is not a vulnerability, and here is why" — for a single-user local tool, that
is a normal outcome rather than a dismissal. This is a single-maintainer project
with no bug bounty and no response-time guarantee. If that matters to you, treat it
as the constraint it is rather than a promise someone forgot to make.

## What an attacker would have to do

The interesting question for this application is not "is there a remote attack
surface" — there is none — but "what would it take to read the vault":

1. **Read the files on disk.** The vault is plain JSON and PDF, not encrypted at
   rest. Your disk's encryption (FileVault, BitLocker, LUKS) and your file
   permissions are the control; the application adds an advisory lock that stops a
   second *window* from writing, and nothing more. Do not run it from a shared
   account, and do not put the vault on a network share.
2. **Get a document in front of the PDF reader.** The reader is the only component
   that parses input the person did not type, and it is written to refuse rather
   than guess: a compressed-object layout, an encrypted file, a font with no
   character map and a stream that will not inflate are all reported as problems
   rather than half-read. It has no code path that writes, executes or fetches
   anything. A malformed PDF that makes it panic is a bug worth reporting: the
   window would close, and the vault would be untouched.
3. **Convince the application that a `fatura-recibo` is yours.** It will only record
   an invoice whose issuer NIF matches the profile's, and it refuses one whose
   customer NIF is the profile's — that is somebody else's invoice, and recording it
   as income would inflate every estimate that follows. A document that identifies
   neither party is recorded only with a warning, which the form shows.

## What is deliberately not protected

- **No encryption at rest.** Stated in `README.md` and in `docs/PRIVACY-SECURITY.md`;
  it is the first thing to add if this ever supports a shared machine.
- **No integrity protection of the vault.** The document index carries a SHA-256 of
  each archived file, so a *document* that changed is detectable, but `profile.json`
  and the ledgers have no signature: somebody with write access to the folder can
  edit them, and the application will believe them. That is the same threat as
  editing the files with a text editor.
- **No sandboxing.** The binary is an ordinary desktop application. If it is run as
  your user, it can read what your user can read.

## Supported versions

Pre-1.0. Only the latest commit on `main` is supported: there are no maintenance
branches and no backports.

Two things are versioned separately from the code:

- **the rule pack** (`rules/pt/2026.json`), which carries its own `packVersion`,
  checksum and per-value provenance, and shows on the Regras & fontes page;
- **the vault format**, which is the file layout under `~/.vn-finance`. It has not
  changed across the port from TypeScript to Rust, and a folder from the older
  implementation opens in this one unchanged.

## Dependency hygiene

`THIRD-PARTY-NOTICES.md` is generated from `cargo metadata` by `cargo xtask notices`,
and `cargo xtask ci` fails if it has drifted from the tree. Every crate in it is one
the build actually resolves to, all pure Rust: the single C dependency the
JavaScript implementation had (zlib, for PDF streams) was replaced by
`miniz_oxide`.
