# daily_git Usage

`daily_git` 用来从一个或多个 Git 仓库中汇总指定时间范围内的提交，并生成日报、周报，必要时还可以额外输出 HTML PPT deck。

## 快速开始

单仓库日报：

```bash
daily_git daily \
  --repo /path/to/project \
  --date 2025-02-14
```

单仓库周报：

```bash
daily_git weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --days 7
```

输出默认写到当前目录；如果指定 `--output-dir`，则写到目标目录。

如果需要让脚本或 agent 稳定解析生成结果，可以加 `--json`：

```bash
daily_git daily \
  --repo /path/to/project \
  --date 2025-02-14 \
  --output-dir ./reports \
  --no-polish \
  --json
```

输出为 JSON，包含报告路径、PPT 路径和润色状态。

## 多仓库聚合

`--repo` 可以重复传入。工具会把多个项目目录聚合成一份报告，而不是生成多份。

```bash
daily_git weekly \
  --repo /path/to/project-a \
  --repo /path/to/project-b \
  --end-date 2025-02-14 \
  --days 7
```

生成的报告里会明确列出：

- 关联仓库列表
- 每条提交的来源仓库
- 聚合后的总提交数、总文件数和模块概览

适合“工作横跨多个仓库，但日报/周报仍只想输出一份”的场景。

## 作者精确过滤

如果多个仓库里有多人协作，建议始终配合 `--author` 使用，避免把其他人的提交混进来。

支持三种匹配模式：

- `--author-match name`
  只按 commit author name 精确匹配
- `--author-match email`
  只按 commit author email 精确匹配
- `--author-match name_or_email`
  默认值，同时接受 name 或 email 精确匹配

示例：

```bash
daily_git weekly \
  --repo /path/to/project-a \
  --repo /path/to/project-b \
  --end-date 2025-02-14 \
  --days 7 \
  --author "raphael@example.com" \
  --author-match email
```

这里的过滤发生在 commit 被解析之后，不依赖 `git log --author` 的模糊匹配规则，因此对多仓库和团队协作更稳定。

## 配置文件

如果当前目录存在 `config.yaml` / `config.yml`，工具会自动加载。

示例：

```yaml
repo: .
# repos:
#   - ../project-a
#   - ../project-b

# author: raphael@example.com
# author_match: email

template: ./templates/周报与日报_markdown_模板.md
output_dir: ./reports
docs: []
max_docs: 6
max_doc_chars: 280

polish:
  enabled: true
  timeout_secs: 90
  # codex_home: /Users/yourname/.codex

daily: {}

weekly:
  days: 7
  ppt:
    enabled: false
    # output_dir: ./reports/weekly-ppt
```

优先级：

1. CLI 参数
2. `config.yaml`
3. 内置默认值

如果既配置了 `repo`，又配置了 `repos`，优先使用 `repos`。

## HTML PPT deck

周报支持额外产出一份 HTML 幻灯片 deck：

```bash
daily_git weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --days 7 \
  --ppt
```

默认会在输出目录下生成类似：

- `weekly-my-repo-2025-02-14.md`
- `weekly-my-repo-2025-02-14-ppt/index.html`

这项能力依赖本机已安装 `html-ppt` skill。

## Agent Skill Wrapper

仓库内的 `skills/daily-git-skill/run.sh` 是面向 agent 的最小封装。

默认行为：

- 添加 `--json`
- 添加 `--no-polish`
- 周报添加 `--no-ppt`
- 支持 `doctor` 预检
- 只允许 `daily` / `weekly` / `doctor`

预检示例：

```bash
skills/daily-git-skill/run.sh doctor weekly \
  --repo /path/to/project \
  --output-dir ./reports
```

`doctor` 不写报告文件。若存在失败项，命令会返回非 0；若只有警告项，则仍视为可继续。

示例：

```bash
skills/daily-git-skill/run.sh weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --days 7 \
  --output-dir ./reports
```

如需使用已安装版本而不是当前仓库构建产物，可以设置：

```bash
DAILY_GIT_BIN=/usr/local/bin/daily_git skills/daily-git-skill/run.sh daily --repo /path/to/project
```

## Codex 润色与提交摘要

工具默认会尝试调用本机 `codex`：

- 对整份 Markdown 做自然语言润色
- 对 commit 预览生成一句简短中文摘要

如果 `codex` 不可用、未登录、超时或执行失败，会自动回退到原始文本，不影响报告主流程。

## CODEX_HOME 和 sessions 权限问题

如果你看到类似下面的报错：

- `permission denied`
- `Codex cannot access session files`
- `~/.codex/sessions` 无法写入

说明当前进程对默认 `CODEX_HOME=~/.codex` 没有写权限。

推荐绕行方案：

```bash
mkdir -p ./.codex-home
cp ~/.codex/auth.json ./.codex-home/auth.json

daily_git weekly \
  --repo /path/to/project \
  --end-date 2025-02-14 \
  --codex-home ./.codex-home
```

`daily_git` 会把相对 `--codex-home` 自动转成绝对路径，所以在多仓库模式下也能正常工作。

如果你想直接修复默认目录，可手动检查：

```bash
ls -ldO ~/.codex ~/.codex/sessions
ls -le ~/.codex ~/.codex/sessions
xattr -lr ~/.codex/sessions | head -n 50
touch ~/.codex/sessions/.write_test
```

若确认是所有权或 ACL 问题，再执行：

```bash
sudo chown -R $(whoami):staff ~/.codex
chmod -R u+rwX ~/.codex
```

## 测试与预览输出

项目的 `.gitignore` 已经默认忽略这些常见本地输出：

- `reports*/`
- `preview*/`
- `tmp_*_output*/`
- `daily-*.md`
- `weekly-*.md`
- `daily-*-ppt/`
- `weekly-*-ppt/`

这样你可以直接在仓库里做测试和预览，不必每次提交前手动清理旧输出。
