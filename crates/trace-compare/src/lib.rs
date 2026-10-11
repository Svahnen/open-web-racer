//! Library for comparing two project trace CSVs (format: docs/TESTING.md).

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    pub columns: Vec<String>,
    /// (tick, values aligned with `columns[1..]`)
    pub rows: Vec<(u64, Vec<f64>)>,
}

/// Parse a trace. The header is the first non-empty line, optionally prefixed
/// with `# `. Other `#` text is a comment. The first column must be `tick`.
pub fn parse_trace(text: &str) -> Result<Trace, String> {
    let mut columns: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let n = n + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (is_comment, body) = match trimmed.strip_prefix('#') {
            Some(rest) => (true, rest.trim()),
            None => (false, trimmed),
        };
        if columns.is_none() {
            if body.split(',').next().map(str::trim) == Some("tick") {
                columns = Some(body.split(',').map(|c| c.trim().to_string()).collect());
                continue;
            }
            if is_comment {
                continue;
            }
            return Err(format!("line {n}: data before a `tick,...` header"));
        }
        if is_comment {
            continue;
        }
        let cols = columns.as_ref().unwrap();
        let fields: Vec<&str> = body.split(',').map(str::trim).collect();
        if fields.len() != cols.len() {
            return Err(format!(
                "line {n}: expected {} fields, got {}",
                cols.len(),
                fields.len()
            ));
        }
        let tick = fields[0]
            .parse::<u64>()
            .map_err(|e| format!("line {n}: bad tick: {e}"))?;
        let mut values = Vec::with_capacity(fields.len() - 1);
        for (c, f) in cols.iter().zip(&fields).skip(1) {
            let v = f
                .parse::<f64>()
                .map_err(|e| format!("line {n}: bad value for {c}: {e}"))?;
            if !v.is_finite() {
                return Err(format!("line {n}: non-finite value for {c}"));
            }
            values.push(v);
        }
        rows.push((tick, values));
    }
    let columns = columns.ok_or("missing `tick,...` header")?;
    Ok(Trace { columns, rows })
}

#[derive(Clone, Debug, Default)]
pub struct Tolerances {
    pub default: f64,
    pub per_channel: BTreeMap<String, f64>,
}

