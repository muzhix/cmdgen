mod common;

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use cmdgen::shell::integration::{
    install, parse_keybinding, remove_managed_block, uninstall, upsert_managed_block,
    IntegrationShell, MANAGED_BEGIN,
};
use predicates::str::contains;

const FAKE_CMDGEN: &str = r#"#!/bin/sh
if [ -n "${CMDGEN_FAKE_SLEEP-}" ]; then
  sleep "$CMDGEN_FAKE_SLEEP"
fi
printf '%s\n' "SHELL=${CMDGEN_SHELL-}" >> "$CMDGEN_FAKE_LOG"
printf '%s\n' "ARGS=$*" >> "$CMDGEN_FAKE_LOG"
if [ "${CMDGEN_FAKE_FAIL-}" = "1" ]; then
  echo "模拟失败" >&2
  exit 1
fi
printf '%b\n' "${CMDGEN_FAKE_OUTPUT-echo ok}"
"#;

#[test]
fn managed_block_is_idempotent_and_uninstall_keeps_user_lines() {
    let original = "export KEEP=1\n";
    let once =
        upsert_managed_block(original, "source \"$HOME/.config/cmdgen/integration.zsh\"").unwrap();
    let twice =
        upsert_managed_block(&once, "source \"$HOME/.config/cmdgen/integration.zsh\"").unwrap();
    assert_eq!(once, twice);
    assert_eq!(once.matches(MANAGED_BEGIN).count(), 1);
    assert!(once.contains("export KEEP=1"));
    let removed = remove_managed_block(&twice).unwrap().unwrap();
    assert_eq!(removed, original);
    assert!(remove_managed_block(original).unwrap().is_none());
}

#[test]
fn broken_marker_is_not_rewritten() {
    let broken = format!("{MANAGED_BEGIN}\nsource old\nexport KEEP=1\n");
    let err = upsert_managed_block(&broken, "source x").unwrap_err();
    assert!(err.to_string().contains("不完整"));
}

#[test]
fn keybinding_maps_to_zsh_and_bash() {
    let binding = parse_keybinding("Ctrl-G").unwrap();
    assert_eq!(binding.canonical, "ctrl-g");
    assert_eq!(binding.zsh, "^G");
    assert_eq!(binding.bash, "\\C-g");
    assert!(parse_keybinding("alt-g").is_err());
}

#[test]
fn install_twice_and_uninstall_round_trip() {
    let sandbox = common::Sandbox::new();
    let home = &sandbox.home;
    let dir = sandbox.config_dir();
    fs::write(
        home.join(".zshrc"),
        "export KEEP=1\nbindkey '^G' existing\n",
    )
    .unwrap();
    let first = install(IntegrationShell::Zsh, home, &dir, "ctrl-g").unwrap();
    assert!(first.changed);
    assert!(first.created_backup);
    assert!(first.binding_warning.is_some());
    let second = install(IntegrationShell::Zsh, home, &dir, "ctrl-g").unwrap();
    assert!(!second.changed);
    assert!(!second.created_backup);
    let rc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert_eq!(rc.matches(MANAGED_BEGIN).count(), 1);
    assert!(rc.contains("export KEEP=1"));
    let script = fs::read_to_string(dir.join("integration.zsh")).unwrap();
    assert!(script.contains("bindkey '^G'"));
    assert!(script.contains("CMDGEN_SHELL=zsh"));
    assert!(script.contains("command cmdgen --"));
    assert!(script.contains("zle redisplay"));
    assert!(!script.contains("zle reset-prompt"));
    assert!(!script.contains("eval "));

    let reports = uninstall(&[IntegrationShell::Zsh], home, &dir).unwrap();
    assert!(reports[0].removed_block);
    assert!(reports[0].removed_script);
    let rc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(!rc.contains(MANAGED_BEGIN));
    assert!(rc.contains("export KEEP=1"));
    assert!(rc.contains("bindkey '^G' existing"));
    assert!(!dir.join("integration.zsh").exists());
}

#[test]
fn cli_install_and_uninstall_are_idempotent() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .args(["shell", "install", "zsh"])
        .assert()
        .success()
        .stdout(contains("source ~/.zshrc"));
    sandbox
        .command()
        .args(["shell", "install", "zsh"])
        .assert()
        .success()
        .stdout(contains("未重复修改"));
    let rc = fs::read_to_string(sandbox.home.join(".zshrc")).unwrap();
    assert_eq!(rc.matches(MANAGED_BEGIN).count(), 1);
    sandbox
        .command()
        .args(["shell", "uninstall", "zsh"])
        .assert()
        .success()
        .stdout(contains("已卸载"));
    let rc = fs::read_to_string(sandbox.home.join(".zshrc")).unwrap();
    assert!(!rc.contains(MANAGED_BEGIN));
}

