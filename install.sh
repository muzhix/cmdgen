#!/usr/bin/env bash
# cmdgen 一键安装。
#
#   curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
#
# 只安装预编译二进制，不修改 ~/.zshrc / ~/.bashrc。
# 需要能在 macOS 自带的 bash 3.2 下通过管道执行。

if [ -z "${BASH_VERSION:-}" ]; then
  echo "请使用 bash 运行这个脚本：curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash" >&2
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
  printf '警告：%s\n' "$*" >&2
}

die() {
  printf '错误：%s\n' "$*" >&2
  exit 1
}

usage() {
  cat >&2 <<'EOF'
用法：
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --version v0.1.1
  curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --uninstall

环境变量：
  CMDGEN_BIN_DIR           安装目录。默认选一个已在 PATH 中且当前用户可写的目录
  CMDGEN_VERSION           版本，例如 v0.1.1。默认安装最新版
  CMDGEN_REPO              GitHub 仓库，默认 muzhix/cmdgen
  CMDGEN_RELEASE_BASE_URL  直接指定放有压缩包的目录，供测试使用

脚本只复制 cmdgen 可执行文件，不修改 shell 启动文件。
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
    *[!A-Za-z0-9._-]*) die "版本号无效：$version" ;;
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
    MINGW*|MSYS*|CYGWIN*) die "Windows 不在支持范围内。" ;;
    *) die "不支持的操作系统：$os" ;;
  esac

  case "$arch" in
    x86_64|amd64|x64) arch_norm="x86_64" ;;
    aarch64|arm64) arch_norm="aarch64" ;;
    *) die "不支持的架构：$arch" ;;
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
    die "当前系统使用 musl（Alpine）。发布包链接的是 glibc，不能在这里运行。"
  fi
  if command -v ldd >/dev/null 2>&1; then
    if ldd --version 2>&1 | grep -qi musl; then
      die "当前系统使用 musl。发布包链接的是 glibc，不能在这里运行。"
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
    *) die "CMDGEN_REPO 应为 owner/name，当前是：$repo" ;;
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
  die "需要 curl 或 wget 才能下载。"
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
  die "需要 sha256sum 或 shasum 来校验下载文件。"
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
    die "校验文件里没有 $asset"
  fi
  actual=$(sha256_file "$file")
  expected=$(printf '%s' "$expected" | tr 'A-F' 'a-f')
  actual=$(printf '%s' "$actual" | tr 'A-F' 'a-f')
  if [ "$expected" != "$actual" ]; then
    die "校验失败：${asset} 的 sha256 是 ${actual}，发布记录里是 ${expected}"
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
    die "安装位置不是目录：$dir"
  fi
  if [ ! -d "$dir" ]; then
    mkdir -p "$dir" || die "无法创建安装目录：$dir"
  fi
  if [ ! -w "$dir" ]; then
    die "没有写入权限：${dir}。可以设置 CMDGEN_BIN_DIR 换一个目录。"
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

  info "目标：$triple"
  if [ -n "$REQUESTED_VERSION" ]; then
    info "版本：$REQUESTED_VERSION"
  else
    info "版本：最新"
  fi

  tarball="$tmpdir/$asset"
  info "正在下载 $asset"
  fetch_url "$base/$asset" "$tarball"

  sums="$tmpdir/SHA256SUMS"
  if fetch_url "$base/SHA256SUMS" "$sums" 2>"$tmpdir/sums.err"; then
    verify_sha256 "$tarball" "$sums" "$asset"
    info "已校验 sha256"
  else
    rm -f "$sums"
    warn "这个版本没有 SHA256SUMS，已跳过校验。"
  fi

  tar -tzf "$tarball" >"$tmpdir/listing"
  member=$(archive_member "$tmpdir/listing")
  if [ -z "$member" ]; then
    die "压缩包里没有 cmdgen 可执行文件。"
  fi
  case "$member" in
    *..*) die "压缩包路径不安全：$member" ;;
  esac

  mkdir -p "$tmpdir/extract"
  tar -xzf "$tarball" -C "$tmpdir/extract" "$member"
  src="$tmpdir/extract/$member"
  if [ ! -f "$src" ]; then
    die "解压后找不到 $member"
  fi

  staged_bin=$(mktemp "$dest/cmdgen.XXXXXX")
  cp "$src" "$staged_bin"
  chmod 755 "$staged_bin"
  mv -f "$staged_bin" "$dest/cmdgen"
  staged_bin=""

  if ! "$dest/cmdgen" --version >"$tmpdir/version.out" 2>"$tmpdir/version.err"; then
    rm -f "$dest/cmdgen"
    die "安装后的 cmdgen 无法运行：$(cat "$tmpdir/version.err")"
  fi

  version_line=$(head -n 1 "$tmpdir/version.out")
  info "已安装 ${version_line:-cmdgen}"
  info "位置：$dest/cmdgen"

  if on_path "$dest"; then
    info "当前 PATH 可以找到这个目录。新开的终端里直接运行 cmdgen。"
  else
    info "这个目录不在 PATH 里。把下面一行加到 shell 启动文件后，重新打开终端："
    case "$dest" in
      "$HOME/.local/bin") info '  export PATH="$HOME/.local/bin:$PATH"' ;;
      "$HOME/bin") info '  export PATH="$HOME/bin:$PATH"' ;;
      *) info "  export PATH=\"$dest:\$PATH\"" ;;
    esac
    case "$(basename "${SHELL:-}")" in
      zsh) info "启动文件一般是 ~/.zshrc" ;;
      bash) info "启动文件一般是 ~/.bashrc" ;;
    esac
  fi

  info ""
  info "下一步："
  info "  cmdgen config set-key"
  info ""
  info "Shell 快捷键不会写入启动文件。需要时再运行："
  info "  cmdgen shell install zsh"
}

uninstall_cmdgen() {
  local dest bin
  dest=$(choose_bin_dir)
  bin="$dest/cmdgen"

  if [ -e "$bin" ] && has_integration_marker; then
    die "检测到 Shell 集成仍在 ~/.zshrc 或 ~/.bashrc。请先运行 cmdgen shell uninstall，再卸载二进制。"
  fi

  if [ -e "$bin" ]; then
    rm -f "$bin"
    info "已删除 $bin"
  else
    info "未找到 $bin"
  fi

  if has_integration_marker; then
    die "启动文件里还有 cmdgen 集成标记，但这里已经没有可执行文件。请手工删除 # >>> cmdgen managed integration >>> 与 # <<< cmdgen managed integration <<< 之间的内容。"
  fi

  info "配置和凭据仍留在配置目录，这次没有删除。"
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
          die "--version 需要一个版本号"
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
        die "未知参数：$1"
        ;;
    esac
    shift
  done
  REQUESTED_VERSION=$(normalize_version "$REQUESTED_VERSION")
}

main() {
  if [ -z "${HOME:-}" ]; then
    die "没有 HOME，无法选择安装目录。"
  fi
  parse_args "$@"
  tmpdir=$(mktemp -d)
  trap 'rm -rf "${tmpdir:-}"; rm -f "${staged_bin:-}"' EXIT
  case "$ACTION" in
    install) install_cmdgen ;;
    uninstall) uninstall_cmdgen ;;
    *) die "未知动作：$ACTION" ;;
  esac
}

# 被 source 时只提供函数；通过管道喂给 bash 时 BASH_SOURCE 在 bash 3.2 里是空的，仍要执行。
if [ -n "${BASH_SOURCE[0]:-}" ] && [ "${BASH_SOURCE[0]}" != "$0" ]; then
  :
else
  main "$@"
fi
