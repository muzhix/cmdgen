mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use predicates::str::contains;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "sk-test-not-real";

fn provider_env(cmd: &mut assert_cmd::Command, base_url: &str) {
    cmd.env("CMDGEN_API_KEY", SECRET);
    cmd.env("CMDGEN_BASE_URL", base_url);
    cmd.env("CMDGEN_PROVIDER", "deepseek");
    cmd.env("CMDGEN_SHELL", "zsh");
}

async fn mount_content(server: &MockServer, status: u16, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn success_prints_only_the_command() {
    let server = MockServer::start().await;
    mount_content(
        &server,
        200,
        json!({"choices":[{"message":{"role":"assistant","content":"docker ps -a"}}]}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.arg("列出所有 docker 容器");
    cmd.assert().success().stdout("docker ps -a\n").stderr("");

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.url.path(), "/chat/completions");
    assert_eq!(
        request
            .headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("Bearer {SECRET}")
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["model"], "deepseek-flash");
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["stream"], false);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("tools").is_none());
    assert_eq!(body["messages"][1]["content"], "列出所有 docker 容器");
    let system = body["messages"][0]["content"].as_str().unwrap();
    assert!(system.contains("Target shell: zsh"));
    assert!(system.contains("Target operating system:"));
    assert!(!String::from_utf8_lossy(&request.body).contains(SECRET));
}

#[tokio::test]
async fn double_dash_keeps_reserved_words_as_prompts() {
    let server = MockServer::start().await;
    mount_content(
        &server,
        200,
        json!({"choices":[{"message":{"content":"echo config"}}]}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.args(["--", "config"]);
    cmd.assert().success().stdout("echo config\n");
    let body: serde_json::Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["messages"][1]["content"], "config");
}

#[tokio::test]
async fn api_failure_keeps_stdout_empty_and_redacts_secrets() {
    let server = MockServer::start().await;
    mount_content(
        &server,
        401,
        json!({"error":{"message": format!("bad key {SECRET}")}}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.arg("列出容器");
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("401"));
    assert!(stderr.contains("[REDACTED]"));
    assert!(!stderr.contains("Generating"));
    common::assert_no_secret(&stderr, SECRET);
}

#[tokio::test]
async fn invalid_json_and_explanation_fail_without_stdout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not-json"))
        .mount(&server)
        .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.arg("列出容器");
    cmd.assert().failure().stdout("").stderr(contains("JSON"));
}

#[tokio::test]
async fn verbose_omits_prompt_and_key() {
    let server = MockServer::start().await;
    mount_content(
        &server,
        200,
        json!({"choices":[{"message":{"content":"echo ok"}}]}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.args(["--verbose", "UNIQUE_PROMPT_TOKEN"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "echo ok\n");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("deepseek-flash"));
    assert!(stderr.contains("/chat/completions"));
    assert!(stderr.contains("elapsed"));
    assert!(!stderr.contains("UNIQUE_PROMPT_TOKEN"));
    common::assert_no_secret(&stderr, SECRET);
}

#[tokio::test]
async fn timeout_reports_on_stderr() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(json!({"choices":[{"message":{"content":"ls"}}]})),
        )
        .mount(&server)
        .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.args(["--timeout", "1", "列出文件"]);
    cmd.assert()
        .failure()
        .stdout("")
        .stderr(contains("timed out"));
}

#[tokio::test]
async fn tty_shows_loading_then_only_the_command() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(500))
                .set_body_json(json!({"choices":[{"message":{"content":"ls"}}]})),
        )
        .mount(&server)
        .await;

    let sandbox = common::Sandbox::new();
    let output = tty_cmdgen(&sandbox, &server.uri(), &["列出文件"]);
    let raw = String::from_utf8_lossy(&output.stdout);
    let visible = terminal_text(&raw);
    assert!(
        output.status.success(),
        "status={:?}\nraw={raw:?}\nvisible={visible:?}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        raw.contains('⠋') && raw.contains("Generating"),
        "raw={raw:?}"
    );
    assert_eq!(visible, "ls");
}

#[tokio::test]
async fn tty_loading_clears_before_the_error() {
    let server = MockServer::start().await;
    mount_content(&server, 403, json!({"error":{"message":"denied"}})).await;

    let sandbox = common::Sandbox::new();
    let output = tty_cmdgen(&sandbox, &server.uri(), &["列出文件"]);
    let raw = String::from_utf8_lossy(&output.stdout);
    let visible = terminal_text(&raw);
    assert!(
        !output.status.success(),
        "status={:?}\nraw={raw:?}\nvisible={visible:?}",
        output.status
    );
    assert!(raw.contains("Generating"), "raw={raw:?}");
    assert!(visible.contains("403"), "visible={visible:?}");
    assert!(!visible.contains("Generating"), "visible={visible:?}");
}

