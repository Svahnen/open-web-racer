//! Validation issue collection shared by every document validator.

use std::fmt;

/// One problem found in a document, with a JSON-path-like location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

/// Collector passed through validators so every problem is reported at once.
#[derive(Clone, Debug, Default)]
pub struct Issues(pub Vec<Issue>);

impl Issues {
    pub fn add(&mut self, path: &str, message: impl Into<String>) {
        self.0.push(Issue {
            path: path.to_string(),
            message: message.into(),
        });
    }

    pub fn finish(self) -> Result<(), ValidationError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(ValidationError { issues: self.0 })
        }
    }

    /// Require `lo <= v <= hi` and finite.
    pub fn range(&mut self, path: &str, v: f32, lo: f32, hi: f32) {
        if !v.is_finite() || v < lo || v > hi {
            self.add(path, format!("value {v} outside [{lo}, {hi}]"));
        }
    }

    /// Require a finite, strictly positive value.
    pub fn positive(&mut self, path: &str, v: f32) {
        if !v.is_finite() || v <= 0.0 {
            self.add(path, format!("value {v} must be finite and > 0"));
        }
    }

    pub fn finite(&mut self, path: &str, v: &[f32]) {
        if v.iter().any(|x| !x.is_finite()) {
            self.add(path, "contains a non-finite number");
        }
    }

    /// Require `idx < len` for a cross-reference.
    pub fn index(&mut self, path: &str, idx: usize, len: usize, what: &str) {
        if idx >= len {
            self.add(
                path,
                format!("{what} index {idx} out of range ({len} available)"),
            );
        }
    }
}

/// All issues found by a validator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub issues: Vec<Issue>,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} validation issue(s)", self.issues.len())?;
        for i in &self.issues {
            write!(f, "\n  - {i}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationError {}

/// Relative forward-slash path with no empty, `.` or `..` components.
pub fn is_safe_relative_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && !p.split('/').any(|s| s.is_empty() || s == "." || s == "..")
}
