//! `config.yaml` 读取与路径解析。
//!
//! 规则：
//! - 显式 `--config <path>` 优先
//! - 若未显式指定，则尝试读取当前目录下的 `config.yaml` / `config.yml`
//! - 配置文件内的相对路径，以配置文件所在目录为基准解析

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use serde::Deserialize;

/// YAML 配置文件的完整结构。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReportFileConfig {
    pub repo: Option<PathBuf>,
    pub template: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    #[serde(default)]
    pub docs: Vec<PathBuf>,
    pub author: Option<String>,
    pub max_docs: Option<usize>,
    pub max_doc_chars: Option<usize>,
    #[serde(default)]
    pub polish: PolishFileConfig,
    #[serde(default)]
    pub daily: DailyFileConfig,
    #[serde(default)]
    pub weekly: WeeklyFileConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PolishFileConfig {
    pub enabled: Option<bool>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
    pub codex_home: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DailyFileConfig {
    pub date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WeeklyFileConfig {
    pub end_date: Option<NaiveDate>,
    pub days: Option<i64>,
}

/// 已加载配置及其路径基准。
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub path: PathBuf,
    pub base_dir: PathBuf,
    pub values: ReportFileConfig,
}

impl LoadedConfig {
    /// 将配置中的相对路径解析为基于配置文件目录的路径。
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        }
    }

    /// 批量解析配置里的路径列表。
    pub fn resolve_paths(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths.iter().map(|path| self.resolve_path(path)).collect()
    }
}

/// 读取配置文件。
///
/// 若 `explicit_path` 为空，则按默认文件名探测当前目录。
pub fn load_config(explicit_path: Option<&Path>) -> Result<Option<LoadedConfig>> {
    let path = match explicit_path {
        Some(path) => Some(path.to_path_buf()),
        None => default_config_path()?,
    };

    let Some(path) = path else {
        return Ok(None);
    };

    let absolute_path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("failed to determine current directory")?
            .join(path)
    };

    let content = fs::read_to_string(&absolute_path)
        .with_context(|| format!("failed to read config {}", absolute_path.display()))?;
    let values: ReportFileConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("failed to parse yaml config {}", absolute_path.display()))?;

    let base_dir = absolute_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    Ok(Some(LoadedConfig {
        path: absolute_path,
        base_dir,
        values,
    }))
}

fn default_config_path() -> Result<Option<PathBuf>> {
    let cwd = std::env::current_dir().context("failed to determine current directory")?;
    for name in ["config.yaml", "config.yml"] {
        let candidate = cwd.join(name);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_against_config_dir() {
        let loaded = LoadedConfig {
            path: PathBuf::from("/tmp/demo/config.yaml"),
            base_dir: PathBuf::from("/tmp/demo"),
            values: ReportFileConfig::default(),
        };

        assert_eq!(
            loaded.resolve_path(Path::new("templates/daily.md.hbs")),
            PathBuf::from("/tmp/demo/templates/daily.md.hbs")
        );
    }

    #[test]
    fn parses_yaml_config() {
        let config: ReportFileConfig = serde_yaml::from_str(
            r#"
repo: .
template: ./templates/周报与日报_markdown_模板.md
author: Raphael
max_docs: 8
polish:
  enabled: true
  timeout_secs: 120
weekly:
  days: 5
"#,
        )
        .unwrap();

        assert_eq!(config.author.as_deref(), Some("Raphael"));
        assert_eq!(config.max_docs, Some(8));
        assert_eq!(config.polish.timeout_secs, Some(120));
        assert_eq!(config.weekly.days, Some(5));
    }
}