fn tty_cmdgen(sandbox: &common::Sandbox, base_url: &str, args: &[&str]) -> std::process::Output {
    let mut cmd = assert_cmd::Command::new("python3");
    cmd.arg("-c")
        .arg(include_str!("tty_loading.py"))
        .arg(env!("CARGO_BIN_EXE_cmdgen"))
        .args(args)
        .env("HOME", &sandbox.home)
        .env("XDG_CONFIG_HOME", &sandbox.config_home)
        .env("CMDGEN_API_KEY", SECRET)
        .env("CMDGEN_BASE_URL", base_url)
        .env("CMDGEN_PROVIDER", "deepseek")
        .env("CMDGEN_SHELL", "zsh")
        .env_remove("CMDGEN_DEBUG")
        .timeout(std::time::Duration::from_secs(15));
    cmd.output().expect("python3 pty")
}

fn terminal_text(raw: &str) -> String {
    let mut line = String::new();
    let mut lines = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                lines.push(std::mem::take(&mut line));
            }
            '\r' => line.clear(),
            '\n' => lines.push(std::mem::take(&mut line)),
            '\u{1b}' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for code in chars.by_ref() {
                        if code.is_ascii_alphabetic() {
                            if code == 'K' {
                                line.clear();
                            }
                            break;
                        }
                    }
                }
            }
            other => line.push(other),
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn missing_key_has_empty_stdout_and_setup_help() {
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    cmd.env("CMDGEN_PROVIDER", "cmdgen-test-missing");
    cmd.env("CMDGEN_BASE_URL", "https://example.com/v1");
    cmd.env("CMDGEN_MODEL", "test-model");
    cmd.arg("列出容器");
    let output = cmd.output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("CMDGEN_API_KEY"));
    assert!(stderr.contains("set-key"));
}

#[test]
fn version_help_and_reserved_subcommand() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains("cmdgen 0.1.4"));
    sandbox
        .command()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("does not run the generated command"));
    sandbox.command().args(["config"]).assert().failure();
}

