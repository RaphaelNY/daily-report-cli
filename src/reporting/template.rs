//! 模板上下文构建与 Markdown 渲染。

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Datelike, Days, Local};
use handlebars::Handlebars;

use crate::core::types::{
    CommitInfo, DailyLogInfo, DocumentInfo, RepoInfo, RepoSnapshot, ReportContext, ReportInfo,
    ReportKind, ReportRequest, SummaryInfo,
};
use crate::core::utils::{
    collect_modules, join_or_dash, normalize_whitespace, sanitize_name, weekday_label,
};

const DEFAULT_DAILY_TEMPLATE: &str = include_str!("../../templates/daily.md.hbs");
const DEFAULT_WEEKLY_TEMPLATE: &str = include_str!("../../templates/weekly.md.hbs");

/// 根据原始 Git / 文档数据构建模板上下文。
pub(crate) fn build_context(
    request: &ReportRequest,
    repo: RepoInfo,
    repos: Vec<RepoSnapshot>,
    commits: Vec<CommitInfo>,
    docs: Vec<DocumentInfo>,
) -> ReportContext {
    let modules = collect_modules(commits.iter().flat_map(|commit| commit.files.iter()));
    let summary = build_summary(&commits, &modules);
    let report = build_report_info(request, commits.len(), collect_file_count(&commits));
    let daily_logs = build_daily_logs(request, &commits, &docs);

    ReportContext {
        generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        repo,
        repos,
        report,
        summary,
        commits,
        docs,
        daily_logs,
    }
}

/// 渲染报告 Markdown。
pub(crate) fn render_markdown(
    kind: ReportKind,
    template_path: Option<&Path>,
    context: &ReportContext,
) -> Result<String> {
    let template = match template_path {
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("failed to read template {}", path.display()))?,
        None => default_template(kind).to_string(),
    };

    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(false);
    handlebars
        .register_template_string("report", template)
        .context("failed to register template")?;
    handlebars
        .render("report", context)
        .context("failed to render markdown report")
}

/// 根据请求推导输出文件路径。
pub(crate) fn resolve_output_path(request: &ReportRequest, repo_name: &str) -> Result<PathBuf> {
    if let Some(path) = &request.output_path {
        return Ok(path.clone());
    }

    let base_dir = match &request.output_dir {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().context("failed to determine current directory")?,
    };
    let date = request.end_date.format("%Y-%m-%d");
    let file_name = format!(
        "{}-{}-{}.md",
        request.kind.as_str(),
        sanitize_name(repo_name),
        date
    );
    Ok(base_dir.join(file_name))
}

fn default_template(kind: ReportKind) -> &'static str {
    match kind {
        ReportKind::Daily => DEFAULT_DAILY_TEMPLATE,
        ReportKind::Weekly => DEFAULT_WEEKLY_TEMPLATE,
    }
}

fn build_summary(commits: &[CommitInfo], modules: &[String]) -> SummaryInfo {
    let mut highlights = Vec::new();
    let mut seen = HashSet::new();
    let multi_repo = commits
        .iter()
        .map(|commit| commit.repo_name.as_str())
        .collect::<HashSet<_>>()
        .len()
        > 1;
    for commit in commits {
        let normalized = normalize_whitespace(&commit.summary);
        let display = if multi_repo {
            format!("{}：{}", commit.repo_name, normalized)
        } else {
            normalized.clone()
        };
        if !normalized.is_empty() && seen.insert(display.clone()) {
            highlights.push(display);
        }
    }

    let risks = commits
        .iter()
        .flat_map(|commit| [commit.subject.as_str(), commit.body.as_str()])
        .filter_map(extract_risk)
        .collect();

    SummaryInfo {
        highlights,
        modules: modules.to_vec(),
        modules_display: join_or_dash(modules),
        risks,
    }
}

fn extract_risk(text: &str) -> Option<String> {
    let lowered = text.to_ascii_lowercase();
    let keywords = ["todo", "fixme", "wip", "risk", "blocker", "follow-up"];
    if keywords.iter().any(|keyword| lowered.contains(keyword)) {
        Some(normalize_whitespace(text))
    } else {
        None
    }
}

fn build_report_info(
    request: &ReportRequest,
    commit_count: usize,
    file_count: usize,
) -> ReportInfo {
    ReportInfo {
        kind: request.kind.as_str().to_string(),
        title: request.kind.display_name().to_string(),
        start_date: request.start_date.format("%Y-%m-%d").to_string(),
        end_date: request.end_date.format("%Y-%m-%d").to_string(),
        repo_count: request.repo_paths.len(),
        commit_count,
        file_count,
        is_daily: matches!(request.kind, ReportKind::Daily),
        is_weekly: matches!(request.kind, ReportKind::Weekly),
    }
}

fn collect_file_count(commits: &[CommitInfo]) -> usize {
    commits
        .iter()
        .flat_map(|commit| commit.files.iter())
        .collect::<BTreeSet<_>>()
        .len()
}

