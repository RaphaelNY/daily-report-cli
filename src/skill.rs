//! Install and remove the bundled agent skill.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

const SKILL_DIR_NAME: &str = "daily-git-skill";
const SKILL_MD: &str = include_str!("../skills/daily-git-skill/SKILL.md");
const RUN_SH_TEMPLATE: &str = include_str!("../skills/daily-git-skill/run.sh");

#[derive(Debug, Clone)]
pub enum SkillAction {
    Install,
    Uninstall,
    Status,
}

#[derive(Debug, Clone)]
pub struct SkillOptions {
    pub action: SkillAction,
    pub codex_home: Option<PathBuf>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillResult {
    pub ok: bool,
    pub action: &'static str,
    pub skill_name: &'static str,
    pub skill_path: String,
    pub message: String,
}

pub fn run_skill_command(options: &SkillOptions) -> Result<SkillResult> {
    match options.action {
        SkillAction::Install => install_skill(options),
        SkillAction::Uninstall => uninstall_skill(options),
        SkillAction::Status => status_skill(options),
    }
}

fn install_skill(options: &SkillOptions) -> Result<SkillResult> {
    let skill_path = skill_path(options.codex_home.as_deref())?;
    if skill_path.exists() {
        if !options.force {
            bail!(
                "skill already exists at {}; pass --force to overwrite",
                skill_path.display()
            );
        }
        fs::remove_dir_all(&skill_path)
            .with_context(|| format!("failed to remove existing skill {}", skill_path.display()))?;
    }

    fs::create_dir_all(&skill_path)
        .with_context(|| format!("failed to create skill dir {}", skill_path.display()))?;
    fs::write(skill_path.join("SKILL.md"), SKILL_MD)
        .with_context(|| format!("failed to write {}", skill_path.join("SKILL.md").display()))?;

    let current_exe = env::current_exe().context("failed to determine current executable path")?;
    let run_sh = RUN_SH_TEMPLATE.replace("@DAILY_GIT_BIN@", &current_exe.display().to_string());
    let run_path = skill_path.join("run.sh");
    fs::write(&run_path, run_sh)
        .with_context(|| format!("failed to write {}", run_path.display()))?;
    make_executable(&run_path)?;

    Ok(SkillResult {
        ok: true,
        action: "install",
        skill_name: "daily-git",
        skill_path: absolute_display(&skill_path),
        message: "installed".to_string(),
    })
}

fn uninstall_skill(options: &SkillOptions) -> Result<SkillResult> {
    let skill_path = skill_path(options.codex_home.as_deref())?;
    if !skill_path.exists() {
        return Ok(SkillResult {
            ok: true,
            action: "uninstall",
            skill_name: "daily-git",
            skill_path: absolute_display(&skill_path),
            message: "not installed".to_string(),
        });
    }

    if !skill_path.join("SKILL.md").is_file() {
        bail!(
            "refusing to remove {}; SKILL.md was not found",
            skill_path.display()
        );
    }

    fs::remove_dir_all(&skill_path)
        .with_context(|| format!("failed to remove skill {}", skill_path.display()))?;
    Ok(SkillResult {
        ok: true,
        action: "uninstall",
        skill_name: "daily-git",
        skill_path: absolute_display(&skill_path),
        message: "uninstalled".to_string(),
    })
}

fn status_skill(options: &SkillOptions) -> Result<SkillResult> {
    let skill_path = skill_path(options.codex_home.as_deref())?;
    let installed = skill_path.join("SKILL.md").is_file() && skill_path.join("run.sh").is_file();
    Ok(SkillResult {
        ok: installed,
        action: "status",
        skill_name: "daily-git",
        skill_path: absolute_display(&skill_path),
        message: if installed {
            "installed"
        } else {
            "not installed"
        }
        .to_string(),
    })
}

fn skill_path(codex_home_override: Option<&Path>) -> Result<PathBuf> {
    Ok(resolve_codex_home(codex_home_override)?
        .join("skills")
        .join(SKILL_DIR_NAME))
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

fn absolute_display(path: &Path) -> String {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    path.display().to_string()
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)
        .with_context(|| format!("failed to read permissions for {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set executable bit on {}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_uninstalls_skill_under_codex_home() {
        let temp = tempfile::tempdir().unwrap();
        let install_options = SkillOptions {
            action: SkillAction::Install,
            codex_home: Some(temp.path().to_path_buf()),
            force: false,
        };

        let result = run_skill_command(&install_options).unwrap();
        assert!(result.ok);
        let skill_root = temp.path().join("skills/daily-git-skill");
        assert!(skill_root.join("SKILL.md").is_file());
        assert!(skill_root.join("run.sh").is_file());

        let status = run_skill_command(&SkillOptions {
            action: SkillAction::Status,
            codex_home: Some(temp.path().to_path_buf()),
            force: false,
        })
        .unwrap();
        assert!(status.ok);
        assert_eq!(status.message, "installed");

        let uninstall = run_skill_command(&SkillOptions {
            action: SkillAction::Uninstall,
            codex_home: Some(temp.path().to_path_buf()),
            force: false,
        })
        .unwrap();
        assert!(uninstall.ok);
        assert!(!skill_root.exists());
    }
}
