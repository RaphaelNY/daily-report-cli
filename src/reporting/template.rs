//! 模板上下文构建与 Markdown 渲染。

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{Datelike, Days, Local};
use handlebars::Handlebars;

use crate::core::types::{
    CommitInfo, DailyLogInfo, DocumentInfo, RepoInfo, ReportContext, ReportInfo, ReportKind,
    ReportRequest, SummaryInfo,
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
    commits: Vec<CommitInfo>,
    docs: Vec<DocumentInfo>,
) -> ReportContext {
    let modules = collect_modules(commits.iter().flat_map(|commit| commit.files.iter()));
    let summary = build_summary(&commits, &modules);
    let report = build_report_info(request, commits.len(), collect_file_count(&commits));
    let daily_logs = build_daily_logs(request, &commits);

    ReportContext {
        generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        repo,
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
    for commit in commits {
        let normalized = normalize_whitespace(&commit.subject);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            highlights.push(normalized);
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

fn build_daily_logs(request: &ReportRequest, commits: &[CommitInfo]) -> Vec<DailyLogInfo> {
    let mut logs = Vec::new();
    let mut cursor = request.start_date;
    while cursor <= request.end_date {
        let date = cursor.format("%Y-%m-%d").to_string();
        let items = commits
            .iter()
            .filter(|commit| commit.date == date)
            .map(|commit| commit.subject.clone())
            .collect::<Vec<_>>();
        let risks = commits
            .iter()
            .filter(|commit| commit.date == date)
            .flat_map(|commit| [commit.subject.as_str(), commit.body.as_str()])
            .filter_map(extract_risk)
            .collect::<Vec<_>>();

        logs.push(DailyLogInfo {
            label: weekday_label(cursor.weekday()).to_string(),
            date,
            items_display: join_or_dash(&items),
            risks_display: join_or_dash(&risks),
            items,
            risks,
        });

        let Some(next_day) = cursor.checked_add_days(Days::new(1)) else {
            break;
        };
        cursor = next_day;
    }
    logs
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
            repo_path: PathBuf::from("."),
            template_path: None,
            output_path: None,
            output_dir: Some(PathBuf::from("reports")),
            doc_paths: Vec::new(),
            author: None,
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
}
