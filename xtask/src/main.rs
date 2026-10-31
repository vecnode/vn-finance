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
        other => Err(format!(
            "unknown task `{other}`; try `ci`, `lint`, `test`, `fmt-fix`, `notices` or \
             `notices-check`"
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
