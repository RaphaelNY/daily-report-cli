//! 模板上下文构建与 Markdown 渲染。

use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    handlebars.register_escape_fn(handlebars::no_escape);
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
    let multi_repo = commits
        .iter()
        .map(|commit| commit.repo_name.as_str())
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let highlights = summarize_commit_groups(commits, multi_repo);
    let work_items = build_work_items(commits, modules, multi_repo);

    let risks = commits
        .iter()
        .flat_map(|commit| [commit.subject.as_str(), commit.body.as_str()])
        .filter_map(extract_risk)
        .fold(Vec::new(), |mut acc, risk| {
            if !acc.contains(&risk) {
                acc.push(risk);
            }
            acc
        });
    let plan_items = build_plan_items(&highlights, modules, &risks);

    SummaryInfo {
        highlights,
        work_items,
        plan_items,
        modules: modules.to_vec(),
        modules_display: join_or_dash(modules),
        risks,
    }
}

fn build_work_items(commits: &[CommitInfo], modules: &[String], multi_repo: bool) -> Vec<String> {
    if commits.is_empty() {
        return Vec::new();
    }

    struct WorkItemCandidate {
        module: String,
        item: String,
        commit_count: usize,
        has_non_routine: bool,
    }

    let mut candidates = Vec::new();
    for module in modules {
        let related = commits
            .iter()
            .filter(|commit| commit.modules.iter().any(|item| item == module))
            .collect::<Vec<_>>();
        if related.is_empty() {
            continue;
        }

        let mut routine_summaries = Vec::new();
        let mut non_routine_summaries = Vec::new();
        let mut summary_seen = HashSet::new();
        for commit in &related {
            let summary = summarized_highlight_text(&commit.summary);
            if !summary.is_empty() && summary_seen.insert(summary.clone()) {
                if is_routine_release_item(&summary) {
                    routine_summaries.push(summary);
                } else {
                    non_routine_summaries.push(summary);
                }
            }
        }

        let has_non_routine = !non_routine_summaries.is_empty();
        let summaries = if has_non_routine {
            &non_routine_summaries
        } else {
            &routine_summaries
        };
        let details = if summaries.is_empty() {
            "整理相关文件与提交记录".to_string()
        } else {
            summaries
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join("；")
        };
        let scope = if multi_repo {
            let mut repos = Vec::new();
            for commit in &related {
                if !repos.contains(&commit.repo_name) {
                    repos.push(commit.repo_name.clone());
                }
            }
            format!("{}：", repos.join("、"))
        } else {
            String::new()
        };
        let item = format!(
            "{scope}围绕 `{module}` 完成 {} 次提交，主要涉及：{details}",
            related.len()
        );
        candidates.push(WorkItemCandidate {
            module: module.clone(),
            item,
            commit_count: related.len(),
            has_non_routine,
        });
    }

    candidates.sort_by(|left, right| {
        right
            .has_non_routine
            .cmp(&left.has_non_routine)
            .then_with(|| right.commit_count.cmp(&left.commit_count))
            .then_with(|| left.module.cmp(&right.module))
    });

    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for candidate in candidates {
        if seen.insert(candidate.item.clone()) {
            items.push(candidate.item);
        }
        if items.len() >= 5 {
            break;
        }
    }

    if items.is_empty() {
        return summarize_commit_groups(commits, multi_repo)
            .into_iter()
            .take(5)
            .map(|item| format!("推进相关提交：{item}"))
            .collect();
    }

    items
}

fn build_plan_items(highlights: &[String], modules: &[String], risks: &[String]) -> Vec<String> {
    if !risks.is_empty() {
        return risks
            .iter()
            .take(5)
            .map(|risk| format!("优先跟进：{risk}"))
            .collect();
    }

    let mut plans = Vec::new();
    let mut seen = HashSet::new();

    for highlight in highlights {
        if is_routine_release_item(highlight) {
            continue;
        }

        let item = format!("跟进“{highlight}”的验证、联调与收尾工作");
        if seen.insert(item.clone()) {
            plans.push(item);
        }

        if plans.len() >= 5 {
            return plans;
        }
    }

    for highlight in highlights {
        let item = format!("跟进“{highlight}”的后续验证与说明补充");
        if seen.insert(item.clone()) {
            plans.push(item);
        }

        if plans.len() >= 5 {
            return plans;
        }
    }

    for module in modules.iter().take(5) {
        let item = format!("围绕 `{module}` 模块继续补充验证与完善工作");
        if seen.insert(item.clone()) {
            plans.push(item);
        }
    }

    plans
}

