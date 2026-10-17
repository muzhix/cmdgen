mod common;

use cmdgen::config::{
    delete_api_key, load_effective, merge, parse_config, resolve_api_key, save_api_key,
    CredentialFile, EnvMap, FileConfig, MemoryStore, Overrides, SavedKey, SecretStore,
    DEFAULT_CONFIG_TOML,
};
use cmdgen::shell::detect::resolve_shell;

struct UnavailableStore;

impl SecretStore for UnavailableStore {
    fn get(&self, _account: &str) -> Result<Option<String>, cmdgen::Error> {
        Err(cmdgen::Error::CredentialStoreUnavailable {
            reason: "测试环境没有凭据存储".into(),
        })
    }

    fn set(&self, _account: &str, _secret: &str) -> Result<(), cmdgen::Error> {
        Err(cmdgen::Error::CredentialStoreUnavailable {
            reason: "测试环境没有凭据存储".into(),
        })
    }

    fn delete(&self, _account: &str) -> Result<bool, cmdgen::Error> {
        Err(cmdgen::Error::CredentialStoreUnavailable {
            reason: "测试环境没有凭据存储".into(),
        })
    }
}

fn credential_file(sandbox: &common::Sandbox) -> CredentialFile {
    CredentialFile::new(sandbox.config_dir().join("credentials.toml"))
}

fn unix_mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

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
fn api_key_priority_prefers_env_then_store_then_file() {
    let sandbox = common::Sandbox::new();
    let store = MemoryStore::default();
    store.set("deepseek", "from-store-secret").unwrap();
    let file = credential_file(&sandbox);
    file.set("deepseek", "from-file-secret").unwrap();
    let mut env = EnvMap::new();
    env.set("CMDGEN_API_KEY", "from-cmdgen-secret");
    env.set("DEEPSEEK_API_KEY", "from-deepseek-secret");
    env.set("OPENAI_API_KEY", "from-openai-secret");

    let resolved = resolve_api_key("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-cmdgen-secret");
    assert!(resolved.source_label.contains("CMDGEN_API_KEY"));

    let mut env = EnvMap::new();
    env.set("DEEPSEEK_API_KEY", "from-deepseek-secret");
    env.set("OPENAI_API_KEY", "from-openai-secret");
    let resolved = resolve_api_key("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-deepseek-secret");

    let resolved = resolve_api_key("openai", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-openai-secret");

    let env = EnvMap::new();
    store.set("custom", "from-custom-store").unwrap();
    let resolved = resolve_api_key("custom", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-custom-store");
    assert!(resolved.source_label.contains("custom"));

    let resolved = resolve_api_key("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-store-secret");
    assert!(resolved.source_label.contains("系统凭据存储"));

    let empty = MemoryStore::default();
    let resolved = resolve_api_key("deepseek", &env, &empty, &file).unwrap();
    assert_eq!(resolved.secret, "from-file-secret");
    assert!(resolved.source_label.contains("本地凭据文件"));
    assert!(!resolved.source_label.contains("from-file-secret"));

    let resolved = resolve_api_key("deepseek", &env, &UnavailableStore, &file).unwrap();
    assert_eq!(resolved.secret, "from-file-secret");
}

#[test]
fn missing_key_message_mentions_file_fallback() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let Err(err) = resolve_api_key("deepseek", &EnvMap::new(), &MemoryStore::default(), &file)
    else {
        panic!("应当缺少 API Key");
    };
    let text = err.to_string();
    assert!(text.contains("CMDGEN_API_KEY"));
    assert!(text.contains("DEEPSEEK_API_KEY"));
    assert!(text.contains("set-key"));
    assert!(text.contains("credentials.toml"));

    let Err(err) = resolve_api_key("deepseek", &EnvMap::new(), &UnavailableStore, &file) else {
        panic!("应当缺少 API Key");
    };
    let text = err.to_string();
    assert!(text.contains("系统凭据存储当前不可用"));
    assert!(text.contains("credentials.toml"));
}

#[test]
fn saving_to_the_store_removes_the_same_provider_from_the_file() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    file.set("deepseek", "old-file-secret").unwrap();
    file.set("openai", "keep-openai-secret").unwrap();
    let store = MemoryStore::default();
    let saved = save_api_key("deepseek", " new-store-secret ", &store, &file).unwrap();
    assert_eq!(
        saved,
        SavedKey::Keyring {
            removed_file_entry: true
        }
    );
    assert_eq!(
        store.get("deepseek").unwrap().as_deref(),
        Some("new-store-secret")
    );
    assert!(file.get("deepseek").unwrap().is_none());
    assert_eq!(
        file.get("openai").unwrap().as_deref(),
        Some("keep-openai-secret")
    );
    assert!(file.path().exists());
}

#[test]
fn saving_without_a_store_writes_a_private_credentials_file() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let saved = save_api_key("my.provider", " file-secret ", &UnavailableStore, &file).unwrap();
    assert_eq!(
        saved,
        SavedKey::File {
            path: file.path().to_path_buf()
        }
    );
    assert_eq!(
        file.get("my.provider").unwrap().as_deref(),
        Some("file-secret")
    );
    assert_eq!(unix_mode(file.path()), 0o600);
    assert_eq!(unix_mode(file.path().parent().unwrap()), 0o700);

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    file.set("my.provider", "rotated-secret").unwrap();
    assert_eq!(unix_mode(file.path()), 0o600);
    assert_eq!(
        file.get("my.provider").unwrap().as_deref(),
        Some("rotated-secret")
    );
}

#[test]
fn delete_removes_store_and_file_entries() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let store = MemoryStore::default();
    store.set("deepseek", "store-secret").unwrap();
    file.set("deepseek", "file-secret").unwrap();
    let deleted = delete_api_key("deepseek", &store, &file).unwrap();
    assert!(deleted.keyring && deleted.file);
    assert!(store.get("deepseek").unwrap().is_none());
    assert!(!file.path().exists());

    file.set("deepseek", "file-only-secret").unwrap();
    let deleted = delete_api_key("deepseek", &UnavailableStore, &file).unwrap();
    assert!(!deleted.keyring && deleted.file);
    assert!(!file.path().exists());

    store.set("openai", "store-only-secret").unwrap();
    let deleted = delete_api_key("openai", &store, &file).unwrap();
    assert!(deleted.keyring && !deleted.file);

    let err = delete_api_key("deepseek", &MemoryStore::default(), &file).unwrap_err();
    assert!(err.to_string().contains("没有可删除"));
    let err = delete_api_key("deepseek", &UnavailableStore, &file).unwrap_err();
    assert!(err.to_string().contains("系统凭据存储当前不可用"));
}

#[test]
fn empty_api_key_is_rejected_before_writing_a_file() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let err = save_api_key("deepseek", "  ", &MemoryStore::default(), &file).unwrap_err();
    assert!(err.to_string().contains("不能为空"));
    assert!(!file.path().exists());
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
