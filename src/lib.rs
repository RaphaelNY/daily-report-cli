//! `daily_git` library entry point.
//!
//! 该库负责：
//! - 收集目标仓库的 Git 提交
//! - 扫描关联项目文档
//! - 按模板生成 Markdown 日报 / 周报
//! - 默认尝试调用本机 `codex` 做自然语言润色

mod collectors;
mod config;
mod core;
mod reporting;
mod update;

pub use config::{load_config, LoadedConfig, ReportFileConfig};
pub use core::types::{
    AuthorMatchMode, GeneratedReport, PolishOptions, PolishState, PptOptions, ReportKind,
    ReportRequest,
};
pub use reporting::generate_report;
pub use update::{run_update, UpdateOptions, UpdateResult, UpdateState};
