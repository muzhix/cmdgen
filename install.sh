#!/usr/bin/env bash
# cmdgen 一键安装。
#
#   curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
#
# 只安装预编译二进制，不修改 ~/.zshrc / ~/.bashrc。
# 需要能在 macOS 自带的 bash 3.2 下通过管道执行。

if [ -z "${BASH_VERSION:-}" ]; then
  echo "Run this script with bash: curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash" >&2
  exit 1
fi

set -euo pipefail
unset CDPATH

CMDGEN_REPO_DEFAULT="muzhix/cmdgen"
REQUESTED_VERSION="${CMDGEN_VERSION:-}"
ACTION="install"
# 安装过程中的临时文件。放在全局，EXIT trap 才能在 set -u 下安全删除。
staged_bin=""

info() {
  printf '%s\n' "$*" >&2
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat >&2 <<'EOF'
Usage:
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --version v0.1.1
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --uninstall

Environment:
  CMDGEN_BIN_DIR           Install directory. Defaults to a writable directory already on PATH.
  CMDGEN_VERSION           Release, for example v0.1.1. Defaults to the latest release.
  CMDGEN_REPO              GitHub repository. Defaults to muzhix/cmdgen.
  CMDGEN_RELEASE_BASE_URL  Directory that holds the archives. Used by tests.

The script copies only the cmdgen binary. It does not edit shell startup files.
EOF
}

expand_tilde() {
  case "$1" in
    \~) printf '%s\n' "$HOME" ;;
    \~/*) printf '%s\n' "$HOME/${1#\~/}" ;;
    *) printf '%s\n' "$1" ;;
  esac
}

normalize_version() {
  local version="$1"
  if [ -z "$version" ] || [ "$version" = "latest" ]; then
    printf '\n'
    return
  fi
  case "$version" in
    *[!A-Za-z0-9._-]*) die "invalid version: $version" ;;
  esac
  case "$version" in
    v*) printf '%s\n' "$version" ;;
    *) printf 'v%s\n' "$version" ;;
  esac
}

on_path() {
  local dir="${1%/}"
  local rest="${PATH:-}"
  local part
  while [ -n "$rest" ]; do
    part="${rest%%:*}"
    case "$rest" in
      *:*) rest="${rest#*:}" ;;
      *) rest="" ;;
    esac
    part="${part%/}"
    if [ -n "$part" ] && [ "$part" = "$dir" ]; then
      return 0
    fi
  done
  return 1
}

can_install_to() {
  local dir="$1"
  if [ -d "$dir" ]; then
    [ -w "$dir" ]
    return
  fi
  local parent
  parent=$(dirname "$dir")
  [ -n "$parent" ] && [ -d "$parent" ] && [ -w "$parent" ]
}

choose_bin_dir() {
  if [ -n "${CMDGEN_BIN_DIR:-}" ]; then
    expand_tilde "$CMDGEN_BIN_DIR"
    return
  fi
  local d
  for d in "$HOME/.local/bin" "$HOME/bin" "/usr/local/bin"; do
    if on_path "$d" && can_install_to "$d"; then
      printf '%s\n' "$d"
      return
    fi
  done
  printf '%s\n' "$HOME/.local/bin"
}

triple_for() {
  local os="$1"
  local arch="$2"
  local os_norm arch_norm

  case "$os" in
    Darwin|darwin) os_norm="apple-darwin" ;;
    Linux|linux) os_norm="unknown-linux-gnu" ;;
    MINGW*|MSYS*|CYGWIN*) die "Windows is not supported." ;;
    *) die "unsupported operating system: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64|x64) arch_norm="x86_64" ;;
    aarch64|arm64) arch_norm="aarch64" ;;
    *) die "unsupported architecture: $arch" ;;
  esac

  printf '%s-%s\n' "$arch_norm" "$os_norm"
}

target_triple() {
  triple_for "$(uname -s)" "$(uname -m)"
}

assert_glibc_linux() {
  local os="$1"
  if [ "$os" != "Linux" ] && [ "$os" != "linux" ]; then
    return 0
  fi
  if [ -f /etc/alpine-release ]; then
    die "This system uses musl (Alpine). Release binaries link against glibc and cannot run here."
  fi
  if command -v ldd >/dev/null 2>&1; then
    if ldd --version 2>&1 | grep -qi musl; then
      die "This system uses musl. Release binaries link against glibc and cannot run here."
    fi
  fi
}

release_base_url() {
  if [ -n "${CMDGEN_RELEASE_BASE_URL:-}" ]; then
    printf '%s\n' "${CMDGEN_RELEASE_BASE_URL%/}"
    return
  fi
  local repo="${CMDGEN_REPO:-$CMDGEN_REPO_DEFAULT}"
  case "$repo" in
    */*) ;;
    *) die "CMDGEN_REPO must be owner/name, got: $repo" ;;
  esac
  if [ -n "$REQUESTED_VERSION" ]; then
    printf 'https://github.com/%s/releases/download/%s\n' "$repo" "$REQUESTED_VERSION"
  else
    printf 'https://github.com/%s/releases/latest/download\n' "$repo"
  fi
}

