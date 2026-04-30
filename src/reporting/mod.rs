//! 报告生成主流程编排。

mod polish;
mod ppt;
mod template;

use std::fs;

use anyhow::{Context, Result};

use crate::collectors::docs::collect_docs;
use crate::collectors::git::{collect_commits, ensure_git_repo, load_repo_info};
use crate::core::types::{GeneratedReport, ReportRequest};
use crate::core::utils::collect_modules;

use self::polish::polish_markdown;
use self::ppt::generate_weekly_ppt;
use self::template::{build_context, render_markdown, resolve_output_path};

/// 生成单份日报或周报。
///
/// 流程上先保证原始 Markdown 一定可生成，再尝试用 Codex 做润色；
/// 即便润色失败，也会保留原始结果写入文件。
pub fn generate_report(request: &ReportRequest) -> Result<GeneratedReport> {
    let repo_path = request
        .repo_path
        .canonicalize()
        .with_context(|| format!("failed to access repo path {}", request.repo_path.display()))?;

    ensure_git_repo(&repo_path)?;

    let repo = load_repo_info(&repo_path)?;
    let commits = collect_commits(request, &repo_path)?;
    let modules = collect_modules(commits.iter().flat_map(|commit| commit.files.iter()));
    let docs = collect_docs(request, &repo_path, &modules)?;
    let context = build_context(request, repo.clone(), commits, docs);

    let rendered = render_markdown(request.kind, request.template_path.as_deref(), &context)?;
    let polished = polish_markdown(&request.polish, &repo, request.kind, &rendered);

    let output_path = resolve_output_path(request, &repo.name)?;
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir {}", parent.display()))?;
    }
    fs::write(&output_path, polished.content)
        .with_context(|| format!("failed to write report {}", output_path.display()))?;

    let ppt_path = if request.ppt.enabled {
        Some(generate_weekly_ppt(request, &context)?)
    } else {
        None
    };

    Ok(GeneratedReport {
        output_path,
        polish_state: polished.state,
        ppt_path,
    })
}
