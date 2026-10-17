# cmdgen Zsh integration. Managed by `cmdgen shell`.
# keybinding: __CMDGEN_KEYBINDING_NAME__
# This widget never executes the generated command.
# Ghost text is display-only until Tab or Right accepts it.
# Re-sourcing must replace an already-loaded widget. An early return here
# kept the previous function after `cmdgen shell install` and `source ~/.zshrc`.

typeset -g CMDGEN_HIGHLIGHT=${CMDGEN_HIGHLIGHT:-fg=8}
typeset -g CMDGEN_HIGHLIGHT_DESTRUCTIVE=${CMDGEN_HIGHLIGHT_DESTRUCTIVE:-fg=red}
typeset -g _CMDGEN_ACTIVE=0
typeset -g _CMDGEN_SUGGESTION=""
typeset -g _CMDGEN_DESTRUCTIVE=0
typeset -g _CMDGEN_BUFFER_AT_SUGGESTION=""
typeset -g _CMDGEN_ORIG_TAB=""
typeset -g _CMDGEN_LAST_HIGHLIGHT=""
typeset -gA _CMDGEN_ORIG_RIGHT=()

_cmdgen_sanitize() {
  emulate -L zsh
  setopt localoptions extendedglob
  local sanitized=$1 prev="" esc=$'\e'
  while [[ $sanitized == *${esc}\[* && $sanitized != "$prev" ]]; do
    prev=$sanitized
    sanitized=${sanitized//${esc}\[[0-9;]#[A-Za-z]/}
  done
  sanitized=${sanitized//${esc}/}
  sanitized=${sanitized//[$'\0'-$'\b'$'\v'-$'\x1f'$'\x7f']/}
  sanitized=${sanitized##[[:space:]]##}
  sanitized=${sanitized%%[[:space:]]##}
  print -r -- "$sanitized"
}

_cmdgen_ends_in_escaped_newline() {
  emulate -L zsh
  local index=${#1} trailing=0
  while (( index > 0 )) && [[ ${1[index]} == '\' ]]; do
    trailing=$(( trailing + 1 ))
    index=$(( index - 1 ))
  done
  (( trailing % 2 == 1 ))
}

_cmdgen_has_comment() {
  emulate -L zsh
  local word
  for word in "$@"; do
    [[ $word == '#'* ]] && return 0
  done
  return 1
}

_cmdgen_ends_in_list_operator() {
  [[ $1 == '&&' || $1 == '||' ]]
}

_cmdgen_has_heredoc() {
  emulate -L zsh
  setopt localoptions extendedglob
  local word
  for word in "$@"; do
    [[ $word == *'<<'(|-) && $word != *'<<<' ]] && return 0
  done
  return 1
}

# Drop suggestions that would leave the interactive shell waiting for more input.
_cmdgen_is_complete() {
  emulate -L zsh
  setopt localoptions extendedglob
  _cmdgen_ends_in_escaped_newline "$1" && return 1
  local -a words=(${(z)1})
  _cmdgen_has_comment "${words[@]}" && return 1
  _cmdgen_ends_in_list_operator "${words[-1]:-}" && return 1
  _cmdgen_has_heredoc "${words[@]}" && return 1
  command zsh -f -n -c "$1" >/dev/null 2>&1
}

_cmdgen_is_destructive() {
  emulate -L zsh
  setopt localoptions extendedglob
  local -a words=(${(z)1}) kept=()
  local word
  for word in "${words[@]}"; do
    [[ $word == [A-Za-z_][A-Za-z0-9_]#=* ]] && continue
    kept+=("$word")
  done
  (( ${#kept} == 0 )) && return 1

  local i=1
  while (( i <= ${#kept} )); do
    case ${kept[i]} in
      sudo|command|time|builtin|nocorrect) (( i++ )) ;;
      *) break ;;
    esac
  done
  case ${kept[i]:-} in
    rm|rmdir|unlink|shred|kill|killall|pkill|mkfs|dd|shutdown|reboot|halt|poweroff|truncate)
      return 0
      ;;
  esac

  local j k n=${#kept}
  for (( j = 1; j <= n; j++ )); do
    case ${kept[j]} in
      sed)
        for (( k = j + 1; k <= n; k++ )); do
          case ${kept[k]} in
            ';'|'|'|'||'|'&&') break ;;
            -i|--in-place|-i?*) return 0 ;;
          esac
        done
        ;;
      perl)
        for (( k = j + 1; k <= n; k++ )); do
          case ${kept[k]} in
            ';'|'|'|'||'|'&&') break ;;
            -pi|--in-place|-i|-pi?*) return 0 ;;
          esac
        done
        ;;
      git)
        case ${kept[j+1]:-} in
          clean) return 0 ;;
          reset)
            for (( k = j + 2; k <= n; k++ )); do
              case ${kept[k]} in
                ';'|'|'|'||'|'&&') break ;;
                --hard) return 0 ;;
              esac
            done
            ;;
          push)
            for (( k = j + 2; k <= n; k++ )); do
              case ${kept[k]} in
                ';'|'|'|'||'|'&&') break ;;
                -f|--force|--force-with-lease|-[A-Za-z]#f[A-Za-z]#) return 0 ;;
              esac
            done
            ;;
        esac
        ;;
      history)
        for (( k = j + 1; k <= n; k++ )); do
          case ${kept[k]} in
            ';'|'|'|'||'|'&&') break ;;
            -c) return 0 ;;
          esac
        done
        ;;
      truncate) return 0 ;;
    esac
  done
  return 1
}

