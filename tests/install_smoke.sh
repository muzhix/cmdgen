#!/usr/bin/env bash
# 用本地压缩包验证 install.sh：平台识别、校验、安装、重复安装、卸载。
# 不访问网络。用 /bin/bash 覆盖 macOS 自带的 bash 3.2。
set -euo pipefail

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
installer="$root/install.sh"
bash_bin="/bin/bash"
if [ ! -x "$bash_bin" ]; then
  bash_bin="bash"
fi

fail() {
  printf '失败：%s\n' "$*" >&2
  exit 1
}

assert_eq() {
  if [ "$1" != "$2" ]; then
    fail "期望 [$2]，实际 [$1]（$3）"
  fi
}

# shellcheck disable=SC1090
source "$installer"

assert_eq "$(triple_for Darwin arm64)" "aarch64-apple-darwin" "darwin arm64"
assert_eq "$(triple_for Darwin x86_64)" "x86_64-apple-darwin" "darwin x86_64"
assert_eq "$(triple_for Linux x86_64)" "x86_64-unknown-linux-gnu" "linux x86_64"
assert_eq "$(triple_for Linux aarch64)" "aarch64-unknown-linux-gnu" "linux aarch64"
assert_eq "$(triple_for Linux arm64)" "aarch64-unknown-linux-gnu" "linux arm64"
# die 会 exit，放进子 shell 才不会带走测试进程。
if (triple_for Linux riscv64) >/dev/null 2>&1; then
  fail "riscv64 不应被接受"
fi
if (triple_for Windows x86_64) >/dev/null 2>&1; then
  fail "Windows 不应被接受"
fi

host_triple=$(target_triple)
case "$host_triple" in
  aarch64-apple-darwin|x86_64-apple-darwin|x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;;
  *) fail "当前平台的目标三元组异常：$host_triple" ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

home="$work/home"
bin_dir="$work/bin"
release="$work/release"
mkdir -p "$home" "$release"

stage="cmdgen-$host_triple"
mkdir -p "$release/$stage"
cat >"$release/$stage/cmdgen" <<'EOF'
#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  echo "cmdgen 9.9.9"
  exit 0
fi
echo "stub"
exit 0
EOF
chmod 755 "$release/$stage/cmdgen"
tar -czf "$release/$stage.tar.gz" -C "$release" "$stage"
rm -rf "$release/$stage"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$release" && sha256sum -- "$stage.tar.gz" > SHA256SUMS)
elif command -v shasum >/dev/null 2>&1; then
  (cd "$release" && shasum -a 256 "$stage.tar.gz" > SHA256SUMS)
else
  fail "需要 sha256sum 或 shasum"
fi

base_url="file://$release"
run_installer() {
  local version="$1"
  shift
  # 环境变量要传给被管道启动的 bash，不能写在脚本内部。
  env \
    HOME="$home" \
    PATH="/usr/bin:/bin" \
    SHELL="/bin/zsh" \
    CMDGEN_BIN_DIR="$bin_dir" \
    CMDGEN_RELEASE_BASE_URL="$base_url" \
    CMDGEN_VERSION="$version" \
    "$bash_bin" -s -- "$@" <"$installer"
}

set +e
output=$(run_installer v9.9.9 2>&1)
status=$?
set -e
if [ "$status" -ne 0 ]; then
  printf '%s\n' "$output" >&2
  fail "安装失败，退出码 $status"
fi
printf '%s\n' "$output" | grep -F -q "已安装 cmdgen 9.9.9" || fail "安装成功信息缺失：$output"
printf '%s\n' "$output" | grep -F -q "已校验 sha256" || fail "没有校验 sha256：$output"
[ -x "$bin_dir/cmdgen" ] || fail "二进制没有安装"
assert_eq "$("$bin_dir/cmdgen" --version)" "cmdgen 9.9.9" "安装后的版本"

# 安装不得改写启动文件。
printf '# user rc\n' >"$home/.zshrc"
before=$(cat "$home/.zshrc")
run_installer v9.9.9 >/dev/null 2>&1
assert_eq "$(cat "$home/.zshrc")" "$before" "重复安装改写了 zshrc"
[ -x "$bin_dir/cmdgen" ] || fail "重复安装后二进制消失"

