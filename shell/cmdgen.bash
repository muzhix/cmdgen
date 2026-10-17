# cmdgen Bash integration. Managed by `cmdgen shell`.
# keybinding: __CMDGEN_KEYBINDING_NAME__
# This function never executes the generated command.
# bind -x requires Bash 4 or newer.

_cmdgen_bash() {
  local prompt="$READLINE_LINE"
  if [[ -z "${prompt//[[:space:]]/}" ]]; then
    return 0
  fi

  local outfile errfile
  outfile="$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX")" || {
    printf '\ncmdgen: 无法创建临时文件\n' >&2
    return 1
  }
  errfile="$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX")" || {
    command rm -f "$outfile"
    printf '\ncmdgen: 无法创建临时文件\n' >&2
    return 1
  }

  local had_monitor=0
  case $- in
    *m*) had_monitor=1 ;;
  esac
  set +m

  CMDGEN_SHELL=bash command cmdgen -- "$prompt" >"$outfile" 2>"$errfile" &
  local pid=$!
  local spinner='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
  local i=0 cancelled=0 key="" delay=0.1 read_flag="-n" read_status=0
  if [[ -z ${BASH_VERSINFO[0]+x} || ${BASH_VERSINFO[0]} -lt 4 ]]; then
    delay=1
  else
    # -N keeps Enter, so a newline cancels instead of being swallowed as a delimiter.
    read_flag="-N"
  fi

  while kill -0 "$pid" 2>/dev/null; do
    printf '\r\033[K%s' "${spinner:$((i % 10)):1}" >&2
    key=""
    read_status=0
    # `||` keeps `set -e` from leaving the spinner on screen when read times out.
    IFS= read -r "$read_flag" 1 -t "$delay" key || read_status=$?
    if [[ $read_status -eq 0 && -n $key ]]; then
      kill "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null || true
      cancelled=1
      break
    elif [[ $read_status -le 128 ]]; then
      # EOF or a read error: stop polling and let cmdgen finish.
      break
    fi
    i=$((i + 1))
  done

  local status=0
  if [[ $cancelled -eq 0 ]]; then
    wait "$pid" || status=$?
  fi

  printf '\r\033[K' >&2
  if [[ $had_monitor -eq 1 ]]; then
    set -m
  fi

  if [[ $cancelled -eq 1 ]]; then
    command rm -f "$outfile" "$errfile"
    return 0
  fi

  local result err
  result="$(cat "$outfile")"
  if [[ $status -eq 0 && -n $result ]]; then
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

  command rm -f "$outfile" "$errfile"
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
