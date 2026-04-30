---
name: daily-git
description: Generate daily or weekly Markdown reports from one or more local Git repositories. Use this when an agent needs a repeatable report artifact instead of manually summarizing commits.
---

# daily-git

Use this skill to generate daily or weekly Markdown reports from local Git history and optional Markdown project docs.

## When To Use

- The user asks for a daily report, weekly report, or Git activity summary artifact.
- The report should be written to disk as Markdown.
- The source data should come from local repositories, not from GitHub issues or PRs.

## Safe Defaults

- The wrapper runs `daily_git` with `--json` so agents can parse the result reliably.
- The wrapper adds `--no-polish` unless `--polish` is explicitly passed.
- Weekly PPT generation is off unless `--ppt` is explicitly passed.
- Do not expose or run `daily_git update` from this skill.
- Use `doctor` before generation when the agent needs to validate paths and optional integrations without writing reports.

## Inputs

Required:

- `daily` or `weekly`
- At least one `--repo <PATH>` unless the current directory or config file is intentionally used.

Common optional inputs:

- `--date YYYY-MM-DD` for daily reports
- `--end-date YYYY-MM-DD` and `--days N` for weekly reports
- `--output <PATH>` or `--output-dir <DIR>`
- `--template <PATH>`
- `--doc <PATH>`
- `--author <NAME_OR_EMAIL>` and `--author-match name|email|name_or_email`

Optional capabilities:

- `--polish` enables Codex polishing.
- `--ppt` enables weekly HTML PPT generation if the `html-ppt` skill is installed.

## Output

The wrapper prints JSON from `daily_git`:

```json
{
  "ok": true,
  "kind": "weekly",
  "output_path": "reports/weekly-demo-2026-05-01.md",
  "ppt_path": null,
  "polish": {
    "status": "skipped",
    "message": "润色功能已关闭"
  }
}
```

## Side Effects

- Writes a Markdown report to `output_path` or `output_dir`.
- If `--ppt` is used, writes an HTML deck directory and copies assets.
- Reads local Git history and Markdown docs.

## Examples

Preflight a repository without writing a report:

```bash
skills/daily-git-skill/run.sh doctor daily --repo /path/to/repo --output-dir /path/to/reports
```

Generate a daily report without model polishing:

```bash
skills/daily-git-skill/run.sh daily --repo /path/to/repo --date 2026-05-01 --output-dir /path/to/reports
```

Generate a weekly report for multiple repos:

```bash
skills/daily-git-skill/run.sh weekly --repo /path/a --repo /path/b --end-date 2026-05-01 --days 7 --output-dir /path/to/reports
```

Generate weekly Markdown plus PPT:

```bash
skills/daily-git-skill/run.sh weekly --repo /path/to/repo --end-date 2026-05-01 --ppt --output-dir /path/to/reports
```
