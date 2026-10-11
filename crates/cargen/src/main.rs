//! `racer-cargen [--out DIR] [--check]`
//!
//! Writes the demo-car content folder (default `content/demo/cars`).
//! With `--check` nothing is written; the exit status says whether the folder
//! already matches the generator output.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = PathBuf::from("content/demo/cars");
    let mut check = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => match args.next() {
                Some(dir) => out = PathBuf::from(dir),
                None => return usage(),
            },
            "--check" => check = true,
            _ => return usage(),
        }
    }
    let files = racer_cargen::generate();
    let mut stale = 0;
    for (path, bytes) in &files {
        let target = out.join(path);
        if check {
            if std::fs::read(&target).ok().as_deref() != Some(bytes.as_slice()) {
                eprintln!("out of date: {}", target.display());
                stale += 1;
            }
            continue;
        }
        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("cannot create {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
        if let Err(e) = std::fs::write(&target, bytes) {
            eprintln!("cannot write {}: {e}", target.display());
            return ExitCode::FAILURE;
        }
    }
    println!(
        "{} {} files, digest {}",
        if check { "checked" } else { "wrote" },
        files.len(),
        racer_cargen::digest(&files)
    );
    if stale > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: racer-cargen [--out DIR] [--check]");
    ExitCode::from(2)
}
