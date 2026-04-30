//! 基于本机 `codex` CLI 的自然语言润色。
//!
//! 设计原则：
//! - 默认启用，但失败时保留原始 Markdown，保证主流程可用
//! - 优先复用用户当前设备的 `codex login` 认证状态
//! - 仅做措辞润色，不应凭空补充事实

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use wait_timeout::ChildExt;

use crate::core::types::{CommitInfo, PolishOptions, PolishState, RepoInfo, ReportKind};

pub(crate) struct PolishResult {
    pub(crate) content: String,
    pub(crate) state: PolishState,
}

/// 尝试为提交生成简要摘要。失败时返回原始 subject。
pub(crate) fn summarize_commits(
    options: &PolishOptions,
    repo: &RepoInfo,
    commits: &[CommitInfo],
) -> Vec<String> {
    if commits.is_empty() {
        return Vec::new();
    }

    match check_codex_ready(options) {
        CodexReady::Ready => {}
        CodexReady::Skipped(_) => return commits.iter().map(fallback_summary).collect(),
    }

    match run_codex_exec(options, repo, build_commit_summaries_prompt(commits)) {
        Ok(raw) => parse_commit_summaries(&raw, commits),
        Err(_) => commits.iter().map(fallback_summary).collect(),
    }
}

/// 尝试使用本机 `codex exec` 对渲染结果做润色。
pub(crate) fn polish_markdown(
    options: &PolishOptions,
    repo: &RepoInfo,
    kind: ReportKind,
    markdown: &str,
) -> PolishResult {
    if !options.enabled {
        return fallback(markdown, PolishState::Skipped("润色功能已关闭".to_string()));
    }

    if markdown.trim().is_empty() {
        return fallback(
            markdown,
            PolishState::Skipped("渲染后的 Markdown 为空".to_string()),
        );
    }

    match check_codex_ready(options) {
        CodexReady::Ready => {}
        CodexReady::Skipped(reason) => return fallback(markdown, PolishState::Skipped(reason)),
    }

    match run_codex_exec(options, repo, build_prompt(repo, kind, markdown)) {
        Ok(polished) if polished.trim().is_empty() => fallback(
            markdown,
            PolishState::Failed("Codex 返回了空内容".to_string()),
        ),
        Ok(polished) => PolishResult {
            content: polished,
            state: PolishState::Applied,
        },
        Err(reason) => fallback(markdown, PolishState::Failed(reason)),
    }
}

enum CodexReady {
    Ready,
    Skipped(String),
}

fn check_codex_ready(options: &PolishOptions) -> CodexReady {
    let mut command = Command::new("codex");
    command.args(["login", "status"]);
    apply_codex_env(&mut command, options);

    match command.output() {
        Ok(output) if output.status.success() => CodexReady::Ready,
        Ok(output) => {
            let message = command_message(&output.stdout, &output.stderr);
            CodexReady::Skipped(format!("Codex 当前不可用：{message}"))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            CodexReady::Skipped("未找到 `codex` 命令".to_string())
        }
        Err(error) => CodexReady::Skipped(format!("无法执行 `codex login status`：{error}")),
    }
}

fn run_codex_exec(
    options: &PolishOptions,
    repo: &RepoInfo,
    prompt: String,
) -> Result<String, String> {
    let output_file = temp_output_path();

    let mut command = Command::new("codex");
    command
        .arg("exec")
        .arg("--skip-git-repo-check")
        .arg("--color")
        .arg("never")
        .arg("--sandbox")
        .arg("read-only")
        .arg("--output-last-message")
        .arg(&output_file)
        .arg("-");
    if let Some(model) = &options.model {
        command.arg("--model").arg(model);
    }
    apply_codex_env(&mut command, options);
    command
        .current_dir(&repo.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 `codex exec` 失败：{error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|error| format!("向 Codex 发送提示失败：{error}"))?;
    }

    let timeout = Duration::from_secs(options.timeout_secs.max(1));
    let status = match child.wait_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&output_file);
            return Err(format!("Codex 润色超时（{} 秒）", timeout.as_secs()));
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&output_file);
            return Err(format!("等待 Codex 结果失败：{error}"));
        }
    };

    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }

    if !status.success() {
        let _ = fs::remove_file(&output_file);
        return Err(format!("`codex exec` 执行失败：{}", stderr.trim()));
    }

    let result = fs::read_to_string(&output_file)
        .map_err(|error| format!("读取 Codex 输出文件失败：{error}"))?;
    let _ = fs::remove_file(&output_file);
    Ok(result)
}

