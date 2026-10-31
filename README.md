<div align="center">

# vn-finance

**Um assistente fiscal local para quem trabalha por conta própria.**

[![Licença: AGPL-3.0-only](https://img.shields.io/badge/licen%C3%A7a-AGPL--3.0--only-3fb950.svg)](LICENSE)
[![Rust 1.95+](https://img.shields.io/badge/rust-1.95%2B-dea584.svg?logo=rust&logoColor=white)](#construir)
[![Edição 2024](https://img.shields.io/badge/edi%C3%A7%C3%A3o-2024-dea584.svg)](#construir)
[![egui 0.37](https://img.shields.io/badge/interface-egui%200.37-8b5cf6.svg)](https://github.com/emilk/egui)
[![Plataformas](https://img.shields.io/badge/plataformas-Windows%20%7C%20macOS%20%7C%20Linux-4c8dff.svg)](#instala%C3%A7%C3%A3o)

</div>

Focado no trabalhador independente em Portugal. 
Serve para quem emite *recibos verdes* em Portugal: monta o calendário das
obrigações do ano, calcula IVA, retenções e contribuições a partir do que
faturavas, lê as faturas em PDF, e diz-te quanto deves ter de lado para o IRS.

Não substitui um contabilista certificado e nunca entrega nada por ti.

## Construir

O espaço de trabalho usa um único conjunto de comandos:

```bash
cargo run --release              # compilar e abrir a janela
cargo build --release            # compilar sem abrir  ->  target/release/vnfin.exe

cargo xtask ci                   # formatação, lints e testes: exatamente o que a CI corre
cargo xtask fmt-fix              # aplicar a formatação
cargo xtask lint                 # clippy sobre todo o espaço de trabalho, avisos como erros
cargo xtask test                 # a bateria de testes
```

Para testar a versão de distribuição no Windows, sem instalador nenhum:

```powershell
cargo build --release -p vnfin
.\target\release\vnfin.exe
```


## Segurança e licença

- **AGPL-3.0-only** — ver [`LICENSE`](LICENSE). O vn-finance é *copyleft*: podes
  executá-lo, estudá-lo, alterá-lo e partilhá-lo, e uma versão que distribuías ou
  serves por rede tem de levar o código-fonte com a mesma licença.
- A promessa de "sem rede" é verificável em dois sítios: uma varredura ao código das
  crates que entram no binário, e uma varredura ao `Cargo.lock` às crates que lhe
  dariam uma ligação. Ambas correm em `cargo xtask ci`.
- [`SECURITY.md`](SECURITY.md): o modelo de ameaças, o que um atacante teria de fazer,
  o que fica deliberadamente por proteger, e como reportar uma vulnerabilidade em
  privado.
- [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) é gerado a partir de
  `cargo metadata` e a CI falha se ficar desatualizado. Todas as crates são Rust puro:
  a única dependência em C que a versão JavaScript tinha (o zlib, para os fluxos dos
  PDFs) foi substituída por `miniz_oxide`.