impl Tolerances {
    pub fn for_channel(&self, name: &str) -> f64 {
        self.per_channel.get(name).copied().unwrap_or(self.default)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChannelReport {
    pub name: String,
    pub tolerance: f64,
    pub max_abs_error: f64,
    pub max_error_tick: u64,
    pub rms_error: f64,
    pub first_exceed_tick: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub compared_ticks: usize,
    pub channels: Vec<ChannelReport>,
    /// Structural problems: tick sets or channel sets differ.
    pub structural: Vec<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.structural.is_empty() && self.channels.iter().all(|c| c.first_exceed_tick.is_none())
    }
}

/// Compare `expected` (reference) with `actual` over channels common to both.
/// Rows are joined on tick; differing tick sets are structural failures.
pub fn compare(expected: &Trace, actual: &Trace, tol: &Tolerances) -> Report {
    let mut structural = Vec::new();
    let mut shared = Vec::new();
    for (ei, name) in expected.columns.iter().enumerate().skip(1) {
        match actual.columns.iter().position(|c| c == name) {
            Some(ai) => shared.push((name.clone(), ei - 1, ai - 1)),
            None => structural.push(format!("channel `{name}` missing from actual")),
        }
    }
    for name in actual.columns.iter().skip(1) {
        if !expected.columns.contains(name) {
            structural.push(format!("channel `{name}` missing from expected"));
        }
    }
    let actual_by_tick: BTreeMap<u64, &Vec<f64>> =
        actual.rows.iter().map(|(t, v)| (*t, v)).collect();
    let mut acc: Vec<(f64, u64, f64, Option<u64>)> = vec![(0.0, 0, 0.0, None); shared.len()];
    let mut compared = 0usize;
    for (tick, ev) in &expected.rows {
        let Some(av) = actual_by_tick.get(tick) else {
            structural.push(format!("tick {tick} missing from actual"));
            continue;
        };
        compared += 1;
        for (k, (name, ei, ai)) in shared.iter().enumerate() {
            let err = (ev[*ei] - av[*ai]).abs();
            let a = &mut acc[k];
            if err > a.0 {
                a.0 = err;
                a.1 = *tick;
            }
            a.2 += err * err;
            if err > tol.for_channel(name) && a.3.is_none() {
                a.3 = Some(*tick);
            }
        }
    }
    let expected_ticks: std::collections::BTreeSet<u64> =
        expected.rows.iter().map(|r| r.0).collect();
    for (tick, _) in &actual.rows {
        if !expected_ticks.contains(tick) {
            structural.push(format!("tick {tick} missing from expected"));
        }
    }
    let channels = shared
        .iter()
        .zip(acc)
        .map(
            |((name, _, _), (max, max_tick, sumsq, first))| ChannelReport {
                name: name.clone(),
                tolerance: tol.for_channel(name),
                max_abs_error: max,
                max_error_tick: max_tick,
                rms_error: if compared > 0 {
                    (sumsq / compared as f64).sqrt()
                } else {
                    0.0
                },
                first_exceed_tick: first,
            },
        )
        .collect();
    Report {
        compared_ticks: compared,
        channels,
        structural,
    }
}

pub fn format_report(r: &Report) -> String {
    let mut s = format!("compared ticks: {}\n", r.compared_ticks);
    for c in &r.channels {
        s.push_str(&format!(
            "{:<16} tol={:<10e} max={:<12e} (tick {}) rms={:<12e} first_exceed={}\n",
            c.name,
            c.tolerance,
            c.max_abs_error,
            c.max_error_tick,
            c.rms_error,
            c.first_exceed_tick
                .map_or("none".to_string(), |t| t.to_string())
        ));
    }
    for m in &r.structural {
        s.push_str(&format!("STRUCTURE: {m}\n"));
    }
    s.push_str(if r.passed() {
        "RESULT: PASS\n"
    } else {
        "RESULT: FAIL\n"
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "# tick,x,speed\n1,0.0,1.0\n2,1.0,2.0\n3,2.0,3.0\n";

    fn tol(d: f64) -> Tolerances {
        Tolerances {
            default: d,
            per_channel: BTreeMap::new(),
        }
    }

    #[test]
    fn identical_traces_pass() {
        let t = parse_trace(A).unwrap();
        let r = compare(&t, &t, &tol(0.0));
        assert!(r.passed());
        assert_eq!(r.compared_ticks, 3);
    }

    #[test]
    fn reports_max_rms_and_first_exceed() {
        let e = parse_trace(A).unwrap();
        let a = parse_trace("tick,x,speed\n1,0.0,1.0\n2,1.5,2.0\n3,2.0,3.5\n").unwrap();
        let r = compare(&e, &a, &tol(0.1));
        assert!(!r.passed());
        let x = &r.channels[0];
        assert_eq!(
            (x.max_abs_error, x.max_error_tick, x.first_exceed_tick),
            (0.5, 2, Some(2))
        );
        assert!((x.rms_error - (0.25f64 / 3.0).sqrt()).abs() < 1e-12);
        assert_eq!(r.channels[1].first_exceed_tick, Some(3));
    }

    #[test]
    fn per_channel_tolerance_overrides_default() {
        let e = parse_trace(A).unwrap();
        let a = parse_trace("tick,x,speed\n1,0.0,1.0\n2,1.5,2.0\n3,2.0,3.0\n").unwrap();
        let mut t = tol(0.0);
        t.per_channel.insert("x".into(), 0.5);
        assert!(compare(&e, &a, &t).passed());
    }

    #[test]
    fn structural_mismatches_fail() {
        let e = parse_trace(A).unwrap();
        let missing_tick = parse_trace("tick,x,speed\n1,0,1\n2,1,2\n").unwrap();
        assert!(!compare(&e, &missing_tick, &tol(1.0)).passed());
        let missing_col = parse_trace("tick,x\n1,0\n2,1\n3,2\n").unwrap();
        assert!(!compare(&e, &missing_col, &tol(1.0)).passed());
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!(parse_trace("1,2,3\n").is_err());
        assert!(parse_trace("tick,x\n1,abc\n").is_err());
        assert!(parse_trace("tick,x\n1,2,3\n").is_err());
        assert!(parse_trace("tick,x\n1,NaN\n").is_err());
        assert!(parse_trace("").is_err());
    }
}