fn is_routine_release_item(highlight: &str) -> bool {
    let lowered = highlight.to_ascii_lowercase();
    (lowered.contains("版本") || lowered.contains("release") || lowered.contains("锁文件"))
        && !lowered.contains("多仓库")
        && !lowered.contains("报告")
        && !lowered.contains("模板")
}

fn extract_risk(text: &str) -> Option<String> {
    let lowered = text.to_ascii_lowercase();
    let strong_keywords = [
        "fixme",
        "wip",
        "blocker",
        "rollback",
        "revert",
        "failed",
        "failure",
        "permission denied",
        "待处理",
        "未完成",
        "临时",
        "回退",
        "失败",
        "异常",
        "权限",
        "阻塞",
        "风险",
    ];
    let contextual_keywords = [
        "todo",
        "pending",
        "temporary",
        "follow-up",
        "followup",
        "跟进",
    ];
    let has_strong_keyword = strong_keywords
        .iter()
        .any(|keyword| lowered.contains(keyword));
    let has_risk_phrase = lowered.starts_with("risk:")
        || lowered.starts_with("risk ")
        || lowered.contains(" known risk")
        || lowered.contains(" at risk")
        || lowered.contains(" high risk")
        || lowered.contains(" low risk")
        || lowered.contains(" medium risk");
    let has_contextual_keyword = contextual_keywords
        .iter()
        .any(|keyword| lowered.contains(keyword));
    let looks_like_issue_statement = text.contains("问题")
        || text.contains("困难")
        || text.contains("待确认")
        || text.contains(":")
        || text.contains("：");

    if has_strong_keyword
        || has_risk_phrase
        || (has_contextual_keyword && looks_like_issue_statement)
    {
        Some(normalize_whitespace(text))
    } else {
        None
    }
}

fn aggregate_commit_summaries(commits: &[CommitInfo]) -> Vec<(String, Vec<String>)> {
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    let mut indexes: BTreeMap<String, usize> = BTreeMap::new();

    for commit in commits {
        let summary = summarized_highlight_text(&commit.summary);
        if summary.is_empty() {
            continue;
        }

        if let Some(index) = indexes.get(&summary) {
            let repos = &mut grouped[*index].1;
            if !repos.contains(&commit.repo_name) {
                repos.push(commit.repo_name.clone());
            }
            continue;
        }

        indexes.insert(summary.clone(), grouped.len());
        grouped.push((summary, vec![commit.repo_name.clone()]));
    }

    grouped
}

fn summarize_commit_groups(commits: &[CommitInfo], multi_repo: bool) -> Vec<String> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    if multi_repo {
        for (summary, repos) in aggregate_commit_summaries(commits) {
            let display = format!("{}：{}", repos.join("、"), summary);
            if seen.insert(display.clone()) {
                items.push(display);
            }
        }
        return items;
    }

    for commit in commits {
        let normalized = summarized_highlight_text(&commit.summary);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            items.push(normalized);
        }
    }
    items
}