fetch_url() {
  local url="$1"
  local dest="$2"
  if command -v curl >/dev/null 2>&1; then
    case "$url" in
      https://*)
        curl --fail --silent --show-error --location --retry 3 --retry-delay 1 \
          --connect-timeout 15 --max-time 180 --proto '=https' \
          "$url" -o "$dest"
        ;;
      http://*)
        curl --fail --silent --show-error --location --retry 3 --retry-delay 1 \
          --connect-timeout 15 --max-time 180 --proto '=http' \
          "$url" -o "$dest"
        ;;
      *)
        curl --fail --silent --show-error --location \
          "$url" -o "$dest"
        ;;
    esac
    return $?
  fi
  if command -v wget >/dev/null 2>&1; then
    wget -q -O "$dest" --timeout=180 "$url"
    return $?
  fi
  die "curl or wget is required to download."
}

sha256_file() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{ print $1 }'
    return
  fi
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{ print $1 }'
    return
  fi
  die "sha256sum or shasum is required to verify the download."
}

verify_sha256() {
  local file="$1"
  local sums="$2"
  local asset="$3"
  local expected actual
  expected=$(awk -v name="$asset" '
    function base(value) {
      sub(/^\*/, "", value)
      sub(/^\.\//, "", value)
      return value
    }
    base($2) == name { print $1; exit }
  ' "$sums")
  if [ -z "$expected" ]; then
    die "checksum file has no entry for $asset"
  fi
  actual=$(sha256_file "$file")
  expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
  actual=$(printf '%s' "$actual" | tr 'A-F' 'a-f')
  if [ "$expected" != "$actual" ]; then
    die "checksum mismatch for ${asset}: got ${actual}, release record has ${expected}"
  fi
}

archive_member() {
  local listing="$1"
  awk '
    {
      name = $0
      sub(/^\.\//, "", name)
      if (name ~ /\//) {
        n = split(name, parts, "/")
        leaf = parts[n]
      } else {
        leaf = name
      }
      if (leaf == "cmdgen" && name !~ /\/$/) {
        print name
        exit
      }
    }
  ' "$listing"
}

has_integration_marker() {
  local f
  for f in "$HOME/.zshrc" "$HOME/.bashrc"; do
    if [ -f "$f" ] && grep -F -q 'cmdgen managed integration' "$f"; then
      return 0
    fi
  done
  return 1
}

prepare_bin_dir() {
  local dir="$1"
  if [ -e "$dir" ] && [ ! -d "$dir" ]; then
    die "install location is not a directory: $dir"
  fi
  if [ ! -d "$dir" ]; then
    mkdir -p "$dir" || die "cannot create install directory: $dir"
  fi
  if [ ! -w "$dir" ]; then
    die "no write permission for ${dir}. Set CMDGEN_BIN_DIR to choose another directory."
  fi
}

install_cmdgen() {
  local os triple base asset tarball sums member src dest version_line
  os=$(uname -s)
  assert_glibc_linux "$os"
  triple=$(target_triple)
  base=$(release_base_url)
  asset="cmdgen-${triple}.tar.gz"
  dest=$(choose_bin_dir)
  prepare_bin_dir "$dest"

  info "target: $triple"
  if [ -n "$REQUESTED_VERSION" ]; then
    info "version: $REQUESTED_VERSION"
  else
    info "version: latest"
  fi

  tarball="$tmpdir/$asset"
  info "downloading $asset"
  fetch_url "$base/$asset" "$tarball"

  sums="$tmpdir/SHA256SUMS"
  if fetch_url "$base/SHA256SUMS" "$sums" 2>"$tmpdir/sums.err"; then
    verify_sha256 "$tarball" "$sums" "$asset"
    info "verified sha256"
  else
    rm -f "$sums"
    warn "this release has no SHA256SUMS; skipped checksum verification."
  fi

  tar -tzf "$tarball" >"$tmpdir/listing"
  member=$(archive_member "$tmpdir/listing")
  if [ -z "$member" ]; then
    die "archive does not contain the cmdgen executable."
  fi
  case "$member" in
    *..*) die "unsafe path in archive: $member" ;;
  esac

  mkdir -p "$tmpdir/extract"
  tar -xzf "$tarball" -C "$tmpdir/extract" "$member"
  src="$tmpdir/extract/$member"
  if [ ! -f "$src" ]; then
    die "extracted file not found: $member"
  fi

  staged_bin=$(mktemp "$dest/cmdgen.XXXXXX")
  cp "$src" "$staged_bin"
  chmod 755 "$staged_bin"
  mv -f "$staged_bin" "$dest/cmdgen"
  staged_bin=""

  if ! "$dest/cmdgen" --version >"$tmpdir/version.out" 2>"$tmpdir/version.err"; then
    rm -f "$dest/cmdgen"
    die "installed cmdgen failed to run: $(cat "$tmpdir/version.err")"
  fi

  version_line=$(head -n 1 "$tmpdir/version.out")
  info "installed ${version_line:-cmdgen}"
  info "location: $dest/cmdgen"

  if on_path "$dest"; then
    info "This directory is on PATH. Open a new terminal and run cmdgen."
  else
    info "This directory is not on PATH. Add the line below to your shell startup file, then open a new terminal:"
    case "$dest" in
      "$HOME/.local/bin") info '  export PATH="$HOME/.local/bin:$PATH"' ;;
      "$HOME/bin") info '  export PATH="$HOME/bin:$PATH"' ;;
      *) info "  export PATH=\"$dest:\$PATH\"" ;;
    esac
    case "$(basename "${SHELL:-}")" in
      zsh) info "The startup file is usually ~/.zshrc." ;;
      bash) info "The startup file is usually ~/.bashrc." ;;
    esac
  fi

  info ""
  info "Next:"
  info "  cmdgen config set-key"
  info ""
  info "Shell keybindings are not written into startup files. To add them, run:"
  info "  cmdgen shell install zsh"
}

