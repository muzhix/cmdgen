use crate::config::{
    config_dir, home_dir, load_effective, resolve_api_key, EnvMap, Overrides, SecretStore,
};
use crate::error::Error;
use crate::fsutil;
use crate::generator::client::{completions_endpoint, probe_reachable};
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
        let mut out = String::from("cmdgen doctor\n\n");
        for check in &self.checks {
            out.push_str(match check.level {
                Level::Pass => "[通过] ",
                Level::Warn => "[警告] ",
                Level::Fail => "[失败] ",
                Level::Info => "[信息] ",
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
            "\n结果: 存在需要处理的问题\n"
        } else {
            "\n结果: 可用\n"
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
    let config = load_effective(env, overrides)?;
    let shell = resolve_shell(
        overrides.shell.as_deref(),
        &config.shell_default,
        env.get("CMDGEN_SHELL"),
        env.get("SHELL"),
    )?;
    let mut checks = Vec::new();

    checks.push(if config.config_exists {
        Check {
            level: Level::Pass,
            name: "配置文件".into(),
            detail: config.config_path.display().to_string(),
        }
    } else {
        Check {
            level: Level::Warn,
            name: "配置文件".into(),
            detail: format!("{} 不存在，使用内置默认值。", config.config_path.display()),
        }
    });

    if config.config_exists {
        if let Ok(mode) = fsutil::mode_of(&config.config_path) {
            if mode & 0o077 != 0 {
                checks.push(Check {
                    level: Level::Warn,
                    name: "配置文件权限".into(),
                    detail: format!("当前权限为 {mode:o}，建议设为 600。"),
                });
            }
        }
    }

    checks.push(Check {
        level: Level::Pass,
        name: "Provider".into(),
        detail: format!(
            "name={} model={}\nbase_url={}",
            config.provider_name, config.model, config.base_url
        ),
    });

    checks.push(Check {
        level: Level::Info,
        name: "运行环境".into(),
        detail: format!("os={} shell={shell}", os_label()),
    });

    match resolve_api_key(&config.provider_name, env, store) {
        Ok(key) => checks.push(Check {
            level: Level::Pass,
            name: "API Key".into(),
            detail: format!("{}（已脱敏，不显示内容）", key.source_label),
        }),
        Err(err) => checks.push(Check {
            level: Level::Fail,
            name: "API Key".into(),
            detail: err.to_string(),
        }),
    }

    match crate::config::secrets::keyring_status() {
        Ok(()) => checks.push(Check {
            level: Level::Pass,
            name: "凭据存储".into(),
            detail: "可以查询系统凭据存储。未显示任何 Key。".into(),
        }),
        Err(reason) => checks.push(Check {
            level: Level::Warn,
            name: "凭据存储".into(),
            detail: format!("当前不可用: {reason}\n仍可使用环境变量 CMDGEN_API_KEY。"),
        }),
    }

    checks.push(network_check(
        &config.base_url,
        config.timeout_secs,
        probe_network,
    ));

    let home = home_dir(env)?;
    let dir = config_dir(env)?;
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
        name: "Shell 集成".into(),
        detail: integration_lines.join("\n"),
    });

    Ok(Report { checks })
}

fn network_check(base_url: &str, timeout_secs: u64, probe_network: bool) -> Check {
    if !probe_network {
        return Check {
            level: Level::Info,
            name: "网络".into(),
            detail: "已跳过连通性检查。".into(),
        };
    }
    if let Err(err) = completions_endpoint(base_url) {
        return Check {
            level: Level::Fail,
            name: "网络".into(),
            detail: err.to_string(),
        };
    }
    let timeout = timeout_secs.clamp(3, 10);
    match probe_reachable(base_url, timeout) {
        Ok(status) => Check {
            level: Level::Pass,
            name: "网络".into(),
            detail: format!("可以访问 {base_url}（HTTP {status}）。未发送 API Key。"),
        },
        Err(err) => Check {
            level: Level::Fail,
            name: "网络".into(),
            detail: err.to_string(),
        },
    }
}
