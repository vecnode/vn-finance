//! The repository's build harness.
//!
//! `cargo xtask ci` is exactly what CI runs, so a contributor and the runner
//! cannot disagree about what "checked" means. Beyond formatting, lints and tests
//! it enforces the two promises a compiler cannot check:
//!
//! - **the shipped application has no socket.** No source file under `crates/` or
//!   `apps/` mentions a networking API, and no crate in `Cargo.lock` is one that
//!   would give us a socket.
//! - **`unsafe_code` is forbidden in every crate we write**, `xtask` included.
//!   The lint table says so; this catches the `#[allow(unsafe_code)]` that would
//!   quietly undo it.
//!
//! It also scans the Rust sources for mojibake, because the interface is European
//! Portuguese and a mangled accent is a bug the compiler cannot see.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Substrings that mean the shipped application could open a connection.
const NETWORK: [&str; 12] = [
    "std::net",
    "TcpListener",
    "TcpStream",
    "UdpSocket",
    "reqwest",
    "hyper::",
    "rustls",
    "native_tls",
    "openssl",
    "ureq",
    "curl::",
    "tokio::net",
];

/// Crates that would put a socket in the binary.
const BANNED_CRATES: [&str; 11] = [
    "reqwest",
    "hyper",
    "rustls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "ureq",
    "curl",
    "curl-sys",
    "isahc",
    "surf",
];

/// What a UTF-8 accent looks like after somebody decoded it as cp1252.
const MOJIBAKE: [&str; 5] = [
    "\u{c3}\u{a3}",
    "\u{c3}\u{a7}",
    "\u{c3}\u{b5}",
    "\u{e2}\u{20ac}",
    "\u{fffd}",
];

/// Directories whose contents are linked into the shipped binary.
const SHIPPED: [&str; 2] = ["crates", "apps"];

/// Directories we wrote, whether or not they ship.
const OUR_CODE: [&str; 3] = ["crates", "apps", "xtask"];

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let task = arguments.next().unwrap_or_else(|| "ci".to_owned());
    let outcome = match task.as_str() {
        "ci" => ci(),
        "fmt-fix" => run("cargo", &["fmt", "--all"]),
        "lint" => lint(),
        "test" => run("cargo", &["test", "--workspace", "--locked"]),
        "notices" => notices(),
        "notices-check" => notices_check(),
        "rules" => rules(arguments),
        other => Err(format!(
            "unknown task `{other}`; try `ci`, `lint`, `test`, `fmt-fix`, `notices`, \
             `notices-check` or `rules`"
        )),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("xtask: {problem}");
            ExitCode::FAILURE
        }
    }
}

/// Everything CI runs, in the order a contributor wants to see a failure.
fn ci() -> Result<(), String> {
    run("cargo", &["fmt", "--all", "--check"])?;
    lint()?;
    run("cargo", &["test", "--workspace", "--locked"])?;
    audit_sources()?;
    audit_lockfile()?;
    notices_check()?;
    println!("xtask: ci passed");
    Ok(())
}

/// Clippy over every target, with warnings denied.
fn lint() -> Result<(), String> {
    run(
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
    )
}

/// Run one program and fail with its name if it does not succeed.
fn run(program: &str, arguments: &[&str]) -> Result<(), String> {
    println!("$ {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed ({status})", arguments.join(" ")))
    }
}

/// `cargo xtask rules <scaffold|diff>`: next year's pack, and what changed.
///
/// Both are bookkeeping a person would otherwise do by hand and with no record. The
/// domain owns the two answers — [`vnf_core::rollover`] — so this parses arguments,
/// reads and writes files, and prints what it did; the rules about *which* dates carry
/// forward live in one place with tests around them.
fn rules(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let arguments: Vec<String> = arguments.collect();
    match arguments.first().map(String::as_str) {
        Some("scaffold") => rules_scaffold(&arguments[1..]),
        Some("diff") => rules_diff(&arguments[1..]),
        Some(other) => Err(format!(
            "unknown `rules` action `{other}`; try `rules scaffold` or `rules diff`"
        )),
        None => Err(
            "`rules` needs an action: `rules scaffold --from 2026 --to 2027`, or `rules diff 2026 2027`"
                .to_owned(),
        ),
    }
}

