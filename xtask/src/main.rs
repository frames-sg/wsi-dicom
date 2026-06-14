use std::env;
use std::ffi::OsString;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask failed: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os();
    let _program = args.next();
    let task = args
        .next()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "help".to_string());
    match task.as_str() {
        "fmt" => fmt(),
        "clippy" => clippy(),
        "test" => test(),
        "deny" => deny(),
        "package" => package(),
        "docs-strict" => docs_strict(),
        "coverage" => coverage(),
        "semver" => semver(),
        "release-test" => release_test(),
        "validate-dicom" => validate_dicom(args.collect()),
        "ci" => ci(),
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown task `{other}`")),
    }
}

fn ci() -> Result<(), String> {
    fmt()?;
    clippy()?;
    test()?;
    docs_strict()?;
    coverage()?;
    semver()?;
    deny()?;
    package()
}

fn fmt() -> Result<(), String> {
    run_cargo(&["fmt", "--all", "--", "--check"])
}

fn clippy() -> Result<(), String> {
    run_cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ])
}

fn test() -> Result<(), String> {
    run_cargo(&["test", "--workspace", "--all-targets"])
}

fn deny() -> Result<(), String> {
    run_cargo(&["deny", "check", "advisories", "bans", "licenses", "sources"])
}

fn package() -> Result<(), String> {
    ensure_clean_worktree()?;
    run_cargo(&["package"])
}

fn docs_strict() -> Result<(), String> {
    run_cargo(&[
        "rustdoc",
        "--lib",
        "--no-default-features",
        "--",
        "-D",
        "missing_docs",
    ])
}

fn coverage() -> Result<(), String> {
    run_cargo(&[
        "llvm-cov",
        "--package",
        "wsi-dicom",
        "--lib",
        "--bins",
        "--tests",
        "--no-default-features",
        "--summary-only",
        "--fail-under-lines",
        "80",
    ])
}

fn semver() -> Result<(), String> {
    run_cargo(&["semver-checks", "check-release", "--default-features"])
}

fn release_test() -> Result<(), String> {
    run_cargo(&["test", "--workspace", "--all-targets", "--release"])
}

fn print_help() {
    println!(
        "usage: cargo xtask <task>\n\n\
         tasks:\n\
           ci           fmt, clippy, test, docs, coverage, semver, deny, and package\n\
           fmt          check rustfmt\n\
           clippy       run clippy with warnings denied\n\
           test         run tests and compile examples\n\
           deny         run cargo-deny advisories, bans, licenses, and sources checks\n\
           docs-strict  build public API docs with missing docs denied\n\
           coverage     run core library coverage with the 1.0 threshold\n\
           semver       run cargo-semver-checks against the latest release\n\
           package      package from a clean worktree with cargo verification\n\
           release-test run release-mode tests\n\
           validate-dicom <path> [args]\n\
                        run wsi-dicom validate through cargo"
    );
}

fn ensure_clean_worktree() -> Result<(), String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|err| format!("failed to start `git status --porcelain`: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "`git status --porcelain` exited with {}",
            output.status
        ));
    }
    let status = String::from_utf8_lossy(&output.stdout);
    if status.trim().is_empty() {
        Ok(())
    } else {
        Err(format!(
            "working tree must be clean before packaging:\n{status}"
        ))
    }
}

fn run_cargo(args: &[&str]) -> Result<(), String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    run_cargo_os(&args)
}

fn run_cargo_os(args: &[OsString]) -> Result<(), String> {
    run_program(cargo(), args)
}

fn validate_dicom(args: Vec<OsString>) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: cargo xtask validate-dicom <path> [wsi-dicom validate args]".into());
    }
    run_cargo_os(&validate_dicom_cargo_args(args))
}

fn validate_dicom_cargo_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    [
        OsString::from("run"),
        OsString::from("--no-default-features"),
        OsString::from("--"),
        OsString::from("validate"),
    ]
    .into_iter()
    .chain(args)
    .collect()
}

fn run_program(program: OsString, args: &[OsString]) -> Result<(), String> {
    let display = program.to_string_lossy();
    let args_display = args
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    eprintln!("+ {} {}", display, args_display);
    let status = Command::new(&program)
        .args(args)
        .status()
        .map_err(|err| format!("failed to start `{display}`: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`{display}` exited with {status}"))
    }
}

fn cargo() -> OsString {
    env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

#[cfg(test)]
mod tests {
    use super::validate_dicom_cargo_args;
    use std::ffi::OsString;

    #[test]
    fn validate_dicom_forwards_to_public_validate_command() {
        let args = validate_dicom_cargo_args([
            OsString::from("dicom-out"),
            OsString::from("--strict"),
            OsString::from("--max-pixel-frames"),
            OsString::from("0"),
        ]);

        assert_eq!(
            args,
            vec![
                OsString::from("run"),
                OsString::from("--no-default-features"),
                OsString::from("--"),
                OsString::from("validate"),
                OsString::from("dicom-out"),
                OsString::from("--strict"),
                OsString::from("--max-pixel-frames"),
                OsString::from("0"),
            ]
        );
    }
}
