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
        "benchmark" => benchmark(args.collect()),
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
    run_program(OsString::from("scripts/check-semver.sh"), &[])
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
           coverage     run core library coverage with the 80% line threshold\n\
           semver       verify the exact reviewed 0.7.1-to-0.7.5 API break set\n\
           package      package from a clean worktree with cargo verification\n\
           release-test run release-mode tests\n\
           benchmark <wsi-dicom-bench challenge arguments>\n\
                        build the release candidate and run the installed benchmark\n\
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

fn benchmark(args: Vec<OsString>) -> Result<(), String> {
    let candidate = build_release_candidate()?;
    let program =
        env::var_os("WSI_DICOM_BENCH").unwrap_or_else(|| OsString::from("wsi-dicom-bench"));
    run_program(program, &benchmark_args(candidate, args)?)
}

fn benchmark_args(
    candidate: OsString,
    args: impl IntoIterator<Item = OsString>,
) -> Result<Vec<OsString>, String> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.iter().any(|argument| {
        let argument = argument.to_string_lossy();
        argument == "--wsi-dicom" || argument.starts_with("--wsi-dicom=")
    }) {
        return Err(
            "cargo xtask benchmark selects the freshly built candidate; do not pass --wsi-dicom"
                .into(),
        );
    }
    Ok([OsString::from("challenge"), OsString::from("run")]
        .into_iter()
        .chain(args)
        .chain([OsString::from("--wsi-dicom"), candidate])
        .collect())
}

fn build_release_candidate() -> Result<OsString, String> {
    let output = Command::new(cargo())
        .args([
            "build",
            "--release",
            "--locked",
            "--package",
            "wsi-dicom",
            "--bin",
            "wsi-dicom",
            "--message-format=json",
        ])
        .output()
        .map_err(|err| format!("failed to start `cargo build`: {err}"))?;
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(format!("`cargo build` exited with {}", output.status));
    }
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .find_map(|message| {
            (message["reason"] == "compiler-artifact" && message["target"]["name"] == "wsi-dicom")
                .then(|| message["executable"].as_str().map(OsString::from))
                .flatten()
        })
        .ok_or_else(|| "cargo did not report the built wsi-dicom executable".into())
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
    use super::{benchmark_args, validate_dicom_cargo_args};
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

    #[test]
    fn benchmark_supplies_the_built_candidate_and_forwards_suite_arguments() {
        let args = benchmark_args(
            OsString::from("target/release/wsi-dicom"),
            [
                OsString::from("--suite"),
                OsString::from("suite/manifest.json"),
                OsString::from("--output"),
                OsString::from("evidence"),
            ],
        )
        .expect("benchmark arguments");

        assert_eq!(
            args,
            vec![
                OsString::from("challenge"),
                OsString::from("run"),
                OsString::from("--suite"),
                OsString::from("suite/manifest.json"),
                OsString::from("--output"),
                OsString::from("evidence"),
                OsString::from("--wsi-dicom"),
                OsString::from("target/release/wsi-dicom"),
            ]
        );

        assert!(benchmark_args(
            OsString::from("target/release/wsi-dicom"),
            [OsString::from("--wsi-dicom=other")]
        )
        .is_err());
    }
}
