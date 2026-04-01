use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process;

mod check;
mod generate;
mod language;
mod loader;
mod report;
mod spec;
mod validate;

#[derive(Parser)]
#[command(name = "basis", about = "Architectural governance CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Validate a basis.yaml spec file
    Validate {
        /// Path to the basis.yaml spec
        #[arg(default_value = "basis.yaml")]
        spec: PathBuf,
    },
    /// Check a codebase against a basis.yaml spec
    Check {
        /// Path to the basis.yaml spec
        #[arg(long, default_value = "basis.yaml")]
        spec: PathBuf,
        /// Path to the codebase root
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Which axes to check (default: all). Options: placement, values, completeness, purity
        #[arg(long, value_delimiter = ',')]
        axes: Option<Vec<String>>,
        /// Output format: text (default) or json
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Generate code skeletons from basis.yaml
    Generate {
        /// Path to the basis.yaml spec
        #[arg(long, default_value = "basis.yaml")]
        spec: PathBuf,
        /// Target language (python, rust, js, go, java, kotlin, ruby, swift, csharp)
        #[arg(long)]
        lang: String,
        /// Output directory
        #[arg(long, default_value = "generated")]
        output: PathBuf,
    },
    /// Save current violations as a baseline for trend comparison
    Baseline {
        /// Path to the basis.yaml spec
        #[arg(long, default_value = "basis.yaml")]
        spec: PathBuf,
        /// Path to the codebase root
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Which axes to check (default: all)
        #[arg(long, value_delimiter = ',')]
        axes: Option<Vec<String>>,
        /// Output file path (default: .basis-baseline.json next to spec)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compare current violations against a baseline
    Trend {
        /// Path to the basis.yaml spec (used in live mode)
        #[arg(long, default_value = "basis.yaml")]
        spec: PathBuf,
        /// Path to the codebase root (used in live mode)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Which axes to check (default: all)
        #[arg(long, value_delimiter = ',')]
        axes: Option<Vec<String>>,
        /// Path to baseline file
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// Path to existing check result JSON (offline mode — skips running check)
        #[arg(long)]
        current: Option<PathBuf>,
        /// Output format: text (default) or json
        #[arg(long, default_value = "text")]
        format: String,
        /// Exit 1 if any new violations exist (strict)
        #[arg(long)]
        fail_on_regression: bool,
        /// Exit 1 only if total violation count increased (lenient)
        #[arg(long)]
        fail_on_net_regression: bool,
    },
    /// Generate a governance report
    Report {
        /// Path to the basis.yaml spec
        #[arg(long, default_value = "basis.yaml")]
        spec: PathBuf,
        /// Output format (text or json)
        #[arg(long, default_value = "text")]
        format: String,
    },
}

/// Run all requested checks and return unified violations + list of axes checked.
fn run_check(
    spec: &spec::BasisSpec,
    path: &std::path::Path,
    axes: &Option<Vec<String>>,
) -> (Vec<check::output::UnifiedViolation>, Vec<String>) {
    let all_axes = axes.is_none();
    let axes_set: std::collections::HashSet<String> = axes
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.to_lowercase())
        .collect();
    let run = |axis: &str| all_axes || axes_set.contains(axis);

    let registry = language::LangRegistry::new();
    let mut violations = Vec::new();
    let mut axes_checked = Vec::new();

    if run("placement") {
        axes_checked.push("placement".to_string());
        let hits = check::placement::check_placement(spec, path, &registry);
        for v in &hits {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }

    if run("values") {
        axes_checked.push("values".to_string());
        let hits = check::values::check_values(spec, path, &registry);
        for v in &hits {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }

    if run("completeness") {
        axes_checked.push("completeness".to_string());
        let hits = check::completeness::check_completeness(spec, path, &registry);
        for v in &hits {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }

    if run("purity") {
        axes_checked.push("purity".to_string());
        let hits = check::purity::check_purity(spec, path, &registry);
        for v in &hits {
            violations.push(check::output::UnifiedViolation::from(v));
        }
    }

    (violations, axes_checked)
}

/// Load and validate a spec, exiting on error.
fn load_and_validate(spec_path: &std::path::Path) -> spec::BasisSpec {
    let spec = match loader::load_spec(spec_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            process::exit(1);
        }
    };

    let errors = validate::validate(&spec);
    if !errors.is_empty() {
        eprintln!("Spec validation errors:");
        for err in &errors {
            eprintln!("  - {err}");
        }
        process::exit(1);
    }

    spec
}

/// Resolve the default baseline path (next to the spec file).
fn default_baseline_path(spec_path: &std::path::Path) -> PathBuf {
    spec_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(".basis-baseline.json")
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Validate { spec: spec_path } => {
            let spec = match loader::load_spec(&spec_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };

            let errors = validate::validate(&spec);
            if errors.is_empty() {
                println!("Valid: {}", spec_path.display());
                println!(
                    "  {} layers, {} newtypes, {} unions, {} boundary rules",
                    spec.layers.len(),
                    spec.newtypes.as_ref().map_or(0, |n| n.types.len()),
                    spec.exhaustive_matching
                        .as_ref()
                        .map_or(0, |e| e.unions.len()),
                    spec.boundaries.as_ref().map_or(0, |b| b.rules.len()),
                );
            } else {
                eprintln!("Invalid: {}", spec_path.display());
                for err in &errors {
                    eprintln!("  - {err}");
                }
                process::exit(1);
            }
        }
        Command::Check {
            spec: spec_path,
            path,
            axes,
            format,
        } => {
            let spec = load_and_validate(&spec_path);

            if format == "json" {
                let (violations, axes_checked) = run_check(&spec, &path, &axes);
                let result = check::output::CheckResult::new(
                    &spec_path.display().to_string(),
                    &path.display().to_string(),
                    axes_checked,
                    violations.clone(),
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
                        eprintln!("Error serializing JSON: {e}");
                        process::exit(1);
                    })
                );
                if !violations.is_empty() {
                    process::exit(1);
                }
            } else {
                // Text output — preserve original human-readable stderr behavior
                let all_axes = axes.is_none();
                let axes_set: std::collections::HashSet<String> = axes
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| a.to_lowercase())
                    .collect();
                let run = |axis: &str| all_axes || axes_set.contains(axis);

                let registry = language::LangRegistry::new();
                let mut total_violations = 0;

                if run("placement") {
                    let violations =
                        check::placement::check_placement(&spec, &path, &registry);
                    for v in &violations {
                        eprintln!("{v}\n");
                    }
                    total_violations += violations.len();
                }

                if run("values") {
                    let violations = check::values::check_values(&spec, &path, &registry);
                    for v in &violations {
                        eprintln!("{v}\n");
                    }
                    total_violations += violations.len();
                }

                if run("completeness") {
                    let violations =
                        check::completeness::check_completeness(&spec, &path, &registry);
                    for v in &violations {
                        eprintln!("{v}\n");
                    }
                    total_violations += violations.len();
                }

                if run("purity") {
                    let violations =
                        check::purity::check_purity(&spec, &path, &registry);
                    for v in &violations {
                        eprintln!("{v}\n");
                    }
                    total_violations += violations.len();
                }

                if total_violations == 0 {
                    println!("basis check passed.");
                } else {
                    eprintln!(
                        "error: aborting due to {total_violations} basis violation(s)"
                    );
                    process::exit(1);
                }
            }
        }
        Command::Generate {
            spec: spec_path,
            lang: lang_name,
            output,
        } => {
            let spec = match loader::load_spec(&spec_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };

            let registry = language::LangRegistry::new();
            let lang = match registry.for_name(&lang_name) {
                Some(l) => l,
                None => {
                    eprintln!("Error: unknown language \"{lang_name}\"");
                    eprintln!(
                        "  Available: python, rust, js, go, java, kotlin, ruby, swift, csharp"
                    );
                    process::exit(1);
                }
            };

            match generate::generate(&spec, lang) {
                Ok(files) => {
                    std::fs::create_dir_all(&output).unwrap_or_else(|e| {
                        eprintln!("Error creating output directory: {e}");
                        process::exit(1);
                    });
                    for file in &files {
                        let path = output.join(&file.relative_path);
                        std::fs::write(&path, &file.content).unwrap_or_else(|e| {
                            eprintln!("Error writing {}: {e}", path.display());
                            process::exit(1);
                        });
                        println!("  wrote {}", path.display());
                    }
                    println!("Generated {} file(s) for {}", files.len(), lang_name);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            }
        }
        Command::Baseline {
            spec: spec_path,
            path,
            axes,
            output,
        } => {
            let spec = load_and_validate(&spec_path);
            let (violations, axes_checked) = run_check(&spec, &path, &axes);

            let result = check::output::CheckResult::new(
                &spec_path.display().to_string(),
                &path.display().to_string(),
                axes_checked,
                violations,
            );

            let output_path = output.unwrap_or_else(|| default_baseline_path(&spec_path));
            let json = serde_json::to_string_pretty(&result).unwrap_or_else(|e| {
                eprintln!("Error serializing JSON: {e}");
                process::exit(1);
            });

            std::fs::write(&output_path, json).unwrap_or_else(|e| {
                eprintln!("Error writing baseline: {e}");
                process::exit(1);
            });

            println!(
                "Baseline saved: {} violations to {}",
                result.summary.total,
                output_path.display()
            );
        }
        Command::Trend {
            spec: spec_path,
            path,
            axes,
            baseline,
            current,
            format,
            fail_on_regression,
            fail_on_net_regression,
        } => {
            // Load baseline
            let baseline_path = baseline.unwrap_or_else(|| default_baseline_path(&spec_path));
            let baseline_json = match std::fs::read_to_string(&baseline_path) {
                Ok(s) => s,
                Err(_) => {
                    eprintln!(
                        "No baseline found at {}. Run `basis baseline` first.",
                        baseline_path.display()
                    );
                    process::exit(1);
                }
            };
            let baseline_result: check::output::CheckResult =
                match serde_json::from_str(&baseline_json) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error parsing baseline JSON: {e}");
                        process::exit(1);
                    }
                };

            // Get current result: either from file (offline) or by running check (live)
            let current_result = if let Some(current_path) = current {
                let json = match std::fs::read_to_string(&current_path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading current result: {e}");
                        process::exit(1);
                    }
                };
                match serde_json::from_str(&json) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Error parsing current JSON: {e}");
                        process::exit(1);
                    }
                }
            } else {
                let spec = load_and_validate(&spec_path);
                let (violations, axes_checked) = run_check(&spec, &path, &axes);
                check::output::CheckResult::new(
                    &spec_path.display().to_string(),
                    &path.display().to_string(),
                    axes_checked,
                    violations,
                )
            };

            // Diff
            let mut trend = check::output::diff(&baseline_result, &current_result);
            trend.baseline_path = baseline_path.display().to_string();

            if format == "json" {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&trend).unwrap_or_else(|e| {
                        eprintln!("Error serializing JSON: {e}");
                        process::exit(1);
                    })
                );
            } else {
                print!("{}", check::output::render_trend_text(&trend));
            }

            // Exit code logic
            if fail_on_regression && !trend.regressed.is_empty() {
                process::exit(1);
            }
            if fail_on_net_regression && trend.net_change() > 0 {
                process::exit(1);
            }
        }
        Command::Report {
            spec: spec_path,
            format,
        } => {
            let spec = match loader::load_spec(&spec_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }
            };

            let errors = validate::validate(&spec);
            if !errors.is_empty() {
                eprintln!("Warning: spec has validation errors:");
                for err in &errors {
                    eprintln!("  - {err}");
                }
            }

            print!("{}", report::generate_report(&spec, &format));
        }
    }
}
