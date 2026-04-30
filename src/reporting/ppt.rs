//! 周报 HTML PPT deck 生成。
//!
//! 这里复用用户本机已安装的 `html-ppt` skill 资产：
//! - 复制共享 CSS / JS 到输出目录
//! - 复用 `weekly-report` deck 的视觉风格
//! - 基于已采集的周报上下文本地渲染 HTML，避免模型凭空补充事实

use std::cmp::max;
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use handlebars::Handlebars;
use serde::Serialize;
use walkdir::WalkDir;

use crate::core::types::{CommitInfo, ReportContext, ReportKind, ReportRequest};
use crate::core::utils::{normalize_whitespace, sanitize_name};

const WEEKLY_PPT_TEMPLATE: &str = include_str!("../../templates/weekly-ppt.html.hbs");
const EXTRA_STYLE: &str = r#"
.tpl-weekly-report .ship-detail{font-size:13px;margin:2px 0 0}
.tpl-weekly-report .chart-pill{background:var(--surface-2);color:var(--text-2)}
.tpl-weekly-report .chart-caption{font-size:13px;margin-top:36px}
.tpl-weekly-report .module-list{display:flex;flex-direction:column;gap:14px;max-width:980px}
.tpl-weekly-report .module-row{display:grid;grid-template-columns:minmax(180px,240px) 1fr 92px;gap:18px;align-items:center}
.tpl-weekly-report .module-meta{display:flex;flex-direction:column;gap:4px}
.tpl-weekly-report .module-name{font-weight:700;color:var(--text-1)}
.tpl-weekly-report .module-summary{font-size:12px;color:var(--text-3)}
.tpl-weekly-report .module-bar{height:14px;background:var(--surface-2);border-radius:999px;overflow:hidden}
.tpl-weekly-report .module-bar span{display:block;height:100%;background:var(--grad);border-radius:999px;min-width:12px}
.tpl-weekly-report .module-count{font-family:'JetBrains Mono',monospace;font-size:12px;color:var(--text-2);text-align:right}
.tpl-weekly-report .doc-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:18px}
.tpl-weekly-report .doc-card{background:var(--surface);border:1px solid var(--border);border-radius:var(--radius);padding:22px;box-shadow:var(--shadow);min-height:210px}
.tpl-weekly-report .doc-card h3{font-size:20px;line-height:1.25;margin:10px 0 12px}
.tpl-weekly-report .doc-card p{font-size:13px;color:var(--text-2);line-height:1.6;margin:0}
.tpl-weekly-report .doc-path{font-family:'JetBrains Mono',monospace;font-size:11px;color:var(--accent)}
.tpl-weekly-report .blocker.ok{border-left-color:var(--good)}
.tpl-weekly-report .blocker.ok h4{color:var(--good)}
"#;
const SLIDE_TOTAL: &str = "7";

#[derive(Debug, Serialize)]
struct WeeklyDeckContext {
    deck_title: String,
    repo_name: String,
    repo_branch: String,
    week_range: String,
    generated_at: String,
    cover_headline: String,
    cover_summary: String,
    cover_notes: String,
    slide_total: &'static str,
    metrics: Vec<KpiCard>,
    metrics_footer: String,
    metrics_notes: String,
    highlight_count_label: String,
    highlights: Vec<HighlightItem>,
    highlights_footer: String,
    highlights_notes: String,
    activity_days: Vec<ActivityDay>,
    activity_badge: String,
    activity_caption: String,
    activity_footer: String,
    activity_notes: String,
    modules: Vec<ModuleRow>,
    modules_footer: String,
    modules_notes: String,
    docs: Vec<DocCard>,
    docs_footer: String,
    docs_notes: String,
    risks: Vec<RiskItem>,
    risks_footer: String,
    risks_notes: String,
}

#[derive(Debug, Serialize)]
struct KpiCard {
    label: String,
    value: String,
    delta: String,
    tone_class: String,
    delta_class: String,
}

#[derive(Debug, Serialize)]
struct HighlightItem {
    tag_class: String,
    tag_label: String,
    title: String,
    detail: String,
    owner: String,
}

#[derive(Debug, Serialize)]
struct ActivityDay {
    label: String,
    count_label: String,
    height_percent: String,
}

