//! Agent / script preflight checks.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::WalkDir;

use crate::collectors::git::{ensure_git_repo, load_repo_info};
use crate::core::types::{DoctorCheck, DoctorCheckStatus, DoctorReport, ReportKind, ReportRequest};

/// Validate inputs and optional integrations without writing report artifacts.
pub fn run_doctor(request: &ReportRequest) -> DoctorReport {
    let mut checks = Vec::new();

    check_repos(request, &mut checks);
    check_template(request, &mut checks);
    check_docs(request, &mut checks);
    check_output_target(request, &mut checks);
    check_polish(request, &mut checks);
    check_ppt(request, &mut checks);

    let ok = !checks
        .iter()
        .any(|check| check.status == DoctorCheckStatus::Fail);
    DoctorReport { ok, checks }
}

fn check_repos(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    if request.repo_paths.is_empty() {
        checks.push(fail("repos", "no repository paths configured"));
        return;
    }

    for repo_path in &request.repo_paths {
        match repo_path.canonicalize() {
            Ok(path) => match ensure_git_repo(&path).and_then(|_| load_repo_info(&path)) {
                Ok(repo) => checks.push(pass(
                    "repo",
                    format!("{} on branch {}", absolute_display(&path), repo.branch),
                )),
                Err(error) => checks.push(fail(
                    "repo",
                    format!("{} is not usable: {error}", absolute_display(&path)),
                )),
            },
            Err(error) => checks.push(fail(
                "repo",
                format!("{} cannot be accessed: {error}", repo_path.display()),
            )),
        }
    }
}

fn check_template(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    let Some(path) = &request.template_path else {
        checks.push(pass("template", "using built-in template"));
        return;
    };

    if path.is_file() {
        checks.push(pass(
            "template",
            format!("{} exists", absolute_display(path)),
        ));
    } else {
        checks.push(fail(
            "template",
            format!("{} is not a file", absolute_display(path)),
        ));
    }
}

fn check_docs(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    if request.doc_paths.is_empty() {
        checks.push(pass("docs", "using automatic Markdown discovery"));
        return;
    }

    for repo_path in &request.repo_paths {
        let repo_base = repo_path
            .canonicalize()
            .unwrap_or_else(|_| repo_path.clone());
        for doc_path in &request.doc_paths {
            let resolved = if doc_path.is_absolute() {
                doc_path.clone()
            } else {
                repo_base.join(doc_path)
            };
            if resolved.is_file() {
                checks.push(pass(
                    "doc",
                    format!("{} exists", absolute_display(&resolved)),
                ));
            } else {
                checks.push(fail(
                    "doc",
                    format!("{} is not a file", absolute_display(&resolved)),
                ));
            }
        }
    }
}

fn check_output_target(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    if let Some(path) = &request.output_path {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        if parent.exists() && parent.is_dir() {
            checks.push(pass(
                "output",
                format!("parent directory exists: {}", absolute_display(parent)),
            ));
        } else if parent.exists() {
            checks.push(fail(
                "output",
                format!("parent is not a directory: {}", absolute_display(parent)),
            ));
        } else {
            checks.push(warn(
                "output",
                format!(
                    "parent directory will be created: {}",
                    absolute_display(parent)
                ),
            ));
        }
        return;
    }

    if let Some(path) = &request.output_dir {
        if path.exists() && path.is_dir() {
            checks.push(pass(
                "output",
                format!("output directory exists: {}", absolute_display(path)),
            ));
        } else if path.exists() {
            checks.push(fail(
                "output",
                format!(
                    "output target is not a directory: {}",
                    absolute_display(path)
                ),
            ));
        } else {
            checks.push(warn(
                "output",
                format!(
                    "output directory will be created: {}",
                    absolute_display(path)
                ),
            ));
        }
        return;
    }

    checks.push(pass("output", "using current directory output target"));
}

fn check_polish(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    if !request.polish.enabled {
        checks.push(pass("polish", "disabled"));
        return;
    }

    let mut command = Command::new("codex");
    command.args(["login", "status"]);
    if let Some(codex_home) = &request.polish.codex_home {
        command.env("CODEX_HOME", codex_home);
    }

    match command.output() {
        Ok(output) if output.status.success() => checks.push(pass("polish", "codex is available")),
        Ok(output) => checks.push(warn(
            "polish",
            format!(
                "codex unavailable: {}",
                command_message(&output.stdout, &output.stderr)
            ),
        )),
        Err(error) => checks.push(warn("polish", format!("codex check failed: {error}"))),
    }
}

fn check_ppt(request: &ReportRequest, checks: &mut Vec<DoctorCheck>) {
    if !request.ppt.enabled {
        checks.push(pass("ppt", "disabled"));
        return;
    }

    if !matches!(request.kind, ReportKind::Weekly) {
        checks.push(fail(
            "ppt",
            "PPT generation is only supported for weekly reports",
        ));
        return;
    }

    match find_html_ppt_skill_root(request.polish.codex_home.as_deref()) {
        Ok(path) => checks.push(pass(
            "ppt",
            format!("html-ppt skill found at {}", absolute_display(&path)),
        )),
        Err(message) => checks.push(fail("ppt", message)),
    }
}

fn find_html_ppt_skill_root(codex_home_override: Option<&Path>) -> Result<PathBuf, String> {
    let codex_home = resolve_codex_home(codex_home_override)?;
    let skills_dir = codex_home.join("skills");
    if !skills_dir.is_dir() {
        return Err(format!(
            "{} does not contain a skills directory",
            absolute_display(&codex_home)
        ));
    }

    let direct = skills_dir.join("html-ppt-skill");
    if direct.join("SKILL.md").is_file() {
        return Ok(direct);
    }

    for entry in WalkDir::new(&skills_dir)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        if entry.file_name() != "SKILL.md" || !entry.file_type().is_file() {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if content.lines().any(|line| line.trim() == "name: html-ppt") {
            if let Some(root) = entry.path().parent() {
                return Ok(root.to_path_buf());
            }
        }
    }

    Err(format!(
        "html-ppt skill not found under {}",
        absolute_display(&skills_dir)
    ))
}

fn resolve_codex_home(codex_home_override: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = codex_home_override {
        return Ok(path.to_path_buf());
    }

    if let Some(path) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(path));
    }

    env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".codex"))
        .ok_or_else(|| "failed to determine CODEX_HOME".to_string())
}

fn command_message(stdout: &[u8], stderr: &[u8]) -> String {
    let message = if stderr.is_empty() { stdout } else { stderr };
    String::from_utf8_lossy(message).trim().to_string()
}

fn absolute_display(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    path.display().to_string()
}

fn pass(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    check(name, DoctorCheckStatus::Pass, message)
}

fn warn(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    check(name, DoctorCheckStatus::Warn, message)
}

fn fail(name: impl Into<String>, message: impl Into<String>) -> DoctorCheck {
    check(name, DoctorCheckStatus::Fail, message)
}

fn check(
    name: impl Into<String>,
    status: DoctorCheckStatus,
    message: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        name: name.into(),
        status,
        message: message.into(),
    }
}
