//! 报告生成主流程编排。

mod polish;
mod ppt;
mod template;

use std::fs;

use anyhow::{Context, Result};

use crate::collectors::docs::collect_docs;
use crate::collectors::git::{collect_commits, ensure_git_repo, load_repo_info};
use crate::core::types::{GeneratedReport, RepoSnapshot, ReportRequest};
use crate::core::utils::{collect_modules, summarize_repo_names};

use self::polish::{polish_markdown, summarize_commits};
use self::ppt::generate_weekly_ppt;
use self::template::{build_context, render_markdown, resolve_output_path};

/// 生成单份日报或周报。
///
/// 流程上先保证原始 Markdown 一定可生成，再尝试用 Codex 做润色；
/// 即便润色失败，也会保留原始结果写入文件。
pub fn generate_report(request: &ReportRequest) -> Result<GeneratedReport> {
    let mut repos = Vec::new();
    let mut repo_snapshots = Vec::new();
    let mut commits = Vec::new();
    let mut docs = Vec::new();

    for repo_path in &request.repo_paths {
        let canonical_repo_path = repo_path
            .canonicalize()
            .with_context(|| format!("failed to access repo path {}", repo_path.display()))?;

        ensure_git_repo(&canonical_repo_path)?;

        let repo = load_repo_info(&canonical_repo_path)?;
        let repo_commits = collect_commits(request, &repo, &canonical_repo_path)?;
        let repo_modules =
            collect_modules(repo_commits.iter().flat_map(|commit| commit.files.iter()));
        let repo_docs = collect_docs(request, &repo, &canonical_repo_path, &repo_modules)?;

        repo_snapshots.push(RepoSnapshot {
            name: repo.name.clone(),
            path: repo.path.clone(),
            branch: repo.branch.clone(),
        });
        repos.push(repo);
        commits.extend(repo_commits);
        docs.extend(repo_docs);
    }

    commits.sort_by(|left, right| {
        right
            .date
            .cmp(&left.date)
            .then_with(|| right.hash.cmp(&left.hash))
    });
    docs.sort_by(|left, right| left.path.cmp(&right.path));

    let exec_repo = repos
        .first()
        .cloned()
        .context("at least one repository is required to generate a report")?;
    let primary_repo = primary_repo_snapshot(&repos);
    let commit_summaries = summarize_commits(&request.polish, &exec_repo, &commits);
    for (commit, summary) in commits.iter_mut().zip(commit_summaries) {
        commit.summary = summary;
    }
    let context = build_context(request, primary_repo.clone(), repo_snapshots, commits, docs);

    let rendered = render_markdown(request.kind, request.template_path.as_deref(), &context)?;
    let polished = polish_markdown(&request.polish, &exec_repo, request.kind, &rendered);

    let output_path = resolve_output_path(request, &primary_repo.name)?;
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

fn primary_repo_snapshot(repos: &[crate::core::types::RepoInfo]) -> crate::core::types::RepoInfo {
    if repos.len() == 1 {
        return repos[0].clone();
    }

    let repo_names = repos
        .iter()
        .map(|repo| repo.name.clone())
        .collect::<Vec<_>>();
    crate::core::types::RepoInfo {
        name: summarize_repo_names(&repo_names),
        path: repos
            .iter()
            .map(|repo| repo.path.clone())
            .collect::<Vec<_>>()
            .join(", "),
        branch: repos
            .iter()
            .map(|repo| format!("{}:{}", repo.name, repo.branch))
            .collect::<Vec<_>>()
            .join(", "),
    }
}
