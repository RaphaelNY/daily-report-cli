# daily_git

一个使用 Rust 编写的本地 CLI，用于读取指定项目的 Git 提交与项目文档，并按 Markdown 模板生成日报、周报。

## 功能

- 读取指定仓库的当日或最近一周 Git 提交
- 支持通过 `config.yaml` 固化仓库、模板、输出目录、润色策略等默认值
- 支持 `--author` 过滤指定作者
- 自动扫描仓库根目录 `README*.md` 与 `docs/`、`doc/` 下的 Markdown 文档
- 支持通过 `--doc` 显式指定文档路径
- 支持内置模板，也支持通过 `--template` 指定自定义 Markdown 模板
- 默认尝试调用本机 `codex` CLI 对生成结果做自然语言润色
- 在当前目录或指定目录输出 `.md` 报告文件

## 安装与运行

```bash
cargo run -- daily --repo /path/to/project
cargo run -- weekly --repo /path/to/project --end-date 2025-02-14
```

编译后可直接执行：

```bash
cargo build --release
./target/release/daily_git daily --repo /path/to/project
```

如果当前目录存在 `config.yaml`，工具会自动加载；也可以显式指定：

```bash
daily_git --config ./config.yaml daily
```

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
template: ./templates/周报与日报_markdown_模板.md
output_dir: ./reports
docs: []
max_docs: 6
max_doc_chars: 280

polish:
  enabled: true
  timeout_secs: 90

daily: {}

weekly:
  days: 7
```

支持的主要字段：

- `repo`
- `template`
- `output`
- `output_dir`
- `docs`
- `author`
- `max_docs`
- `max_doc_chars`
- `polish.enabled`
- `polish.model`
- `polish.timeout_secs`
- `polish.codex_home`
- `daily.date`
- `weekly.end_date`
- `weekly.days`

其中，配置文件中的相对路径会基于该 `config.yaml` 所在目录解析。

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
