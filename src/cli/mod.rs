//! CLI 参数解析。
//!
//! 这里单独拆出命令行相关代码，避免 `main.rs` 混入业务逻辑。

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Local, NaiveDate};
use clap::ArgAction;
use clap::{Args, Parser, Subcommand};

use daily_git::{
    load_config, LoadedConfig, PolishOptions, PptOptions, ReportFileConfig, ReportKind,
    ReportRequest, UpdateOptions,
};

/// 顶层命令定义。
#[derive(Parser, Debug)]
#[command(
    name = "daily_git",
    version,
    about = "Generate daily and weekly Git markdown reports from commits and project docs."
)]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

pub(crate) enum AppCommand {
    Generate(ReportRequest),
    Update(UpdateOptions),
}

#[derive(Subcommand, Debug)]
enum Command {
    Daily(DailyArgs),
    Weekly(WeeklyArgs),
    Update(UpdateArgs),
}

#[derive(Args, Debug)]
struct CommonArgs {
    #[arg(long)]
    repo: Option<PathBuf>,

    #[arg(long)]
    template: Option<PathBuf>,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long)]
    output_dir: Option<PathBuf>,

    #[arg(long = "doc")]
    docs: Vec<PathBuf>,

    #[arg(long)]
    author: Option<String>,

    #[arg(long)]
    max_docs: Option<usize>,

    #[arg(long)]
    max_doc_chars: Option<usize>,

    #[arg(long)]
    no_polish: bool,

    #[arg(long, action = ArgAction::SetTrue, overrides_with = "no_polish")]
    polish: bool,

    #[arg(long)]
    polish_model: Option<String>,

    #[arg(long)]
    polish_timeout_secs: Option<u64>,

    #[arg(long)]
    codex_home: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct DailyArgs {
    #[command(flatten)]
    common: CommonArgs,

    #[arg(long, value_parser = parse_date)]
    date: Option<NaiveDate>,
}

#[derive(Args, Debug)]
struct WeeklyArgs {
    #[command(flatten)]
    common: CommonArgs,

    #[arg(long, value_parser = parse_date)]
    end_date: Option<NaiveDate>,

    #[arg(long)]
    days: Option<i64>,

    #[arg(long)]
    no_ppt: bool,

    #[arg(long, action = ArgAction::SetTrue, overrides_with = "no_ppt")]
    ppt: bool,

    #[arg(long)]
    ppt_output_dir: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct UpdateArgs {
    #[arg(long)]
    check: bool,

    #[arg(long)]
    version: Option<String>,

    #[arg(long)]
    force: bool,

    #[arg(long, hide = true)]
    release_repo: Option<String>,
}

impl Cli {
    /// 将命令行参数转换成内部请求对象。
    pub(crate) fn into_command(self) -> Result<AppCommand> {
        let loaded_config = load_config(self.config.as_deref())?;
        match self.command {
            Command::Daily(args) => {
                let date = args
                    .date
                    .or_else(|| {
                        loaded_config
                            .as_ref()
                            .and_then(|config| config.values.daily.date)
                    })
                    .unwrap_or_else(|| Local::now().date_naive());
                build_request(
                    args.common,
                    ReportKind::Daily,
                    date,
                    date,
                    None,
                    loaded_config,
                )
                .map(AppCommand::Generate)
            }
            Command::Weekly(args) => {
                let end_date = args
                    .end_date
                    .or_else(|| {
                        loaded_config
                            .as_ref()
                            .and_then(|config| config.values.weekly.end_date)
                    })
                    .unwrap_or_else(|| Local::now().date_naive());
                let days = args
                    .days
                    .or_else(|| {
                        loaded_config
                            .as_ref()
                            .and_then(|config| config.values.weekly.days)
                    })
                    .unwrap_or(7)
                    .max(1);
                let start_date = end_date
                    .checked_sub_signed(Duration::days(days - 1))
                    .context("failed to calculate weekly start date")?;
                build_request(
                    args.common,
                    ReportKind::Weekly,
                    start_date,
                    end_date,
                    Some(WeeklyPptArgs {
                        enabled: args.ppt,
                        disabled: args.no_ppt,
                        output_dir: args.ppt_output_dir,
                    }),
                    loaded_config,
                )
                .map(AppCommand::Generate)
            }
            Command::Update(args) => Ok(AppCommand::Update(UpdateOptions {
                check_only: args.check,
                requested_version: args.version,
                force: args.force,
                release_repo: args.release_repo,
            })),
        }
    }
}

fn parse_date(value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|error| format!("invalid date `{value}`: {error}"))
}

