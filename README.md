# daily_git

一个使用 Rust 编写的本地 CLI，用于读取指定项目的 Git 提交与项目文档，并按 Markdown 模板生成日报、周报。

## 功能

- 读取一个或多个仓库的当日或最近一周 Git 提交，并聚合成一份日报 / 周报
- 支持通过 `config.yaml` 固化仓库、模板、输出目录、润色策略等默认值
- 支持 `--author` 过滤指定提交者，并通过 `--author-match` 精确匹配姓名 / 邮箱
- 自动扫描仓库根目录 `README*.md` 与 `docs/`、`doc/` 下的 Markdown 文档
- 支持通过 `--doc` 显式指定文档路径
- 支持内置模板，也支持通过 `--template` 指定自定义 Markdown 模板
- 支持 `--json` 输出机器可读的生成结果，便于 agent 或脚本调用
- 提供 `skills/daily-git-skill` 作为 agent skill 封装示例
- 默认尝试调用本机 `codex` CLI 对生成结果做自然语言润色
- 支持在生成周报 Markdown 的同时，额外产出一份基于 `html-ppt` skill 资产的 HTML PPT deck
- 在当前目录或指定目录输出 `.md` 报告文件

## 安装与运行

```bash
cargo run -- daily --repo /path/to/project
cargo run -- weekly --repo /path/to/project --end-date 2025-02-14
```

聚合多个项目目录：

```bash
cargo run -- weekly \
  --repo /path/to/project-a \
  --repo /path/to/project-b \
  --author "raphael@example.com" \
  --author-match email
```

编译后可直接执行：

```bash
cargo build --release
./target/release/daily_git daily --repo /path/to/project
```

如果当前目录存在 `config.yaml`，工具会自动加载；也可以显式指定：

```bash
daily_git daily --config ./config.yaml
```

更完整的命令、配置和多仓库使用说明见 [USAGE.md](./USAGE.md)。

如果只是想在另一台设备上使用，不一定需要重新安装 Rust。这个项目已经适合按“预编译二进制 + 压缩包”分发。

## 分发到其他设备

推荐分发内容：

- 平台压缩包：`daily_git-<version>-<target>.tar.gz`
- 安装脚本：`daily_git-installer.sh`

其中：

- 默认日报 / 周报模板已经内置到二进制里，但中文模板 `templates/周报与日报_markdown_模板.md` 仍然作为共享资源一起发布
- 安装脚本会把文件安装到 `<prefix>/bin` 与 `<prefix>/share/daily_git`
- 如果 `<prefix>/bin` 不在当前 PATH 中，安装脚本会自动向 shell 启动文件追加 PATH 配置
- `codex` CLI 仅用于润色；目标机器没有安装也能正常生成报告，只是会跳过润色

### 本地打包

macOS / Linux 上可以直接执行：

```bash
./scripts/package-release.sh
```

脚本会：

- 执行 `cargo build --locked --release`
- 生成平台压缩包
- 复制安装脚本到 `target/packages/`
- 在 `target/packages/` 下生成类似下面的文件

```bash
target/packages/daily_git-0.1.4-aarch64-apple-darwin.tar.gz
target/packages/daily_git-installer.sh
```

如果你需要为当前机器以外的 Rust target 打包，可以显式传入 target triple：

```bash
./scripts/package-release.sh x86_64-apple-darwin
```

### 目标机器安装

最推荐的方式是直接使用 release 安装脚本：

```bash
curl -fsSL https://github.com/RaphaelNY/daily-report-cli/releases/latest/download/daily_git-installer.sh | bash -s -- --prefix "$HOME/.local"
```

也可以安装指定版本：

```bash
curl -fsSL https://github.com/RaphaelNY/daily-report-cli/releases/latest/download/daily_git-installer.sh | bash -s -- --prefix "$HOME/.local" --version 0.1.4
```

安装脚本支持：

- `--prefix <DIR>`：指定安装根目录，实际会写入 `<DIR>/bin` 与 `<DIR>/share/daily_git`
- `--version <VER>`：安装指定 release 版本
- `--archive <PATH>`：从本地压缩包离线安装
- `--skip-path`：跳过 PATH 修改

如果你已经手里有压缩包，也可以离线安装：

```bash
bash ./daily_git-installer.sh \
  --archive ./daily_git-0.1.4-aarch64-apple-darwin.tar.gz \
  --prefix "$HOME/.local"
```

安装完成后：

- 可执行文件位于 `<prefix>/bin/daily_git`
- 模板和示例配置位于 `<prefix>/share/daily_git`
- 如果 PATH 不是立刻生效，重新打开 shell，或手动 `source ~/.zshrc` / `source ~/.bashrc`

手工解压并复制二进制仍然可行，但已经不再是推荐路径。

### 自更新

安装完成后，可以直接用 CLI 自更新：

```bash
daily_git update
daily_git update --check
daily_git update --version 0.1.4
```

说明：