/// The pack file for a year or a path.
fn pack_file(argument: &str) -> PathBuf {
    if argument.ends_with(".json") {
        PathBuf::from(argument)
    } else {
        PathBuf::from("rules").join("pt").join(format!("{argument}.json"))
    }
}

/// One flag's value, and the index after it.
fn flag_value(arguments: &[String], index: usize, name: &str) -> Result<String, String> {
    arguments
        .get(index + 1)
        .cloned()
        .ok_or_else(|| format!("{name} needs a value"))
}

/// A year from a flag.
fn flag_year(value: &str, name: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{name} expects a year, got `{value}`"))
}

/// Start next year's pack from this one, and validate it before writing.
///
/// Refusing to write a pack that does not load is the whole safety of this command: a
/// scaffolded file that the validator rejects would be discovered at the next
/// `cargo xtask ci`, in a commit that also contains the work of transcribing a
/// calendar.
fn rules_scaffold(arguments: &[String]) -> Result<(), String> {
    let mut from: Option<i64> = None;
    let mut to: Option<i64> = None;
    let mut on: Option<String> = None;
    let mut force = false;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--from" => {
                let value = flag_value(arguments, index, "--from")?;
                from = Some(flag_year(&value, "--from")?);
                index += 1;
            }
            "--to" => {
                let value = flag_value(arguments, index, "--to")?;
                to = Some(flag_year(&value, "--to")?);
                index += 1;
            }
            "--on" => {
                on = Some(flag_value(arguments, index, "--on")?);
                index += 1;
            }
            "--force" => force = true,
            other => return Err(format!("unknown argument `{other}` for `rules scaffold`")),
        }
        index += 1;
    }

    let from = from.ok_or("`rules scaffold` needs --from <year>")?;
    let to = to.ok_or("`rules scaffold` needs --to <year>")?;
    if to <= from {
        return Err(format!("--to {to} must be after --from {from}"));
    }
    let on = on.unwrap_or_else(today_utc);

    let source = pack_file(&from.to_string());
    let target = pack_file(&to.to_string());
    if target.exists() && !force {
        return Err(format!(
            "{} already exists; pass --force to replace it, or start from it with --from {}",
            target.display(),
            to
        ));
    }

    let text = std::fs::read_to_string(&source)
        .map_err(|error| format!("could not read {}: {error}", source.display()))?;
    let raw: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{} is not JSON: {error}", source.display()))?;
    let next = vnf_core::rollover::scaffold(&raw, to, &on)
        .map_err(|error| format!("the pack could not be scaffolded: {error}"))?;

    let rendered = serde_json::to_string_pretty(&next)
        .map_err(|error| format!("the scaffolded pack could not be written: {error}"))?;
    let rendered = format!("{rendered}\n");
    // Validate before writing: the file this command produces has to be one the
    // application would load, not one a person discovers at the next gate.
    let loaded = vnf_core::rules::load_rule_pack(&rendered, &target.display().to_string())
        .map_err(|error| format!("the scaffolded pack does not validate:\n{error}"))?;

    std::fs::write(&target, &rendered)
        .map_err(|error| format!("could not write {}: {error}", target.display()))?;

    println!(
        "xtask: wrote {} — {} rules, all stale, no published dates",
        target.display(),
        loaded.pack.obligations.len()
    );
    println!(
        "xtask: packVersion {}, generatedAt {on}",
        loaded.pack.pack_version
    );
    if let Some(note) = loaded.pack.todo.last() {
        println!("xtask: {note}");
    }
    println!(
        "xtask: next — transcribe the {to} calendar into it, re-check every constant, then \
         `cargo xtask rules diff {from} {to}`"
    );
    Ok(())
}

/// Print what differs between two packs.
fn rules_diff(arguments: &[String]) -> Result<(), String> {
    let [from_argument, to_argument] = arguments else {
        return Err("`rules diff` needs two packs: `rules diff 2026 2027`, or two paths".to_owned());
    };
    let from_path = pack_file(from_argument);
    let to_path = pack_file(to_argument);
    let from = vnf_core::rules::load_file(&from_path)
        .map_err(|error| format!("could not load {}: {error}", from_path.display()))?;
    let to = vnf_core::rules::load_file(&to_path)
        .map_err(|error| format!("could not load {}: {error}", to_path.display()))?;

    let report = vnf_core::rollover::diff(&from.pack, &to.pack);
    print_diff(&report);
    Ok(())
}

