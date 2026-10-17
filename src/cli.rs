use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};

use crate::config::{
    config_dir, delete_api_key, home_dir, init_config, key_status_from, load_effective,
    resolve_api_key, save_api_key, CredentialFile, EnvMap, InitOutcome, KeyringStore, Overrides,
    SavedKey,
};
use crate::doctor;
use crate::error::Error;
use crate::generator::client::{
    build_request_body, complete, completions_endpoint, CompletionRequest, RequestOptions,
};
use crate::generator::output::normalize_command;
use crate::generator::prompt::{system_prompt, user_message};
use crate::shell::detect::{os_label, resolve_shell};
use crate::shell::integration::{install, uninstall, IntegrationShell};

const SECRET_ARG_ERROR: &str = "\
config set-key 不接受命令行参数，以免 API Key 写入 Shell 历史。\
请运行 `cmdgen config set-key`，再在隐藏提示中输入。";

#[derive(Debug, Parser)]
#[command(
    name = "cmdgen",
    version,
    about = "cmdgen — AI-powered shell command generation for your terminal.",
    after_long_help = "\
cmdgen 只把命令打印到标准输出，或在 Shell 集成中填回输入行。它不会执行生成的命令。

选项需要写在描述之前。描述以 - 开头时，请使用: cmdgen -- \"-n 列出文件\"
要让 config、doctor、shell 这类词作为描述，请使用: cmdgen -- shell",
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// 用自然语言描述你想执行的操作
    #[arg(
        value_name = "PROMPT",
        required = true,
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    prompt: Vec<String>,

    /// 覆盖 provider 名称
    #[arg(long, global = true)]
    provider: Option<String>,

    /// 覆盖模型 ID
    #[arg(long, global = true)]
    model: Option<String>,

    /// 覆盖 API 前缀，不要包含 /chat/completions
    #[arg(long, global = true, value_name = "URL")]
    base_url: Option<String>,

    /// 覆盖目标 shell，例如 zsh 或 bash
    #[arg(long, global = true, id = "shell_override")]
    shell: Option<String>,

    /// 覆盖请求超时（秒）
    #[arg(long, global = true, value_name = "SECS")]
    timeout: Option<u64>,

    /// 向 stderr 输出模型、接口和耗时，不包含 API Key 或完整 prompt
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 管理配置文件与 API Key
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// 安装或卸载 Shell 输入行集成
    Shell {
        #[command(subcommand)]
        action: IntegrationCommand,
    },
    /// 检查配置、凭据来源、网络和集成状态
    Doctor,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// 创建默认配置文件
    Init,
    /// 显示有效配置（API Key 脱敏）
    Show,
    /// 隐藏输入 API Key。优先保存到系统凭据存储；不可用时写入本地凭据文件
    SetKey,
    /// 删除当前 provider 在系统凭据存储和本地凭据文件中的 API Key
    DeleteKey {
        /// 跳过确认提示
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
enum IntegrationCommand {
    /// 安装输入行集成
    Install {
        #[arg(value_enum)]
        shell: ShellArg,
    },
    /// 卸载集成；省略 shell 时同时卸载 zsh 和 bash
    Uninstall {
        #[arg(value_enum)]
        shell: Option<ShellArg>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ShellArg {
    Zsh,
    Bash,
}

impl From<ShellArg> for IntegrationShell {
    fn from(shell: ShellArg) -> Self {
        match shell {
            ShellArg::Zsh => IntegrationShell::Zsh,
            ShellArg::Bash => IntegrationShell::Bash,
        }
    }
}

pub fn dispatch<I, T>(args: I) -> Result<i32, Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let raw: Vec<OsString> = args.into_iter().map(Into::into).collect();
    reject_inline_secret(&raw)?;
    match Cli::try_parse_from(raw) {
        Ok(cli) => execute(cli),
        Err(err) => {
            let code = err.exit_code();
            let _ = err.print();
            Ok(code)
        }
    }
}

fn execute(cli: Cli) -> Result<i32, Error> {
    let Cli {
        command,
        prompt,
        provider,
        model,
        base_url,
        shell,
        timeout,
        verbose,
    } = cli;
    let overrides = Overrides {
        provider,
        base_url,
        model,
        shell,
        timeout_secs: timeout,
    };
    match command {
        Some(Command::Config { action }) => config_command(&overrides, action),
        Some(Command::Shell { action }) => integration_command(&overrides, action),
        Some(Command::Doctor) => doctor_command(&overrides),
        None => generate(&prompt, &overrides, verbose),
    }
}

fn generate(prompt: &[String], overrides: &Overrides, verbose: bool) -> Result<i32, Error> {
    let prompt = prompt.join(" ");
    if prompt.trim().is_empty() {
        return Err(Error::Usage(
            "请提供自然语言描述。示例: cmdgen \"列出所有 docker 容器\"".into(),
        ));
    }

    let env = EnvMap::from_process();
    let config = load_effective(&env, overrides)?;
    let shell = resolve_shell(
        overrides.shell.as_deref(),
        &config.shell_default,
        env.get("CMDGEN_SHELL"),
        env.get("SHELL"),
    )?;
    let endpoint = completions_endpoint(&config.base_url)?;
    let key = resolve_api_key(
        &config.provider_name,
        &env,
        &KeyringStore,
        &CredentialFile::from_env(&env)?,
    )?;
    if verbose {
        eprintln!("模型: {}", config.model);
        eprintln!("接口: {endpoint}");
    }

    let cwd = if config.include_cwd {
        std::env::current_dir().ok()
    } else {
        None
    };
    let system = system_prompt(os_label(), &shell);
    let user = user_message(&prompt, cwd.as_deref());
    let body = build_request_body(&RequestOptions {
        model: &config.model,
        temperature: config.temperature,
        max_tokens: config.max_tokens,
        system: &system,
        user: &user,
        disable_thinking: config.provider_name == "deepseek",
    });
    let started = Instant::now();
    let result = complete(&CompletionRequest {
        endpoint: &endpoint,
        api_key: &key.secret,
        body: &body,
        timeout_secs: config.timeout_secs,
    });
    if verbose {
        eprintln!("耗时: {} ms", started.elapsed().as_millis());
    }
    let command = normalize_command(&result?)?;
    println!("{command}");
    Ok(0)
}

fn config_command(overrides: &Overrides, action: ConfigCommand) -> Result<i32, Error> {
    let env = EnvMap::from_process();
    match action {
        ConfigCommand::Init => {
            let path = crate::config::config_path(&env)?;
            match init_config(&path)? {
                InitOutcome::Created(path) => {
                    println!("已创建配置文件: {}", path.display());
                    println!(
                        "API Key 不会写入该文件。请运行 `cmdgen config set-key` 或设置环境变量。"
                    );
                }
                InitOutcome::AlreadyExists(path) => {
                    println!("配置文件已存在，未修改: {}", path.display());
                }
            }
            Ok(0)
        }
        ConfigCommand::Show => {
            let config = load_effective(&env, overrides)?;
            let shell = resolve_shell(
                overrides.shell.as_deref(),
                &config.shell_default,
                env.get("CMDGEN_SHELL"),
                env.get("SHELL"),
            )?;
            let resolved = resolve_api_key(
                &config.provider_name,
                &env,
                &KeyringStore,
                &CredentialFile::from_env(&env)?,
            );
            let status = key_status_from(resolved.as_ref());
            print!("{}", crate::config::render_show(&config, &shell, &status));
            Ok(0)
        }
        ConfigCommand::SetKey => {
            let config = load_effective(&env, overrides)?;
            let secret = rpassword::prompt_password("API Key（输入不可见）: ").map_err(|_| {
                Error::Usage(
                    "无法隐藏读取 API Key。请在交互式终端中运行 `cmdgen config set-key`。".into(),
                )
            })?;
            let file = CredentialFile::from_env(&env)?;
            match save_api_key(&config.provider_name, &secret, &KeyringStore, &file)? {
                SavedKey::Keyring { removed_file_entry } => {
                    println!(
                        "已保存到系统凭据存储（服务 cmdgen，账号 {}）。未写入配置文件。",
                        config.provider_name
                    );
                    if removed_file_entry {
                        println!("已删除本地凭据文件中该 provider 的旧 Key。");
                    }
                }
                SavedKey::File { path } => {
                    println!(
                        "系统凭据存储不可用，已保存到本地凭据文件: {}",
                        path.display()
                    );
                    println!(
                        "文件权限为 0600。同一用户下的其他进程可以读取该文件。未写入 config.toml。"
                    );
                }
            }
            Ok(0)
        }
        ConfigCommand::DeleteKey { yes } => {
            let config = load_effective(&env, overrides)?;
            if !yes && !confirm_delete()? {
                println!("已取消，未删除。");
                return Ok(0);
            }
            let file = CredentialFile::from_env(&env)?;
            let deleted = delete_api_key(&config.provider_name, &KeyringStore, &file)?;
            match (deleted.keyring, deleted.file) {
                (true, true) => println!(
                    "已删除系统凭据存储和本地凭据文件中的 API Key（账号 {}）。",
                    config.provider_name
                ),
                (true, false) => println!(
                    "已删除系统凭据存储中的 API Key（服务 cmdgen，账号 {}）。",
                    config.provider_name
                ),
                (false, true) => {
                    println!("已删除本地凭据文件中的 API Key: {}", file.path().display())
                }
                (false, false) => unreachable!("任一侧未删除时 delete_api_key 会返回错误"),
            }
            Ok(0)
        }
    }
}

fn integration_command(overrides: &Overrides, action: IntegrationCommand) -> Result<i32, Error> {
    let env = EnvMap::from_process();
    let config = load_effective(&env, overrides)?;
    let home = home_dir(&env)?;
    let dir = config_dir(&env)?;
    match action {
        IntegrationCommand::Install { shell } => {
            let shell = IntegrationShell::from(shell);
            let report = install(shell, &home, &dir, &config.keybinding)?;
            if report.changed {
                println!("已安装 {} 集成。", display_shell(shell));
            } else {
                println!("{} 集成已存在，未重复修改。", display_shell(shell));
            }
            println!("脚本: {}", report.script_path.display());
            println!("配置: {}", report.rc_path.display());
            println!("快捷键: {}", report.display_binding);
            if report.created_backup {
                println!(
                    "备份: {}",
                    report
                        .rc_path
                        .with_file_name(format!(
                            "{}.cmdgen.bak",
                            report
                                .rc_path
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "rc".into())
                        ))
                        .display()
                );
            }
            println!("请运行: {}", report.reload);
            if let Some(warning) = report.binding_warning {
                eprintln!("警告: {warning}");
            }
            if shell == IntegrationShell::Bash {
                eprintln!("提示: Bash 输入行集成需要 Bash 4 或更高版本。macOS 自带的 Bash 3.2 不支持 bind -x。");
            }
            Ok(0)
        }
        IntegrationCommand::Uninstall { shell } => {
            let shells: Vec<IntegrationShell> =
                shell.into_iter().map(IntegrationShell::from).collect();
            let reports = uninstall(&shells, &home, &dir)?;
            for report in reports {
                if report.removed_block || report.removed_script {
                    println!("已卸载 {} 集成。", display_shell(report.shell));
                } else {
                    println!("未发现 {} 集成，无需卸载。", display_shell(report.shell));
                }
                if report.removed_block {
                    println!(
                        "请运行: source ~/{}",
                        report
                            .rc_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    );
                }
            }
            Ok(0)
        }
    }
}

fn doctor_command(overrides: &Overrides) -> Result<i32, Error> {
    let env = EnvMap::from_process();
    let report = doctor::diagnose(&env, overrides, &KeyringStore, true)?;
    print!("{}", report.render());
    Ok(if report.has_failure() { 1 } else { 0 })
}

fn display_shell(shell: IntegrationShell) -> &'static str {
    match shell {
        IntegrationShell::Zsh => "Zsh",
        IntegrationShell::Bash => "Bash",
    }
}

