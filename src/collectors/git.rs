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

use crate::core::types::{AuthorMatchMode, CommitInfo, RepoInfo, ReportRequest};
use crate::core::utils::{collect_modules, format_git_date, join_or_dash, normalize_whitespace};

const RECORD_SEPARATOR: char = '\u{1e}';
const FIELD_SEPARATOR: char = '\u{1f}';
const NUL_SEPARATOR: char = '\0';

#[derive(Debug, Clone)]
struct RawCommit {
    hash: String,
    author: String,
    email: String,
    date: String,
    subject: String,
    body: String,
    files: Vec<String>,
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
    repo: &RepoInfo,
    repo_path: &Path,
) -> Result<Vec<CommitInfo>> {
    let since = format!("{}T00:00:00", request.start_date.format("%Y-%m-%d"));
    let until = format!("{}T23:59:59", request.end_date.format("%Y-%m-%d"));

    let args = vec![
        "log".to_string(),
        format!("--since={since}"),
        format!("--until={until}"),
        "--date=iso-strict".to_string(),
        "--name-only".to_string(),
        "--no-renames".to_string(),
        "-z".to_string(),
        format!(
            "--pretty=format:{RECORD_SEPARATOR}%H{FIELD_SEPARATOR}%an{FIELD_SEPARATOR}%ae{FIELD_SEPARATOR}%aI{FIELD_SEPARATOR}%s{FIELD_SEPARATOR}%b%x00"
        ),
    ];

    let raw_output = git_output_owned(repo_path, args)?;
    let commits = parse_commits(&raw_output)
        .into_iter()
        .map(|raw_commit| {
            let files = raw_commit.files;
            let modules = collect_modules(files.iter());
            CommitInfo {
                repo_name: repo.name.clone(),
                repo_path: repo.path.clone(),
                short_hash: raw_commit.hash.chars().take(7).collect(),
                hash: raw_commit.hash,
                author: raw_commit.author,
                email: raw_commit.email,
                date: format_git_date(&raw_commit.date),
                subject: normalize_whitespace(&raw_commit.subject),
                summary: String::new(),
                body: raw_commit.body.trim().to_string(),
                files_display: join_or_dash(&files),
                modules_display: join_or_dash(&modules),
                files,
                modules,
            }
        })
        .collect::<Vec<_>>();

    Ok(filter_commits_by_author(
        commits,
        request.author.as_deref(),
        request.author_match_mode,
    ))
}

fn parse_commits(output: &str) -> Vec<RawCommit> {
    let mut commits = Vec::new();

    for record in output.split(RECORD_SEPARATOR) {
        let trimmed = record.trim_matches(|character| matches!(character, '\n' | '\r' | '\0'));
        if trimmed.is_empty() {
            continue;
        }

        let (header, file_block) = match trimmed.split_once(NUL_SEPARATOR) {
            Some((header, file_block)) if !header.trim().is_empty() => (header.trim(), file_block),
            _ => continue,
        };

        let mut parts = header.splitn(6, FIELD_SEPARATOR);
        let Some(hash) = parts.next() else {
            continue;
        };
        let Some(author) = parts.next() else {
            continue;
        };
        let Some(email) = parts.next() else {
            continue;
        };
        let Some(date) = parts.next() else {
            continue;
        };
        let Some(subject) = parts.next() else {
            continue;
        };
        let body = parts.next().unwrap_or_default();
        commits.push(RawCommit {
            hash: hash.to_string(),
            author: author.to_string(),
            email: email.to_string(),
            date: date.to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            files: parse_file_list(file_block),
        });
    }

    commits
}

fn parse_file_list(output: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    output
        .split(['\n', '\0'])
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| seen.insert((*line).to_string()))
        .map(ToString::to_string)
        .collect()
}

fn filter_commits_by_author(
    commits: Vec<CommitInfo>,
    author: Option<&str>,
    match_mode: AuthorMatchMode,
) -> Vec<CommitInfo> {
    let Some(author) = author.map(str::trim).filter(|value| !value.is_empty()) else {
        return commits;
    };

    commits
        .into_iter()
        .filter(|commit| author_matches(commit, author, match_mode))
        .collect()
}

fn author_matches(commit: &CommitInfo, author: &str, match_mode: AuthorMatchMode) -> bool {
    match match_mode {
        AuthorMatchMode::Name => normalized_eq(&commit.author, author),
        AuthorMatchMode::Email => normalized_eq(&commit.email, author),
        AuthorMatchMode::NameOrEmail => {
            normalized_eq(&commit.author, author) || normalized_eq(&commit.email, author)
        }
    }
}

fn normalized_eq(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
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
            "{RECORD_SEPARATOR}abc{FIELD_SEPARATOR}Alice{FIELD_SEPARATOR}alice@example.com{FIELD_SEPARATOR}2025-02-14T10:00:00+08:00{FIELD_SEPARATOR}feat: add cli{FIELD_SEPARATOR}body line\nsecond line{NUL_SEPARATOR}src/main.rs{NUL_SEPARATOR}README.md{NUL_SEPARATOR}"
        );
        let commits = parse_commits(&output);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abc");
        assert_eq!(commits[0].subject, "feat: add cli");
        assert_eq!(commits[0].body, "body line\nsecond line");
        assert_eq!(
            commits[0].files,
            vec!["src/main.rs".to_string(), "README.md".to_string()]
        );
    }

    #[test]
    fn filters_commits_by_exact_name_or_email() {
        let commits = vec![
            CommitInfo {
                repo_name: "demo".to_string(),
                repo_path: "/tmp/demo".to_string(),
                hash: "1".to_string(),
                short_hash: "1".to_string(),
                author: "Raphael".to_string(),
                email: "raphael@example.com".to_string(),
                date: "2025-02-14".to_string(),
                subject: "feat: one".to_string(),
                summary: String::new(),
                body: String::new(),
                files: vec!["src/main.rs".to_string()],
                files_display: "src/main.rs".to_string(),
                modules: vec!["src".to_string()],
                modules_display: "src".to_string(),
            },
            CommitInfo {
                repo_name: "demo".to_string(),
                repo_path: "/tmp/demo".to_string(),
                hash: "2".to_string(),
                short_hash: "2".to_string(),
                author: "Another".to_string(),
                email: "another@example.com".to_string(),
                date: "2025-02-14".to_string(),
                subject: "feat: two".to_string(),
                summary: String::new(),
                body: String::new(),
                files: vec!["src/lib.rs".to_string()],
                files_display: "src/lib.rs".to_string(),
                modules: vec!["src".to_string()],
                modules_display: "src".to_string(),
            },
        ];

        let by_name =
            filter_commits_by_author(commits.clone(), Some("Raphael"), AuthorMatchMode::Name);
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].hash, "1");

        let by_email = filter_commits_by_author(
            commits.clone(),
            Some("raphael@example.com"),
            AuthorMatchMode::Email,
        );
        assert_eq!(by_email.len(), 1);
        assert_eq!(by_email[0].hash, "1");

        let no_partial =
            filter_commits_by_author(commits, Some("Rap"), AuthorMatchMode::NameOrEmail);
        assert!(no_partial.is_empty());
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
            .args(["config", "commit.gpgsign", "false"])
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