/// The diff, as a reviewer reads it.
fn print_diff(report: &vnf_core::rollover::PackDiff) {
    println!(
        "{} {} → {} {}",
        report.from_year, report.from_version, report.to_year, report.to_version
    );
    if report.is_empty() {
        println!("  nenhuma diferença de conteúdo.");
        return;
    }

    let group = |title: &str, lines: Vec<String>| {
        if lines.is_empty() {
            return;
        }
        println!("{title}");
        for line in lines {
            println!("  {line}");
        }
    };

    group(
        "constantes",
        report
            .constants
            .iter()
            .map(|change| {
                format!(
                    "~ {}: {} → {}",
                    change.path,
                    change.from.as_deref().unwrap_or("—"),
                    change.to.as_deref().unwrap_or("—")
                )
            })
            .collect(),
    );
    group(
        "obrigações",
        report
            .obligations_added
            .iter()
            .map(|id| format!("+ {id}"))
            .chain(report.obligations_removed.iter().map(|id| format!("− {id}")))
            .collect(),
    );
    group(
        "verificação",
        report
            .verification
            .iter()
            .map(|change| {
                format!(
                    "~ {}: {} → {}",
                    change.id,
                    vnf_core::rollover::verification_label(change.from),
                    vnf_core::rollover::verification_label(change.to)
                )
            })
            .collect(),
    );
    group(
        "datas publicadas",
        report
            .dates
            .iter()
            .map(|change| format!("~ {}: {} → {}", change.id, change.from, change.to))
            .collect(),
    );
    group(
        "fontes",
        report
            .sources_added
            .iter()
            .map(|id| format!("+ {id}"))
            .chain(report.sources_removed.iter().map(|id| format!("− {id}")))
            .chain(report.sources_changed.iter().map(|change| {
                format!(
                    "~ {} · {}: {} → {}",
                    change.id, change.field, change.from, change.to
                )
            }))
            .collect(),
    );
    println!("{} alteração(ões).", report.total());
}

/// The civil date for a count of days since 1970-01-01, in UTC.
///
/// The scaffold's `generatedAt` has to be a real date, and xtask carries no date crate
/// and no time-zone database. This is Howard Hinnant's `civil_from_days`, exact for
/// every date the application will ever write.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (year + i64::from(month <= 2), month, day)
}