#[test]
fn zsh_redisplay_keeps_prompt_around_generated_command() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let widget = sandbox.config_dir().join("integration.zsh");
    let output = Command::new("python3")
        .arg("-c")
        .arg(include_str!("zsh_redraw.py"))
        .env("CMDGEN_WIDGET", &widget)
        .env("CMDGEN_BIN", &bin)
        .env("CMDGEN_FAKE_OUTPUT", "docker ps -a")
        .output()
        .expect("python3");
    let text = format!(
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert!(text.contains("BEFORE_INPUT=True"), "{text}");
    assert!(text.contains("BEFORE_GHOST=True"), "{text}");
    assert!(text.contains("BEFORE_SMASHED=False"), "{text}");
    assert!(
        text.contains("LINE=hanbd@mbp cmdgen % docker ps -a"),
        "{text}"
    );
}

#[test]
fn zsh_widget_shows_ghost_until_accept_and_keeps_input_on_failure() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");

    let success = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER='list docker'
_cmdgen_widget || true
printf 'BUFFER=%s\n' "$BUFFER"
printf 'POST=%s\n' "$POSTDISPLAY"
_cmdgen_accept || true
printf 'ACCEPTED=%s\n' "$BUFFER"
printf 'POST2=%s\n' "$POSTDISPLAY"
"#,
        false,
        "docker ps -a",
    );
    assert!(success.contains("BUFFER=list docker"), "{success}");
    assert!(success.contains("POST= ⇥ docker ps -a"), "{success}");
    assert!(
        success.contains("ACCEPTED=docker ps -a\nPOST2=\n"),
        "{success}"
    );
    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("SHELL=zsh"));
    assert!(logged.contains("ARGS=-- list docker"));

    fs::remove_file(&log).ok();
    let failed = run_zsh(&sandbox, &bin, &log, "list docker", true, "nope");
    assert!(failed.contains("BUFFER=list docker"), "{failed}");

    fs::remove_file(&log).ok();
    let blank = run_zsh(&sandbox, &bin, &log, "   ", false, "nope");
    assert!(blank.contains("BUFFER=   "), "{blank}");
    assert!(!log.exists());
}

#[test]
fn zsh_marks_destructive_commands_and_drops_incomplete_ones() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");

    let marked = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
_cmdgen_is_destructive 'rm -rf /tmp/x' && print YESRM || print NORM
_cmdgen_is_destructive 'ls -la' && print YESLS || print NOLS
_cmdgen_is_destructive 'sudo rm /tmp/x' && print YESSUDO || print NOSUDO
_cmdgen_is_destructive 'git push origin --force' && print YESGIT || print NOGIT
_cmdgen_is_destructive 'sed -i "" x' && print YESSED || print NOSED
san=$(_cmdgen_sanitize $'ls\e[31m')
print -r -- "SAN=$san"
BUFFER='delete temp'
_cmdgen_widget || true
print -r -- "POST=$POSTDISPLAY"
print -r -- "HL=$_CMDGEN_LAST_HIGHLIGHT"
"#,
        false,
        "rm -rf /tmp/x",
    );
    assert!(marked.contains("YESRM"), "{marked}");
    assert!(marked.contains("NOLS"), "{marked}");
    assert!(marked.contains("YESSUDO"), "{marked}");
    assert!(marked.contains("YESGIT"), "{marked}");
    assert!(marked.contains("YESSED"), "{marked}");
    assert!(marked.contains("SAN=ls"), "{marked}");
    assert!(marked.contains("POST= ⇥ ⚠ rm -rf /tmp/x"), "{marked}");
    assert!(marked.contains("fg=red"), "{marked}");

    fs::remove_file(&log).ok();
    let dropped = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER='do something'
_cmdgen_widget || true
printf 'BUFFER=%s\n' "$BUFFER"
printf 'POST=%s\n' "$POSTDISPLAY"
"#,
        false,
        "echo foo &&",
    );
    assert!(dropped.contains("BUFFER=do something"), "{dropped}");
    assert!(dropped.contains("POST=\n"), "{dropped}");
}

#[test]
fn zsh_resourcing_replaces_an_already_loaded_widget() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");
    let output = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
_cmdgen_widget() { print -r -- 'OLD' }
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER='list docker'
_cmdgen_widget || true
printf 'POST=%s\n' "$POSTDISPLAY"
"#,
        false,
        "docker ps -a",
    );
    assert!(!output.contains("OLD"), "{output}");
    assert!(output.contains("POST= ⇥ docker ps -a"), "{output}");
}

#[test]
fn zsh_trigger_again_requests_a_new_command() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");
    let output = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER='list docker'
_cmdgen_widget || true
_cmdgen_widget || true
printf 'BUFFER=%s\n' "$BUFFER"
"#,
        false,
        "docker ps -a",
    );
    assert!(output.contains("BUFFER=list docker"), "{output}");
    let logged = fs::read_to_string(&log).unwrap();
    assert_eq!(logged.matches("ARGS=-- list docker").count(), 2, "{logged}");
}

#[test]
fn zsh_multiline_result_replaces_the_buffer() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Zsh,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-g",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");
    let output = run_zsh_script(
        &sandbox,
        &bin,
        &log,
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER='two lines'
_cmdgen_widget || true
print -r -- "BUFFER=${BUFFER//$'\n'/NL}"
printf 'POST=%s\n' "$POSTDISPLAY"
"#,
        false,
        r"echo a\necho b",
    );
    assert!(output.contains("BUFFER=echo aNLecho b"), "{output}");
    assert!(output.contains("POST=\n"), "{output}");
}

