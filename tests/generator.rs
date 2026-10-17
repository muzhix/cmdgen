use std::fs;
use std::path::Path;

use cmdgen::error::redact;
use cmdgen::generator::client::{
    build_request_body, completions_endpoint, interpret_http, RequestOptions,
};
use cmdgen::generator::output::normalize_command;
use cmdgen::generator::prompt::{system_prompt, user_message};

#[test]
fn endpoint_normalization() {
    assert_eq!(
        completions_endpoint("https://api.deepseek.com/").unwrap(),
        "https://api.deepseek.com/chat/completions"
    );
    assert_eq!(
        completions_endpoint("https://api.deepseek.com//").unwrap(),
        "https://api.deepseek.com/chat/completions"
    );
    assert_eq!(
        completions_endpoint("https://api.openai.com/v1").unwrap(),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        completions_endpoint("http://127.0.0.1:8080/v1/").unwrap(),
        "http://127.0.0.1:8080/v1/chat/completions"
    );
    assert!(completions_endpoint("https://api.deepseek.com/chat/completions").is_err());
    assert!(completions_endpoint("http://example.com").is_err());
    assert!(completions_endpoint("https://api.deepseek.com/v1?x=1").is_err());
}

#[test]
fn prompt_includes_os_and_shell_only() {
    let prompt = system_prompt("macOS", "zsh");
    assert!(prompt.contains("Target operating system: macOS"));
    assert!(prompt.contains("Target shell: zsh"));
    assert!(prompt.contains("Output only the shell command."));
    assert!(prompt.contains("Never execute the generated command."));
    assert!(!prompt.contains("README"));
    let user = user_message("列出容器", None);
    assert_eq!(user, "列出容器");
    let with_cwd = user_message("列出容器", Some(Path::new("/work")));
    assert!(with_cwd.contains("/work"));
    assert!(with_cwd.contains("列出容器"));
}

#[test]
fn deepseek_request_disables_thinking_and_omits_the_key() {
    let body = build_request_body(&RequestOptions {
        model: "deepseek-flash",
        temperature: 0.0,
        max_tokens: 256,
        system: "system",
        user: "列出容器",
        disable_thinking: true,
    });
    assert_eq!(body["model"], "deepseek-flash");
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["stream"], false);
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("tools").is_none());
    assert_eq!(body["messages"][1]["content"], "列出容器");
    let text = body.to_string();
    assert!(!text.contains("sk-"));
}

#[test]
fn compatible_request_does_not_send_thinking() {
    let body = build_request_body(&RequestOptions {
        model: "gpt-4o-mini",
        temperature: 0.0,
        max_tokens: 256,
        system: "system",
        user: "ls",
        disable_thinking: false,
    });
    assert!(body.get("thinking").is_none());
}

#[test]
fn output_normalization_keeps_multiline_commands_and_rejects_prose() {
    assert_eq!(
        normalize_command("```bash\ndocker ps -a\n```").unwrap(),
        "docker ps -a"
    );
    assert_eq!(
        normalize_command("```\nfoo && \\\nbar\n```\n").unwrap(),
        "foo && \\\nbar"
    );
    assert_eq!(
        normalize_command("echo hello # comment\nls; pwd\n").unwrap(),
        "echo hello # comment\nls; pwd"
    );
    assert!(normalize_command("   \n").is_err());
    assert!(normalize_command("```\n\n```").is_err());
    let prose = normalize_command("Here is the command:\ndocker ps -a").unwrap_err();
    assert!(prose.to_string().contains("解释"));
    assert!(normalize_command("以下是命令\nls").is_err());
}

#[test]
fn http_errors_are_redacted_and_invalid_json_is_rejected() {
    let secret = "sk-test-not-real";
    let err = interpret_http(
        401,
        &format!(r#"{{"error":{{"message":"bad key {secret}"}}}}"#),
        secret,
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("401"));
    assert!(text.contains("[REDACTED]"));
    assert!(!text.contains(secret));

    assert!(interpret_http(200, "not-json", secret).is_err());
    assert!(interpret_http(200, r#"{"choices":[]}"#, secret).is_err());
    assert_eq!(
        interpret_http(
            200,
            r#"{"choices":[{"message":{"content":"docker ps -a","reasoning_content":"think"}}]}"#,
            secret,
        )
        .unwrap(),
        "docker ps -a"
    );
    assert!(interpret_http(
        200,
        r#"{"choices":[{"message":{"content":"","reasoning_content":"only thoughts"}}]}"#,
        secret,
    )
    .is_err());
}

#[test]
fn redact_ignores_short_secrets() {
    assert_eq!(redact("abc", "ab"), "abc");
    assert_eq!(redact("sk-test-not-real", "sk-test-not-real"), "[REDACTED]");
}

#[test]
fn product_source_never_executes_commands_or_disables_tls() {
    let mut source = String::new();
    read_tree(Path::new("src"), &mut source);
    read_tree(Path::new("shell"), &mut source);
    assert!(!source.contains("danger_accept_invalid_certs"));
    assert!(!source.contains("Command::new"));
    assert!(!source.contains("sh -c"));
    assert!(!source.contains("bash -c"));
    assert!(!source.contains("eval "));
    assert!(source.contains("command cmdgen --"));
}
fn read_tree(dir: &Path, out: &mut String) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            read_tree(&path, out);
        } else {
            out.push_str(&fs::read_to_string(&path).unwrap());
            out.push('\n');
        }
    }
}
