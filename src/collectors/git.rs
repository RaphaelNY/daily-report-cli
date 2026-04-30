//! Git 数据采集。
//!
//! 这里统一负责：
//! - 仓库合法性检查
//! - 分支信息读取
//! - 提交列表解析
//! - 提交涉及文件读取

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::core::types::{CommitInfo, RepoInfo, ReportRequest};
use crate::core::utils::{collect_modules, format_git_date, join_or_dash, normalize_whitespace};

const RECORD_SEPARATOR: char = '\u{1e}';
const FIELD_SEPARATOR: char = '\u{1f}';

#[derive(Debug, Clone)]
struct RawCommit {
    hash: String,
    author: String,
    email: String,
    date: String,
    subject: String,
    body: String,
}

/// 确认目标目录为 Git 工作区。
pub(crate) fn ensure_git_repo(repo_path: &Path) -> Result<()> {
    let output = git_output(repo_path, ["rev-parse", "--is-inside-work-tree"])?;
    if output.trim() == "true" {
        Ok(())
    } else {
        bail!("{} is not a git repository", repo_path.display())
    }
}

/// 读取仓库基础元信息。
pub(crate) fn load_repo_info(repo_path: &Path) -> Result<RepoInfo> {
    let name = repo_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("repo")
        .to_string();
    let branch = git_output(repo_path, ["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();

    Ok(RepoInfo {
        name,
        path: repo_path.display().to_string(),
        branch,
    })
}

/// 按请求中的时间范围和作者过滤条件收集提交。
pub(crate) fn collect_commits(
    request: &ReportRequest,
    repo_path: &Path,
) -> Result<Vec<CommitInfo>> {
    let since = format!("{}T00:00:00", request.start_date.format("%Y-%m-%d"));
    let until = format!("{}T23:59:59", request.end_date.format("%Y-%m-%d"));

    let mut args = vec![
        "log".to_string(),
        format!("--since={since}"),
        format!("--until={until}"),
        "--date=iso-strict".to_string(),
        format!(
            "--pretty=format:%H{FIELD_SEPARATOR}%an{FIELD_SEPARATOR}%ae{FIELD_SEPARATOR}%aI{FIELD_SEPARATOR}%s{FIELD_SEPARATOR}%b{RECORD_SEPARATOR}"
        ),
    ];

    if let Some(author) = &request.author {
        args.push(format!("--author={author}"));
    }

    let raw_output = git_output_owned(repo_path, args)?;
    let mut commits = Vec::new();
    for raw_commit in parse_commits(&raw_output) {
        let files = git_output_owned(
            repo_path,
            vec![
                "show".to_string(),
                "--pretty=format:".to_string(),
                "--name-only".to_string(),
                "--no-renames".to_string(),
                raw_commit.hash.clone(),
            ],
        )?;
        let files = parse_file_list(&files);
        let modules = collect_modules(files.iter());
        commits.push(CommitInfo {
            short_hash: raw_commit.hash.chars().take(7).collect(),
            hash: raw_commit.hash,
            author: raw_commit.author,
            email: raw_commit.email,
            date: format_git_date(&raw_commit.date),
            subject: normalize_whitespace(&raw_commit.subject),
            body: raw_commit.body.trim().to_string(),
            files_display: join_or_dash(&files),
            modules_display: join_or_dash(&modules),
            files,
            modules,
        });
    }

    Ok(commits)
}

fn parse_commits(output: &str) -> Vec<RawCommit> {
    output
        .split(RECORD_SEPARATOR)
        .filter_map(|record| {
            let trimmed = record.trim();
            if trimmed.is_empty() {
                return None;
            }

            let mut parts = trimmed.splitn(6, FIELD_SEPARATOR);
            Some(RawCommit {
                hash: parts.next()?.to_string(),
                author: parts.next()?.to_string(),
                email: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                subject: parts.next()?.to_string(),
                body: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

fn parse_file_list(output: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| seen.insert((*line).to_string()))
        .map(ToString::to_string)
        .collect()
}

fn git_output<const N: usize>(repo_path: &Path, args: [&str; N]) -> Result<String> {
    git_output_owned(
        repo_path,
        args.into_iter().map(ToString::to_string).collect(),
    )
}

fn git_output_owned(repo_path: &Path, args: Vec<String>) -> Result<String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-C")
        .arg(repo_path)
        .args(&args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_commits_from_git_log_output() {
        let output = format!(
            "abc{FIELD_SEPARATOR}Alice{FIELD_SEPARATOR}alice@example.com{FIELD_SEPARATOR}2025-02-14T10:00:00+08:00{FIELD_SEPARATOR}feat: add cli{FIELD_SEPARATOR}body line{RECORD_SEPARATOR}"
        );
        let commits = parse_commits(&output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abc");
        assert_eq!(commits[0].subject, "feat: add cli");
    }

    #[test]
    fn git_output_uses_unquoted_unicode_paths() {
        let temp = tempdir().unwrap();
        let repo_path = temp.path();

        let status = Command::new("git")
            .arg("init")
            .arg(repo_path)
            .status()
            .unwrap();
        assert!(status.success());

        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["config", "user.name", "Test User"])
            .status()
            .unwrap();
        assert!(status.success());

        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["config", "user.email", "test@example.com"])
            .status()
            .unwrap();
        assert!(status.success());

        let file_path = repo_path.join("templates/周报与日报_markdown_模板.md");
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, "# demo").unwrap();

        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["add", "."])
            .status()
            .unwrap();
        assert!(status.success());

        let status = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(["commit", "-m", "test"])
            .status()
            .unwrap();
        assert!(status.success());

        let output = git_output_owned(
            repo_path,
            vec![
                "show".to_string(),
                "--pretty=format:".to_string(),
                "--name-only".to_string(),
                "HEAD".to_string(),
            ],
        )
        .unwrap();

        assert!(output.contains("templates/周报与日报_markdown_模板.md"));
        assert!(!output.contains("\\345"));
        assert!(!output.contains('\"'));
    }
}