uninstall_cmdgen() {
  local dest bin
  dest=$(choose_bin_dir)
  bin="$dest/cmdgen"

  if [ -e "$bin" ] && has_integration_marker; then
    die "Shell integration is still present in ~/.zshrc or ~/.bashrc. Run \`cmdgen shell uninstall\` before removing the binary."
  fi

  if [ -e "$bin" ]; then
    rm -f "$bin"
    info "removed $bin"
  else
    info "not found: $bin"
  fi

  if has_integration_marker; then
    die "A cmdgen integration marker remains in a startup file, but the binary is already gone. Delete the block between \`# >>> cmdgen managed integration >>>\` and \`# <<< cmdgen managed integration <<<\` by hand."
  fi

  info "Config and credentials were left in the config directory."
}

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --uninstall)
        ACTION="uninstall"
        ;;
      --version)
        shift
        if [ $# -eq 0 ] || [ -z "${1:-}" ]; then
          die "--version requires a version"
        fi
        REQUESTED_VERSION="$1"
        ;;
      --version=*)
        REQUESTED_VERSION="${1#--version=}"
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        die "unknown argument: $1"
        ;;
    esac
    shift
  done
  REQUESTED_VERSION=$(normalize_version "$REQUESTED_VERSION")
}

main() {
  if [ -z "${HOME:-}" ]; then
    die "HOME is unset, so an install directory cannot be chosen."
  fi
  parse_args "$@"
  tmpdir=$(mktemp -d)
  trap 'rm -rf "${tmpdir:-}"; rm -f "${staged_bin:-}"' EXIT
  case "$ACTION" in
    install) install_cmdgen ;;
    uninstall) uninstall_cmdgen ;;
    *) die "unknown action: $ACTION" ;;
  esac
}

# 被 source 时只提供函数；通过管道喂给 bash 时 BASH_SOURCE 在 bash 3.2 里是空的，仍要执行。
if [ -n "${BASH_SOURCE[0]:-}" ] && [ "${BASH_SOURCE[0]}" != "$0" ]; then
  :
else
  main "$@"
fi
