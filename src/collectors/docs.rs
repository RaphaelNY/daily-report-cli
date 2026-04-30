//! 项目文档采集与摘要。

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use walkdir::WalkDir;

use crate::core::types::{DocumentInfo, RepoInfo, ReportRequest};
use crate::core::utils::{normalize_whitespace, relative_display};

/// 收集报告上下文里会引用到的文档。
pub(crate) fn collect_docs(
    request: &ReportRequest,
    repo: &RepoInfo,
    repo_path: &Path,
    modules: &[String],
) -> Result<Vec<DocumentInfo>> {
    let mut doc_paths = if request.doc_paths.is_empty() {
        discover_default_docs(repo_path)
    } else {
        resolve_explicit_docs(repo_path, &request.doc_paths)
    };

    if request.max_docs == 0 {
        doc_paths.clear();
    }

    let ranked_paths = rank_docs(doc_paths, modules, request.max_docs);
    let mut docs = Vec::new();
    for path in ranked_paths {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read doc {}", path.display()))?;
        docs.push(DocumentInfo {
            repo_name: repo.name.clone(),
            repo_path: repo.path.clone(),
            path: relative_display(repo_path, &path),
            title: extract_title(&content, &path),
            excerpt: extract_excerpt(&content, request.max_doc_chars),
            content: content.clone(),
            entry_date: extract_entry_date(&path),
        });
    }
    Ok(docs)
}

fn resolve_explicit_docs(repo_path: &Path, docs: &[PathBuf]) -> Vec<PathBuf> {
    docs.iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                repo_path.join(path)
            }
        })
        .collect()
}

fn discover_default_docs(repo_path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for entry in WalkDir::new(repo_path)
        .follow_links(true)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        if path
            .components()
            .any(|component| component.as_os_str() == ".git" || component.as_os_str() == "target")
        {
            continue;
        }

        let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
        let is_markdown = path.extension().and_then(OsStr::to_str) == Some("md");
        let in_docs_dir = path
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("docs" | "doc")));
        let is_root_readme = path.parent() == Some(repo_path)
            && file_name.to_ascii_lowercase().starts_with("readme")
            && is_markdown;

        if is_root_readme || (is_markdown && in_docs_dir) {
            candidates.push(path.to_path_buf());
        }
    }
    candidates.sort();
    candidates.dedup();
    candidates
}

fn rank_docs(mut docs: Vec<PathBuf>, modules: &[String], max_docs: usize) -> Vec<PathBuf> {
    docs.sort_by_key(|path| {
        let path_str = path.to_string_lossy().to_ascii_lowercase();
        let readme_score = if path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase()
            .starts_with("readme")
        {
            100
        } else {
            0
        };
        let docs_score = if path_str.contains("/docs/") || path_str.contains("/doc/") {
            20
        } else {
            0
        };
        let module_score = modules
            .iter()
            .filter(|module| path_str.contains(&module.to_ascii_lowercase()))
            .count() as i32
            * 10;
        -(readme_score + docs_score + module_score)
    });
    docs.truncate(max_docs);
    docs
}

fn extract_title(content: &str, path: &Path) -> String {
    content
        .lines()
        .find_map(|line| line.strip_prefix("# ").map(normalize_whitespace))
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("Document")
                .to_string()
        })
}

fn extract_excerpt(content: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let merged = content
        .lines()
        .map(str::trim)
        .filter(|line| is_excerpt_candidate(line))
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    let compact = normalize_whitespace(&merged);
    if compact.chars().count() <= max_chars {
        compact
    } else {
        let mut excerpt: String = compact.chars().take(max_chars).collect();
        excerpt.push('…');
        excerpt
    }
}

fn is_excerpt_candidate(line: &str) -> bool {
    if line.is_empty() || line.starts_with('#') || line.starts_with("```") {
        return false;
    }

    if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
        return line.chars().count() >= 16
            && !looks_like_command(line.trim_start_matches(['-', '*', '+', ' ']));
    }

    !looks_like_command(line)
}

fn looks_like_command(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("cargo ")
        || trimmed.starts_with("daily_git ")
        || trimmed.starts_with("./")
        || trimmed.starts_with("curl ")
        || trimmed.starts_with("bash ")
        || trimmed.starts_with("git ")
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
}

fn extract_entry_date(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.trim();
    let normalized = stem.replace('_', "-");
    for candidate in [normalized.as_str(), stem] {
        if let Ok(date) = NaiveDate::parse_from_str(candidate, "%Y-%m-%d") {
            return Some(date.format("%Y-%m-%d").to_string());
        }
        if let Ok(date) = NaiveDate::parse_from_str(candidate, "%Y-%-m-%-d") {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title_from_markdown_heading() {
        assert_eq!(
            extract_title("# Hello\n\nBody", Path::new("README.md")),
            "Hello".to_string()
        );
    }

    #[test]
    fn extracts_entry_date_from_daily_doc_filename() {
        assert_eq!(
            extract_entry_date(Path::new("2026-4-28.md")),
            Some("2026-04-28".to_string())
        );
        assert_eq!(
            extract_entry_date(Path::new("2026-04-29.md")),
            Some("2026-04-29".to_string())
        );
        assert_eq!(extract_entry_date(Path::new("README.md")), None);
    }

    #[test]
    fn excerpt_skips_command_lines_and_short_list_noise() {
        let excerpt = extract_excerpt(
            "# Demo\n\ncargo run -- daily\n- 短项\n一个使用 Rust 编写的本地 CLI，用于读取 Git 提交并生成日报。\n- 支持读取多个仓库并汇总输出",
            120,
        );

        assert!(excerpt.contains("一个使用 Rust 编写的本地 CLI"));
        assert!(!excerpt.contains("cargo run"));
        assert!(!excerpt.contains("短项"));
    }
}
