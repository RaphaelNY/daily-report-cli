#!/usr/bin/env bash

set -euo pipefail

REPO="${DAILY_GIT_RELEASE_REPO:-RaphaelNY/daily-report-cli}"
PREFIX="${DAILY_GIT_INSTALL_PREFIX:-$HOME/.local}"
VERSION=""
ARCHIVE_PATH=""
SKIP_PATH=0

usage() {
  cat <<'EOF'
Usage: daily_git-installer.sh [options]

Options:
  --prefix <dir>     Install under <dir>/bin and <dir>/share/daily_git
  --version <ver>    Install a specific version, for example 0.1.2
  --archive <path>   Install from a local archive instead of GitHub Release
  --skip-path        Do not modify shell PATH startup files
  -h, --help         Show this help message
EOF
}

log() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    die "required command not found: $1"
  fi
}

download_file() {
  local url="$1"
  local destination="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --retry 3 -o "$destination" "$url"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -qO "$destination" "$url"
    return
  fi

  die "either curl or wget is required to download release assets"
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os:$arch" in
    Darwin:x86_64)
      printf 'x86_64-apple-darwin\n'
      ;;
    Darwin:arm64 | Darwin:aarch64)
      printf 'aarch64-apple-darwin\n'
      ;;
    Linux:x86_64)
      printf 'x86_64-unknown-linux-gnu\n'
      ;;
    *)
      die "unsupported platform: $os / $arch"
      ;;
  esac
}

resolve_latest_version() {
  local latest_url resolved tag
  latest_url="https://github.com/$REPO/releases/latest"

  if command -v curl >/dev/null 2>&1; then
    resolved="$(curl -fsSIL -o /dev/null -w '%{url_effective}' "$latest_url")"
  elif command -v wget >/dev/null 2>&1; then
    resolved="$(wget -qSO- --max-redirect=20 "$latest_url" 2>&1 | awk '/^  Location: / {print $2}' | tail -n1 | tr -d '\r')"
  else
    die "either curl or wget is required to resolve the latest release"
  fi

  tag="${resolved##*/}"
  if [[ -z "$tag" || "$tag" == "latest" ]]; then
    die "failed to resolve the latest release tag from $resolved"
  fi
  printf '%s\n' "${tag#v}"
}

append_sh_path_block() {
  local file="$1"
  local marker="# >>> daily_git installer >>>"

  if [[ -f "$file" ]] && grep -F "$marker" "$file" >/dev/null 2>&1; then
    return
  fi

  mkdir -p "$(dirname "$file")"
  cat >>"$file" <<EOF

# >>> daily_git installer >>>
case ":\$PATH:" in
  *:"$BIN_DIR":*) ;;
  *) export PATH="$BIN_DIR:\$PATH" ;;
esac
# <<< daily_git installer <<<
EOF
}

append_fish_path_block() {
  local file="$1"
  local marker="# >>> daily_git installer >>>"

  if [[ -f "$file" ]] && grep -F "$marker" "$file" >/dev/null 2>&1; then
    return
  fi

  mkdir -p "$(dirname "$file")"
  cat >>"$file" <<EOF
# >>> daily_git installer >>>
if not contains -- "$BIN_DIR" \$PATH
    set -gx PATH "$BIN_DIR" \$PATH
end
# <<< daily_git installer <<<
EOF
}

ensure_path() {
  if [[ "$SKIP_PATH" -eq 1 ]]; then
    return
  fi

  case ":${PATH:-}:" in
    *:"$BIN_DIR":*)
      return
      ;;
  esac

  case "$(basename "${SHELL:-}")" in
    zsh)
      append_sh_path_block "$HOME/.zprofile"
      append_sh_path_block "$HOME/.zshrc"
      ;;
    bash)
      append_sh_path_block "$HOME/.bash_profile"
      append_sh_path_block "$HOME/.bashrc"
      append_sh_path_block "$HOME/.profile"
      ;;
    fish)
      append_fish_path_block "$HOME/.config/fish/conf.d/daily_git.fish"
      append_sh_path_block "$HOME/.profile"
      ;;
    *)
      append_sh_path_block "$HOME/.profile"
      ;;
  esac
}

find_package_dir() {
  local root="$1"
  local dir
  dir="$(find "$root" -mindepth 1 -maxdepth 1 -type d | head -n1)"
  [[ -n "$dir" ]] || die "failed to locate extracted package directory"
  printf '%s\n' "$dir"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      [[ $# -ge 2 ]] || die "--prefix requires a value"
      PREFIX="$2"
      shift 2
      ;;
    --version)
      [[ $# -ge 2 ]] || die "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --archive)
      [[ $# -ge 2 ]] || die "--archive requires a value"
      ARCHIVE_PATH="$2"
      shift 2
      ;;
    --skip-path)
      SKIP_PATH=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

require_command tar
TARGET_TRIPLE="$(detect_target)"
BIN_DIR="$PREFIX/bin"
SHARE_DIR="$PREFIX/share/daily_git"
TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ -z "$ARCHIVE_PATH" ]]; then
  if [[ -z "$VERSION" ]]; then
    VERSION="$(resolve_latest_version)"
  fi
  ARCHIVE_PATH="$TMP_DIR/daily_git-${VERSION}-${TARGET_TRIPLE}.tar.gz"
  ASSET_URL="https://github.com/$REPO/releases/download/v${VERSION}/daily_git-${VERSION}-${TARGET_TRIPLE}.tar.gz"
  log "Downloading $ASSET_URL"
  download_file "$ASSET_URL" "$ARCHIVE_PATH"
elif [[ ! -f "$ARCHIVE_PATH" ]]; then
  die "archive not found: $ARCHIVE_PATH"
fi

EXTRACT_DIR="$TMP_DIR/extract"
mkdir -p "$EXTRACT_DIR"
tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"
PACKAGE_DIR="$(find_package_dir "$EXTRACT_DIR")"

[[ -f "$PACKAGE_DIR/daily_git" ]] || die "package is missing the daily_git binary"
[[ -d "$PACKAGE_DIR/templates" ]] || die "package is missing the templates directory"

mkdir -p "$BIN_DIR" "$SHARE_DIR/templates"
install -m 755 "$PACKAGE_DIR/daily_git" "$BIN_DIR/daily_git"
rm -rf "$SHARE_DIR/templates"
mkdir -p "$SHARE_DIR/templates"
cp -R "$PACKAGE_DIR/templates/." "$SHARE_DIR/templates/"
cp "$PACKAGE_DIR/config.example.yaml" "$SHARE_DIR/config.example.yaml"
cp "$PACKAGE_DIR/README.md" "$SHARE_DIR/README.md"
cp "$PACKAGE_DIR/LICENSE" "$SHARE_DIR/LICENSE"
if [[ -n "$VERSION" ]]; then
  printf '%s\n' "$VERSION" > "$SHARE_DIR/VERSION"
fi

ensure_path

log "Installed daily_git to $BIN_DIR/daily_git"
log "Shared files installed to $SHARE_DIR"
if [[ "$SKIP_PATH" -eq 0 ]]; then
  log "If the command is not available immediately, open a new shell or source your shell profile."
fi
