mod cli;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use cli::{AppCommand, Cli};
use serde::Serialize;

use daily_git::{
    generate_report, run_doctor, run_skill_command, run_update, DoctorCheckStatus, DoctorReport,
    GeneratedReport, PolishState, ReportKind, SkillResult, UpdateState,
};

#[derive(Serialize)]
struct GenerateJsonOutput {
    ok: bool,
    kind: &'static str,
    output_path: String,
    ppt_path: Option<String>,
    polish: PolishJsonOutput,
}

#[derive(Serialize)]
struct PolishJsonOutput {
    status: &'static str,
    message: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().into_command()? {
        AppCommand::Generate { request, json } => {
            let generated = generate_report(&request)?;
            if json {
                print_generate_json(request.kind, &generated)?;
            } else {
                println!("{}", generated.output_path.display());
                if let Some(ppt_path) = generated.ppt_path {
                    eprintln!("ppt: {}", ppt_path.display());
                }
                eprintln!("polish: {}", generated.polish_state);
            }
        }
        AppCommand::Doctor { request, json } => {
            let report = run_doctor(&request);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_doctor_text(&report);
            }
            if !report.ok {
                std::process::exit(1);
            }
        }
        AppCommand::Skill { options, json } => {
            let result = run_skill_command(&options)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                print_skill_text(&result);
            }
            if !result.ok {
                std::process::exit(1);
            }
        }
        AppCommand::Update(options) => {
            let result = run_update(&options)?;
            match result.state {
                UpdateState::UpToDate => {
                    println!(
                        "already up to date: {} ({})",
                        result.target_version,
                        result.executable_path.display()
                    );
                }
                UpdateState::Available => {
                    println!(
                        "update available: {} -> {}",
                        result.current_version, result.target_version
                    );
                }
                UpdateState::Updated => {
                    println!(
                        "updated {} -> {} ({})",
                        result.current_version,
                        result.target_version,
                        result.executable_path.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn print_skill_text(result: &SkillResult) {
    println!(
        "{}: {} ({})",
        result.action, result.message, result.skill_path
    );
}

fn print_doctor_text(report: &DoctorReport) {
    for check in &report.checks {
        let status = match check.status {
            DoctorCheckStatus::Pass => "PASS",
            DoctorCheckStatus::Warn => "WARN",
            DoctorCheckStatus::Fail => "FAIL",
        };
        println!("[{status}] {}: {}", check.name, check.message);
    }
}

fn print_generate_json(kind: ReportKind, generated: &GeneratedReport) -> Result<()> {
    let output = GenerateJsonOutput {
        ok: true,
        kind: kind.as_str(),
        output_path: json_path(&generated.output_path).display().to_string(),
        ppt_path: generated
            .ppt_path
            .as_ref()
            .map(|path| json_path(path).display().to_string()),
        polish: polish_json(&generated.polish_state),
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn json_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn polish_json(state: &PolishState) -> PolishJsonOutput {
    match state {
        PolishState::Applied => PolishJsonOutput {
            status: "applied",
            message: None,
        },
        PolishState::Skipped(reason) => PolishJsonOutput {
            status: "skipped",
            message: Some(reason.clone()),
        },
        PolishState::Failed(reason) => PolishJsonOutput {
            status: "failed",
            message: Some(reason.clone()),
        },
    }
}
