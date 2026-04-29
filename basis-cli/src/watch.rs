use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, RecursiveMode, Watcher};

use crate::check;
use crate::language;
use crate::spec;

/// Directories to skip when filtering watch events (mirrors check::walk).
const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "__pycache__",
    ".git",
    "venv",
    ".venv",
];

/// Outcome of a single check pass.
pub struct CheckOutcome {
    pub violations: Vec<check::output::UnifiedViolation>,
    pub identities: HashSet<String>,
    pub elapsed: Duration,
}

/// Run all enabled axis checks and return the outcome with timing.
pub fn timed_check(
    spec: &spec::BasisSpec,
    root: &Path,
    axes: &Option<Vec<String>>,
) -> CheckOutcome {
    let start = Instant::now();

    let registry = language::LangRegistry::new();
    let all_axes = axes.is_none();
    let axes_set: HashSet<String> = axes
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.to_lowercase())
        .collect();
    let run = |axis: &str| all_axes || axes_set.contains(axis);

    let mut violations = Vec::new();

    if run("placement") {
        for v in &check::placement::check_placement(spec, root, &registry) {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }
    if run("values") {
        for v in &check::values::check_values(spec, root, &registry) {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }
    if run("completeness") {
        let result = check::completeness::check_completeness(spec, root, &registry);
        for v in &result.violations {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }
    if run("purity") {
        for v in &check::purity::check_purity(spec, root, &registry) {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }
    if run("granularity") {
        for v in &check::granularity::check_granularity(spec, root, &registry) {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }

    let identities = violations.iter().map(|v| v.identity.clone()).collect();
    CheckOutcome {
        violations,
        identities,
        elapsed: start.elapsed(),
    }
}

/// Print a compact single-line summary to stderr.
pub fn print_summary(outcome: &CheckOutcome) {
    let count = outcome.violations.len();
    let status = if count == 0 { "PASS" } else { "FAIL" };
    eprintln!(
        "[watch] {status} — {count} violation(s) ({:.2}s)",
        outcome.elapsed.as_secs_f64()
    );
}

/// Print full violation details to stderr.
pub fn print_violations(violations: &[check::output::UnifiedViolation]) {
    eprintln!();
    for v in violations {
        eprintln!("{v}");
        eprintln!();
    }
}

/// Run `basis check` in watch mode: re-check on file changes, print compact
/// summaries, and show the full report only when violations change.
pub fn run_watch(
    spec: &spec::BasisSpec,
    spec_path: &Path,
    root: &Path,
    axes: &Option<Vec<String>>,
) -> Result<(), String> {
    // Initial check — always show full report
    let outcome = timed_check(spec, root, axes);
    print_summary(&outcome);
    if !outcome.violations.is_empty() {
        print_violations(&outcome.violations);
    }
    let mut prev_identities = outcome.identities;

    // Set up file watcher with debounced event channel
    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            if !event.paths.iter().all(|p| is_skipped(p)) {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|e| format!("Failed to create file watcher: {e}"))?;

    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| format!("Failed to watch {}: {e}", root.display()))?;

    // Also watch the spec file itself
    if let Some(spec_dir) = spec_path.parent() {
        let _ = watcher.watch(spec_dir, RecursiveMode::NonRecursive);
    }

    eprintln!("[watch] watching {} for changes…", root.display());

    let debounce = Duration::from_millis(200);

    loop {
        // Block until first filesystem event
        if rx.recv().is_err() {
            break;
        }

        // Drain additional events within the debounce window
        let deadline = Instant::now() + debounce;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rx.recv_timeout(remaining) {
                Ok(()) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }

        // Re-run check
        let outcome = timed_check(spec, root, axes);
        print_summary(&outcome);

        // Full report only when violations changed
        let changed = outcome.identities != prev_identities;
        if changed && !outcome.violations.is_empty() {
            print_violations(&outcome.violations);
        }

        prev_identities = outcome.identities;
    }

    Ok(())
}

/// Check if a path falls under a skipped directory.
fn is_skipped(path: &Path) -> bool {
    for component in path.components() {
        if let std::path::Component::Normal(name) = component {
            let s = name.to_string_lossy();
            if s.starts_with('.') || SKIP_DIRS.contains(&s.as_ref()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_skipped_filters_target_dir() {
        assert!(is_skipped(Path::new("foo/target/debug/main.rs")));
        assert!(is_skipped(Path::new(".git/config")));
        assert!(is_skipped(Path::new("node_modules/pkg/index.js")));
    }

    #[test]
    fn is_skipped_allows_source_files() {
        assert!(!is_skipped(Path::new("src/main.rs")));
        assert!(!is_skipped(Path::new("basis-cli/src/watch.rs")));
    }
}
