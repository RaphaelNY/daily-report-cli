#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  printf 'usage: %s daily|weekly [daily_git options]\n' "$0" >&2
  exit 64
fi

command_name="$1"
shift

if [[ "$command_name" != "daily" && "$command_name" != "weekly" && "$command_name" != "doctor" ]]; then
  printf 'daily-git skill only supports daily, weekly, or doctor, got: %s\n' "$command_name" >&2
  exit 64
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
binary="${DAILY_GIT_BIN:-}"

if [[ -z "$binary" ]]; then
  if [[ -x "$repo_root/target/debug/daily_git" ]]; then
    binary="$repo_root/target/debug/daily_git"
  elif command -v daily_git >/dev/null 2>&1; then
    binary="daily_git"
  else
    printf 'daily_git binary not found; run cargo build or set DAILY_GIT_BIN\n' >&2
    exit 69
  fi
fi

has_json=false
has_polish_choice=false
has_ppt_choice=false
for arg in "$@"; do
  case "$arg" in
    --json)
      has_json=true
      ;;
    --polish|--no-polish)
      has_polish_choice=true
      ;;
    --ppt|--no-ppt)
      has_ppt_choice=true
      ;;
  esac
done

args=("$command_name")
if [[ "$has_json" == false ]]; then
  args+=("--json")
fi
if [[ "$has_polish_choice" == false ]]; then
  args+=("--no-polish")
fi
if [[ ("$command_name" == "weekly" || "$command_name" == "doctor") && "$has_ppt_choice" == false ]]; then
  args+=("--no-ppt")
fi
args+=("$@")

exec "$binary" "${args[@]}"
