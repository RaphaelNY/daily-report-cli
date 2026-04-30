//! 与领域无关的通用小工具函数。

use std::collections::BTreeSet;
use std::path::Path;

use chrono::Weekday;

/// 从文件路径中提取一级模块名，用于概览和周报聚合。
pub(crate) fn collect_modules<'a>(files: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut modules = BTreeSet::new();
    for file in files {
        let module = file
            .split('/')
            .next()
            .unwrap_or(file)
            .trim()
            .trim_matches('.');
        if !module.is_empty() {
            modules.insert(module.to_string());
        }
    }
    modules.into_iter().collect()
}

/// 将列表展示成逗号分隔文本；若为空则返回 `-`。
pub(crate) fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

/// 合并多余空白，避免 commit subject / 文档摘要过于松散。
pub(crate) fn normalize_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 将仓库名转成适合输出文件名的安全字符串。
pub(crate) fn sanitize_name(input: &str) -> String {
    let mut sanitized = String::new();
    for character in input.chars() {
        if character.is_ascii_alphanumeric() {
            sanitized.push(character.to_ascii_lowercase());
        } else if character == '-' || character == '_' {
            sanitized.push(character);
        } else {
            sanitized.push('-');
        }
    }

    while sanitized.contains("--") {
        sanitized = sanitized.replace("--", "-");
    }

    sanitized.trim_matches('-').to_string()
}

/// 根据多个仓库名生成稳定可读的聚合标签。
pub(crate) fn summarize_repo_names(names: &[String]) -> String {
    if names.is_empty() {
        return "repo".to_string();
    }

    if names.len() == 1 {
        return names[0].clone();
    }

    let mut sorted = names.to_vec();
    sorted.sort();
    sorted.dedup();

    let preview = sorted.iter().take(3).cloned().collect::<Vec<_>>();
    let mut label = preview.join("-");
    if sorted.len() > 3 {
        label.push_str(&format!("-and-{}-more", sorted.len() - 3));
    }
    label
}

/// 将 Git ISO 时间转换成报告里更易读的日期格式。
pub(crate) fn format_git_date(input: &str) -> String {
    input
        .split('T')
        .next()
        .map(ToString::to_string)
        .unwrap_or_else(|| input.to_string())
}

/// 尽量输出相对路径，方便报告阅读。
pub(crate) fn relative_display(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// 将星期枚举转成人类可读的中文标签。
pub(crate) fn weekday_label(weekday: Weekday) -> &'static str {
    match weekday {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_modules_from_paths() {
        let files = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "docs/usage.md".to_string(),
        ];
        let modules = collect_modules(files.iter());
        assert_eq!(modules, vec!["docs".to_string(), "src".to_string()]);
    }

    #[test]
    fn sanitizes_repo_name_for_output() {
        assert_eq!(sanitize_name("My Repo!"), "my-repo");
    }

    #[test]
    fn summarizes_multiple_repo_names() {
        assert_eq!(
            summarize_repo_names(&[
                "workspace-b".to_string(),
                "workspace-a".to_string(),
                "workspace-c".to_string(),
                "workspace-d".to_string()
            ]),
            "workspace-a-workspace-b-workspace-c-and-1-more".to_string()
        );
    }
}