- `daily_git update` 会从 GitHub Release 下载当前平台的最新包并原地替换当前可执行文件
- `daily_git update --check` 只检查，不执行安装
- `daily_git update --version <VER>` 可回退或切换到指定版本
- 当前 `update` 支持 Linux/macOS 的安装包目标
- 在该命令真正可用之前，仓库本身需要先发布至少一个 GitHub Release
- 如果你把工具装到了系统目录，例如 `/usr/local/bin`，更新时需要当前用户对该路径有写权限

### 自动发布 Release

仓库里已经增加了 GitHub Actions 工作流 [`.github/workflows/release.yml`](.github/workflows/release.yml)，支持：

- macOS Apple Silicon: `aarch64-apple-darwin`
- Linux x86_64: `x86_64-unknown-linux-gnu`
- Windows x86_64: `x86_64-pc-windows-msvc`

触发方式：

```bash
git tag v0.1.4
git push origin v0.1.4
```

之后 GitHub Release 会自动附带：

- 对应平台的压缩包
- `daily_git-installer.sh` 安装脚本

当前官方 Release 不再构建 macOS Intel 包；如确有需求，建议开发者在 Intel 机器上本地编译，或后续单独恢复一个可用的 Intel runner 配置。

对“在别的设备上获取这个工具”来说，这已经是最省事、维护成本最低的路径。

### 其他可选方案

- `cargo install --git <repo-url>`：适合开发者设备，但要求目标机器先装 Rust
- Homebrew tap：适合长期给多台 macOS 设备分发，但维护成本高于直接发 Release
- 只同步源码：最简单，但每台设备都要重新编译，不适合非开发环境

## 常用命令

生成日报：

```bash
daily_git daily \
  --repo /path/to/project \
  --date 2025-02-14 \
  --author "Raphael"
```

生成周报：

```bash
daily_git weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --days 7 \
  --output-dir ./reports
```

聚合多个仓库并按作者邮箱精确过滤：

```bash
daily_git weekly \
  --repo /path/to/project-a \
  --repo /path/to/project-b \
  --end-date 2025-02-14 \
  --days 7 \
  --author "raphael@example.com" \
  --author-match email
```

生成周报并同时输出 HTML PPT deck：

```bash
daily_git weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --days 7 \
  --output-dir ./reports \
  --ppt
```

使用自定义模板和显式文档：

```bash
daily_git daily \
  --repo /path/to/project \
  --template ./templates/my-daily.md.hbs \
  --doc README.md \
  --doc docs/architecture.md \
  --output ./reports/custom-daily.md
```

关闭润色或指定 Codex 模型：

```bash
daily_git weekly \
  --repo /path/to/project \
  --template ./templates/周报与日报_markdown_模板.md \
  --no-polish

daily_git daily \
  --repo /path/to/project \
  --polish-model gpt-5 \
  --polish-timeout-secs 120
```

基于配置文件生成：

```bash
daily_git daily
daily_git weekly --config ./config.yaml
```

## 配置文件

仓库根目录已提供示例文件：`config.yaml`

默认加载顺序如下：

- 显式传入 `--config <PATH>` 时，加载指定文件
- 否则，若当前目录存在 `config.yaml` 或 `config.yml`，自动加载
- CLI 参数始终覆盖配置文件中的同名项

示例：

```yaml
repo: .
repos:
  - ../project-a
  - ../project-b
template: ./templates/周报与日报_markdown_模板.md
output_dir: ./reports
docs: []
author: raphael@example.com
author_match: email
max_docs: 6
max_doc_chars: 280

polish:
  enabled: true
  timeout_secs: 90

daily: {}

weekly:
  days: 7
  ppt:
    enabled: false
    # output_dir: ./reports/weekly-ppt
```

支持的主要字段：

- `repo`
- `repos`
- `template`
- `output`
- `output_dir`
- `docs`
- `author`
- `author_match`
- `max_docs`
- `max_doc_chars`
- `polish.enabled`
- `polish.model`
- `polish.timeout_secs`
- `polish.codex_home`
- `daily.date`
- `weekly.end_date`
- `weekly.days`
- `weekly.ppt.enabled`
- `weekly.ppt.output_dir`

其中，配置文件中的相对路径会基于该 `config.yaml` 所在目录解析。

作者过滤说明：

- `--author <VALUE>` / `author: <VALUE>`：指定要保留的提交者
- `--author-match name`：只匹配 commit author name
- `--author-match email`：只匹配 commit author email
- `--author-match name_or_email`：默认值，同时接受姓名或邮箱精确匹配

这里的匹配发生在 commit 被解析之后，不再依赖 `git log --author` 的模糊正则式过滤，所以更适合多仓库汇总和团队协作场景。

如果你希望集中查看“怎么用这个工具”，包括多仓库、PPT、`CODEX_HOME` 和本地测试输出忽略规则，建议直接看 [USAGE.md](./USAGE.md)。

## 周报 PPT

`weekly --ppt` 会在写出 Markdown 周报后，额外生成一份 HTML 幻灯片 deck：

- 默认输出目录形如 `weekly-仓库名-YYYY-MM-DD-ppt/`
- deck 入口文件为 `index.html`
- 同目录会自动复制 `html-ppt` skill 所需的 `assets/` 和 `style.css`

