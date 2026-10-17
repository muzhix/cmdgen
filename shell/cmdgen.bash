# cmdgen Bash integration. Managed by `cmdgen integration`.
# keybinding: __CMDGEN_KEYBINDING_NAME__
# This function never executes the generated command.
# bind -x requires Bash 4 or newer.

_cmdgen_bash() {
  local prompt="$READLINE_LINE"
  if [[ -z "${prompt//[[:space:]]/}" ]]; then
    return 0
  fi

  local errfile result status err
  errfile="$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX")" || {
    printf '\ncmdgen: 无法创建临时文件\n' >&2
    return 1
  }

  printf '\r\033[Kcmdgen: 正在生成…' >&2

  result="$(CMDGEN_SHELL=bash command cmdgen -- "$prompt" 2>"$errfile")"
  status=$?

  printf '\r\033[K' >&2

  if [[ $status -eq 0 && -n "$result" ]]; then
    READLINE_LINE="$result"
    READLINE_POINT=${#READLINE_LINE}
  else
    err="$(cat "$errfile")"
    if [[ -z "$err" ]]; then
      err="cmdgen 失败（退出码 ${status}），已保留原始输入"
    fi
    if [[ ${#err} -gt 500 ]]; then
      err="${err:0:500}"
    fi
    printf '\n%s\n' "$err" >&2
  fi

  command rm -f "$errfile"
}

case $- in
  *i*)
    if [[ -n "${BASH_VERSINFO[0]+x}" && "${BASH_VERSINFO[0]}" -ge 4 ]]; then
      bind -x '"__CMDGEN_BASH_BINDING__": _cmdgen_bash'
    else
      printf 'cmdgen: 当前 Bash %s 不支持 bind -x，输入行集成需要 Bash 4 或更高版本。\n' "${BASH_VERSION:-unknown}" >&2
    fi
    ;;
esac