#[derive(Debug, Serialize)]
struct ModuleRow {
    name: String,
    summary: String,
    bar_width: String,
    count_label: String,
}

#[derive(Debug, Serialize)]
struct DocCard {
    path: String,
    title: String,
    excerpt: String,
}

#[derive(Debug, Serialize)]
struct RiskItem {
    title: String,
    detail: String,
    meta: String,
    tone_class: String,
}

/// 生成周报 deck，返回入口 HTML 文件路径。
pub(crate) fn generate_weekly_ppt(
    request: &ReportRequest,
    context: &ReportContext,
) -> Result<PathBuf> {
    if !request.ppt.enabled {
        bail!("weekly PPT generation is disabled");
    }

    if !matches!(request.kind, ReportKind::Weekly) {
        bail!("weekly PPT is only supported for weekly reports");
    }

    let skill_root = find_html_ppt_skill_root(request.polish.codex_home.as_deref())?;
    let output_dir = resolve_ppt_output_dir(request, &context.repo.name)?;

    fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create PPT output dir {}", output_dir.display()))?;
    copy_shared_assets(&skill_root, &output_dir)?;
    write_style_file(&skill_root, &output_dir)?;

    let html = render_weekly_ppt(&build_deck_context(context))?;
    let index_path = output_dir.join("index.html");
    fs::write(&index_path, html)
        .with_context(|| format!("failed to write PPT deck {}", index_path.display()))?;
    Ok(index_path)
}

fn render_weekly_ppt(context: &WeeklyDeckContext) -> Result<String> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(false);
    handlebars
        .register_template_string("weekly_ppt", WEEKLY_PPT_TEMPLATE)
        .context("failed to register weekly PPT template")?;
    handlebars
        .render("weekly_ppt", context)
        .context("failed to render weekly PPT")
}

fn resolve_ppt_output_dir(request: &ReportRequest, repo_name: &str) -> Result<PathBuf> {
    if let Some(path) = &request.ppt.output_dir {
        return Ok(path.clone());
    }

    let base_dir = if let Some(output_path) = &request.output_path {
        output_path
            .parent()
            .map(Path::to_path_buf)
            .or_else(|| env::current_dir().ok())
            .context("failed to determine PPT output base dir")?
    } else if let Some(output_dir) = &request.output_dir {
        output_dir.clone()
    } else {
        env::current_dir().context("failed to determine current directory")?
    };

    Ok(base_dir.join(format!(
        "{}-{}-{}-ppt",
        request.kind.as_str(),
        sanitize_name(repo_name),
        request.end_date.format("%Y-%m-%d")
    )))
}

fn find_html_ppt_skill_root(codex_home_override: Option<&Path>) -> Result<PathBuf> {
    let codex_home = resolve_codex_home(codex_home_override)?;
    let skills_dir = codex_home.join("skills");
    if !skills_dir.is_dir() {
        bail!(
            "html-ppt skill not found: {} does not contain a skills directory",
            codex_home.display()
        );
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

        let content = fs::read_to_string(entry.path())
            .with_context(|| format!("failed to read skill manifest {}", entry.path().display()))?;
        if content.lines().any(|line| line.trim() == "name: html-ppt") {
            let root = entry
                .path()
                .parent()
                .context("skill manifest missing parent directory")?;
            return Ok(root.to_path_buf());
        }
    }

    bail!(
        "html-ppt skill not found under {}. Install it or set --codex-home correctly",
        skills_dir.display()
    )
}

fn resolve_codex_home(codex_home_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = codex_home_override {
        return Ok(path.to_path_buf());
    }

    if let Some(path) = env::var_os("CODEX_HOME") {
        return Ok(PathBuf::from(path));
    }

    let Some(home) = env::var_os("HOME") else {
        bail!("failed to determine CODEX_HOME; pass --codex-home or set CODEX_HOME");
    };

    Ok(PathBuf::from(home).join(".codex"))
}