注意：

- 这项能力依赖本机已安装 `html-ppt` skill
- skill 查找基于 `--codex-home`、`CODEX_HOME` 或默认的 `~/.codex`
- 产物是静态 HTML deck，不是 `.pptx`
- 当前仅支持周报，不支持日报

## Agent Skill

仓库内提供了一个最小可用的 agent skill 封装：`skills/daily-git-skill`。

它适合让 agent 通过稳定命令生成日报 / 周报，而不是自己拼接和解析 Git 提交。默认行为更偏自动化安全：

- 自动添加 `--json`，输出稳定 JSON
- 自动添加 `--no-polish`，避免默认依赖 Codex 润色
- 周报自动添加 `--no-ppt`，除非显式传入 `--ppt`
- 暴露 `doctor` 预检命令，用于在写文件前检查路径和可选依赖
- 只暴露 `daily` / `weekly` / `doctor`，不暴露 `update`

安装到 Codex skills 目录：

```bash
daily_git skill install
daily_git skill status
daily_git skill uninstall
```

默认安装到 `$CODEX_HOME/skills/daily-git-skill`；如果没有设置 `CODEX_HOME`，则使用 `~/.codex/skills/daily-git-skill`。也可以显式指定：

```bash
daily_git skill install --codex-home /path/to/.codex --force
```

安装命令会写入 `SKILL.md` 和 `run.sh`，并把当前 `daily_git` 可执行文件路径注入 wrapper，便于 agent 调用已安装的二进制。

预检示例：

```bash
skills/daily-git-skill/run.sh doctor daily \
  --repo /path/to/project \
  --output-dir /path/to/reports
```

示例：

```bash
skills/daily-git-skill/run.sh daily \
  --repo /path/to/project \
  --date 2026-05-01 \
  --output-dir /path/to/reports
```

输出示例：

```json
{
  "ok": true,
  "kind": "daily",
  "output_path": "/path/to/reports/daily-project-2026-05-01.md",
  "ppt_path": null,
  "polish": {
    "status": "skipped",
    "message": "润色功能已关闭"
  }
}
```

`doctor` 的 JSON 输出包含 `checks` 数组，每项有 `name`、`status` 和 `message`。如果存在 `fail` 项，命令会以非 0 退出；`warn` 仅表示生成时会回退或自动创建目录。

wrapper 会优先使用当前仓库的 `target/debug/daily_git`，再回退到系统 PATH 中的 `daily_git`。也可以通过 `DAILY_GIT_BIN=/path/to/daily_git` 指定二进制。

## 模板说明

内置模板位于：

- `templates/daily.md.hbs`
- `templates/weekly.md.hbs`
- `templates/周报与日报_markdown_模板.md`

其中 `templates/周报与日报_markdown_模板.md` 已经改造成可直接渲染的中文模板，可通过 `--template templates/周报与日报_markdown_模板.md` 直接生成更贴近日报 / 周报结构的内容。

自定义模板同样使用 Handlebars 语法，可使用以下主要字段：

- `generated_at`
- `repo.name`
- `repo.path`
- `repo.branch`
- `report.kind`
- `report.title`
- `report.start_date`
- `report.end_date`
- `report.commit_count`
- `report.file_count`
- `summary.highlights`
- `summary.work_items`
- `summary.plan_items`
- `summary.modules`
- `summary.modules_display`
- `summary.risks`
- `commits`
- `docs`

其中 `commits` 中常用字段包括：

- `short_hash`
- `hash`
- `author`
- `email`
- `date`
- `subject`
- `body`
- `files`
- `files_display`
- `files_compact_display`
- `modules`
- `modules_display`

其中 `docs` 中常用字段包括：

- `path`
- `title`
- `excerpt`

## Codex 润色

默认情况下，工具会在模板渲染完成后调用本机 `codex exec` 对 Markdown 做一次润色，特点如下：

- 默认复用当前设备的 `codex login` 认证状态
- 只做中文表达优化，不应改动事实、日期、提交、路径等信息
- 如果 `codex` 不可用、未登录、超时或执行失败，会自动回退到原始模板输出

相关参数：

- `--no-polish`：关闭 Codex 润色
- `--polish-model <MODEL>`：指定 Codex 使用的模型
- `--polish-timeout-secs <SECONDS>`：指定润色超时，默认 90 秒
- `--codex-home <PATH>`：指定 `CODEX_HOME`，适合高级自定义场景

## 默认输出规则

如果没有显式传入 `--output`：

- 日报输出到当前目录：`daily-仓库名-YYYY-MM-DD.md`
- 周报输出到当前目录：`weekly-仓库名-YYYY-MM-DD.md`

如果传入 `--output-dir`，则输出到该目录下。

## 文档扫描规则

未显式指定 `--doc` 时，会自动收集以下文档：

- 仓库根目录下的 `README*.md`
- `docs/` 目录中的 `.md` 文件
- `doc/` 目录中的 `.md` 文件

默认最多使用 6 份文档，可通过 `--max-docs` 调整；默认每份文档摘要最多 280 个字符，可通过 `--max-doc-chars` 调整。
