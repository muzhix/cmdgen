use crate::config::{
    config_dir, home_dir, load_effective, resolve_api_key, CredentialFile, EnvMap, Overrides,
    SecretStore,
};
use crate::error::Error;
use crate::fsutil;
use crate::generator::client::{completions_endpoint, probe_reachable};
use crate::i18n::{self, fill};
use crate::shell::detect::{os_label, resolve_shell};
use crate::shell::integration::{integration_is_broken, integration_state, IntegrationShell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub level: Level,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    pub fn has_failure(&self) -> bool {
        self.checks.iter().any(|check| check.level == Level::Fail)
    }

    pub fn render(&self) -> String {
        let m = i18n::t();
        let mut out = String::from(m.doctor_title);
        for check in &self.checks {
            out.push_str(match check.level {
                Level::Pass => m.level_pass,
                Level::Warn => m.level_warn,
                Level::Fail => m.level_fail,
                Level::Info => m.level_info,
            });
            out.push_str(&check.name);
            out.push('\n');
            for line in check.detail.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out.push_str(if self.has_failure() {
            m.result_bad
        } else {
            m.result_ok
        });
        out
    }
}

pub fn diagnose(
    env: &EnvMap,
    overrides: &Overrides,
    store: &dyn SecretStore,
    probe_network: bool,
) -> Result<Report, Error> {
    let mut env = env.clone();
    let config = load_effective(&env, overrides)?;
    if let Some(var) = config.api_key_env.clone() {
        env.include_process_var(&var);
    }
    let shell = resolve_shell(
        overrides.shell.as_deref(),
        &config.shell_default,
        env.get("CMDGEN_SHELL"),
        env.get("SHELL"),
    )?;
    let mut checks = Vec::new();

    let m = i18n::t();
    checks.push(if config.config_exists {
        Check {
            level: Level::Pass,
            name: m.check_config.into(),
            detail: config.config_path.display().to_string(),
        }
    } else {
        Check {
            level: Level::Warn,
            name: m.check_config.into(),
            detail: fill(
                m.check_config_missing,
                &[("path", &config.config_path.display().to_string())],
            ),
        }
    });

    if config.config_exists {
        if let Ok(mode) = fsutil::mode_of(&config.config_path) {
            if mode & 0o077 != 0 {
                checks.push(Check {
                    level: Level::Warn,
                    name: i18n::t().check_perms.into(),
                    detail: fill(
                        i18n::t().check_perms_detail,
                        &[("mode", &format!("{mode:o}"))],
                    ),
                });
            }
        }
    }

    let m = i18n::t();
    checks.push(Check {
        level: Level::Pass,
        name: m.check_provider.into(),
        detail: fill(
            m.check_provider_detail,
            &[
                ("name", &config.provider_name),
                ("model", &config.model),
                ("base_url", &config.base_url),
                ("configured", &configured_names(&config)),
            ],
        ),
    });

    checks.push(Check {
        level: Level::Info,
        name: m.check_runtime.into(),
        detail: fill(
            m.check_runtime_detail,
            &[("os", os_label()), ("shell", &shell)],
        ),
    });

    let credentials = CredentialFile::from_env(&env)?;
    match resolve_api_key(
        &config.provider_name,
        config.api_key_env.as_deref(),
        &env,
        store,
        &credentials,
    ) {
        Ok(key) => checks.push(Check {
            level: Level::Pass,
            name: i18n::t().check_key.into(),
            detail: fill(
                i18n::t().check_key_redacted,
                &[("source", &key.source_label)],
            ),
        }),
        Err(err) => checks.push(Check {
            level: Level::Fail,
            name: i18n::t().check_key.into(),
            detail: err.to_string(),
        }),
    }

    match crate::config::secrets::keyring_status() {
        Ok(()) => checks.push(Check {
            level: Level::Pass,
            name: i18n::t().check_store.into(),
            detail: i18n::t().check_store_ok.into(),
        }),
        Err(reason) => checks.push(Check {
            level: Level::Warn,
            name: i18n::t().check_store.into(),
            detail: fill(i18n::t().check_store_bad, &[("reason", &reason)]),
        }),
    }

    checks.push(network_check(
        &config.base_url,
        config.timeout_secs,
        probe_network,
    ));

    let home = home_dir(&env)?;
    let dir = config_dir(&env)?;
    let mut integration_lines = Vec::new();
    let mut broken = false;
    for shell in [IntegrationShell::Zsh, IntegrationShell::Bash] {
        if integration_is_broken(&home, &dir, shell) {
            broken = true;
        }
        integration_lines.push(integration_state(&home, &dir, shell));
    }
    checks.push(Check {
        level: if broken { Level::Fail } else { Level::Info },
        name: i18n::t().check_shell.into(),
        detail: integration_lines.join("\n"),
    });

    Ok(Report { checks })
}

fn configured_names(config: &crate::config::EffectiveConfig) -> String {
    if config.configured_providers.is_empty() {
        i18n::t().configured_none.to_string()
    } else {
        config
            .configured_providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn network_check(base_url: &str, timeout_secs: u64, probe_network: bool) -> Check {
    if !probe_network {
        return Check {
            level: Level::Info,
            name: i18n::t().check_network.into(),
            detail: i18n::t().network_skipped.into(),
        };
    }
    if let Err(err) = completions_endpoint(base_url) {
        return Check {
            level: Level::Fail,
            name: i18n::t().check_network.into(),
            detail: err.to_string(),
        };
    }
    let timeout = timeout_secs.clamp(3, 10);
    match probe_reachable(base_url, timeout) {
        Ok(status) => Check {
            level: Level::Pass,
            name: i18n::t().check_network.into(),
            detail: fill(
                i18n::t().network_ok,
                &[("base_url", base_url), ("status", &status.to_string())],
            ),
        },
        Err(err) => Check {
            level: Level::Fail,
            name: i18n::t().check_network.into(),
            detail: err.to_string(),
        },
    }
}