fn build_daily_logs(
    request: &ReportRequest,
    commits: &[CommitInfo],
    docs: &[DocumentInfo],
) -> Vec<DailyLogInfo> {
    let mut logs = Vec::new();
    let mut cursor = request.start_date;
    while cursor <= request.end_date {
        let date = cursor.format("%Y-%m-%d").to_string();
        let commit_items = commits
            .iter()
            .filter(|commit| commit.date == date)
            .map(|commit| {
                if request.repo_paths.len() > 1 {
                    format!("{}：{}", commit.repo_name, commit.summary)
                } else {
                    commit.summary.clone()
                }
            })
            .collect::<Vec<_>>();
        let commit_risks = commits
            .iter()
            .filter(|commit| commit.date == date)
            .flat_map(|commit| [commit.subject.as_str(), commit.body.as_str()])
            .filter_map(extract_risk)
            .collect::<Vec<_>>();
        let day_doc_entries = docs
            .iter()
            .filter(|doc| doc.entry_date.as_deref() == Some(date.as_str()))
            .collect::<Vec<_>>();

        let mut items = extract_daily_doc_sections(&day_doc_entries, "工作内容");
        if items.is_empty() {
            items = commit_items;
        }

        let mut risks = extract_daily_doc_sections(&day_doc_entries, "问题");
        if risks.is_empty() {
            let extra = extract_daily_doc_sections(&day_doc_entries, "困难");
            if !extra.is_empty() {
                risks = extra;
            }
        }
        if risks.is_empty() {
            risks = commit_risks;
        }

        let mut solutions = extract_daily_doc_sections(&day_doc_entries, "解决方案");
        if solutions.is_empty() {
            solutions = extract_daily_doc_sections(&day_doc_entries, "进展");
        }
        if solutions.is_empty() && !items.is_empty() {
            solutions.push("已形成对应提交与文档记录，可在周报中补充业务结果。".to_string());
        }

        logs.push(DailyLogInfo {
            label: weekday_label(cursor.weekday()).to_string(),
            date,
            items_display: join_or_dash(&items),
            risks_display: join_or_dash(&risks),
            solutions_display: join_or_dash(&solutions),
            items,
            risks,
            solutions,
        });

        let Some(next_day) = cursor.checked_add_days(Days::new(1)) else {
            break;
        };
        cursor = next_day;
    }
    logs
}

fn extract_daily_doc_sections(docs: &[&DocumentInfo], keyword: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let lowered_keyword = keyword.to_ascii_lowercase();

    for doc in docs {
        for line in doc.content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let normalized = normalize_whitespace(trimmed);
            let lowered = normalized.to_ascii_lowercase();
            let prefixes = [
                format!("{keyword}："),
                format!("{keyword}:"),
                format!("## {keyword}"),
                format!("### {keyword}"),
                format!("- {keyword}："),
                format!("- {keyword}:"),
            ];

            let mut matched = None;
            for prefix in &prefixes {
                if normalized.starts_with(prefix) {
                    matched = Some(normalized[prefix.len()..].trim().to_string());
                    break;
                }
            }

            if let Some(value) = matched {
                if !value.is_empty() {
                    entries.push(value);
                }
                continue;
            }

            if lowered.contains(&lowered_keyword) && trimmed.starts_with("- ") {
                let value = normalized.trim_start_matches("- ").trim().to_string();
                if !value.is_empty() {
                    entries.push(value);
                }
            }
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use super::*;
    use crate::core::types::{PolishOptions, PptOptions, ReportRequest};

    #[test]
    fn resolves_default_output_name() {
        let request = ReportRequest {
            kind: ReportKind::Daily,
            repo_paths: vec![PathBuf::from(".")],
            template_path: None,
            output_path: None,
            output_dir: Some(PathBuf::from("reports")),
            doc_paths: Vec::new(),
            author: None,
            author_match_mode: crate::core::types::AuthorMatchMode::NameOrEmail,
            start_date: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            max_docs: 6,
            max_doc_chars: 280,
            polish: PolishOptions::default(),
            ppt: PptOptions::default(),
        };
        let output = resolve_output_path(&request, "My Repo").unwrap();
        assert_eq!(output, PathBuf::from("reports/daily-my-repo-2025-02-14.md"));
    }

    #[test]
    fn daily_logs_prefer_explicit_daily_doc_content() {
        let request = ReportRequest {
            kind: ReportKind::Weekly,
            repo_paths: vec![PathBuf::from(".")],
            template_path: None,
            output_path: None,
            output_dir: Some(PathBuf::from("reports")),
            doc_paths: Vec::new(),
            author: None,
            author_match_mode: crate::core::types::AuthorMatchMode::NameOrEmail,
            start_date: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            max_docs: 6,
            max_doc_chars: 280,
            polish: PolishOptions::default(),
            ppt: PptOptions::default(),
        };

        let commits = vec![CommitInfo {
            repo_name: "demo".to_string(),
            repo_path: "/tmp/demo".to_string(),
            hash: "abc".to_string(),
            short_hash: "abc".to_string(),
            author: "Raphael".to_string(),
            email: "raphael@example.com".to_string(),
            date: "2025-02-14".to_string(),
            subject: "feat: add cli".to_string(),
            summary: "完善命令行入口".to_string(),
            body: String::new(),
            files: vec!["src/main.rs".to_string()],
            files_display: "src/main.rs".to_string(),
            modules: vec!["src".to_string()],
            modules_display: "src".to_string(),
        }];

        let docs = vec![DocumentInfo {
            path: "2025-02-14.md".to_string(),
            title: "2025-02-14".to_string(),
            excerpt: "工作内容：联调支付流程".to_string(),
            content: "工作内容：联调支付流程\n问题：测试环境回调不稳定\n解决方案：补充重试和日志"
                .to_string(),
            entry_date: Some("2025-02-14".to_string()),
        }];

        let logs = build_daily_logs(&request, &commits, &docs);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].items_display, "联调支付流程".to_string());
        assert_eq!(logs[0].risks_display, "测试环境回调不稳定".to_string());
        assert_eq!(logs[0].solutions_display, "补充重试和日志".to_string());
    }
}