fn copy_shared_assets(skill_root: &Path, output_dir: &Path) -> Result<()> {
    for relative in [
        Path::new("assets/fonts.css"),
        Path::new("assets/base.css"),
        Path::new("assets/runtime.js"),
        Path::new("assets/animations/animations.css"),
    ] {
        let source = skill_root.join(relative);
        let target = output_dir.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create asset dir {}", parent.display()))?;
        }
        fs::copy(&source, &target).with_context(|| {
            format!(
                "failed to copy html-ppt asset {} to {}",
                source.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn write_style_file(skill_root: &Path, output_dir: &Path) -> Result<()> {
    let style_path = skill_root.join("templates/full-decks/weekly-report/style.css");
    let mut style = fs::read_to_string(&style_path).with_context(|| {
        format!(
            "failed to read weekly-report style {}",
            style_path.display()
        )
    })?;
    style.push_str(EXTRA_STYLE);
    fs::write(output_dir.join("style.css"), style).with_context(|| {
        format!(
            "failed to write weekly-report style {}",
            output_dir.join("style.css").display()
        )
    })
}

fn build_deck_context(context: &ReportContext) -> WeeklyDeckContext {
    let commit_count = context.report.commit_count;
    let file_count = context.report.file_count;
    let module_count = context.summary.modules.len();
    let docs_count = context.docs.len();
    let risk_count = context.summary.risks.len();
    let active_days = context
        .daily_logs
        .iter()
        .filter(|log| !log.items.is_empty())
        .count();
    let unique_authors = context
        .commits
        .iter()
        .map(|commit| commit.author.clone())
        .collect::<BTreeSet<_>>()
        .len();

    WeeklyDeckContext {
        deck_title: format!(
            "{} 周报 {} ~ {}",
            context.repo.name, context.report.start_date, context.report.end_date
        ),
        repo_name: context.repo.name.clone(),
        repo_branch: context.repo.branch.clone(),
        week_range: format!("{} → {}", context.report.start_date, context.report.end_date),
        generated_at: context.generated_at.clone(),
        cover_headline: build_cover_headline(commit_count, file_count),
        cover_summary: build_cover_summary(active_days, module_count, docs_count, risk_count),
        cover_notes: format!(
            "本页用于说明本周时间范围与整体工作量。统计区间为 {} 到 {}，数据来源于 Git 提交与自动扫描到的 Markdown 文档。",
            context.report.start_date, context.report.end_date
        ),
        slide_total: SLIDE_TOTAL,
        metrics: build_metrics(
            commit_count,
            file_count,
            module_count,
            docs_count,
            active_days,
            unique_authors,
            risk_count,
            &context.repo.branch,
        ),
        metrics_footer: format!(
            "{} 个活跃作者 · {} 天周期",
            unique_authors,
            context.daily_logs.len()
        ),
        metrics_notes: "这一页适合先讲总量，再强调风险项是否需要额外关注。".to_string(),
        highlight_count_label: format!("{} items", context.commits.len().min(6)),
        highlights: build_highlights(&context.commits),
        highlights_footer: format!("按提交时间倒序展示，最多保留 6 条。"),
        highlights_notes: "高亮条目直接来自 commit subject，不额外改写事实。可重点挑 2 到 3 条业务或工程影响最大的变更展开。".to_string(),
        activity_days: build_activity_days(&context.daily_logs),
        activity_badge: build_activity_badge(&context.daily_logs),
        activity_caption: build_activity_caption(&context.daily_logs),
        activity_footer: "按天统计提交条数，帮助快速判断活跃时段。".to_string(),
        activity_notes: "如果某两天明显更活跃，可以顺势解释当时集中推进的模块或交付。".to_string(),
        modules: build_modules(&context.commits),
        modules_footer: format!("共覆盖 {} 个一级模块。", module_count),
        modules_notes: "模块分布按变更文件聚合，能够反映本周精力主要落在哪些目录。".to_string(),
        docs: build_docs(context),
        docs_footer: if docs_count > 0 {
            format!("自动引用 {} 份 Markdown 文档。", docs_count)
        } else {
            "本周未引用额外 Markdown 文档。".to_string()
        },
        docs_notes: "文档摘要来自自动扫描结果，适合补充本周涉及的设计、说明或 README 背景。".to_string(),
        risks: build_risks(context),
        risks_footer: if risk_count > 0 {
            format!("自动命中 {} 条风险信号，建议人工复核。", risk_count)
        } else {
            "未从提交文本中识别到显式风险关键词。".to_string()
        },
        risks_notes: "风险页只基于规则命中的提交文本信号生成，适合作为会前检查清单，而不是最终结论。".to_string(),
    }
}

fn build_cover_headline(commit_count: usize, file_count: usize) -> String {
    if commit_count == 0 {
        "本周暂无代码提交。".to_string()
    } else {
        format!(
            "本周共整理 {} 次提交，涉及 {} 个文件。",
            commit_count, file_count
        )
    }
}

fn build_cover_summary(
    active_days: usize,
    module_count: usize,
    docs_count: usize,
    risk_count: usize,
) -> String {
    let mut parts = vec![
        format!("{active_days} 天有提交"),
        format!("覆盖 {module_count} 个模块"),
    ];
    if docs_count > 0 {
        parts.push(format!("参考 {docs_count} 份文档"));
    }
    if risk_count > 0 {
        parts.push(format!("{risk_count} 条风险信号待人工确认"));
    } else {
        parts.push("未识别到显式风险关键词".to_string());
    }
    format!("{}。", parts.join("，"))
}

fn build_metrics(
    commit_count: usize,
    file_count: usize,
    module_count: usize,
    docs_count: usize,
    active_days: usize,
    unique_authors: usize,
    risk_count: usize,
    branch: &str,
) -> Vec<KpiCard> {
    vec![
        metric_card(
            "Commits",
            commit_count.to_string(),
            "本周提交总数",
            commit_count > 0,
            false,
        ),
        metric_card(
            "Files",
            file_count.to_string(),
            "涉及唯一文件数",
            file_count > 0,
            false,
        ),
        metric_card(
            "Modules",
            module_count.to_string(),
            "一级目录聚合",
            module_count > 0,
            false,
        ),
        metric_card(
            "Docs",
            docs_count.to_string(),
            "自动扫描到的文档",
            docs_count > 0,
            false,
        ),
        metric_card(
            "Active days",
            active_days.to_string(),
            "区间内有提交的天数",
            active_days > 0,
            false,
        ),
        metric_card(
            "Authors",
            unique_authors.to_string(),
            "参与提交的作者数",
            unique_authors > 1,
            false,
        ),
        metric_card(
            "Risks",
            risk_count.to_string(),
            "命中风险关键词条数",
            risk_count == 0,
            risk_count > 0,
        ),
        KpiCard {
            label: "Branch".to_string(),
            value: truncate(branch, 16),
            delta: "当前分支".to_string(),
            tone_class: String::new(),
            delta_class: "flat".to_string(),
        },
    ]
}

fn metric_card(
    label: &str,
    value: String,
    delta: &str,
    is_positive: bool,
    is_risky: bool,
) -> KpiCard {
    let tone_class = if is_risky {
        "warn"
    } else if is_positive {
        "good"
    } else {
        ""
    };
    let delta_class = if is_risky {
        "down"
    } else if is_positive {
        "up"
    } else {
        "flat"
    };

    KpiCard {
        label: label.to_string(),
        value,
        delta: delta.to_string(),
        tone_class: tone_class.to_string(),
        delta_class: delta_class.to_string(),
    }
}

fn build_highlights(commits: &[CommitInfo]) -> Vec<HighlightItem> {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();

    for commit in commits {
        let title = normalize_whitespace(&commit.subject);
        if title.is_empty() || !seen.insert(title.clone()) {
            continue;
        }

        let (tag_class, tag_label) = classify_commit(&title);
        let detail = if !commit.modules.is_empty() {
            format!(
                "{} · 模块 {}",
                commit.date,
                truncate(&commit.modules_display, 56)
            )
        } else if !commit.files.is_empty() {
            format!("{} · {}", commit.date, truncate(&commit.files_display, 56))
        } else {
            format!("{} · 未记录具体文件", commit.date)
        };

        items.push(HighlightItem {
            tag_class: tag_class.to_string(),
            tag_label: tag_label.to_string(),
            title,
            detail,
            owner: commit.author.clone(),
        });

        if items.len() >= 6 {
            break;
        }
    }

    if items.is_empty() {
        items.push(HighlightItem {
            tag_class: "exp".to_string(),
            tag_label: "INFO".to_string(),
            title: "本周没有匹配到可展示的提交亮点".to_string(),
            detail: "当前时间范围内未读取到 commit subject。".to_string(),
            owner: "-".to_string(),
        });
    }

    items
}

fn classify_commit(subject: &str) -> (&'static str, &'static str) {
    let lowered = subject.to_ascii_lowercase();
    if lowered.starts_with("fix") || lowered.contains("bug") || lowered.contains("hotfix") {
        ("fix", "FIX")
    } else if lowered.starts_with("feat")
        || lowered.starts_with("add")
        || lowered.starts_with("docs")
        || lowered.starts_with("refactor")
    {
        ("feat", "FEAT")
    } else if lowered.starts_with("chore")
        || lowered.starts_with("build")
        || lowered.starts_with("ci")
        || lowered.starts_with("infra")
    {
        ("infra", "INFRA")
    } else {
        ("exp", "CHANGE")
    }
}

fn build_activity_days(daily_logs: &[crate::core::types::DailyLogInfo]) -> Vec<ActivityDay> {
    let max_count = daily_logs
        .iter()
        .map(|log| log.items.len())
        .max()
        .unwrap_or(0);
    let scale = max(1, max_count);

    daily_logs
        .iter()
        .map(|log| {
            let count = log.items.len();
            let height = if count == 0 {
                12
            } else {
                18 + (count * 70 / scale)
            };
            ActivityDay {
                label: short_date(&log.date),
                count_label: format!("{count} 条"),
                height_percent: format!("{height}%"),
            }
        })
        .collect()
}

fn build_activity_badge(daily_logs: &[crate::core::types::DailyLogInfo]) -> String {
    let peak = daily_logs
        .iter()
        .map(|log| log.items.len())
        .max()
        .unwrap_or(0);
    if peak == 0 {
        "暂无提交".to_string()
    } else {
        format!("峰值：{} 条/天", peak)
    }
}

fn build_activity_caption(daily_logs: &[crate::core::types::DailyLogInfo]) -> String {
    let active = daily_logs
        .iter()
        .filter(|log| !log.items.is_empty())
        .map(|log| log.label.clone())
        .collect::<Vec<_>>();
    if active.is_empty() {
        "本周没有收集到提交记录。".to_string()
    } else {
        format!("活跃日：{}。", active.join("、"))
    }
}

fn build_modules(commits: &[CommitInfo]) -> Vec<ModuleRow> {
    let mut modules: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for commit in commits {
        for file in &commit.files {
            let module = file
                .split('/')
                .next()
                .unwrap_or(file)
                .trim()
                .trim_matches('.');
            if module.is_empty() {
                continue;
            }
            modules
                .entry(module.to_string())
                .or_default()
                .insert(file.clone());
        }
    }

    let mut rows = modules
        .into_iter()
        .map(|(name, files)| {
            let count = files.len();
            let summary = truncate(
                &files.into_iter().take(3).collect::<Vec<_>>().join(" / "),
                72,
            );
            (name, count, summary)
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let max_count = rows.first().map(|row| row.1).unwrap_or(1);
    let mut items = rows
        .into_iter()
        .take(6)
        .map(|(name, count, summary)| ModuleRow {
            name,
            summary: if summary.is_empty() {
                "-".to_string()
            } else {
                summary
            },
            bar_width: format!("{}%", 20 + (count * 80 / max_count.max(1))),
            count_label: format!("{count} files"),
        })
        .collect::<Vec<_>>();

    if items.is_empty() {
        items.push(ModuleRow {
            name: "暂无模块变更".to_string(),
            summary: "本周没有读取到变更文件。".to_string(),
            bar_width: "20%".to_string(),
            count_label: "0 files".to_string(),
        });
    }

    items
}

fn build_docs(context: &ReportContext) -> Vec<DocCard> {
    let mut docs = context
        .docs
        .iter()
        .take(4)
        .map(|doc| DocCard {
            path: doc.path.clone(),
            title: doc.title.clone(),
            excerpt: truncate(&normalize_whitespace(&doc.excerpt), 180),
        })
        .collect::<Vec<_>>();

    if docs.is_empty() {
        docs.push(DocCard {
            path: "README / docs / doc".to_string(),
            title: "未发现额外参考文档".to_string(),
            excerpt: "当前周报仅基于 Git 提交生成，未扫描到可引用的 Markdown 文档。".to_string(),
        });
    }

    docs
}

fn build_risks(context: &ReportContext) -> Vec<RiskItem> {
    let mut seen = BTreeSet::new();
    let mut items = context
        .summary
        .risks
        .iter()
        .filter(|risk| seen.insert((*risk).clone()))
        .take(4)
        .map(|risk| RiskItem {
            title: truncate(risk, 96),
            detail:
                "由提交标题或正文中的关键词自动抽取，建议在正式汇报前人工确认影响范围与优先级。"
                    .to_string(),
            meta: format!(
                "repo: {} · range: {} → {}",
                context.repo.name, context.report.start_date, context.report.end_date
            ),
            tone_class: String::new(),
        })
        .collect::<Vec<_>>();

    if items.is_empty() {
        items.push(RiskItem {
            title: "未识别到显式风险关键词".to_string(),
            detail: "commit 文本中未出现 todo / fixme / wip / blocker 等规则信号。".to_string(),
            meta: format!("branch: {}", context.repo.branch),
            tone_class: "ok".to_string(),
        });
    }

    items
}

fn short_date(date: &str) -> String {
    if date.len() >= 10 {
        date[5..10].replace('-', "/")
    } else {
        date.to_string()
    }
}

fn truncate(input: &str, max_chars: usize) -> String {
    let normalized = normalize_whitespace(input);
    let mut truncated = normalized.chars().take(max_chars).collect::<String>();
    if normalized.chars().count() > max_chars {
        truncated.push('…');
    }
    truncated
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use super::*;
    use crate::core::types::{
        AuthorMatchMode, DailyLogInfo, DocumentInfo, PolishOptions, RepoInfo, RepoSnapshot,
        ReportInfo, ReportRequest, SummaryInfo,
    };

    #[test]
    fn finds_installed_skill_by_manifest_name() {
        let temp = tempfile::tempdir().unwrap();
        let skill_root = temp.path().join("skills/custom-weekly-deck");
        fs::create_dir_all(&skill_root).unwrap();
        fs::write(skill_root.join("SKILL.md"), "---\nname: html-ppt\n---\n").unwrap();

        let found = find_html_ppt_skill_root(Some(temp.path())).unwrap();
        assert_eq!(found, skill_root);
    }

    #[test]
    fn resolves_default_ppt_output_dir() {
        let request = ReportRequest {
            kind: ReportKind::Weekly,
            repo_paths: vec![PathBuf::from(".")],
            template_path: None,
            output_path: None,
            output_dir: Some(PathBuf::from("reports")),
            doc_paths: Vec::new(),
            author: None,
            author_match_mode: AuthorMatchMode::NameOrEmail,
            start_date: NaiveDate::from_ymd_opt(2025, 2, 10).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2025, 2, 14).unwrap(),
            max_docs: 6,
            max_doc_chars: 280,
            polish: PolishOptions::default(),
            ppt: crate::core::types::PptOptions {
                enabled: true,
                output_dir: None,
            },
        };

        let path = resolve_ppt_output_dir(&request, "My Repo").unwrap();
        assert_eq!(path, PathBuf::from("reports/weekly-my-repo-2025-02-14-ppt"));
    }

    #[test]
    fn renders_weekly_ppt_with_repo_name() {
        let context = build_deck_context(&sample_context());
        let rendered = render_weekly_ppt(&context).unwrap();
        assert!(rendered.contains("demo-repo"));
        assert!(rendered.contains("每日提交分布"));
    }

    fn sample_context() -> ReportContext {
        ReportContext {
            generated_at: "2025-02-14 18:00:00".to_string(),
            repo: RepoInfo {
                name: "demo-repo".to_string(),
                path: "/tmp/demo-repo".to_string(),
                branch: "main".to_string(),
            },
            repos: vec![RepoSnapshot {
                name: "demo-repo".to_string(),
                path: "/tmp/demo-repo".to_string(),
                branch: "main".to_string(),
            }],
            report: ReportInfo {
                kind: "weekly".to_string(),
                title: "周报".to_string(),
                start_date: "2025-02-10".to_string(),
                end_date: "2025-02-14".to_string(),
                repo_count: 1,
                commit_count: 2,
                file_count: 3,
                is_daily: false,
                is_weekly: true,
            },
            summary: SummaryInfo {
                highlights: vec!["feat: add cli".to_string()],
                plan_items: vec!["跟进“feat: add cli”的验证与收尾工作".to_string()],
                modules: vec!["src".to_string(), "docs".to_string()],
                modules_display: "src, docs".to_string(),
                risks: vec!["todo: follow up release".to_string()],
            },
            commits: vec![
                CommitInfo {
                    repo_name: "demo-repo".to_string(),
                    repo_path: "/tmp/demo-repo".to_string(),
                    hash: "abc".to_string(),
                    short_hash: "abc".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-14".to_string(),
                    subject: "feat: add cli".to_string(),
                    summary: "新增命令行入口支持".to_string(),
                    body: String::new(),
                    files: vec!["src/main.rs".to_string(), "src/cli/mod.rs".to_string()],
                    files_display: "src/main.rs, src/cli/mod.rs".to_string(),
                    files_compact_display: "src/main.rs, src/cli/mod.rs".to_string(),
                    modules: vec!["src".to_string()],
                    modules_display: "src".to_string(),
                },
                CommitInfo {
                    repo_name: "demo-repo".to_string(),
                    repo_path: "/tmp/demo-repo".to_string(),
                    hash: "def".to_string(),
                    short_hash: "def".to_string(),
                    author: "Raphael".to_string(),
                    email: "raphael@example.com".to_string(),
                    date: "2025-02-13".to_string(),
                    subject: "docs: update readme".to_string(),
                    summary: "更新项目说明文档".to_string(),
                    body: "todo: follow up release".to_string(),
                    files: vec!["README.md".to_string()],
                    files_display: "README.md".to_string(),
                    files_compact_display: "README.md".to_string(),
                    modules: vec!["README.md".to_string()],
                    modules_display: "README.md".to_string(),
                },
            ],
            docs: vec![DocumentInfo {
                repo_name: "demo-repo".to_string(),
                repo_path: "/tmp/demo-repo".to_string(),
                path: "README.md".to_string(),
                title: "README".to_string(),
                excerpt: "Project overview".to_string(),
                content: "Project overview".to_string(),
                entry_date: None,
            }],
            daily_logs: vec![
                DailyLogInfo {
                    label: "周一".to_string(),
                    date: "2025-02-10".to_string(),
                    items: Vec::new(),
                    items_display: "-".to_string(),
                    risks: Vec::new(),
                    risks_display: "-".to_string(),
                    solutions: Vec::new(),
                    solutions_display: "-".to_string(),
                },
                DailyLogInfo {
                    label: "周二".to_string(),
                    date: "2025-02-11".to_string(),
                    items: Vec::new(),
                    items_display: "-".to_string(),
                    risks: Vec::new(),
                    risks_display: "-".to_string(),
                    solutions: Vec::new(),
                    solutions_display: "-".to_string(),
                },
                DailyLogInfo {
                    label: "周三".to_string(),
                    date: "2025-02-12".to_string(),
                    items: Vec::new(),
                    items_display: "-".to_string(),
                    risks: Vec::new(),
                    risks_display: "-".to_string(),
                    solutions: Vec::new(),
                    solutions_display: "-".to_string(),
                },
                DailyLogInfo {
                    label: "周四".to_string(),
                    date: "2025-02-13".to_string(),
                    items: vec!["docs: update readme".to_string()],
                    items_display: "docs: update readme".to_string(),
                    risks: vec!["todo: follow up release".to_string()],
                    risks_display: "todo: follow up release".to_string(),
                    solutions: Vec::new(),
                    solutions_display: "-".to_string(),
                },
                DailyLogInfo {
                    label: "周五".to_string(),
                    date: "2025-02-14".to_string(),
                    items: vec!["feat: add cli".to_string()],
                    items_display: "feat: add cli".to_string(),
                    risks: Vec::new(),
                    risks_display: "-".to_string(),
                    solutions: Vec::new(),
                    solutions_display: "-".to_string(),
                },
            ],
        }
    }
}
