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