fn confirm_delete() -> Result<bool, Error> {
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(
            "删除凭据需要确认。请在终端中运行，或加上 --yes。".into(),
        ));
    }
    eprint!("将删除当前 provider 在系统凭据存储和本地凭据文件中的 API Key。输入 yes 确认: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|err| Error::io("stdin", err))?;
    Ok(line.trim() == "yes")
}

fn reject_inline_secret(args: &[OsString]) -> Result<(), Error> {
    const VALUE_FLAGS: &[&str] = &[
        "--provider",
        "--model",
        "--base-url",
        "--shell",
        "--timeout",
    ];
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let text = args[index].to_string_lossy();
        if text == "--" {
            if positionals.last().map(String::as_str) == Some("set-key")
                && positionals.iter().rev().nth(1).map(String::as_str) == Some("config")
            {
                return Err(Error::Usage(SECRET_ARG_ERROR.into()));
            }
            break;
        }
        if let Some(flag) = VALUE_FLAGS.iter().find(|flag| text == **flag) {
            let _ = flag;
            index += 2;
            continue;
        }
        if VALUE_FLAGS
            .iter()
            .any(|flag| text.starts_with(&format!("{flag}=")))
            || text.starts_with('-')
        {
            index += 1;
            continue;
        }
        positionals.push(text.into_owned());
        if positionals.len() >= 3
            && positionals[positionals.len() - 3] == "config"
            && positionals[positionals.len() - 2] == "set-key"
        {
            return Err(Error::Usage(SECRET_ARG_ERROR.into()));
        }
        index += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_after_double_dash_is_not_a_subcommand() {
        let cli = Cli::try_parse_from(["cmdgen", "--", "config"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.prompt, vec!["config".to_string()]);
    }

    #[test]
    fn config_show_is_a_subcommand() {
        let cli = Cli::try_parse_from(["cmdgen", "config", "show"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Config {
                action: ConfigCommand::Show
            })
        ));
    }

    #[test]
    fn doctor_is_a_subcommand() {
        let cli = Cli::try_parse_from(["cmdgen", "doctor"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Doctor)));
    }

    #[test]
    fn flags_before_prompt_are_overrides() {
        let cli = Cli::try_parse_from([
            "cmdgen",
            "--shell",
            "bash",
            "--model",
            "deepseek-flash",
            "列出",
            "容器",
        ])
        .unwrap();
        assert_eq!(cli.shell.as_deref(), Some("bash"));
        assert_eq!(cli.model.as_deref(), Some("deepseek-flash"));
        assert_eq!(cli.prompt, vec!["列出".to_string(), "容器".to_string()]);
    }

    #[test]
    fn set_key_rejects_positional_secret_without_echoing_it() {
        let err = reject_inline_secret(&[
            "cmdgen".into(),
            "config".into(),
            "set-key".into(),
            "SENTINEL_SECRET_VALUE".into(),
        ])
        .unwrap_err();
        let text = err.to_string();
        assert!(!text.contains("SENTINEL_SECRET_VALUE"));
        assert!(text.contains("set-key"));
    }
}
