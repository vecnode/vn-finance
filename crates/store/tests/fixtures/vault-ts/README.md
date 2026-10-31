# A vault written by the TypeScript application

This folder was produced by `node tools/golden/vault.ts`, which calls the
**reference implementation's own `Vault` class** — not a hand-written JSON
imitation. `crates/store/tests/legacy_vault.rs` opens it and reads it, which is
how the port proves that a folder somebody has been using for a year opens in
the new application unchanged.

Every value is invented: an example name, an example NIF that happens to satisfy
the check digit, two example invoices, and a PDF that is three lines of ASCII.
Nothing here is or was anybody's data. The timestamps are the ones the
generator ran at, which is why the Rust test asserts structure rather than
equality on them.

The archived document is `documents/283ced137973-fatura-recibo-2026-1.pdf`.