#[test]
fn config_init_is_private_and_idempotent() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(contains("created"));
    let path = sandbox.config_dir().join("config.toml");
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let dir_mode = fs::metadata(sandbox.config_dir())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700);
    let original = fs::read_to_string(&path).unwrap();
    assert!(original.lines().all(|line| {
        let code = line
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let key = code
            .split_once('=')
            .map(|(key, _)| key.trim())
            .unwrap_or("");
        !matches!(key, "api_key" | "apikey" | "token" | "secret" | "password")
    }));
    assert!(original.contains("[providers.deepseek]"));
    assert!(original.contains("[providers.bailian]"));
    assert!(original.contains("active = \"deepseek\""));
    assert!(original.contains("qwen3.8-flash"));
    assert!(original.contains("language = \"en\""));
    sandbox
        .command()
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(contains("already exists"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn config_show_redacts_key_and_set_key_rejects_argv() {
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    cmd.env("CMDGEN_API_KEY", SECRET);
    cmd.args(["config", "show"]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stdout.contains("environment variable CMDGEN_API_KEY"));
    assert!(stdout.contains("language: en"));
    assert!(stdout.contains("deepseek-flash"));
    common::assert_no_secret(&stdout, SECRET);
    common::assert_no_secret(&stderr, SECRET);

    let output = sandbox
        .command()
        .args(["config", "set-key", "SENTINEL_SECRET_VALUE"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    common::assert_no_secret(&combined, "SENTINEL_SECRET_VALUE");
}

#[tokio::test]
async fn doctor_reports_without_printing_the_key_or_calling_completions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    cmd.env("CMDGEN_API_KEY", SECRET);
    cmd.env("CMDGEN_BASE_URL", server.uri());
    cmd.args(["doctor"]);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "stderr={stderr}\nstdout={stdout}");
    assert!(stdout.contains("result: ready"));
    assert!(stdout.contains("redacted"));
    common::assert_no_secret(&stdout, SECRET);
    common::assert_no_secret(&stderr, SECRET);
    let requests = server.received_requests().await.unwrap();
    assert!(requests
        .iter()
        .all(|request| request.method.as_str() != "POST"));
}

#[tokio::test]
async fn generated_command_is_never_executed() {
    let server = MockServer::start().await;
    let marker = common::Sandbox::new();
    let marker_path = marker.dir.path().join("should-not-exist");
    let command = format!("touch {}", marker_path.display());
    mount_content(
        &server,
        200,
        json!({"choices":[{"message":{"content": command}}]}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let mut cmd = sandbox.command();
    provider_env(&mut cmd, &server.uri());
    cmd.arg("创建文件");
    cmd.assert().success().stdout(format!("{command}\n"));
    assert!(!marker_path.exists());
}

#[test]
fn config_use_switches_the_saved_provider() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .args(["config", "init"])
        .assert()
        .success();
    let path = sandbox.config_dir().join("config.toml");

    sandbox
        .command()
        .args(["config", "providers"])
        .assert()
        .success()
        .stdout(contains("* deepseek"))
        .stdout(contains("bailian"));

    sandbox
        .command()
        .args(["config", "use", "bailian"])
        .assert()
        .success()
        .stdout(contains("switched to provider: bailian"));
    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("active = \"bailian\"") || updated.contains("active = 'bailian'"));
    assert!(updated.contains("https://api.deepseek.com"));

    sandbox
        .command()
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(contains("provider.name: bailian"))
        .stdout(contains("dashscope.aliyuncs.com"));
}

#[test]
fn config_set_writes_the_provider_endpoint() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .args(["config", "init"])
        .assert()
        .success();
    let path = sandbox.config_dir().join("config.toml");
    let original = fs::read_to_string(&path).unwrap();

    sandbox
        .command()
        .args(["config", "set"])
        .assert()
        .failure()
        .stderr(contains("--model"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    sandbox
        .command()
        .args([
            "config",
            "show",
            "--provider",
            "bailian",
            "--base-url",
            "https://preview.example/v1",
        ])
        .assert()
        .success()
        .stdout(contains("https://preview.example/v1"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    sandbox
        .command()
        .env("CMDGEN_BASE_URL", "https://evil.example/v1")
        .args([
            "config",
            "set",
            "--provider",
            "bailian",
            "--base-url",
            "https://llm.example.com/compatible-mode/v1",
            "--model",
            "qwen-plus",
        ])
        .assert()
        .success()
        .stdout(contains("updated provider: bailian"))
        .stdout(contains("https://llm.example.com/compatible-mode/v1"))
        .stdout(contains("qwen-plus"));

    let updated = fs::read_to_string(&path).unwrap();
    assert!(updated.contains("active = \"deepseek\"") || updated.contains("active = 'deepseek'"));
    assert!(updated.contains("https://api.deepseek.com"));
    assert!(updated.contains("https://llm.example.com/compatible-mode/v1"));
    assert!(updated.contains("qwen-plus"));
    assert!(!updated.contains("evil.example"));
    assert!(updated.contains("DASHSCOPE_API_KEY"));

    sandbox
        .command()
        .args(["config", "show", "--provider", "bailian"])
        .assert()
        .success()
        .stdout(contains("provider.name: bailian"))
        .stdout(contains("https://llm.example.com/compatible-mode/v1"))
        .stdout(contains("provider.model: qwen-plus"));
}

#[tokio::test]
async fn named_provider_uses_its_endpoint_without_thinking() {
    let server = MockServer::start().await;
    mount_content(
        &server,
        200,
        json!({"choices":[{"message":{"content":"ls -lh"}}]}),
    )
    .await;
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "active = \"deepseek\"\n\n[providers.deepseek]\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-flash\"\n\n[providers.bailian]\nbase_url = \"{}\"\nmodel = \"qwen-plus\"\n",
            server.uri()
        ),
    )
    .unwrap();
    let mut cmd = sandbox.command();
    cmd.env("CMDGEN_API_KEY", SECRET);
    cmd.args(["--provider", "bailian", "列出大文件"]);
    cmd.assert().success().stdout("ls -lh\n");
    let body: serde_json::Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["model"], "qwen-plus");
    assert!(body.get("thinking").is_none());
}

#[test]
fn chinese_init_and_use_messages() {
    let sandbox = common::Sandbox::new();
    sandbox
        .command()
        .args(["config", "init", "--lang", "zh"])
        .assert()
        .success()
        .stdout(contains("已创建"));
    let text = fs::read_to_string(sandbox.config_dir().join("config.toml")).unwrap();
    assert!(text.contains("language = \"zh\""));
    sandbox
        .command()
        .args(["config", "use", "bailian", "--lang", "zh"])
        .assert()
        .success()
        .stdout(contains("已切换到 provider: bailian"));
}
