mod common;

use std::fs;

fn has_cjk(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

fn write_config(sandbox: &common::Sandbox, body: &str) {
    let dir = sandbox.config_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), body).unwrap();
}

fn stdout_of(sandbox: &common::Sandbox, args: &[&str]) -> String {
    let output = sandbox.command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn default_help_is_english_without_config_or_env() {
    let sandbox = common::Sandbox::new();
    let stdout = stdout_of(&sandbox, &["--help"]);
    assert!(stdout.contains("does not run the generated command"));
    assert!(!has_cjk(&stdout), "{stdout}");
}

#[test]
fn flag_and_env_select_chinese_help() {
    let sandbox = common::Sandbox::new();
    let stdout = stdout_of(&sandbox, &["--lang", "zh", "--help"]);
    assert!(stdout.contains("不会执行生成的命令"));

    let output = sandbox
        .command()
        .env("CMDGEN_LANG", "zh")
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("不会执行生成的命令"));

    let stdout = stdout_of(&sandbox, &["--lang", "zh-CN", "--help"]);
    assert!(stdout.contains("不会执行生成的命令"));
}

#[test]
fn config_language_drives_help_show_and_doctor() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "language = \"zh\"\n");

    let stdout = stdout_of(&sandbox, &["--help"]);
    assert!(stdout.contains("不会执行生成的命令"));

    let output = sandbox
        .command()
        .env("CMDGEN_API_KEY", "sk-test-not-real")
        .args(["config", "show"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("language: zh"));
    assert!(stdout.contains("已脱敏"));
    assert!(stdout.contains("配置文件"));

    let output = sandbox
        .command()
        .env("CMDGEN_API_KEY", "sk-test-not-real")
        .env("CMDGEN_BASE_URL", "http://127.0.0.1:1")
        .arg("doctor")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("配置文件"), "{stdout}");
    assert!(stdout.contains("结果:"), "{stdout}");
    assert!(has_cjk(&stdout));
}

#[test]
fn flag_and_env_override_config_chinese() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "language = \"zh\"\n");

    let stdout = stdout_of(&sandbox, &["--lang", "en", "--help"]);
    assert!(stdout.contains("does not run the generated command"));
    assert!(!has_cjk(&stdout), "{stdout}");

    let output = sandbox
        .command()
        .env("CMDGEN_LANG", "en")
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!has_cjk(&stdout), "{stdout}");

    let output = sandbox
        .command()
        .env("CMDGEN_LANG", "zh")
        .args(["--lang", "en", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!has_cjk(&stdout), "{stdout}");

    let output = sandbox
        .command()
        .env("CMDGEN_API_KEY", "sk-test-not-real")
        .args(["config", "show", "--lang", "en"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("language: en"));
    assert!(stdout.contains("redacted"));
    assert!(!stdout.contains("已脱敏"));
}

#[test]
fn invalid_lang_flag_and_config_value_exit_nonzero() {
    let sandbox = common::Sandbox::new();
    let output = sandbox.command().args(["--lang", "fr"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("fr"));
    assert!(stderr.contains("supported: en, zh"));
    assert!(!has_cjk(&stderr), "{stderr}");

    write_config(&sandbox, "language = \"fr\"\n");
    let output = sandbox.command().args(["config", "show"]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("fr"));
    assert!(stderr.contains("supported: en, zh"));
    assert!(!has_cjk(&stderr), "{stderr}");
}

#[test]
fn missing_language_key_stays_english_and_init_writes_it() {
    let sandbox = common::Sandbox::new();
    write_config(
        &sandbox,
        "active = \"deepseek\"\n\n[providers.deepseek]\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-flash\"\n",
    );
    let stdout = stdout_of(&sandbox, &["config", "show"]);
    assert!(stdout.contains("language: en"));
    assert!(!has_cjk(&stdout), "{stdout}");

    let fresh = common::Sandbox::new();
    fresh.command().args(["config", "init"]).assert().success();
    let text = fs::read_to_string(fresh.config_dir().join("config.toml")).unwrap();
    assert!(text.contains("language = \"en\""));

    let zh = common::Sandbox::new();
    zh.command()
        .env("CMDGEN_LANG", "zh-CN")
        .args(["config", "init"])
        .assert()
        .success();
    let text = fs::read_to_string(zh.config_dir().join("config.toml")).unwrap();
    assert!(text.contains("language = \"zh\""));
    assert!(!text.contains("language = \"zh-CN\""));
}

#[test]
fn help_ignores_a_broken_config_file() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "this is not toml {{\n");
    let stdout = stdout_of(&sandbox, &["--help"]);
    assert!(stdout.contains("does not run the generated command"));
    assert!(!has_cjk(&stdout));
}

