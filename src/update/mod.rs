//! 通过 GitHub Release 分发的安装与自更新逻辑。

use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use tempfile::tempdir;
use ureq::Agent;

const DEFAULT_RELEASE_REPO: &str = "RaphaelNY/daily-report-cli";
const DEFAULT_API_BASE: &str = "https://api.github.com";
const BINARY_NAME: &str = "daily_git";
const SHARE_DIR_NAME: &str = "daily_git";

#[derive(Debug, Clone, Default)]
pub struct UpdateOptions {
    pub check_only: bool,
    pub requested_version: Option<String>,
    pub force: bool,
    pub release_repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    UpToDate,
    Available,
    Updated,
}

#[derive(Debug, Clone)]
pub struct UpdateResult {
    pub state: UpdateState,
    pub current_version: String,
    pub target_version: String,
    pub executable_path: PathBuf,
    pub release_repo: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

struct InstallLayout {
    share_dir: PathBuf,
}

pub fn run_update(options: &UpdateOptions) -> Result<UpdateResult> {
    let release_repo = options
        .release_repo
        .clone()
        .or_else(|| std::env::var("DAILY_GIT_RELEASE_REPO").ok())
        .unwrap_or_else(|| DEFAULT_RELEASE_REPO.to_string());
    let current_version =
        Version::parse(env!("CARGO_PKG_VERSION")).context("failed to parse current version")?;
    let current_target = current_target()?;
    let release = fetch_release(&release_repo, options.requested_version.as_deref())?;
    let target_version = parse_version_tag(&release.tag_name)?;

    if target_version < current_version {
        bail!(
            "latest release {} is older than current binary {}",
            target_version,
            current_version
        );
    }

    if options.check_only {
        let state = if target_version > current_version {
            UpdateState::Available
        } else {
            UpdateState::UpToDate
        };
        return Ok(UpdateResult {
            state,
            current_version: current_version.to_string(),
            target_version: target_version.to_string(),
            executable_path: std::env::current_exe()
                .context("failed to determine current executable path")?,
            release_repo,
        });
    }

    if target_version == current_version && !options.force {
        return Ok(UpdateResult {
            state: UpdateState::UpToDate,
            current_version: current_version.to_string(),
            target_version: target_version.to_string(),
            executable_path: std::env::current_exe()
                .context("failed to determine current executable path")?,
            release_repo,
        });
    }

    let asset_name = expected_asset_name(&target_version, current_target);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .with_context(|| format!("release does not contain asset `{asset_name}`"))?;

    let executable_path =
        std::env::current_exe().context("failed to determine current executable path")?;
    let install_layout = resolve_install_layout(&executable_path)?;
    let working_dir = tempdir().context("failed to create temporary directory")?;
    let archive_path = working_dir.path().join(&asset.name);
    let extract_dir = working_dir.path().join("extract");

    fs::create_dir_all(&extract_dir).context("failed to prepare extraction directory")?;
    download_asset(&asset.browser_download_url, &archive_path)?;
    extract_release_archive(&archive_path, &extract_dir)?;

    let package_root = find_package_root(&extract_dir)?;
    let packaged_binary = package_root.join(BINARY_NAME);
    if !packaged_binary.is_file() {
        bail!(
            "release package is missing the `{}` binary",
            packaged_binary.display()
        );
    }

    install_release_files(
        &package_root,
        &install_layout,
        &executable_path,
        &target_version,
    )?;

    Ok(UpdateResult {
        state: UpdateState::Updated,
        current_version: current_version.to_string(),
        target_version: target_version.to_string(),
        executable_path,
        release_repo,
    })
}

fn fetch_release(repo: &str, requested_version: Option<&str>) -> Result<GitHubRelease> {
    let api_base = std::env::var("DAILY_GIT_RELEASE_API_BASE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_API_BASE.to_string());
    let url = match requested_version {
        Some(version) => format!(
            "{api_base}/repos/{repo}/releases/tags/{}",
            normalized_tag(version)
        ),
        None => format!("{api_base}/repos/{repo}/releases/latest"),
    };

    let response = http_agent()
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("User-Agent", &user_agent())
        .call()
        .map_err(|error| format_http_error("failed to fetch release metadata", &url, error))?;

    serde_json::from_reader(response.into_reader())
        .with_context(|| format!("failed to parse release metadata from {url}"))
}

fn download_asset(url: &str, destination: &Path) -> Result<()> {
    let response = http_agent()
        .get(url)
        .set("User-Agent", &user_agent())
        .call()
        .map_err(|error| format_http_error("failed to download release asset", url, error))?;

    let mut output = File::create(destination)
        .with_context(|| format!("failed to create file {}", destination.display()))?;
    io::copy(&mut response.into_reader(), &mut output)
        .with_context(|| format!("failed to write asset to {}", destination.display()))?;
    Ok(())
}

fn extract_release_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    let archive = File::open(archive_path)
        .with_context(|| format!("failed to open archive {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(decoder);
    tar.unpack(destination)
        .with_context(|| format!("failed to unpack archive {}", archive_path.display()))
}

fn find_package_root(extract_dir: &Path) -> Result<PathBuf> {
    let mut entries = fs::read_dir(extract_dir)
        .with_context(|| format!("failed to inspect {}", extract_dir.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    match entries.len() {
        1 => Ok(entries.remove(0)),
        0 => bail!("release archive did not contain a package directory"),
        _ => bail!("release archive contained multiple package directories"),
    }
}

fn install_release_files(
    package_root: &Path,
    install_layout: &InstallLayout,
    executable_path: &Path,
    target_version: &Version,
) -> Result<()> {
    let packaged_binary = package_root.join(BINARY_NAME);
    replace_executable(&packaged_binary, executable_path)?;

    let templates_source = package_root.join("templates");
    if templates_source.is_dir() {
        sync_directory(
            &templates_source,
            &install_layout.share_dir.join("templates"),
        )?;
    }

    copy_if_present(
        &package_root.join("config.example.yaml"),
        &install_layout.share_dir.join("config.example.yaml"),
    )?;
    copy_if_present(
        &package_root.join("README.md"),
        &install_layout.share_dir.join("README.md"),
    )?;
    copy_if_present(
        &package_root.join("LICENSE"),
        &install_layout.share_dir.join("LICENSE"),
    )?;
    fs::write(
        install_layout.share_dir.join("VERSION"),
        format!("{target_version}\n"),
    )
    .with_context(|| {
        format!(
            "failed to write version marker in {}",
            install_layout.share_dir.display()
        )
    })?;

    Ok(())
}

fn replace_executable(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .with_context(|| format!("failed to determine parent of {}", destination.display()))?;
    let temp_destination = parent.join(format!(".{BINARY_NAME}.download"));

    fs::copy(source, &temp_destination).with_context(|| {
        format!(
            "failed to copy new binary from {} to {}",
            source.display(),
            temp_destination.display()
        )
    })?;
    set_executable_permissions(&temp_destination)?;
    fs::rename(&temp_destination, destination).with_context(|| {
        format!(
            "failed to replace executable at {}; check write permissions",
            destination.display()
        )
    })?;
    Ok(())
}

fn copy_if_present(source: &Path, destination: &Path) -> Result<()> {
    if !source.is_file() {
        return Ok(());
    }

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn sync_directory(source: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination)
            .with_context(|| format!("failed to clear {}", destination.display()))?;
    }
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read entry in {}", source.display()))?;
        let target = destination.join(entry.file_name());
        let path = entry.path();
        if path.is_dir() {
            sync_directory(&path, &target)?;
        } else {
            fs::copy(&path, &target).with_context(|| {
                format!("failed to copy {} to {}", path.display(), target.display())
            })?;
        }
    }

    Ok(())
}

fn resolve_install_layout(executable_path: &Path) -> Result<InstallLayout> {
    let executable_dir = executable_path.parent().with_context(|| {
        format!(
            "failed to determine directory of {}",
            executable_path.display()
        )
    })?;
    let prefix_dir = match executable_dir.file_name() {
        Some(name) if name == OsStr::new("bin") => executable_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| executable_dir.to_path_buf()),
        _ => executable_dir.to_path_buf(),
    };

    Ok(InstallLayout {
        share_dir: prefix_dir.join("share").join(SHARE_DIR_NAME),
    })
}

fn current_target() -> Result<&'static str> {
    match env!("DAILY_GIT_BUILD_TARGET") {
        "x86_64-unknown-linux-gnu" => Ok("x86_64-unknown-linux-gnu"),
        "x86_64-apple-darwin" => Ok("x86_64-apple-darwin"),
        "aarch64-apple-darwin" => Ok("aarch64-apple-darwin"),
        other => bail!(
            "`update` currently supports Linux/macOS installer targets only; unsupported target `{other}`"
        ),
    }
}

fn expected_asset_name(version: &Version, target: &str) -> String {
    format!("{BINARY_NAME}-{version}-{target}.tar.gz")
}

fn parse_version_tag(tag: &str) -> Result<Version> {
    Version::parse(tag.trim_start_matches('v'))
        .with_context(|| format!("failed to parse release tag `{tag}` as a semantic version"))
}

fn normalized_tag(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

fn http_agent() -> Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(120))
        .build()
}

