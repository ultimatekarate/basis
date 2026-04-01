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
                eprintln!("Spec validation errors:");
                for err in &errors {
                    eprintln!("  - {err}");
                }
                process::exit(1);
            }

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
                let violations = check::placement::check_placement(&spec, &path, &registry);
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
                let violations = check::completeness::check_completeness(&spec, &path, &registry);
                for v in &violations {
                    eprintln!("{v}\n");
                }
                total_violations += violations.len();
            }

            if run("purity") {
                let violations = check::purity::check_purity(&spec, &path, &registry);
                for v in &violations {
                    eprintln!("{v}\n");
                }
                total_violations += violations.len();
            }

            if total_violations == 0 {
                println!("basis check passed.");
            } else {
                eprintln!("error: aborting due to {total_violations} basis violation(s)");
                process::exit(1);
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