#[test]
fn alias_en_us_in_config_resolves_to_en() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "language = \"en-US\"\n");
    let stdout = stdout_of(&sandbox, &["config", "show"]);
    assert!(stdout.contains("language: en"));
    assert!(!has_cjk(&stdout), "{stdout}");

    write_config(&sandbox, "language = \"zh-Hans\"\n");
    let stdout = stdout_of(&sandbox, &["--help"]);
    assert!(stdout.contains("不会执行生成的命令"));
}

#[test]
fn config_lang_help_follows_the_interface_language() {
    let sandbox = common::Sandbox::new();
    let stdout = stdout_of(&sandbox, &["config", "lang", "--help"]);
    assert!(stdout.contains("arrow keys"));
    assert!(!has_cjk(&stdout), "{stdout}");

    let stdout = stdout_of(&sandbox, &["config", "lang", "--lang", "zh", "--help"]);
    assert!(stdout.contains("上下键"));
}

#[test]
fn config_lang_without_a_terminal_does_not_write() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "language = \"en\"\n");
    let output = sandbox.command().args(["config", "lang"]).output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("interactive terminal"), "{stderr}");
    assert!(!has_cjk(&stderr), "{stderr}");
    let text = fs::read_to_string(sandbox.config_dir().join("config.toml")).unwrap();
    assert!(text.contains("language = \"en\""));

    let output = sandbox
        .command()
        .args(["config", "lang", "--lang", "zh"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("需要交互式终端"), "{stderr}");
    let text = fs::read_to_string(sandbox.config_dir().join("config.toml")).unwrap();
    assert_eq!(text, "language = \"en\"\n");
}

#[test]
fn config_lang_arrow_selects_chinese_and_esc_cancels() {
    let sandbox = common::Sandbox::new();
    write_config(&sandbox, "language = \"en\"\n");
    let output = tty_menu(&sandbox, "1b5b420d", &["config", "lang"]);
    let visible = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "status={:?}\nvisible={visible}",
        output.status
    );
    assert!(visible.contains("已切换界面语言: zh"), "visible={visible}");
    let text = fs::read_to_string(sandbox.config_dir().join("config.toml")).unwrap();
    assert!(text.contains("language = \"zh\""), "{text}");
    assert!(!text.contains("language = \"en\""));
    let shown = stdout_of(&sandbox, &["config", "show"]);
    assert!(shown.contains("language: zh"));
    assert!(shown.contains("配置文件:"));

    write_config(&sandbox, "language = \"en\"\n");
    let output = tty_menu(&sandbox, "1b", &["config", "lang"]);
    let visible = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "status={:?}\nvisible={visible}",
        output.status
    );
    assert!(visible.contains("cancelled"), "visible={visible}");
    let text = fs::read_to_string(sandbox.config_dir().join("config.toml")).unwrap();
    assert_eq!(text, "language = \"en\"\n");
}

fn tty_menu(sandbox: &common::Sandbox, keys_hex: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = assert_cmd::Command::new("python3");
    cmd.arg("-c")
        .arg(include_str!("tty_menu.py"))
        .arg(env!("CARGO_BIN_EXE_cmdgen"))
        .args(args)
        .env("HOME", &sandbox.home)
        .env("XDG_CONFIG_HOME", &sandbox.config_home)
        .env("SHELL", "/bin/zsh")
        .env("CMDGEN_TTY_KEYS", keys_hex)
        .env("CMDGEN_TTY_MARKER", "English (en)")
        .env_remove("CMDGEN_API_KEY")
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("DASHSCOPE_API_KEY")
        .env_remove("CMDGEN_BASE_URL")
        .env_remove("CMDGEN_MODEL")
        .env_remove("CMDGEN_PROVIDER")
        .env_remove("CMDGEN_SHELL")
        .env_remove("CMDGEN_TIMEOUT")
        .env_remove("CMDGEN_LANG")
        .env_remove("CMDGEN_DEBUG")
        .timeout(std::time::Duration::from_secs(15));
    cmd.output().expect("python3 pty")
}
