mod en;
mod zh;

use std::cell::Cell;

/// Interface language. `En` is the built-in default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Zh => "zh",
        }
    }
}

/// Static copy for one language. Every language file fills the same fields.
#[derive(Debug, Clone, Copy)]
pub struct Messages {
    pub about: &'static str,
    pub after_long_help: &'static str,
    pub help_prompt: &'static str,
    pub help_provider: &'static str,
    pub help_model: &'static str,
    pub help_base_url: &'static str,
    pub help_shell: &'static str,
    pub help_timeout: &'static str,
    pub help_verbose: &'static str,
    pub help_lang: &'static str,
    pub help_config: &'static str,
    pub help_shell_cmd: &'static str,
    pub help_doctor: &'static str,
    pub help_config_init: &'static str,
    pub help_config_show: &'static str,
    pub help_config_providers: &'static str,
    pub help_config_use: &'static str,
    pub help_config_use_name: &'static str,
    pub help_config_lang: &'static str,
    pub help_config_set: &'static str,
    pub help_config_set_key: &'static str,
    pub help_config_delete_key: &'static str,
    pub help_delete_key_yes: &'static str,
    pub help_install: &'static str,
    pub help_uninstall: &'static str,
    pub err_missing_arg: &'static str,
    pub err_unknown_arg: &'static str,
    pub err_invalid_value: &'static str,
    pub err_supported_values: &'static str,
    pub secret_arg: &'static str,
    pub err_need_prompt: &'static str,
    pub verbose_model: &'static str,
    pub verbose_endpoint: &'static str,
    pub verbose_elapsed: &'static str,
    pub loading: &'static str,
    pub config_created: &'static str,
    pub config_created_hint: &'static str,
    pub config_exists: &'static str,
    pub switched_provider: &'static str,
    pub switched_language: &'static str,
    pub config_path_line: &'static str,
    pub err_no_config: &'static str,
    pub updated_provider: &'static str,
    pub line_base_url: &'static str,
    pub line_model: &'static str,
    pub prompt_api_key: &'static str,
    pub err_hidden_input: &'static str,
    pub saved_keyring: &'static str,
    pub removed_file_key: &'static str,
    pub saved_file: &'static str,
    pub saved_file_mode: &'static str,
    pub cancelled: &'static str,
    pub deleted_both: &'static str,
    pub deleted_keyring: &'static str,
    pub deleted_file: &'static str,
    pub installed: &'static str,
    pub install_unchanged: &'static str,
    pub line_script: &'static str,
    pub line_rc: &'static str,
    pub line_binding: &'static str,
    pub line_backup: &'static str,
    pub line_run: &'static str,
    pub line_warning: &'static str,
    pub bash_hint: &'static str,
    pub uninstalled: &'static str,
    pub uninstall_absent: &'static str,
    pub line_source: &'static str,
    pub err_confirm_delete: &'static str,
    pub err_lang_not_a_terminal: &'static str,
    pub lang_menu_title: &'static str,
    pub lang_menu_hint: &'static str,
    pub prompt_confirm_delete: &'static str,
    pub err_config_not_found: &'static str,
    pub err_invalid_config: &'static str,
    pub err_credential_store: &'static str,
    pub err_unsupported_provider: &'static str,
    pub err_invalid_base_url: &'static str,
    pub err_network: &'static str,
    pub err_timeout: &'static str,
    pub err_http: &'static str,
    pub err_api: &'static str,
    pub err_invalid_response: &'static str,
    pub err_empty_command: &'static str,
    pub err_shell: &'static str,
    pub err_io: &'static str,
    pub missing_key_steps_with_var: &'static str,
    pub missing_key_steps_without_var: &'static str,
    pub missing_key: &'static str,
    pub store_unavailable_now: &'static str,
    pub err_no_home: &'static str,
    pub err_secret_in_config: &'static str,
    pub err_provider_duplicate: &'static str,
    pub err_api_key_env: &'static str,
    pub err_profile_needs_endpoint: &'static str,
    pub err_legacy_and_providers: &'static str,
    pub err_active_unknown: &'static str,
    pub err_active_needs_providers: &'static str,
    pub err_active_unknown_preset: &'static str,
    pub err_providers_need_active: &'static str,
    pub err_legacy_only_switch: &'static str,
    pub err_unknown_provider: &'static str,
    pub err_set_needs_field: &'static str,
    pub err_legacy_only_edit: &'static str,
    pub err_no_active_for_set: &'static str,
    pub err_new_provider_needs_both: &'static str,
    pub err_model_empty: &'static str,
    pub err_profile_must_be_table: &'static str,
    pub err_providers_must_be_table: &'static str,
    pub err_key_must_be_string: &'static str,
    pub err_active_must_be_string: &'static str,
    pub err_language_must_be_string: &'static str,
    pub expect_number: &'static str,
    pub expect_integer: &'static str,
    pub err_temperature: &'static str,
    pub err_max_tokens: &'static str,
    pub err_timeout_range: &'static str,
    pub file_state_exists: &'static str,
    pub file_state_missing: &'static str,
    pub credentials_redacted: &'static str,
    pub credentials_missing: &'static str,
    pub providers_none: &'static str,
    pub show_template: &'static str,
    pub status_unconfigured: &'static str,
    pub status_store_unavailable: &'static str,
    pub status_unconfigured_detail: &'static str,
    pub providers_current: &'static str,
    pub providers_none_saved: &'static str,
    pub providers_preset_note: &'static str,
    pub err_provider_name: &'static str,
    pub err_timeout_env: &'static str,
    pub err_api_key_empty: &'static str,
    pub err_no_entry: &'static str,
    pub err_no_storage: &'static str,
    pub err_platform_failure: &'static str,
    pub err_memory_lock: &'static str,
    pub err_credentials_unreadable: &'static str,
    pub err_credentials_unwritable: &'static str,
    pub err_nothing_to_delete: &'static str,
    pub source_env: &'static str,
    pub source_keyring: &'static str,
    pub source_file: &'static str,
    pub doctor_title: &'static str,
    pub level_pass: &'static str,
    pub level_warn: &'static str,
    pub level_fail: &'static str,
    pub level_info: &'static str,
    pub result_bad: &'static str,
    pub result_ok: &'static str,
    pub check_config: &'static str,
    pub check_config_missing: &'static str,
    pub check_perms: &'static str,
    pub check_perms_detail: &'static str,
    pub check_provider: &'static str,
    pub check_provider_detail: &'static str,
    pub check_runtime: &'static str,
    pub check_runtime_detail: &'static str,
    pub check_key: &'static str,
    pub check_key_redacted: &'static str,
    pub check_store: &'static str,
    pub check_store_ok: &'static str,
    pub check_store_bad: &'static str,
    pub check_shell: &'static str,
    pub check_network: &'static str,
    pub network_skipped: &'static str,
    pub network_ok: &'static str,
    pub configured_none: &'static str,
    pub base_empty: &'static str,
    pub base_query: &'static str,
    pub base_completions: &'static str,
    pub base_https: &'static str,
    pub base_host: &'static str,
    pub resp_not_json: &'static str,
    pub resp_reasoning: &'static str,
    pub resp_missing_content: &'static str,
    pub resp_content_type: &'static str,
    pub resp_empty_error: &'static str,
    pub resp_too_large: &'static str,
    pub resp_not_utf8: &'static str,
    pub net_connect_timeout: &'static str,
    pub net_connect_failed: &'static str,
    pub net_tls: &'static str,
    pub err_shell_name: &'static str,
    pub err_keybinding: &'static str,
    pub err_source_newline: &'static str,
    pub state_installed: &'static str,
    pub state_mark_no_script: &'static str,
    pub state_script_no_mark: &'static str,
    pub state_absent: &'static str,
    pub err_marker_incomplete: &'static str,
    pub err_template_incomplete: &'static str,
    pub err_cannot_read: &'static str,
    pub binding_warning: &'static str,
    pub err_explanation: &'static str,
}