#[test]
fn bash_function_replaces_on_success_and_keeps_input_on_failure() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Bash,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-x",
    )
    .unwrap();
    let script = fs::read_to_string(sandbox.config_dir().join("integration.bash")).unwrap();
    assert!(script.contains("bind -x '\"\\C-x\": _cmdgen_bash'"));
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");

    let success = run_bash(&sandbox, &bin, &log, "list docker", false, "docker ps -a");
    assert!(success.contains("LINE=docker ps -a"), "{success}");
    assert!(success.contains("POINT=12"), "{success}");
    let logged = fs::read_to_string(&log).unwrap();
    assert!(logged.contains("SHELL=bash"));
    assert!(logged.contains("ARGS=-- list docker"));

    fs::remove_file(&log).ok();
    let failed = run_bash(&sandbox, &bin, &log, "list docker", true, "nope");
    assert!(failed.contains("LINE=list docker"), "{failed}");
}

#[test]
fn bash_cancel_keeps_the_original_line() {
    let sandbox = common::Sandbox::new();
    install(
        IntegrationShell::Bash,
        &sandbox.home,
        &sandbox.config_dir(),
        "ctrl-x",
    )
    .unwrap();
    let bin = sandbox.dir.path().join("bin");
    common::write_executable(&bin.join("cmdgen"), FAKE_CMDGEN);
    let log = sandbox.dir.path().join("log");
    let script = r#"
source "$HOME/.config/cmdgen/integration.bash"
READLINE_LINE='list docker'
READLINE_POINT=${#READLINE_LINE}
_cmdgen_bash
printf 'LINE=%s\n' "$READLINE_LINE"
"#;
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .env("HOME", &sandbox.home)
        .env("PATH", path)
        .env("CMDGEN_FAKE_LOG", &log)
        .env("CMDGEN_FAKE_FAIL", "0")
        .env("CMDGEN_FAKE_OUTPUT", "docker ps -a")
        .env("CMDGEN_FAKE_SLEEP", "5")
        .env_remove("CMDGEN_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("无法运行 bash");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"x")
        .expect("写入取消键");
    drop(child.stdin.take());
    let result = child.wait_with_output().expect("等待 bash");
    let text = format!(
        "status={}\nstdout={}\nstderr={}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(result.status.success(), "{text}");
    assert!(text.contains("LINE=list docker"), "{text}");
}

fn run_zsh(
    sandbox: &common::Sandbox,
    bin: &std::path::Path,
    log: &std::path::Path,
    buffer: &str,
    fail: bool,
    output: &str,
) -> String {
    let script = format!(
        r#"
source "$HOME/.config/cmdgen/integration.zsh"
BUFFER={buffer}
_cmdgen_widget || true
printf 'BUFFER=%s\n' "$BUFFER"
"#,
        buffer = shell_single(buffer),
    );
    run_zsh_script(sandbox, bin, log, &script, fail, output)
}

fn run_zsh_script(
    sandbox: &common::Sandbox,
    bin: &std::path::Path,
    log: &std::path::Path,
    script: &str,
    fail: bool,
    output: &str,
) -> String {
    run_shell(
        "zsh",
        &["-f", "-c", script],
        sandbox,
        bin,
        log,
        fail,
        output,
    )
}

fn run_bash(
    sandbox: &common::Sandbox,
    bin: &std::path::Path,
    log: &std::path::Path,
    line: &str,
    fail: bool,
    output: &str,
) -> String {
    let script = format!(
        r#"
source "$HOME/.config/cmdgen/integration.bash"
READLINE_LINE={line}
READLINE_POINT=${{#READLINE_LINE}}
_cmdgen_bash
printf 'LINE=%s\n' "$READLINE_LINE"
printf 'POINT=%s\n' "$READLINE_POINT"
"#,
        line = shell_single(line),
    );
    run_shell(
        "bash",
        &["--noprofile", "--norc", "-c", &script],
        sandbox,
        bin,
        log,
        fail,
        output,
    )
}

fn run_shell(
    program: &str,
    args: &[&str],
    sandbox: &common::Sandbox,
    bin: &std::path::Path,
    log: &std::path::Path,
    fail: bool,
    output: &str,
) -> String {
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(program);
    command
        .args(args)
        .env("HOME", &sandbox.home)
        .env("PATH", path)
        .env("CMDGEN_FAKE_LOG", log)
        .env("CMDGEN_FAKE_FAIL", if fail { "1" } else { "0" })
        .env("CMDGEN_FAKE_OUTPUT", output)
        .env_remove("CMDGEN_API_KEY")
        .stdin(Stdio::null());
    let result = command
        .output()
        .unwrap_or_else(|err| panic!("无法运行 {program}: {err}"));
    format!(
        "status={}\nstdout={}\nstderr={}",
        result.status,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

fn shell_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