# 管道入口（bash 3.2 下 BASH_SOURCE 为空）必须真的执行。
help_out=$(env HOME="$home" "$bash_bin" -s -- --help <"$installer" 2>&1)
printf '%s\n' "$help_out" | grep -F -q "curl -fsSL" || fail "--help 没有通过管道执行：$help_out"
file_help=$(env HOME="$home" "$bash_bin" "$installer" --help 2>&1)
printf '%s\n' "$file_help" | grep -F -q "curl -fsSL" || fail "直接执行 --help 失败：$file_help"

# 校验失败时不能留下二进制。
rm -f "$bin_dir/cmdgen"
printf '0000000000000000000000000000000000000000000000000000000000000000  %s\n' "$stage.tar.gz" >"$release/SHA256SUMS"
if run_installer v9.9.9 >/dev/null 2>"$work/bad.err"; then
  fail "错误的 sha256 不应该安装成功"
fi
grep -F -q "校验失败" "$work/bad.err" || fail "校验失败时没有说明原因：$(cat "$work/bad.err")"
[ ! -e "$bin_dir/cmdgen" ] || fail "校验失败后仍留下了二进制"

# 没有校验文件时仍可安装，但要明确提示。
rm -f "$release/SHA256SUMS"
loose_out=$(run_installer v9.9.9 2>&1)
printf '%s\n' "$loose_out" | grep -F -q "已跳过校验" || fail "缺少校验文件时没有提示：$loose_out"
assert_eq "$("$bin_dir/cmdgen" --version)" "cmdgen 9.9.9" "跳过校验后的版本"

# 有集成标记时拒绝删除二进制，避免用户无法再执行 shell uninstall。
printf '%s\n' '# >>> cmdgen managed integration >>>' >"$home/.zshrc"
if run_installer v9.9.9 --uninstall >/dev/null 2>"$work/keep.err"; then
  fail "仍有 Shell 集成时不应卸载二进制"
fi
grep -F -q "cmdgen shell uninstall" "$work/keep.err" || fail "没有提示先卸载集成：$(cat "$work/keep.err")"
[ -x "$bin_dir/cmdgen" ] || fail "拒绝卸载时把二进制删掉了"

printf '# user rc\n' >"$home/.zshrc"
remove_out=$(run_installer v9.9.9 --uninstall 2>&1)
printf '%s\n' "$remove_out" | grep -F -q "已删除" || fail "卸载信息缺失：$remove_out"
[ ! -e "$bin_dir/cmdgen" ] || fail "卸载后二进制还在"
assert_eq "$(cat "$home/.zshrc")" "# user rc" "卸载改写了 zshrc"

# 不在 PATH 时给出可复制的一行，且不改启动文件。
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$release" && sha256sum -- "$stage.tar.gz" > SHA256SUMS)
else
  (cd "$release" && shasum -a 256 "$stage.tar.gz" > SHA256SUMS)
fi
path_out=$(env \
  HOME="$home" \
  PATH="/usr/bin:/bin" \
  SHELL="/bin/zsh" \
  CMDGEN_BIN_DIR="$home/.local/bin" \
  CMDGEN_RELEASE_BASE_URL="$base_url" \
  CMDGEN_VERSION="v9.9.9" \
  "$bash_bin" -s <"$installer" 2>&1)
printf '%s\n' "$path_out" | grep -F -q 'export PATH="$HOME/.local/bin:$PATH"' || fail "没有提示 PATH：$path_out"
printf '%s\n' "$path_out" | grep -F -q "~/.zshrc" || fail "没有提示 zshrc：$path_out"
assert_eq "$(cat "$home/.zshrc")" "# user rc" "PATH 提示时改写了 zshrc"
[ -x "$home/.local/bin/cmdgen" ] || fail "没有装到 ~/.local/bin"

(
  unset CMDGEN_BIN_DIR
  HOME="$home"
  mkdir -p "$HOME/.local/bin" "$HOME/bin"
  PATH="$HOME/bin:/usr/bin:/bin"
  result=$(choose_bin_dir)
  [ "$result" = "$HOME/bin" ] || fail "应选中已在 PATH 中的 ~/bin，实际 $result"
  PATH="$HOME/.local/bin:$HOME/bin:/usr/bin"
  result=$(choose_bin_dir)
  [ "$result" = "$HOME/.local/bin" ] || fail "应优先 ~/.local/bin，实际 $result"
)

echo "install.sh smoke ok"