/// Today, in UTC, as `YYYY-MM-DD`.
fn today_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default();
    let (year, month, day) = civil_from_days((seconds / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Write `THIRD-PARTY-NOTICES.md` from what the build actually depends on.
///
/// `cargo metadata` is the only honest source for this: it reports the licence of every
/// crate the workspace resolves to, including the transitive ones nobody chose by name.
/// A hand-written list drifts the first time a dependency changes, and a licence file
/// that has drifted is worse than none — it is a claim that nobody checked.
fn notices() -> Result<(), String> {
    let text = notices_text()?;
    std::fs::write("THIRD-PARTY-NOTICES.md", &text)
        .map_err(|error| format!("could not write THIRD-PARTY-NOTICES.md: {error}"))?;
    println!("xtask: wrote THIRD-PARTY-NOTICES.md ({} bytes)", text.len());
    Ok(())
}

/// Fail if the committed notices no longer describe the dependency tree.
fn notices_check() -> Result<(), String> {
    let expected = notices_text()?;
    let actual = std::fs::read_to_string("THIRD-PARTY-NOTICES.md").unwrap_or_default();
    if actual == expected {
        return Ok(());
    }
    let hint = "THIRD-PARTY-NOTICES.md is out of date; run `cargo xtask notices`";
    if actual.is_empty() {
        return Err(format!("{hint}: the file is missing"));
    }
    // Name the first line that differs, so the fix is obvious rather than rumoured.
    let first = actual
        .lines()
        .zip(expected.lines())
        .position(|(left, right)| left != right)
        .map_or_else(
            || {
                format!(
                    "one file is longer ({} vs {} bytes)",
                    actual.len(),
                    expected.len()
                )
            },
            |line| format!("line {}", line + 1),
        );
    Err(format!("{hint}: first difference at {first}"))
}

/// The notices document, as text.
fn notices_text() -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--all-features"])
        .output()
        .map_err(|error| format!("could not run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed".to_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata did not return JSON: {error}"))?;
    let workspace: Vec<String> = metadata["workspace_members"]
        .as_array()
        .map(|members| {
            members
                .iter()
                .filter_map(|member| member.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let mut rows: Vec<(String, String, String)> = Vec::new();
    if let Some(packages) = metadata["packages"].as_array() {
        for package in packages {
            let id = package["id"].as_str().unwrap_or_default();
            if workspace.contains(&id.to_owned()) {
                continue;
            }
            let name = package["name"].as_str().unwrap_or_default().to_owned();
            let version = package["version"].as_str().unwrap_or_default().to_owned();
            let licence = package["license"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| package["license_file"].as_str().map(|file| format!("see {file}")))
                .unwrap_or_else(|| "unknown".to_owned());
            rows.push((name, version, licence));
        }
    }
    rows.sort();
    rows.dedup();

    let mut text = String::new();
    text.push_str(
        "# Third-party notices\n\n\
         vn-finance is distributed under the GNU Affero General Public License, version 3\n\
         only (see `LICENSE`). It is built from the crates below, each under its own\n\
         licence; this file is generated from `cargo metadata` by `cargo xtask notices`, and\n\
         `cargo xtask ci` fails if it has drifted.\n\n\
         Nothing here is redistributed as source in this repository: they are dependencies\n\
         fetched by Cargo at build time.\n\n",
    );
    text.push_str(&format!("{} crates.\n\n", rows.len()));
    text.push_str("| Crate | Version | Licence |\n| --- | --- | --- |\n");
    for (name, version, licence) in &rows {
        text.push_str(&format!("| `{name}` | {version} | {licence} |\n"));
    }
    text.push_str(
        "\n## What this application does not depend on\n\n\
         There is no HTTP client, no TLS stack, no async runtime and no database engine.\n\
         `cargo xtask ci` checks the lockfile for the crates that would give this\n\
         application a socket, and refuses the build if one appears.\n",
    );
    Ok(text)
}

/// Scan every Rust source in the workspace for the promises it keeps.
fn audit_sources() -> Result<(), String> {
    let mut problems: Vec<String> = Vec::new();
    for path in rust_sources(&OUR_CODE)? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        for needle in MOJIBAKE {
            if text.contains(needle) {
                problems.push(format!(
                    "{}: contains mojibake — the interface is European Portuguese",
                    path.display()
                ));
            }
        }
        for (number, line) in text.lines().enumerate() {
            if line.contains("unsafe") && !line.contains("unsafe_code") {
                problems.push(format!(
                    "{}:{}: `unsafe` outside `unsafe_code`",
                    path.display(),
                    number + 1
                ));
            }
        }
    }
    // The no-socket promise is about the binary this repository ships, so it is
    // checked over the crates that are linked into it. `xtask` is a build tool and
    // names these APIs on purpose.
    for path in rust_sources(&SHIPPED)? {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        for needle in NETWORK {
            if text.contains(needle) {
                problems.push(format!(
                    "{}: mentions {needle:?} — this application has no socket",
                    path.display()
                ));
            }
        }
    }
    report(problems, "source")
}

/// Check that no banned crate reached the lockfile.
fn audit_lockfile() -> Result<(), String> {
    let path = Path::new("Cargo.lock");
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut problems = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("name = \"") else {
            continue;
        };
        let Some(name) = rest.strip_suffix('"') else {
            continue;
        };
        if BANNED_CRATES.contains(&name) {
            problems.push(format!(
                "Cargo.lock: `{name}` would give this application a socket"
            ));
        }
    }
    report(problems, "lockfile")
}

/// Print the problems and turn them into one failure.
fn report(problems: Vec<String>, what: &str) -> Result<(), String> {
    if problems.is_empty() {
        return Ok(());
    }
    for problem in &problems {
        eprintln!("  {problem}");
    }
    Err(format!("{what} audit found {} problem(s)", problems.len()))
}

/// Every `.rs` file under the given roots, sorted for a stable report.
fn rust_sources(roots: &[&str]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for root in roots {
        let path = Path::new(root);
        if path.is_dir() {
            collect(path, &mut found)?;
        }
    }
    found.sort();
    Ok(found)
}

/// Walk a directory, skipping build output.
fn collect(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("could not read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("target") {
                continue;
            }
            collect(&path, found)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            found.push(path);
        }
    }
    Ok(())
}
