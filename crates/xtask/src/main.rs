use std::{
    env,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".into());
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "help" | "--help" | "-h" => {
            println!("cargo xtask <doctor|check|test|e2e|docker-smoke|provenance-check|verify>");
            ExitCode::SUCCESS
        }
        "doctor" => doctor(),
        "provenance-check" => run("python", &["scripts/provenance_check.py".into()]),
        "check" => cargo(&["check", "--workspace"]),
        "test" => cargo(&["test", "--workspace"]),
        "e2e" => run("npm", &["run".into(), "test:e2e".into()]),
        "docker-smoke" => run(
            "docker",
            &["run".into(), "--rm".into(), "hello-world".into()],
        ),
        "verify" => verify(),
        _ => {
            eprintln!("unknown xtask command: {command} {rest:?}");
            ExitCode::from(2)
        }
    }
}

fn cargo(args: &[&str]) -> ExitCode {
    run(
        "cargo",
        &args
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>(),
    )
}

fn run(program: &str, args: &[String]) -> ExitCode {
    eprintln!("+ {program} {}", args.join(" "));
    match Command::new(program).args(args).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => ExitCode::from(status.code().unwrap_or(1).clamp(1, 255) as u8),
        Err(error) => {
            eprintln!("could not start {program}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn doctor() -> ExitCode {
    println!("racer local doctor");
    let mut missing = false;
    for (tool, arg) in [
        ("git", "--version"),
        ("rustc", "--version"),
        ("cargo", "--version"),
        ("node", "--version"),
        ("npm", "--version"),
        ("python", "--version"),
        ("docker", "--version"),
    ] {
        let program = if cfg!(windows) && tool == "npm" {
            "npm.cmd"
        } else {
            tool
        };
        match Command::new(program).arg(arg).output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                println!(
                    "{tool}: {}",
                    if text.is_empty() { "available" } else { &text }
                );
            }
            _ => {
                println!("{tool}: missing or failed");
                missing = true;
            }
        }
    }
    if missing {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn verify() -> ExitCode {
    for (program, args) in [
        ("cargo", vec!["fmt", "--all", "--", "--check"]),
        (
            "cargo",
            vec![
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("cargo", vec!["test", "--workspace"]),
        (
            "cargo",
            vec!["build", "--workspace", "--target", "wasm32-unknown-unknown"],
        ),
    ] {
        if !matches!(Command::new(program).args(&args).status(), Ok(status) if status.success()) {
            return ExitCode::FAILURE;
        }
    }
    if run("python", &["scripts/provenance_check.py".into()]) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