thread_local! {
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::En) };
    static EXPLICIT: Cell<bool> = const { Cell::new(false) };
}

pub fn messages(lang: Lang) -> &'static Messages {
    match lang {
        Lang::En => &en::MESSAGES,
        Lang::Zh => &zh::MESSAGES,
    }
}

pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

pub fn t() -> &'static Messages {
    messages(current())
}

/// Drop any language chosen earlier on this thread and return to `en`.
pub fn reset_language() {
    CURRENT.set(Lang::En);
    EXPLICIT.set(false);
}

pub fn set_current(lang: Lang) {
    CURRENT.set(lang);
}

/// `--lang` or `CMDGEN_LANG`. Config must not override this.
pub fn set_explicit(lang: Lang) {
    CURRENT.set(lang);
    EXPLICIT.set(true);
}

pub fn is_explicit() -> bool {
    EXPLICIT.with(Cell::get)
}

/// Apply a language read from config unless a flag or environment variable won.
pub fn apply_config_language(lang: Lang) {
    if !is_explicit() {
        set_current(lang);
    }
}

/// `en` and `en-US` are English. `zh`, `zh-CN`, and `zh-Hans` are Chinese.
/// Unknown codes return `None`; callers build their own message.
pub fn parse_lang_code(raw: &str) -> Option<Lang> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" => Some(Lang::En),
        "zh" | "zh-cn" | "zh-hans" => Some(Lang::Zh),
        _ => None,
    }
}

/// Unknown codes are always reported in English, with the supported codes listed.
pub fn unknown_language_message(code: &str) -> String {
    format!(
        "unknown language code: {code}\nsupported: en, zh\nen-US is treated as en; zh-CN and zh-Hans are treated as zh"
    )
}

/// Replace `{name}` placeholders. Keys are matched exactly, including the braces.
pub fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in pairs {
        let needle = format!("{{{key}}}");
        out = out.replace(&needle, value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_map_onto_en_and_zh() {
        assert_eq!(parse_lang_code("en"), Some(Lang::En));
        assert_eq!(parse_lang_code("en-US"), Some(Lang::En));
        assert_eq!(parse_lang_code("EN-us"), Some(Lang::En));
        assert_eq!(parse_lang_code("zh"), Some(Lang::Zh));
        assert_eq!(parse_lang_code("zh-CN"), Some(Lang::Zh));
        assert_eq!(parse_lang_code("zh-Hans"), Some(Lang::Zh));
        assert_eq!(parse_lang_code("fr"), None);
        assert_eq!(parse_lang_code("zh-Hant"), None);
    }

    #[test]
    fn fill_replaces_named_placeholders() {
        let text = fill(
            "file {path} ({state})",
            &[("path", "/tmp/a"), ("state", "ok")],
        );
        assert_eq!(text, "file /tmp/a (ok)");
    }

    #[test]
    fn explicit_language_blocks_config() {
        reset_language();
        set_explicit(Lang::En);
        apply_config_language(Lang::Zh);
        assert_eq!(current(), Lang::En);
        reset_language();
        apply_config_language(Lang::Zh);
        assert_eq!(current(), Lang::Zh);
        reset_language();
    }

    #[test]
    fn unknown_language_message_lists_codes_in_english() {
        let text = unknown_language_message("fr");
        assert!(text.contains("fr"));
        assert!(text.contains("en"));
        assert!(text.contains("zh"));
        assert!(!text
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)));
    }
}