fn user_agent() -> String {
    format!("{BINARY_NAME}/{}", env!("CARGO_PKG_VERSION"))
}

fn format_http_error(prefix: &str, url: &str, error: ureq::Error) -> anyhow::Error {
    match error {
        ureq::Error::Status(404, _) => {
            anyhow::anyhow!("{prefix}: no published GitHub Release was found at {url}")
        }
        ureq::Error::Status(code, response) => anyhow::anyhow!(
            "{prefix}: {url}: status code {code} {}",
            response.status_text()
        ),
        ureq::Error::Transport(error) => anyhow::anyhow!("{prefix}: {url}: {error}"),
    }
}

#[cfg(unix)]
fn set_executable_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read metadata for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).with_context(|| {
        format!(
            "failed to set executable permissions for {}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_executable_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_version_tags() {
        assert_eq!(normalized_tag("0.2.0"), "v0.2.0");
        assert_eq!(normalized_tag("v0.2.0"), "v0.2.0");
    }

    #[test]
    fn builds_expected_asset_names() {
        let version = Version::parse("0.3.1").unwrap();
        assert_eq!(
            expected_asset_name(&version, "aarch64-apple-darwin"),
            "daily_git-0.3.1-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn resolves_share_dir_from_bin_prefix() {
        let layout = resolve_install_layout(Path::new("/tmp/daily/bin/daily_git")).unwrap();
        assert_eq!(
            layout.share_dir,
            PathBuf::from("/tmp/daily/share/daily_git")
        );
    }

    #[test]
    fn parses_release_tags() {
        let version = parse_version_tag("v1.4.2").unwrap();
        assert_eq!(version, Version::parse("1.4.2").unwrap());
    }
}
