mod common;

use cmdgen::config::{
    load_effective, merge, parse_config, resolve_api_key, EnvMap, FileConfig, MemoryStore,
    Overrides, SecretStore, DEFAULT_CONFIG_TOML,
};
use cmdgen::shell::detect::resolve_shell;

fn env_with_home(sandbox: &common::Sandbox) -> EnvMap {
    let mut env = EnvMap::new();
    env.set("HOME", sandbox.home.to_string_lossy().as_ref());
    env.set(
        "XDG_CONFIG_HOME",
        sandbox.config_home.to_string_lossy().as_ref(),
    );
    env
}

#[test]
fn default_toml_parses_verified_deepseek_preset() {
    let config = parse_config(DEFAULT_CONFIG_TOML, std::path::Path::new("config.toml")).unwrap();
    assert_eq!(config.provider.name.as_deref(), Some("deepseek"));
    assert_eq!(
        config.provider.base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(config.provider.model.as_deref(), Some("deepseek-flash"));
    assert_eq!(config.generation.temperature, Some(0.0));
    assert_eq!(config.generation.max_tokens, Some(256));
    assert_eq!(config.generation.timeout_secs, Some(20));
    assert_eq!(config.shell.keybinding.as_deref(), Some("ctrl-g"));
    assert_eq!(config.shell.include_cwd, Some(false));
    assert!(!DEFAULT_CONFIG_TOML.to_ascii_lowercase().contains("api_key"));
}

#[test]
fn integer_temperature_and_partial_file_are_accepted() {
    let text = r#"
[provider]
name = "example"
base_url = "https://example.com/v1"
model = "custom-model"

[generation]
temperature = 0
max_tokens = 128
"#;
    let file = parse_config(text, std::path::Path::new("config.toml")).unwrap();
    let config = merge(
        file,
        true,
        std::path::PathBuf::from("config.toml"),
        &EnvMap::new(),
        &Overrides::default(),
    )
    .unwrap();
    assert_eq!(config.provider_name, "example");
    assert_eq!(config.model, "custom-model");
    assert_eq!(config.temperature, 0.0);
    assert_eq!(config.max_tokens, 128);
    assert_eq!(config.timeout_secs, 20);
}

#[test]
fn config_file_cannot_contain_an_api_key_field() {
    let text = "api_key = \"sk-secret-value\"\n";
    let err = parse_config(text, std::path::Path::new("config.toml")).unwrap_err();
    assert!(!err.to_string().contains("sk-secret-value"));
    assert!(err.to_string().contains("API Key"));
}

#[test]
fn non_secret_priority_is_cli_then_env_then_toml_then_preset() {
    let sandbox = common::Sandbox::new();
    let mut env = env_with_home(&sandbox);
    env.set("CMDGEN_MODEL", "from-env");
    env.set("CMDGEN_TIMEOUT", "30");
    let file = parse_config(DEFAULT_CONFIG_TOML, std::path::Path::new("config.toml")).unwrap();
    let overrides = Overrides {
        model: Some("from-cli".into()),
        ..Overrides::default()
    };
    let config = merge(
        file.clone(),
        true,
        sandbox.config_dir().join("config.toml"),
        &env,
        &overrides,
    )
    .unwrap();
    assert_eq!(config.model, "from-cli");
    assert_eq!(config.timeout_secs, 30);
    assert_eq!(config.base_url, "https://api.deepseek.com");

    let without_cli = merge(
        file,
        true,
        sandbox.config_dir().join("config.toml"),
        &env,
        &Overrides::default(),
    )
    .unwrap();
    assert_eq!(without_cli.model, "from-env");
}

#[test]
fn missing_file_uses_deepseek_preset() {
    let sandbox = common::Sandbox::new();
    let config = load_effective(&env_with_home(&sandbox), &Overrides::default()).unwrap();
    assert!(!config.config_exists);
    assert_eq!(config.provider_name, "deepseek");
    assert_eq!(config.model, "deepseek-flash");
    assert!(config.base_url.starts_with("https://"));
}

#[test]
fn unknown_provider_without_endpoint_is_rejected() {
    let mut env = EnvMap::new();
    env.set("HOME", "/tmp");
    env.set("CMDGEN_PROVIDER", "example");
    let err = load_effective(&env, &Overrides::default()).unwrap_err();
    assert!(err.to_string().contains("example"));
}

#[test]
fn api_key_priority_prefers_cmdgen_then_provider_env_then_store() {
    let store = MemoryStore::default();
    store.set("deepseek", "from-store-secret").unwrap();
    let mut env = EnvMap::new();
    env.set("CMDGEN_API_KEY", "from-cmdgen-secret");
    env.set("DEEPSEEK_API_KEY", "from-deepseek-secret");
    env.set("OPENAI_API_KEY", "from-openai-secret");

    let resolved = resolve_api_key("deepseek", &env, &store).unwrap();
    assert_eq!(resolved.secret, "from-cmdgen-secret");
    assert!(resolved.source_label.contains("CMDGEN_API_KEY"));

    let mut env = EnvMap::new();
    env.set("DEEPSEEK_API_KEY", "from-deepseek-secret");
    env.set("OPENAI_API_KEY", "from-openai-secret");
    let resolved = resolve_api_key("deepseek", &env, &store).unwrap();
    assert_eq!(resolved.secret, "from-deepseek-secret");

    let resolved = resolve_api_key("openai", &env, &store).unwrap();
    assert_eq!(resolved.secret, "from-openai-secret");

    let env = EnvMap::new();
    store.set("custom", "from-custom-store").unwrap();
    let resolved = resolve_api_key("custom", &env, &store).unwrap();
    assert_eq!(resolved.secret, "from-custom-store");
    assert!(resolved.source_label.contains("custom"));
}

#[test]
fn missing_key_message_does_not_require_keyring() {
    let Err(err) = resolve_api_key("deepseek", &EnvMap::new(), &MemoryStore::default()) else {
        panic!("应当缺少 API Key");
    };
    let text = err.to_string();
    assert!(text.contains("CMDGEN_API_KEY"));
    assert!(text.contains("DEEPSEEK_API_KEY"));
    assert!(text.contains("set-key"));
}

#[test]
fn shell_detection_priority_and_basename() {
    let shell = resolve_shell(Some("bash"), "zsh", Some("sh"), Some("/bin/zsh")).unwrap();
    assert_eq!(shell, "bash");
    let shell = resolve_shell(Some("auto"), "fish", Some("zsh"), Some("/bin/bash")).unwrap();
    assert_eq!(shell, "zsh");
    let shell = resolve_shell(None, "auto", None, Some("/usr/local/bin/zsh")).unwrap();
    assert_eq!(shell, "zsh");
    let shell = resolve_shell(None, "auto", None, None).unwrap();
    assert_eq!(shell, "sh");
}

#[test]
fn show_output_redacts_the_key() {
    let sandbox = common::Sandbox::new();
    let config = load_effective(&env_with_home(&sandbox), &Overrides::default()).unwrap();
    let status = cmdgen::config::KeyStatus {
        configured: true,
        source_label: "环境变量 CMDGEN_API_KEY".into(),
    };
    let rendered = cmdgen::config::render_show(&config, "zsh", &status);
    assert!(rendered.contains("deepseek-flash"));
    assert!(rendered.contains("已脱敏"));
    assert!(!rendered.contains("sk-test-not-real"));
    let file = FileConfig::default();
    assert!(file.provider.model.is_none());
}
