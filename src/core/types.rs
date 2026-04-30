//! 跨模块共享的数据模型。

use std::fmt;
use std::path::PathBuf;

use chrono::NaiveDate;
use serde::Serialize;

/// 报告类型。
#[derive(Debug, Clone, Copy)]
pub enum ReportKind {
    Daily,
    Weekly,
}

impl ReportKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Daily => "日报",
            Self::Weekly => "周报",
        }
    }
}

/// 大模型润色配置。
#[derive(Debug, Clone)]
pub struct PolishOptions {
    pub enabled: bool,
    pub model: Option<String>,
    pub timeout_secs: u64,
    pub codex_home: Option<PathBuf>,
}

impl Default for PolishOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            model: None,
            timeout_secs: 90,
            codex_home: None,
        }
    }
}

/// 周报 HTML PPT deck 输出配置。
#[derive(Debug, Clone, Default)]
pub struct PptOptions {
    pub enabled: bool,
    pub output_dir: Option<PathBuf>,
}

/// 生成报告所需的完整请求。
#[derive(Debug, Clone)]
pub struct ReportRequest {
    pub kind: ReportKind,
    pub repo_path: PathBuf,
    pub template_path: Option<PathBuf>,
    pub output_path: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub doc_paths: Vec<PathBuf>,
    pub author: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub max_docs: usize,
    pub max_doc_chars: usize,
    pub polish: PolishOptions,
    pub ppt: PptOptions,
}

/// 报告生成结果。
#[derive(Debug, Clone)]
pub struct GeneratedReport {
    pub output_path: PathBuf,
    pub polish_state: PolishState,
    pub ppt_path: Option<PathBuf>,
}

/// Codex 润色状态。
#[derive(Debug, Clone)]
pub enum PolishState {
    Applied,
    Skipped(String),
    Failed(String),
}

impl fmt::Display for PolishState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Applied => write!(f, "已通过 Codex 润色"),
            Self::Skipped(reason) => write!(f, "已跳过（{reason}）"),
            Self::Failed(reason) => write!(f, "润色失败，已回退原始内容（{reason}）"),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct ReportContext {
    pub(crate) generated_at: String,
    pub(crate) repo: RepoInfo,
    pub(crate) report: ReportInfo,
    pub(crate) summary: SummaryInfo,
    pub(crate) commits: Vec<CommitInfo>,
    pub(crate) docs: Vec<DocumentInfo>,
    pub(crate) daily_logs: Vec<DailyLogInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct RepoInfo {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) branch: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct ReportInfo {
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) start_date: String,
    pub(crate) end_date: String,
    pub(crate) commit_count: usize,
    pub(crate) file_count: usize,
    pub(crate) is_daily: bool,
    pub(crate) is_weekly: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct SummaryInfo {
    pub(crate) highlights: Vec<String>,
    pub(crate) modules: Vec<String>,
    pub(crate) modules_display: String,
    pub(crate) risks: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct CommitInfo {
    pub(crate) hash: String,
    pub(crate) short_hash: String,
    pub(crate) author: String,
    pub(crate) email: String,
    pub(crate) date: String,
    pub(crate) subject: String,
    pub(crate) body: String,
    pub(crate) files: Vec<String>,
    pub(crate) files_display: String,
    pub(crate) modules: Vec<String>,
    pub(crate) modules_display: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DocumentInfo {
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) excerpt: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct DailyLogInfo {
    pub(crate) label: String,
    pub(crate) date: String,
    pub(crate) items: Vec<String>,
    pub(crate) items_display: String,
    pub(crate) risks: Vec<String>,
    pub(crate) risks_display: String,
}
