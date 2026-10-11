use std::process::ExitCode;

use racer_trace_compare::{Tolerances, compare, format_report, parse_trace};

const USAGE: &str = "usage: trace-compare <expected.csv> <actual.csv> [--tol <default>] [--channel-tol <name>=<value>]...\nexit codes: 0 pass, 1 tolerance/structure failure, 2 usage or parse error";

fn run() -> Result<bool, String> {
    let mut paths = Vec::new();
    let mut tol = Tolerances::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tol" => {
                let v = args.next().ok_or("--tol needs a value")?;
                tol.default = parse_tol(&v)?;
            }
            "--channel-tol" => {
                let v = args.next().ok_or("--channel-tol needs name=value")?;
                let (name, val) = v.split_once('=').ok_or("--channel-tol needs name=value")?;
                tol.per_channel.insert(name.to_string(), parse_tol(val)?);
            }
            "-h" | "--help" => return Err(String::new()),
            _ if arg.starts_with("--") => return Err(format!("unknown option {arg}")),
            _ => paths.push(arg),
        }
    }
    if paths.len() != 2 {
        return Err("expected exactly two trace paths".into());
    }
    let load = |p: &str| {
        let text = std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?;
        parse_trace(&text).map_err(|e| format!("{p}: {e}"))
    };
    let report = compare(&load(&paths[0])?, &load(&paths[1])?, &tol);
    print!("{}", format_report(&report));
    Ok(report.passed())
}

fn parse_tol(s: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(v),
        _ => Err(format!("invalid tolerance `{s}`")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