_cmdgen_clear_highlight() {
  emulate -L zsh
  if [[ -n $_CMDGEN_LAST_HIGHLIGHT ]]; then
    region_highlight=("${(@)region_highlight:#$_CMDGEN_LAST_HIGHLIGHT}")
    _CMDGEN_LAST_HIGHLIGHT=""
  fi
}

_cmdgen_show_error() {
  emulate -L zsh
  local err=$1
  err=${err//$'\n'/ }
  err=${err//\%/%%}
  if (( ${#err} > 500 )); then
    err=${err:0:500}
  fi
  POSTDISPLAY=""
  _cmdgen_clear_highlight
  if [[ -o zle ]]; then
    zle redisplay
    zle -M -- "$err"
  fi
}

_cmdgen_lookup_widget() {
  emulate -L zsh
  local line
  line=$(bindkey -L -- "$1" 2>/dev/null) || return 0
  [[ -z $line ]] && return 0
  local -a parts=(${(z)line})
  local i=2
  if [[ ${parts[2]:-} == -M ]]; then
    i=4
  fi
  (( i >= ${#parts} )) && return 0
  print -r -- "${parts[i+1]}"
}

_cmdgen_right_sequences() {
  emulate -L zsh
  local -a seqs=('^[[C' '^[OC')
  if [[ -n ${terminfo[kcuf1]:-} ]]; then
    seqs+=("${terminfo[kcuf1]}")
  fi
  local -A seen=()
  local seq
  for seq in "${seqs[@]}"; do
    [[ -z $seq || -n ${seen[$seq]:-} ]] && continue
    seen[$seq]=1
    print -r -- "$seq"
  done
}

_cmdgen_show_ghost() {
  emulate -L zsh
  local suggestion=$1
  _cmdgen_clear_highlight
  if [[ -z $suggestion ]]; then
    POSTDISPLAY=""
    return
  fi

  local destructive=${_CMDGEN_DESTRUCTIVE:-0}
  if [[ $suggestion == "$BUFFER" ]]; then
    if (( destructive )); then
      POSTDISPLAY=" ⚠"
    else
      POSTDISPLAY=""
      return
    fi
  elif [[ -n $BUFFER && $suggestion == "$BUFFER"* ]]; then
    POSTDISPLAY="${suggestion#"$BUFFER"}"
    (( destructive )) && POSTDISPLAY="${POSTDISPLAY} ⚠"
  else
    if (( destructive )); then
      POSTDISPLAY=" ⇥ ⚠ ${suggestion}"
    else
      POSTDISPLAY=" ⇥ ${suggestion}"
    fi
  fi

  local highlight=$CMDGEN_HIGHLIGHT
  (( destructive )) && highlight=$CMDGEN_HIGHLIGHT_DESTRUCTIVE
  local start=$#BUFFER
  local end=$(( start + $#POSTDISPLAY ))
  _CMDGEN_LAST_HIGHLIGHT="$start $end $highlight"
  region_highlight+=("$_CMDGEN_LAST_HIGHLIGHT")
}

_cmdgen_activate() {
  emulate -L zsh
  (( _CMDGEN_ACTIVE )) && return 0
  _CMDGEN_ACTIVE=1
  _CMDGEN_BUFFER_AT_SUGGESTION=$BUFFER
  [[ -o zle ]] || return 0

  _CMDGEN_ORIG_TAB=$(_cmdgen_lookup_widget '^I')
  [[ $_CMDGEN_ORIG_TAB == _cmdgen_accept ]] && _CMDGEN_ORIG_TAB=""
  bindkey '^I' _cmdgen_accept

  local seq widget
  _CMDGEN_ORIG_RIGHT=()
  local -a seqs=(${(f)"$(_cmdgen_right_sequences)"})
  for seq in "${seqs[@]}"; do
    [[ -z $seq ]] && continue
    widget=$(_cmdgen_lookup_widget "$seq")
    [[ $widget == _cmdgen_accept_arrow ]] && widget=""
    _CMDGEN_ORIG_RIGHT[$seq]=$widget
    bindkey "$seq" _cmdgen_accept_arrow
  done
}

_cmdgen_deactivate() {
  emulate -L zsh
  (( _CMDGEN_ACTIVE )) || return 0
  _CMDGEN_ACTIVE=0
  _CMDGEN_SUGGESTION=""
  _CMDGEN_DESTRUCTIVE=0
  _CMDGEN_BUFFER_AT_SUGGESTION=""
  POSTDISPLAY=""
  _cmdgen_clear_highlight
  [[ -o zle ]] || return 0

  if [[ -n $_CMDGEN_ORIG_TAB ]]; then
    bindkey '^I' "$_CMDGEN_ORIG_TAB"
  else
    bindkey '^I' expand-or-complete
  fi
  _CMDGEN_ORIG_TAB=""

  local seq
  for seq in ${(k)_CMDGEN_ORIG_RIGHT}; do
    if [[ -n ${_CMDGEN_ORIG_RIGHT[$seq]} ]]; then
      bindkey "$seq" "${_CMDGEN_ORIG_RIGHT[$seq]}"
    else
      bindkey "$seq" forward-char
    fi
  done
  _CMDGEN_ORIG_RIGHT=()
}

_cmdgen_pre_redraw() {
  emulate -L zsh
  (( _CMDGEN_ACTIVE )) || return 0
  [[ $BUFFER == "$_CMDGEN_BUFFER_AT_SUGGESTION" ]] && return 0
  local current=$_CMDGEN_SUGGESTION
  if [[ -n $BUFFER && -n $current && $current == "$BUFFER"* ]]; then
    _cmdgen_show_ghost "$current"
    _CMDGEN_BUFFER_AT_SUGGESTION=$BUFFER
  else
    _cmdgen_deactivate
  fi
}

_cmdgen_line_finish() {
  emulate -L zsh
  (( _CMDGEN_ACTIVE )) && _cmdgen_deactivate
}

_cmdgen_accept() {
  emulate -L zsh
  if (( _CMDGEN_ACTIVE )) && [[ -n $_CMDGEN_SUGGESTION ]]; then
    BUFFER=$_CMDGEN_SUGGESTION
    CURSOR=$#BUFFER
    _cmdgen_deactivate
    [[ -o zle ]] && zle redisplay
    return 0
  fi
  if [[ -n $_CMDGEN_ORIG_TAB ]]; then
    zle "$_CMDGEN_ORIG_TAB"
  else
    zle expand-or-complete
  fi
}

_cmdgen_accept_arrow() {
  emulate -L zsh
  if (( _CMDGEN_ACTIVE )) && [[ -n $_CMDGEN_SUGGESTION ]]; then
    BUFFER=$_CMDGEN_SUGGESTION
    CURSOR=$#BUFFER
    _cmdgen_deactivate
    [[ -o zle ]] && zle redisplay
    return 0
  fi
  local widget=${_CMDGEN_ORIG_RIGHT[$KEYS]:-}
  if [[ -n $widget ]]; then
    zle "$widget"
  else
    zle forward-char
  fi
}

_cmdgen_apply_result() {
  emulate -L zsh
  local result=$1
  if [[ $result == *$'\n'* ]]; then
    if ! _cmdgen_is_complete "$result"; then
      _cmdgen_show_error "cmdgen: 建议不完整，已保留原始输入"
      return 1
    fi
    BUFFER=$result
    CURSOR=$#BUFFER
    POSTDISPLAY=""
    _cmdgen_clear_highlight
    [[ -o zle ]] && zle redisplay
    return 0
  fi
  if ! _cmdgen_is_complete "$result"; then
    _cmdgen_show_error "cmdgen: 建议不完整，已保留原始输入"
    return 1
  fi
  _CMDGEN_SUGGESTION=$result
  _CMDGEN_DESTRUCTIVE=0
  _cmdgen_is_destructive "$result" && _CMDGEN_DESTRUCTIVE=1
  _cmdgen_show_ghost "$result"
  _cmdgen_activate
  [[ -o zle ]] && zle -R
}

_cmdgen_widget() {
  emulate -L zsh
  setopt localoptions noshwordsplit no_notify no_monitor no_bg_nice

  (( _CMDGEN_ACTIVE )) && _cmdgen_deactivate

  local input=$BUFFER
  if [[ -z ${input//[[:space:]]/} ]]; then
    return 0
  fi

  local outfile errfile
  outfile=$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX") || {
    _cmdgen_show_error "cmdgen: 无法创建临时文件"
    return 1
  }
  errfile=$(mktemp "${TMPDIR:-/tmp}/cmdgen.XXXXXX") || {
    command rm -f "$outfile"
    _cmdgen_show_error "cmdgen: 无法创建临时文件"
    return 1
  }

  CMDGEN_SHELL=zsh command cmdgen -- "$input" >"$outfile" 2>"$errfile" &
  local pid=$!
  local spinner='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏' i=0 cancelled=0
  if [[ -o zle ]]; then
    while kill -0 $pid 2>/dev/null; do
      POSTDISPLAY=" ${spinner:$((i % 10)):1}"
      zle -R
      if read -t 0.1 -k 1; then
        kill $pid 2>/dev/null
        cancelled=1
        break
      fi
      (( i++ ))
    done
  fi
  wait $pid 2>/dev/null
  local cmd_status=$?

  if (( cancelled )); then
    POSTDISPLAY=""
    _cmdgen_clear_highlight
    command rm -f "$outfile" "$errfile"
    zle redisplay
    return 0
  fi

  local result err
  result=$(_cmdgen_sanitize "$(<"$outfile")")
  if [[ $cmd_status -eq 0 && -n $result ]]; then
    _cmdgen_apply_result "$result"
  else
    err=$(<"$errfile")
    if [[ -z $err ]]; then
      err="cmdgen 失败（退出码 ${cmd_status}），已保留原始输入"
    fi
    _cmdgen_show_error "$err"
  fi
  command rm -f "$outfile" "$errfile"
}

if [[ -o interactive ]]; then
  zle -N _cmdgen_widget
  zle -N _cmdgen_accept
  zle -N _cmdgen_accept_arrow
  zle -N _cmdgen_pre_redraw
  zle -N _cmdgen_line_finish
  bindkey '__CMDGEN_ZSH_BINDING__' _cmdgen_widget
  autoload -Uz add-zle-hook-widget 2>/dev/null
  if (( $+functions[add-zle-hook-widget] )); then
    add-zle-hook-widget line-pre-redraw _cmdgen_pre_redraw
    add-zle-hook-widget line-finish _cmdgen_line_finish
  else
    zle -N zle-line-pre-redraw _cmdgen_pre_redraw
    zle -N zle-line-finish _cmdgen_line_finish
  fi
fi
