mod common;

use cmdgen::config::{
    activate_provider, delete_api_key, load_effective, merge, parse_config, resolve_api_key,
    save_api_key, update_provider, CredentialFile, EnvMap, FileConfig, MemoryStore, Overrides,
    SavedKey, SecretStore, DEFAULT_CONFIG_TOML,
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

fn resolve(
    provider: &str,
    env: &EnvMap,
    store: &impl SecretStore,
    file: &CredentialFile,
) -> Result<cmdgen::config::ResolvedKey, cmdgen::Error> {
    cmdgen::config::resolve_api_key(
        provider,
        Some(cmdgen::config::provider_env_var(provider)),
        env,
        store,
        file,
    )
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
    assert_eq!(config.active.as_deref(), Some("deepseek"));
    let deepseek = config.providers.get("deepseek").unwrap();
    assert_eq!(
        deepseek.base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(deepseek.model.as_deref(), Some("deepseek-flash"));
    let bailian = config.providers.get("bailian").unwrap();
    assert_eq!(
        bailian.base_url.as_deref(),
        Some("https://dashscope.aliyuncs.com/compatible-mode/v1")
    );
    assert_eq!(bailian.model.as_deref(), Some("qwen3.8-flash"));
    assert_eq!(bailian.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));
    assert_eq!(config.generation.temperature, Some(0.0));
    assert_eq!(config.generation.max_tokens, Some(256));
    assert_eq!(config.generation.timeout_secs, Some(20));
    assert_eq!(config.shell.keybinding.as_deref(), Some("ctrl-g"));
    assert_eq!(config.shell.include_cwd, Some(false));
    assert!(config_text_has_no_secret(DEFAULT_CONFIG_TOML));
}

fn config_text_has_no_secret(text: &str) -> bool {
    text.lines().all(|line| !line_assigns_secret(line))
}

fn line_assigns_secret(line: &str) -> bool {
    let code = line
        .split('#')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let Some((key, _)) = code.split_once('=') else {
        return false;
    };
    let key = key.trim().trim_matches('"');
    matches!(key, "api_key" | "apikey" | "token" | "secret" | "password")
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

    let resolved = resolve("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-cmdgen-secret");
    assert!(resolved.source_label.contains("CMDGEN_API_KEY"));

    let mut env = EnvMap::new();
    env.set("DEEPSEEK_API_KEY", "from-deepseek-secret");
    env.set("OPENAI_API_KEY", "from-openai-secret");
    let resolved = resolve("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-deepseek-secret");

    let resolved = resolve("openai", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-openai-secret");

    let env = EnvMap::new();
    store.set("custom", "from-custom-store").unwrap();
    let resolved = resolve("custom", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-custom-store");
    assert!(resolved.source_label.contains("custom"));

    let resolved = resolve("deepseek", &env, &store, &file).unwrap();
    assert_eq!(resolved.secret, "from-store-secret");
    assert!(resolved.source_label.contains("credential store"));

    let empty = MemoryStore::default();
    let resolved = resolve("deepseek", &env, &empty, &file).unwrap();
    assert_eq!(resolved.secret, "from-file-secret");
    assert!(resolved.source_label.contains("credential file"));
    assert!(!resolved.source_label.contains("from-file-secret"));

    let resolved = resolve("deepseek", &env, &UnavailableStore, &file).unwrap();
    assert_eq!(resolved.secret, "from-file-secret");
}

#[test]
fn missing_key_message_mentions_file_fallback() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let Err(err) = resolve("deepseek", &EnvMap::new(), &MemoryStore::default(), &file) else {
        panic!("应当缺少 API Key");
    };
    let text = err.to_string();
    assert!(text.contains("CMDGEN_API_KEY"));
    assert!(text.contains("DEEPSEEK_API_KEY"));
    assert!(text.contains("set-key"));
    assert!(text.contains("credentials.toml"));

    let Err(err) = resolve("deepseek", &EnvMap::new(), &UnavailableStore, &file) else {
        panic!("应当缺少 API Key");
    };
    let text = err.to_string();
    assert!(text.contains("currently unavailable"));
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
    assert!(err.to_string().contains("no API key to delete"));
    let err = delete_api_key("deepseek", &UnavailableStore, &file).unwrap_err();
    assert!(err.to_string().contains("currently unavailable"));
}

#[test]
fn empty_api_key_is_rejected_before_writing_a_file() {
    let sandbox = common::Sandbox::new();
    let file = credential_file(&sandbox);
    let err = save_api_key("deepseek", "  ", &MemoryStore::default(), &file).unwrap_err();
    assert!(err.to_string().contains("must not be empty"));
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
        source_label: "environment variable CMDGEN_API_KEY".into(),
    };
    let rendered = cmdgen::config::render_show(&config, "zsh", &status);
    assert!(rendered.contains("deepseek-flash"));
    assert!(rendered.contains("redacted"));
    assert!(rendered.contains("language: en"));
    assert!(!rendered.contains("sk-test-not-real"));
    let file = FileConfig::default();
    assert!(file.provider.model.is_none());
    assert!(rendered.contains("providers: (none)"));

    cmdgen::i18n::set_current(cmdgen::i18n::Lang::Zh);
    let rendered = cmdgen::config::render_show(&config, "zsh", &status);
    assert!(rendered.contains("已脱敏"));
    cmdgen::i18n::reset_language();
}

#[test]
fn saved_provider_keeps_its_endpoint_when_selected() {
    let text = r#"
active = "deepseek"

[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
api_key_env = "DASHSCOPE_API_KEY"
"#;
    let path = std::path::PathBuf::from("config.toml");
    let file = parse_config(text, &path).unwrap();
    let active = merge(
        file.clone(),
        true,
        path.clone(),
        &EnvMap::new(),
        &Overrides::default(),
    )
    .unwrap();
    assert_eq!(active.provider_name, "deepseek");
    assert_eq!(active.base_url, "https://api.deepseek.com");
    assert_eq!(active.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert_eq!(
        active
            .configured_providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>(),
        vec!["bailian", "deepseek"]
    );

    let bailian = merge(
        file.clone(),
        true,
        path.clone(),
        &EnvMap::new(),
        &Overrides {
            provider: Some("bailian".into()),
            ..Overrides::default()
        },
    )
    .unwrap();
    assert_eq!(bailian.provider_name, "bailian");
    assert_eq!(
        bailian.base_url,
        "https://dashscope.aliyuncs.com/compatible-mode/v1"
    );
    assert_eq!(bailian.model, "qwen-plus");
    assert_eq!(bailian.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));

    let mut env = EnvMap::new();
    env.set("CMDGEN_MODEL", "qwen-turbo");
    let overridden = merge(
        file,
        true,
        path,
        &env,
        &Overrides {
            provider: Some("bailian".into()),
            ..Overrides::default()
        },
    )
    .unwrap();
    assert_eq!(overridden.model, "qwen-turbo");
    assert_eq!(
        overridden.base_url,
        "https://dashscope.aliyuncs.com/compatible-mode/v1"
    );
}

#[test]
fn switching_to_bailian_uses_the_preset_instead_of_the_saved_endpoint() {
    let text = r#"
active = "deepseek"
[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"
"#;
    let path = std::path::PathBuf::from("config.toml");
    let file = parse_config(text, &path).unwrap();
    let config = merge(
        file,
        true,
        path,
        &EnvMap::new(),
        &Overrides {
            provider: Some("bailian".into()),
            ..Overrides::default()
        },
    )
    .unwrap();
    assert_eq!(config.provider_name, "bailian");
    assert_eq!(
        config.base_url,
        "https://dashscope.aliyuncs.com/compatible-mode/v1"
    );
    assert_eq!(config.model, "qwen3.8-flash");
    assert_eq!(config.api_key_env.as_deref(), Some("DASHSCOPE_API_KEY"));

    let bare = merge(
        FileConfig::default(),
        false,
        std::path::PathBuf::from("config.toml"),
        &EnvMap::new(),
        &Overrides::default(),
    )
    .unwrap();
    assert_eq!(bare.provider_name, "deepseek");
    assert_eq!(bare.model, "deepseek-flash");
}

#[test]
fn legacy_custom_provider_still_uses_openai_env() {
    let text = r#"
[provider]
name = "example"
base_url = "https://example.com/v1"
model = "custom-model"
"#;
    let path = std::path::PathBuf::from("config.toml");
    let file = parse_config(text, &path).unwrap();
    let config = merge(file, true, path, &EnvMap::new(), &Overrides::default()).unwrap();
    assert_eq!(config.provider_name, "example");
    assert_eq!(config.base_url, "https://example.com/v1");
    assert_eq!(config.api_key_env.as_deref(), Some("OPENAI_API_KEY"));

    let sandbox = common::Sandbox::new();
    let mut env = EnvMap::new();
    env.set("OPENAI_API_KEY", "openai-secret");
    let resolved = resolve_api_key(
        &config.provider_name,
        config.api_key_env.as_deref(),
        &env,
        &MemoryStore::default(),
        &credential_file(&sandbox),
    )
    .unwrap();
    assert_eq!(resolved.secret, "openai-secret");
}

#[test]
fn named_provider_does_not_inherit_openai_env() {
    let text = r#"
active = "bailian"

[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
api_key_env = "DASHSCOPE_API_KEY"
"#;
    let path = std::path::PathBuf::from("config.toml");
    let file = parse_config(text, &path).unwrap();
    let config = merge(file, true, path, &EnvMap::new(), &Overrides::default()).unwrap();
    let sandbox = common::Sandbox::new();
    let credentials = credential_file(&sandbox);
    let store = MemoryStore::default();
    store.set("bailian", "from-store").unwrap();

    let mut env = EnvMap::new();
    env.set("OPENAI_API_KEY", "openai-secret");
    env.set("DASHSCOPE_API_KEY", "dashscope-secret");
    let resolved = resolve_api_key(
        &config.provider_name,
        config.api_key_env.as_deref(),
        &env,
        &store,
        &credentials,
    )
    .unwrap();
    assert_eq!(resolved.secret, "dashscope-secret");

    let mut env = EnvMap::new();
    env.set("OPENAI_API_KEY", "openai-secret");
    let resolved = resolve_api_key(
        &config.provider_name,
        config.api_key_env.as_deref(),
        &env,
        &store,
        &credentials,
    )
    .unwrap();
    assert_eq!(resolved.secret, "from-store");
    assert!(!resolved.source_label.contains("OPENAI_API_KEY"));
}

#[test]
fn mixed_and_incomplete_provider_configs_are_rejected() {
    let path = std::path::Path::new("config.toml");
    let mixed = r#"
active = "deepseek"
[provider]
name = "deepseek"
[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"
"#;
    assert!(parse_config(mixed, path)
        .unwrap_err()
        .to_string()
        .contains("cannot use both"));

    let missing_active = r#"
[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
"#;
    assert!(parse_config(missing_active, path)
        .unwrap_err()
        .to_string()
        .contains("active"));

    let incomplete = r#"
active = "example"
[providers.example]
base_url = "https://example.com/v1"
"#;
    assert!(parse_config(incomplete, path)
        .unwrap_err()
        .to_string()
        .contains("base_url"));
}

#[test]
fn use_rewrites_active_and_keeps_comments() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = r#"# 保留这段说明
active = "deepseek"

[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"

[generation]
temperature = 0.0
"#;
    std::fs::write(&path, original).unwrap();
    let active = activate_provider(&path, "Bailian").unwrap();
    assert_eq!(active, "bailian");
    let updated = std::fs::read_to_string(&path).unwrap();
    assert!(updated.contains("# 保留这段说明"));
    assert!(updated.contains("active = \"bailian\"") || updated.contains("active = 'bailian'"));
    assert!(updated.contains("https://api.deepseek.com"));
    assert!(updated.contains("temperature = 0.0"));
    assert!(!updated.contains("active = \"deepseek\""));

    let err = activate_provider(&path, "missing").unwrap_err();
    assert!(err.to_string().contains("unknown provider"));

    let err = activate_provider(&path, "openai").unwrap_err();
    assert!(err.to_string().contains("unknown provider"));
}

#[test]
fn use_rejects_a_legacy_single_provider_file() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "[provider]\nname = \"example\"\nbase_url = \"https://example.com/v1\"\nmodel = \"custom\"\n",
    )
    .unwrap();
    let err = activate_provider(&path, "openai").unwrap_err();
    assert!(err.to_string().contains("[providers.<name>]"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("name = \"example\""));
    assert!(!text.contains("active"));
}

#[test]
fn set_updates_one_field_and_keeps_the_rest_of_the_profile() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = r#"# 保留这段说明
active = "deepseek"

[providers.Bailian]
# 百炼档案
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"
api_key_env = "DASHSCOPE_API_KEY"

[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"
"#;
    std::fs::write(&path, original).unwrap();

    let updated = update_provider(
        &path,
        Some("bailian"),
        Some("https://llm.example.com/compatible-mode/v1/"),
        None,
    )
    .unwrap();
    assert_eq!(updated.name, "bailian");
    assert_eq!(
        updated.base_url.as_deref(),
        Some("https://llm.example.com/compatible-mode/v1")
    );
    assert!(updated.model.is_none());

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# 保留这段说明"));
    assert!(text.contains("# 百炼档案"));
    assert!(text.contains("[providers.Bailian]"));
    assert!(!text.contains("[providers.bailian]"));
    assert!(text.contains("https://llm.example.com/compatible-mode/v1"));
    assert!(!text.contains("dashscope.aliyuncs.com"));
    assert!(text.contains("model = \"qwen-plus\""));
    assert!(text.contains("api_key_env = \"DASHSCOPE_API_KEY\""));
    assert!(text.contains("active = \"deepseek\""));
    assert!(text.contains("https://api.deepseek.com"));
}

#[test]
fn set_can_create_a_preset_profile_with_one_field() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "active = \"deepseek\"\n\n[providers.deepseek]\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-flash\"\n",
    )
    .unwrap();

    update_provider(
        &path,
        Some("bailian"),
        Some("https://llm.example.com/compatible-mode/v1"),
        None,
    )
    .unwrap();
    let file = parse_config(&std::fs::read_to_string(&path).unwrap(), &path).unwrap();
    let bailian = file.providers.get("bailian").unwrap();
    assert_eq!(
        bailian.base_url.as_deref(),
        Some("https://llm.example.com/compatible-mode/v1")
    );
    assert!(bailian.model.is_none());
}

#[test]
fn set_requires_both_fields_when_creating_a_custom_provider() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        "active = \"deepseek\"\n\n[providers.deepseek]\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-flash\"\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    let err = update_provider(&path, Some("example"), None, Some("custom")).unwrap_err();
    assert!(err.to_string().contains("both"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    let updated = update_provider(
        &path,
        Some("example"),
        Some("https://example.com/v1"),
        Some(" custom "),
    )
    .unwrap();
    assert_eq!(updated.name, "example");
    assert_eq!(updated.model.as_deref(), Some("custom"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[providers.example]"));
    assert!(text.contains("https://example.com/v1"));
    assert!(text.contains("model = \"custom\""));
}

#[test]
fn set_rejects_an_invalid_url_without_writing() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, DEFAULT_CONFIG_TOML).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();

    let err =
        update_provider(&path, Some("bailian"), Some("http://example.com/v1"), None).unwrap_err();
    assert!(err.to_string().contains("HTTPS"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

    let err = update_provider(&path, Some("deepseek"), None, Some("   ")).unwrap_err();
    assert!(err.to_string().contains("must not be empty"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[test]
fn set_rejects_a_legacy_single_provider_file() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original =
        "[provider]\nname = \"example\"\nbase_url = \"https://example.com/v1\"\nmodel = \"custom\"\n";
    std::fs::write(&path, original).unwrap();
    let err = update_provider(&path, Some("example"), None, Some("other")).unwrap_err();
    assert!(err.to_string().contains("[providers.<name>]"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn set_without_a_provider_uses_active_and_missing_file_is_reported() {
    let sandbox = common::Sandbox::new();
    let path = sandbox.config_dir().join("config.toml");
    let err = update_provider(&path, None, None, Some("qwen-plus")).unwrap_err();
    assert!(matches!(err, cmdgen::Error::ConfigNotFound { .. }));

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "[generation]\ntemperature = 0.0\n").unwrap();
    let err = update_provider(&path, None, Some("https://example.com/v1"), None).unwrap_err();
    assert!(err.to_string().contains("active"));

    std::fs::write(
        &path,
        "active = \"deepseek\"\n\n[providers.deepseek]\nbase_url = \"https://api.deepseek.com\"\nmodel = \"deepseek-flash\"\n",
    )
    .unwrap();
    let updated = update_provider(&path, None, None, Some("deepseek-reasoner")).unwrap();
    assert_eq!(updated.name, "deepseek");
    assert_eq!(updated.model.as_deref(), Some("deepseek-reasoner"));
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("https://api.deepseek.com"));
}
