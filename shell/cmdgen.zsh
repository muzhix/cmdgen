# cmdgen Zsh integration. Managed by `cmdgen integration`.
# keybinding: __CMDGEN_KEYBINDING_NAME__
# This widget never executes the generated command.

_cmdgen_widget() {
  emulate -L zsh
  setopt localoptions noshwordsplit

  local prompt="${BUFFER}"
  if [[ -z "${prompt//[[:space:]]/}" ]]; then
    return 0
  fi

  local errfile result cmd_status err
  errfile="$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX")" || {
    zle -M "cmdgen: 无法创建临时文件"
    return 1
  }

  zle -R "cmdgen: 正在生成…"

  result="$(CMDGEN_SHELL=zsh command cmdgen -- "$prompt" 2>"$errfile")"
  cmd_status=$?

  if [[ $cmd_status -eq 0 && -n "$result" ]]; then
    BUFFER="$result"
    CURSOR=${#BUFFER}
  else
    err="$(<"$errfile")"
    err="${err//$'\n'/ }"
    err="${err//\%/%%}"
    if [[ -z "$err" ]]; then
      err="cmdgen 失败（退出码 ${cmd_status}），已保留原始输入"
    fi
    if [[ ${#err} -gt 500 ]]; then
      err="${err:0:500}"
    fi
    zle -M "$err"
  fi

  command rm -f "$errfile"
  zle reset-prompt
}

if [[ -o interactive ]]; then
  zle -N _cmdgen_widget
  bindkey '__CMDGEN_ZSH_BINDING__' _cmdgen_widget
fi