fn build_prompt(repo: &RepoInfo, kind: ReportKind, markdown: &str) -> String {
    format!(
        r#"请将下面的 {kind_name} Markdown 润色成更自然、专业、简洁的中文表达。

硬性要求：
1. 只根据原文改写，不要新增事实、日期、计划结果、文件、作者、提交信息。
2. 保留 Markdown 标题层级、列表、表格、代码片段、路径、commit hash 和日期。
3. 允许把自动生成的描述改得更顺畅，但不要改变原有章节结构。
4. 不要输出解释，不要使用 ``` 包裹，直接输出最终 Markdown。
5. 如果原文中有“暂无”“未发现”等保守表述，请保留其谨慎语气。

仓库：{repo_name}
分支：{branch}

原始 Markdown 如下：

{markdown}"#,
        kind_name = kind.display_name(),
        repo_name = repo.name,
        branch = repo.branch,
        markdown = markdown,
    )
}

fn build_commit_summaries_prompt(commits: &[CommitInfo]) -> String {
    let payload = commits
        .iter()
        .enumerate()
        .map(|(index, commit)| {
            format!(
                "提交 #{index}\n标题：{subject}\n正文：{body}\n涉及文件：{files}\n涉及模块：{modules}",
                index = index + 1,
                subject = commit.subject,
                body = if commit.body.trim().is_empty() {
                    "-"
                } else {
                    commit.body.trim()
                },
                files = commit.files_display,
                modules = commit.modules_display,
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"请根据下面这些 Git 提交信息，为每条提交分别输出一句简短、自然、专业的中文工作摘要。

硬性要求：
1. 只输出 JSON 数组，不要 Markdown，不要解释，不要代码块。
2. 数组长度必须与提交数量一致，按原顺序一一对应。
3. 每个元素都是 1 句话，不要分点。
4. 不要保留 commit hash，不要照抄 Conventional Commit 前缀如 feat:/fix:/docs:。
5. 长度控制在 18 到 40 个中文字符之间。
6. 只能根据给定信息总结，不要脑补业务背景。
7. 如果信息不足，就保守概括修改对象和动作。

示例输出：
["完善周报 PPT 生成链路","修复 Git 中文路径转义问题"]

提交列表如下：

{payload}"#,
        payload = payload,
    )
}

fn parse_commit_summaries(raw: &str, commits: &[CommitInfo]) -> Vec<String> {
    let cleaned = strip_code_fences(raw).trim();
    let parsed = serde_json::from_str::<Value>(cleaned)
        .ok()
        .and_then(|value| value.as_array().cloned());

    let Some(items) = parsed else {
        return commits.iter().map(fallback_summary).collect();
    };

    if items.len() != commits.len() {
        return commits.iter().map(fallback_summary).collect();
    }

    items
        .into_iter()
        .zip(commits.iter())
        .map(|(item, commit)| {
            item.as_str()
                .map(|summary| normalize_summary(summary, &fallback_summary(commit)))
                .unwrap_or_else(|| fallback_summary(commit))
        })
        .collect()
}

fn fallback_summary(commit: &CommitInfo) -> String {
    let cleaned = simplify_subject(&commit.subject);
    if cleaned.is_empty() {
        return commit.subject.clone();
    }

    let modules = if commit.modules.is_empty() {
        String::new()
    } else {
        format!("（{}）", commit.modules_display)
    };

    format!("{}{}", cleaned, modules)
}

fn simplify_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed
        .split_once(':')
        .map(|(_, rest)| rest.trim())
        .filter(|rest| !rest.is_empty())
        .unwrap_or(trimmed);

    let lowered = normalized.to_ascii_lowercase();
    let replacements = [
        ("add ", "新增"),
        ("support ", "支持"),
        ("update ", "更新"),
        ("improve ", "优化"),
        ("fix ", "修复"),
        ("preserve ", "保留"),
        ("drop ", "移除"),
        ("initialize ", "初始化"),
    ];

    for (prefix, chinese) in replacements {
        if lowered.starts_with(prefix) {
            let target = normalized[prefix.len()..].trim();
            if !target.is_empty() {
                return format!("{}{}", chinese, target);
            }
        }
    }

    normalized.to_string()
}

fn strip_code_fences(input: &str) -> &str {
    let trimmed = input.trim();
    if let Some(stripped) = trimmed.strip_prefix("```json") {
        return stripped.strip_suffix("```").unwrap_or(stripped).trim();
    }
    if let Some(stripped) = trimmed.strip_prefix("```") {
        return stripped.strip_suffix("```").unwrap_or(stripped).trim();
    }
    trimmed
}