fn build_request(
    common: CommonArgs,
    kind: ReportKind,
    start_date: NaiveDate,
    end_date: NaiveDate,
    weekly_ppt_args: Option<WeeklyPptArgs>,
    loaded_config: Option<LoadedConfig>,
) -> Result<ReportRequest> {
    let config_values = loaded_config.as_ref().map(|config| &config.values);
    validate_output_conflict(&common, config_values)?;

    let CommonArgs {
        repo,
        template,
        output,
        output_dir,
        docs,
        author,
        max_docs,
        max_doc_chars,
        no_polish,
        polish,
        polish_model,
        polish_timeout_secs,
        codex_home,
    } = common;

    let repo_path = resolve_path(
        repo,
        config_values.and_then(|config| config.repo.clone()),
        loaded_config.as_ref(),
        PathBuf::from("."),
    );

    let template_path = resolve_optional_path(
        template,
        config_values.and_then(|config| config.template.clone()),
        loaded_config.as_ref(),
    );

    let (output_path, output_dir) =
        resolve_output_targets(output, output_dir, config_values, loaded_config.as_ref());

    let doc_paths = if !docs.is_empty() {
        docs
    } else if let Some(config) = loaded_config.as_ref() {
        config.resolve_paths(&config.values.docs)
    } else {
        Vec::new()
    };

    let author = author.or_else(|| config_values.and_then(|config| config.author.clone()));

    let max_docs = max_docs
        .or_else(|| config_values.and_then(|config| config.max_docs))
        .unwrap_or(6);

    let max_doc_chars = max_doc_chars
        .or_else(|| config_values.and_then(|config| config.max_doc_chars))
        .unwrap_or(280);

    let polish_enabled = resolve_polish_enabled(polish, no_polish, config_values);
    let polish_model =
        polish_model.or_else(|| config_values.and_then(|config| config.polish.model.clone()));
    let polish_timeout_secs = polish_timeout_secs
        .or_else(|| config_values.and_then(|config| config.polish.timeout_secs))
        .unwrap_or(90)
        .max(1);
    let codex_home = resolve_optional_path(
        codex_home,
        config_values.and_then(|config| config.polish.codex_home.clone()),
        loaded_config.as_ref(),
    );
    let ppt = resolve_ppt_options(kind, weekly_ppt_args, config_values, loaded_config.as_ref());

    Ok(ReportRequest {
        kind,
        repo_path,
        template_path,
        output_path,
        output_dir,
        doc_paths,
        author,
        start_date,
        end_date,
        max_docs,
        max_doc_chars,
        polish: PolishOptions {
            enabled: polish_enabled,
            model: polish_model,
            timeout_secs: polish_timeout_secs,
            codex_home,
        },
        ppt,
    })
}

#[derive(Debug)]
struct WeeklyPptArgs {
    enabled: bool,
    disabled: bool,
    output_dir: Option<PathBuf>,
}

fn validate_output_conflict(common: &CommonArgs, config: Option<&ReportFileConfig>) -> Result<()> {
    if common.output.is_some() && common.output_dir.is_some() {
        bail!("`--output` and `--output-dir` cannot be used together");
    }

    if common.output.is_none()
        && common.output_dir.is_none()
        && config
            .map(|config| config.output.is_some() && config.output_dir.is_some())
            .unwrap_or(false)
    {
        bail!("`config.yaml` cannot set both `output` and `output_dir`");
    }

    Ok(())
}

fn resolve_output_targets(
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    config: Option<&ReportFileConfig>,
    loaded_config: Option<&LoadedConfig>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if let Some(path) = output {
        return (Some(path), None);
    }

    if let Some(dir) = output_dir {
        return (None, Some(dir));
    }

    let output_path =
        config
            .and_then(|config| config.output.clone())
            .map(|path| match loaded_config {
                Some(config) => config.resolve_path(&path),
                None => path,
            });

    let output_dir = config
        .and_then(|config| config.output_dir.clone())
        .map(|path| match loaded_config {
            Some(config) => config.resolve_path(&path),
            None => path,
        });

    (output_path, output_dir)
}

fn resolve_path(
    cli_value: Option<PathBuf>,
    config_value: Option<PathBuf>,
    loaded_config: Option<&LoadedConfig>,
    default_value: PathBuf,
) -> PathBuf {
    if let Some(path) = cli_value {
        return path;
    }

    if let Some(path) = config_value {
        if let Some(config) = loaded_config {
            return config.resolve_path(&path);
        }
        return path;
    }

    default_value
}

fn resolve_optional_path(
    cli_value: Option<PathBuf>,
    config_value: Option<PathBuf>,
    loaded_config: Option<&LoadedConfig>,
) -> Option<PathBuf> {
    if let Some(path) = cli_value {
        return Some(path);
    }

    config_value.map(|path| match loaded_config {
        Some(config) => config.resolve_path(&path),
        None => path,
    })
}

fn resolve_polish_enabled(
    polish: bool,
    no_polish: bool,
    config: Option<&ReportFileConfig>,
) -> bool {
    if polish {
        return true;
    }

    if no_polish {
        return false;
    }

    config
        .and_then(|config| config.polish.enabled)
        .unwrap_or(true)
}

fn resolve_ppt_options(
    kind: ReportKind,
    weekly_ppt_args: Option<WeeklyPptArgs>,
    config: Option<&ReportFileConfig>,
    loaded_config: Option<&LoadedConfig>,
) -> PptOptions {
    if !matches!(kind, ReportKind::Weekly) {
        return PptOptions::default();
    }

    let config_ppt = config.map(|config| &config.weekly.ppt);
    let Some(weekly_ppt_args) = weekly_ppt_args else {
        return PptOptions {
            enabled: config_ppt.and_then(|ppt| ppt.enabled).unwrap_or(false),
            output_dir: config_ppt
                .and_then(|ppt| resolve_optional_path(None, ppt.output_dir.clone(), loaded_config)),
        };
    };

    let enabled = if weekly_ppt_args.enabled {
        true
    } else if weekly_ppt_args.disabled {
        false
    } else {
        config_ppt.and_then(|ppt| ppt.enabled).unwrap_or(false)
    };

    let output_dir = resolve_optional_path(
        weekly_ppt_args.output_dir,
        config_ppt.and_then(|ppt| ppt.output_dir.clone()),
        loaded_config,
    );

    PptOptions {
        enabled,
        output_dir,
    }
}