fn summarized_highlight_text(summary: &str) -> String {
    let normalized = normalize_whitespace(summary);
    if normalized.is_empty() {
        return normalized;
    }

    if is_routine_release_item(&normalized) {
        return "同步完成版本升级、锁文件刷新与相关说明更新".to_string();
    }

    normalized
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
        let day_commits = commits
            .iter()
            .filter(|commit| commit.date == date)
            .collect::<Vec<_>>();
        let commit_items = summarize_daily_commit_items(&day_commits, request.repo_paths.len() > 1);
        let commit_risks = day_commits
            .iter()
            .flat_map(|commit| [commit.subject.as_str(), commit.body.as_str()])
            .filter_map(extract_risk)
            .fold(Vec::new(), |mut acc, risk| {
                if !acc.contains(&risk) {
                    acc.push(risk);
                }
                acc
            });
        let day_doc_entries = docs
            .iter()
            .filter(|doc| doc.entry_date.as_deref() == Some(date.as_str()))
            .collect::<Vec<_>>();

        let mut items = dedupe_entries(extract_daily_doc_sections(&day_doc_entries, "工作内容"));
        if items.is_empty() {
            items = commit_items;
        }

        let mut risks = dedupe_entries(extract_daily_doc_sections(&day_doc_entries, "问题"));
        if risks.is_empty() {
            let extra = dedupe_entries(extract_daily_doc_sections(&day_doc_entries, "困难"));
            if !extra.is_empty() {
                risks = extra;
            }
        }
        if risks.is_empty() {
            risks = commit_risks;
        }

        let mut solutions =
            dedupe_entries(extract_daily_doc_sections(&day_doc_entries, "解决方案"));
        if solutions.is_empty() {
            solutions = dedupe_entries(extract_daily_doc_sections(&day_doc_entries, "进展"));
        }
        if solutions.is_empty() && !items.is_empty() {
            solutions.push("已形成对应提交与文档记录，可在周报中补充业务结果。".to_string());
        }
        if items.is_empty() && risks.is_empty() && solutions.is_empty() {
            items.push("无相关提交或文档记录。".to_string());
            risks.push("无".to_string());
            solutions.push("无后续处理项。".to_string());
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

fn summarize_daily_commit_items(commits: &[&CommitInfo], multi_repo: bool) -> Vec<String> {
    if multi_repo {
        let owned = commits
            .iter()
            .map(|commit| (*commit).clone())
            .collect::<Vec<_>>();
        return summarize_commit_groups(&owned, true);
    }

    let owned = commits
        .iter()
        .map(|commit| (*commit).clone())
        .collect::<Vec<_>>();
    summarize_commit_groups(&owned, false)
}

fn dedupe_entries(entries: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let normalized = normalize_whitespace(&entry);
        if !normalized.is_empty() && seen.insert(normalized.clone()) {
            deduped.push(normalized);
        }
    }
    deduped
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
            files_compact_display: "src/main.rs".to_string(),
            modules: vec!["src".to_string()],
            modules_display: "src".to_string(),
        }];

        let docs = vec![DocumentInfo {
            repo_name: "demo".to_string(),
            repo_path: "/tmp/demo".to_string(),
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

    #[test]
    fn render_markdown_keeps_markdown_backticks() {
        let context = ReportContext {
            generated_at: "2025-02-14 10:00:00".to_string(),
            repo: RepoInfo {
                name: "demo".to_string(),
                path: "/tmp/demo".to_string(),
                branch: "main".to_string(),
            },
            repos: Vec::new(),
            report: ReportInfo {
                kind: "daily".to_string(),
                title: "日报".to_string(),
                start_date: "2025-02-14".to_string(),
                end_date: "2025-02-14".to_string(),
                repo_count: 1,
                commit_count: 0,
                file_count: 0,
                is_daily: true,
                is_weekly: false,
            },
            summary: SummaryInfo {
                highlights: Vec::new(),
                work_items: Vec::new(),
                plan_items: Vec::new(),
                modules: vec!["src".to_string()],
                modules_display: "src, `README.md`".to_string(),
                risks: Vec::new(),
            },
            commits: Vec::new(),
            docs: Vec::new(),
            daily_logs: Vec::new(),
        };

        let rendered = render_markdown(ReportKind::Daily, None, &context).unwrap();
        assert!(rendered.contains("`README.md`"));
        assert!(!rendered.contains("&#x60;"));
    }

    #[test]
    fn build_summary_merges_same_multi_repo_highlight() {
        let commits = vec![
            CommitInfo {
                repo_name: "repo-a".to_string(),
                repo_path: "/tmp/repo-a".to_string(),
                hash: "1".to_string(),
                short_hash: "1".to_string(),
                author: "Raphael".to_string(),
                email: "raphael@example.com".to_string(),
                date: "2025-02-14".to_string(),
                subject: "feat: add cli".to_string(),
                summary: "完善命令行入口".to_string(),
                body: String::new(),
                files: vec!["src/main.rs".to_string()],
                files_display: "src/main.rs".to_string(),
                files_compact_display: "src/main.rs".to_string(),
                modules: vec!["src".to_string()],
                modules_display: "src".to_string(),
            },
            CommitInfo {
                repo_name: "repo-b".to_string(),
                repo_path: "/tmp/repo-b".to_string(),
                hash: "2".to_string(),
                short_hash: "2".to_string(),
                author: "Raphael".to_string(),
                email: "raphael@example.com".to_string(),
                date: "2025-02-14".to_string(),
                subject: "feat: add cli".to_string(),
                summary: "完善命令行入口".to_string(),
                body: String::new(),
                files: vec!["src/lib.rs".to_string()],
                files_display: "src/lib.rs".to_string(),
                files_compact_display: "src/lib.rs".to_string(),
                modules: vec!["src".to_string()],
                modules_display: "src".to_string(),
            },
        ];

        let summary = build_summary(&commits, &["src".to_string()]);
        assert_eq!(
            summary.highlights,
            vec!["repo-a、repo-b：完善命令行入口".to_string()]
        );
        assert_eq!(
            summary.work_items,
            vec!["repo-a、repo-b：围绕 `src` 完成 2 次提交，主要涉及：完善命令行入口".to_string()]
        );
        assert!(!summary.plan_items.is_empty());
        assert!(summary.plan_items[0].contains("完善命令行入口"));
    }

    #[test]
    fn plan_items_skip_routine_release_bumps_when_other_work_exists() {
        let summary = build_summary(
            &[
                CommitInfo {
                    repo_name: "demo".to_string(),
                    repo_path: "/tmp/demo".to_string(),
                    hash: "1".to_string(),
                    short_hash: "1".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-14".to_string(),
                    subject: "feat: update version to 0.1.3".to_string(),
                    summary: "将版本升级至0.1.3并同步更新说明文档与安装脚本".to_string(),
                    body: String::new(),
                    files: vec!["Cargo.toml".to_string()],
                    files_display: "Cargo.toml".to_string(),
                    files_compact_display: "Cargo.toml".to_string(),
                    modules: vec!["Cargo.toml".to_string()],
                    modules_display: "Cargo.toml".to_string(),
                },
                CommitInfo {
                    repo_name: "demo".to_string(),
                    repo_path: "/tmp/demo".to_string(),
                    hash: "2".to_string(),
                    short_hash: "2".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-14".to_string(),
                    subject: "feat: add weekly ppt".to_string(),
                    summary: "新增周报网页幻灯片生成功能并补充模板与配置支持".to_string(),
                    body: String::new(),
                    files: vec!["src/reporting/ppt.rs".to_string()],
                    files_display: "src/reporting/ppt.rs".to_string(),
                    files_compact_display: "src/reporting/ppt.rs".to_string(),
                    modules: vec!["src".to_string()],
                    modules_display: "src".to_string(),
                },
            ],
            &["src".to_string()],
        );

        assert!(summary
            .plan_items
            .iter()
            .any(|item| item.contains("周报网页幻灯片")));
        assert!(summary.plan_items.len() <= 5);
    }

    #[test]
    fn summary_collapses_multiple_release_bumps_into_one_highlight() {
        let summary = build_summary(
            &[
                CommitInfo {
                    repo_name: "demo".to_string(),
                    repo_path: "/tmp/demo".to_string(),
                    hash: "1".to_string(),
                    short_hash: "1".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-14".to_string(),
                    subject: "feat: bump version".to_string(),
                    summary: "将版本升级至0.1.3并同步更新说明文档与安装脚本".to_string(),
                    body: String::new(),
                    files: vec!["Cargo.toml".to_string()],
                    files_display: "Cargo.toml".to_string(),
                    files_compact_display: "Cargo.toml".to_string(),
                    modules: vec!["Cargo.toml".to_string()],
                    modules_display: "Cargo.toml".to_string(),
                },
                CommitInfo {
                    repo_name: "demo".to_string(),
                    repo_path: "/tmp/demo".to_string(),
                    hash: "2".to_string(),
                    short_hash: "2".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-14".to_string(),
                    subject: "fix: refresh lockfile".to_string(),
                    summary: "为0.1.2版本发布刷新并校准项目依赖锁文件内容".to_string(),
                    body: String::new(),
                    files: vec!["Cargo.lock".to_string()],
                    files_display: "Cargo.lock".to_string(),
                    files_compact_display: "Cargo.lock".to_string(),
                    modules: vec!["Cargo.lock".to_string()],
                    modules_display: "Cargo.lock".to_string(),
                },
            ],
            &["Cargo.toml".to_string()],
        );

        assert_eq!(
            summary.highlights,
            vec!["同步完成版本升级、锁文件刷新与相关说明更新".to_string()]
        );
    }

    #[test]
    fn follow_up_commit_subject_is_not_treated_as_risk() {
        assert_eq!(extract_risk("focus report follow-up plan items"), None);
        assert_eq!(
            extract_risk("todo: follow up release"),
            Some("todo: follow up release".to_string())
        );
    }

    #[test]
    fn plain_improvement_subject_with_risks_word_is_not_treated_as_risk() {
        assert_eq!(extract_risk("avoid false positive report risks"), None);
        assert_eq!(
            extract_risk("risk: release process still needs manual verification"),
            Some("risk: release process still needs manual verification".to_string())
        );
    }
}