fn normalize_summary(summary: &str, fallback: &str) -> String {
    let cleaned = summary
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('。')
        .trim();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn apply_codex_env(command: &mut Command, options: &PolishOptions) {
    if let Some(codex_home) = &options.codex_home {
        command.env("CODEX_HOME", codex_home);
    }
}

fn temp_output_path() -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!(
        "daily_git_codex_{}_{}.md",
        std::process::id(),
        millis
    ))
}

fn command_message(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "未知错误".to_string()
    }
}

fn fallback(markdown: &str, state: PolishState) -> PolishResult {
    PolishResult {
        content: markdown.to_string(),
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::RepoInfo;

    #[test]
    fn builds_polish_prompt_with_repo_context() {
        let prompt = build_prompt(
            &RepoInfo {
                name: "demo".to_string(),
                path: ".".to_string(),
                branch: "main".to_string(),
            },
            ReportKind::Daily,
            "# Title",
        );

        assert!(prompt.contains("日报"));
        assert!(prompt.contains("demo"));
        assert!(prompt.contains("# Title"));
    }

    #[test]
    fn builds_commit_summary_prompt_with_context() {
        let prompt = build_commit_summaries_prompt(&[CommitInfo {
            repo_name: "demo".to_string(),
            repo_path: "/tmp/demo".to_string(),
            hash: "abc".to_string(),
            short_hash: "abc".to_string(),
            author: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            date: "2025-02-14".to_string(),
            subject: "feat: add cli".to_string(),
            summary: String::new(),
            body: "support weekly reports".to_string(),
            files: vec!["src/main.rs".to_string()],
            files_display: "src/main.rs".to_string(),
            files_compact_display: "src/main.rs".to_string(),
            modules: vec!["src".to_string()],
            modules_display: "src".to_string(),
        }]);

        assert!(prompt.contains("提交 #1"));
        assert!(prompt.contains("feat: add cli"));
        assert!(prompt.contains("src/main.rs"));
    }

    #[test]
    fn parses_commit_summaries_from_json_array() {
        let commits = vec![CommitInfo {
            repo_name: "demo".to_string(),
            repo_path: "/tmp/demo".to_string(),
            hash: "abc".to_string(),
            short_hash: "abc".to_string(),
            author: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            date: "2025-02-14".to_string(),
            subject: "feat: add cli".to_string(),
            summary: String::new(),
            body: String::new(),
            files: vec!["src/main.rs".to_string()],
            files_display: "src/main.rs".to_string(),
            files_compact_display: "src/main.rs".to_string(),
            modules: vec!["src".to_string()],
            modules_display: "src".to_string(),
        }];

        let summaries = parse_commit_summaries("[\"完善命令行入口支持\"]", &commits);
        assert_eq!(summaries, vec!["完善命令行入口支持".to_string()]);
    }

    #[test]
    fn fallback_summary_humanizes_subject() {
        let commit = CommitInfo {
            repo_name: "demo".to_string(),
            repo_path: "/tmp/demo".to_string(),
            hash: "abc".to_string(),
            short_hash: "abc".to_string(),
            author: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            date: "2025-02-14".to_string(),
            subject: "feat: add weekly html ppt deck generation".to_string(),
            summary: String::new(),
            body: String::new(),
            files: vec!["src/reporting/ppt.rs".to_string()],
            files_display: "src/reporting/ppt.rs".to_string(),
            files_compact_display: "src/reporting/ppt.rs".to_string(),
            modules: vec!["src".to_string()],
            modules_display: "src".to_string(),
        };

        assert_eq!(
            fallback_summary(&commit),
            "新增weekly html ppt deck generation（src）".to_string()
        );
    }
}
