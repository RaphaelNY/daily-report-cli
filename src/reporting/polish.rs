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

use wait_timeout::ChildExt;

use crate::core::types::{PolishOptions, PolishState, RepoInfo, ReportKind};

pub(crate) struct PolishResult {
    pub(crate) content: String,
    pub(crate) state: PolishState,
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

    match run_codex_exec(options, repo, kind, markdown) {
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
    kind: ReportKind,
    markdown: &str,
) -> Result<String, String> {
    let output_file = temp_output_path();
    let prompt = build_prompt(repo, kind, markdown);

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
}
