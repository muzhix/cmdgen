use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use crossterm::cursor::{Hide, Show};
use crossterm::event::KeyCode;
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::config::{
    activate_provider, config_dir, config_path, create_provider, delete_api_key, home_dir,
    init_config, key_status_from, load_config, load_effective, normalize_provider,
    provider_file_has_content, render_providers, resolve_api_key, save_api_key, set_language,
    update_provider, CredentialFile, EnvMap, FileConfig, InitOutcome, KeyringStore, Overrides,
    SavedKey, BAILIAN_BASE_URL, BAILIAN_MODEL, DEEPSEEK_BASE_URL, DEEPSEEK_MODEL,
};
use crate::doctor;
use crate::error::Error;
use crate::generator::client::{
    build_request_body, complete, completions_endpoint, CompletionRequest, RequestOptions,
};
use crate::generator::output::normalize_command;
use crate::generator::prompt::{system_prompt, user_message};
use crate::i18n::{self, fill, Lang};
use crate::shell::detect::{os_label, resolve_shell};
use crate::shell::integration::{install, uninstall, IntegrationShell};

#[derive(Debug, Parser)]
#[command(
    name = "cmdgen",
    version,
    about = "cmdgen — AI-powered shell command generation for your terminal.",
    after_long_help = "\
cmdgen prints the command to stdout, or inserts it into the shell input line. It does not run the generated command.

Put options before the description. If the description starts with -, use: cmdgen -- \"-n list files\"
To use words such as config, doctor, or shell as the description, use: cmdgen -- shell",
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Natural-language description of what you want to run
    #[arg(
        value_name = "PROMPT",
        required = true,
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    prompt: Vec<String>,

    /// Override the provider name
    #[arg(long, global = true)]
    provider: Option<String>,

    /// Override the model ID
    #[arg(long, global = true)]
    model: Option<String>,

    /// Override the API prefix. Do not include /chat/completions
    #[arg(long, global = true, value_name = "URL")]
    base_url: Option<String>,

    /// Override the target shell, for example zsh or bash
    #[arg(long, global = true, id = "shell_override")]
    shell: Option<String>,

    /// Override the request timeout (seconds)
    #[arg(long, global = true, value_name = "SECS")]
    timeout: Option<u64>,

    /// Print the model, endpoint, and elapsed time to stderr. Does not include the API key or the full prompt
    #[arg(long, global = true)]
    verbose: bool,

    /// Interface language for this run: en or zh. Does not write the config file
    #[arg(long, global = true, value_name = "CODE")]
    lang: Option<String>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage the config file and API key
    Config {
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Install or uninstall shell input-line integration
    Shell {
        #[command(subcommand)]
        action: IntegrationCommand,
    },
    /// Check config, credential source, network, and integration status
    Doctor,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Create the default config file
    Init,
    /// Show the effective config (API key redacted)
    Show,
    /// List configured providers and mark the active one
    Providers,
    /// Switch the active provider and write it to active in the config file
    Use {
        /// Configured provider name, or the deepseek / bailian preset
        name: String,
    },
    /// Choose the interface language with the arrow keys and write it to the config file
    Lang,
    /// Edit one provider's model, base URL, or API key, or add a provider. `config providers` only lists them
    Provider,
}

#[derive(Debug, Subcommand)]
enum IntegrationCommand {
    /// Install input-line integration
    Install {
        #[arg(value_enum)]
        shell: ShellArg,
    },
    /// Uninstall integration. With no shell, uninstall both zsh and bash
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
    prepare_language(&raw)?;
    reject_inline_secret(&raw)?;
    let cmd = localized_command();
    let matches = match cmd.try_get_matches_from(raw) {
        Ok(matches) => matches,
        Err(err) => return Ok(handle_clap_error(err)),
    };
    match Cli::from_arg_matches(&matches) {
        Ok(cli) => execute(cli),
        Err(err) => Ok(handle_clap_error(err)),
    }
}

fn prepare_language(args: &[OsString]) -> Result<(), Error> {
    i18n::reset_language();
    if let Some(code) = flag_value(args, "--lang") {
        let lang = i18n::parse_lang_code(&code)
            .ok_or_else(|| Error::Usage(i18n::unknown_language_message(&code)))?;
        i18n::set_explicit(lang);
        return Ok(());
    }
    if let Ok(code) = std::env::var("CMDGEN_LANG") {
        let code = code.trim();
        if !code.is_empty() {
            let lang = i18n::parse_lang_code(code)
                .ok_or_else(|| Error::Usage(i18n::unknown_language_message(code)))?;
            i18n::set_explicit(lang);
            return Ok(());
        }
    }
    if requests_help(args) {
        let env = EnvMap::from_process();
        if let Some(lang) = crate::config::peek_language(&env) {
            i18n::set_current(lang);
        }
    }
    Ok(())
}

fn requests_help(args: &[OsString]) -> bool {
    for arg in args.iter().skip(1) {
        let text = arg.to_string_lossy();
        if text == "--" {
            break;
        }
        if text == "--help" || text == "-h" {
            return true;
        }
    }
    false
}

/// Last `--lang` / `--lang=CODE` before `--`. `None` when the flag is absent or has no value.
fn flag_value(args: &[OsString], flag: &str) -> Option<String> {
    let mut found = None;
    let mut index = 1;
    let with_eq = format!("{flag}=");
    while index < args.len() {
        let text = args[index].to_string_lossy();
        if text == "--" {
            break;
        }
        if let Some(value) = text.strip_prefix(&with_eq) {
            found = Some(value.to_string());
            index += 1;
            continue;
        }
        if text == flag {
            let next = args.get(index + 1).map(|value| value.to_string_lossy());
            if next.as_deref().is_some_and(|value| value != "--") {
                found = next.map(|value| value.into_owned());
                index += 2;
                continue;
            }
            return None;
        }
        index += 1;
    }
    found
}

fn localized_command() -> clap::Command {
    let m = i18n::t();
    let mut cmd = Cli::command()
        .about(m.about)
        .after_long_help(m.after_long_help);
    cmd = set_arg_help(cmd, "prompt", m.help_prompt);
    cmd = set_arg_help(cmd, "provider", m.help_provider);
    cmd = set_arg_help(cmd, "model", m.help_model);
    cmd = set_arg_help(cmd, "base_url", m.help_base_url);
    cmd = set_arg_help(cmd, "shell_override", m.help_shell);
    cmd = set_arg_help(cmd, "timeout", m.help_timeout);
    cmd = set_arg_help(cmd, "verbose", m.help_verbose);
    cmd = set_arg_help(cmd, "lang", m.help_lang);
    cmd = cmd.mut_subcommand("config", |config| {
        let config = config.about(m.help_config);
        let config = set_about(config, "init", m.help_config_init);
        let config = set_about(config, "show", m.help_config_show);
        let config = set_about(config, "providers", m.help_config_providers);
        let config = set_about(config, "use", m.help_config_use);
        let config = set_about(config, "lang", m.help_config_lang);
        let config = set_about(config, "provider", m.help_config_provider);
        config.mut_subcommand("use", |use_cmd| {
            set_arg_help(use_cmd, "name", m.help_config_use_name)
        })
    });
    cmd.mut_subcommand("shell", |shell| {
        let shell = shell.about(m.help_shell_cmd);
        let shell = set_about(shell, "install", m.help_install);
        set_about(shell, "uninstall", m.help_uninstall)
    })
    .mut_subcommand("doctor", |doctor| doctor.about(m.help_doctor))
}

fn set_arg_help(cmd: clap::Command, id: &str, help: &'static str) -> clap::Command {
    if cmd.get_arguments().any(|arg| arg.get_id().as_str() == id) {
        cmd.mut_arg(id, |arg| arg.help(help))
    } else {
        cmd
    }
}

fn set_about(cmd: clap::Command, name: &str, about: &'static str) -> clap::Command {
    if cmd.find_subcommand(name).is_none() {
        return cmd;
    }
    cmd.mut_subcommand(name, |sub| sub.about(about))
}

fn handle_clap_error(err: clap::Error) -> i32 {
    use clap::error::ErrorKind;
    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = err.print();
            err.exit_code()
        }
        ErrorKind::MissingRequiredArgument
        | ErrorKind::MissingSubcommand
        | ErrorKind::UnknownArgument
        | ErrorKind::InvalidValue
        | ErrorKind::InvalidSubcommand
        | ErrorKind::ValueValidation => {
            eprintln!("cmdgen: {}", localize_clap_error(&err));
            err.exit_code()
        }
        _ => {
            let _ = err.print();
            err.exit_code()
        }
    }
}

fn localize_clap_error(err: &clap::Error) -> String {
    use clap::error::{ContextKind, ErrorKind};
    let m = i18n::t();
    let arg = context_text(err, ContextKind::InvalidArg)
        .or_else(|| context_text(err, ContextKind::InvalidSubcommand))
        .unwrap_or_else(|| "argument".into());
    let head = match err.kind() {
        ErrorKind::MissingRequiredArgument | ErrorKind::MissingSubcommand => {
            fill(m.err_missing_arg, &[("arg", &arg)])
        }
        ErrorKind::UnknownArgument => fill(m.err_unknown_arg, &[("arg", &arg)]),
        ErrorKind::InvalidValue | ErrorKind::InvalidSubcommand | ErrorKind::ValueValidation => {
            let value = context_text(err, ContextKind::InvalidValue).unwrap_or_default();
            let mut text = fill(m.err_invalid_value, &[("value", &value), ("arg", &arg)]);
            if let Some(values) = context_text(err, ContextKind::ValidValue) {
                text.push('\n');
                text.push_str(&fill(m.err_supported_values, &[("values", &values)]));
            }
            text
        }
        _ => err.to_string(),
    };
    if let Some(usage) = context_text(err, ContextKind::Usage) {
        format!("{head}\n\n{usage}")
    } else {
        head
    }
}

fn context_text(err: &clap::Error, kind: clap::error::ContextKind) -> Option<String> {
    use clap::error::ContextValue;
    match err.get(kind)? {
        ContextValue::String(value) => Some(value.clone()),
        ContextValue::Strings(values) => Some(values.join(", ")),
        ContextValue::StyledStr(value) => Some(value.to_string()),
        ContextValue::StyledStrs(values) => Some(
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
        _ => None,
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
        lang: _,
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
        return Err(Error::Usage(i18n::t().err_need_prompt.into()));
    }

    let mut env = EnvMap::from_process();
    let config = load_effective(&env, overrides)?;
    include_provider_env(&mut env, &config);
    let shell = resolve_shell(
        overrides.shell.as_deref(),
        &config.shell_default,
        env.get("CMDGEN_SHELL"),
        env.get("SHELL"),
    )?;
    let endpoint = completions_endpoint(&config.base_url)?;
    let key = resolve_api_key(
        &config.provider_name,
        config.api_key_env.as_deref(),
        &env,
        &KeyringStore,
        &CredentialFile::from_env(&env)?,
    )?;
    if verbose {
        eprintln!(
            "{}",
            fill(i18n::t().verbose_model, &[("model", &config.model)])
        );
        eprintln!(
            "{}",
            fill(i18n::t().verbose_endpoint, &[("endpoint", &endpoint)])
        );
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
    let result = with_loading(i18n::t().loading, || {
        complete(&CompletionRequest {
            endpoint: &endpoint,
            api_key: &key.secret,
            body: &body,
            timeout_secs: config.timeout_secs,
        })
    });
    if verbose {
        eprintln!(
            "{}",
            fill(
                i18n::t().verbose_elapsed,
                &[("ms", &started.elapsed().as_millis().to_string())]
            )
        );
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
            let write_lang = if i18n::is_explicit() {
                i18n::current()
            } else {
                Lang::En
            };
            match init_config(&path, write_lang)? {
                InitOutcome::Created(path) => {
                    println!(
                        "{}",
                        fill(
                            i18n::t().config_created,
                            &[("path", &path.display().to_string())]
                        )
                    );
                    println!("{}", i18n::t().config_created_hint);
                }
                InitOutcome::AlreadyExists(path) => {
                    if !i18n::is_explicit() {
                        if let Some(lang) = crate::config::peek_language(&env) {
                            i18n::set_current(lang);
                        }
                    }
                    println!(
                        "{}",
                        fill(
                            i18n::t().config_exists,
                            &[("path", &path.display().to_string())]
                        )
                    );
                }
            }
            Ok(0)
        }
        ConfigCommand::Show => {
            let mut env = env;
            let config = load_effective(&env, overrides)?;
            include_provider_env(&mut env, &config);
            let shell = resolve_shell(
                overrides.shell.as_deref(),
                &config.shell_default,
                env.get("CMDGEN_SHELL"),
                env.get("SHELL"),
            )?;
            let resolved = resolve_api_key(
                &config.provider_name,
                config.api_key_env.as_deref(),
                &env,
                &KeyringStore,
                &CredentialFile::from_env(&env)?,
            );
            let status = key_status_from(resolved.as_ref());
            print!("{}", crate::config::render_show(&config, &shell, &status));
            Ok(0)
        }
        ConfigCommand::Providers => {
            let config = load_effective(&env, overrides)?;
            print!("{}", render_providers(&config));
            Ok(0)
        }
        ConfigCommand::Use { name } => {
            let path = config_path(&env)?;
            match activate_provider(&path, &name) {
                Ok(active) => {
                    println!(
                        "{}",
                        fill(i18n::t().switched_provider, &[("name", &active)])
                    );
                    println!(
                        "{}",
                        fill(
                            i18n::t().config_path_line,
                            &[("path", &path.display().to_string())]
                        )
                    );
                    Ok(0)
                }
                Err(Error::ConfigNotFound { .. }) => {
                    Err(Error::Usage(i18n::t().err_no_config.into()))
                }
                Err(err) => Err(err),
            }
        }
        ConfigCommand::Lang => config_lang(&env),
        ConfigCommand::Provider => config_provider(&env),
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
            let shell_name = display_shell(shell);
            if report.changed {
                println!("{}", fill(i18n::t().installed, &[("shell", shell_name)]));
            } else {
                println!(
                    "{}",
                    fill(i18n::t().install_unchanged, &[("shell", shell_name)])
                );
            }
            println!(
                "{}",
                fill(
                    i18n::t().line_script,
                    &[("path", &report.script_path.display().to_string())]
                )
            );
            println!(
                "{}",
                fill(
                    i18n::t().line_rc,
                    &[("path", &report.rc_path.display().to_string())]
                )
            );
            println!(
                "{}",
                fill(
                    i18n::t().line_binding,
                    &[("binding", &report.display_binding)]
                )
            );
            if report.created_backup {
                let backup = report.rc_path.with_file_name(format!(
                    "{}.cmdgen.bak",
                    report
                        .rc_path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "rc".into())
                ));
                println!(
                    "{}",
                    fill(
                        i18n::t().line_backup,
                        &[("path", &backup.display().to_string())]
                    )
                );
            }
            println!(
                "{}",
                fill(i18n::t().line_run, &[("command", &report.reload)])
            );
            if let Some(warning) = report.binding_warning {
                eprintln!("{}", fill(i18n::t().line_warning, &[("message", &warning)]));
            }
            if shell == IntegrationShell::Bash {
                eprintln!("{}", i18n::t().bash_hint);
            }
            Ok(0)
        }
        IntegrationCommand::Uninstall { shell } => {
            let shells: Vec<IntegrationShell> =
                shell.into_iter().map(IntegrationShell::from).collect();
            let reports = uninstall(&shells, &home, &dir)?;
            for report in reports {
                let shell_name = display_shell(report.shell);
                if report.removed_block || report.removed_script {
                    println!("{}", fill(i18n::t().uninstalled, &[("shell", shell_name)]));
                } else {
                    println!(
                        "{}",
                        fill(i18n::t().uninstall_absent, &[("shell", shell_name)])
                    );
                }
                if report.removed_block {
                    let name = report
                        .rc_path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    println!(
                        "{}",
                        fill(i18n::t().line_source, &[("name", name.as_ref())])
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

fn include_provider_env(env: &mut EnvMap, config: &crate::config::EffectiveConfig) {
    if let Some(var) = &config.api_key_env {
        env.include_process_var(var);
    }
}

/// 交互式终端上的短暂状态。请求结束或失败后擦掉。
/// 标准输出始终只有命令；标准错误被管道或文件接走时不写转圈。
fn with_loading<T>(label: &str, work: impl FnOnce() -> T) -> T {
    if !io::stderr().is_terminal() {
        return work();
    }

    let (tx, rx) = mpsc::channel();
    let label = label.to_string();
    let spinner = thread::spawn(move || {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let mut err = io::stderr();
        let mut frame = 0usize;
        loop {
            let _ = write!(err, "\r\x1b[2K{} {label}", FRAMES[frame % FRAMES.len()]);
            let _ = err.flush();
            frame += 1;
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        let _ = write!(err, "\r\x1b[2K");
        let _ = err.flush();
    });
    let result = work();
    drop(tx);
    let _ = spinner.join();
    result
}

fn display_shell(shell: IntegrationShell) -> &'static str {
    match shell {
        IntegrationShell::Zsh => "Zsh",
        IntegrationShell::Bash => "Bash",
    }
}

fn config_lang(env: &EnvMap) -> Result<i32, Error> {
    let path = config_path(env)?;
    match load_config(&path) {
        Ok(_) => {}
        Err(Error::ConfigNotFound { .. }) => {
            return Err(Error::Usage(i18n::t().err_no_config.into()));
        }
        Err(err) => return Err(err),
    }
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(i18n::t().err_lang_not_a_terminal.into()));
    }
    let Some(selected) = select_language(i18n::current())? else {
        println!("{}", i18n::t().cancelled);
        return Ok(0);
    };
    match set_language(&path, selected) {
        Ok(()) => {
            let messages = i18n::messages(selected);
            println!(
                "{}",
                fill(
                    messages.switched_language,
                    &[("language", selected.as_str())]
                )
            );
            println!(
                "{}",
                fill(
                    messages.config_path_line,
                    &[("path", &path.display().to_string())]
                )
            );
            Ok(0)
        }
        Err(Error::ConfigNotFound { .. }) => Err(Error::Usage(i18n::t().err_no_config.into())),
        Err(err) => Err(err),
    }
}

const LANGUAGE_CHOICES: [(Lang, &str); 2] = [(Lang::En, "English (en)"), (Lang::Zh, "中文 (zh)")];

/// 在 stderr 上画出语言菜单。`None` 表示用户取消，文件不会被修改。
fn select_language(current: Lang) -> Result<Option<Lang>, Error> {
    let labels: Vec<String> = LANGUAGE_CHOICES
        .iter()
        .map(|(_, label)| (*label).to_string())
        .collect();
    let default = LANGUAGE_CHOICES
        .iter()
        .position(|(lang, _)| *lang == current)
        .unwrap_or(0);
    let Some(index) = select_index(i18n::t().lang_menu_title, &labels, default)? else {
        return Ok(None);
    };
    Ok(Some(LANGUAGE_CHOICES[index].0))
}

struct ProviderRow {
    name: String,
    base_url: String,
    model: String,
}

fn config_provider(env: &EnvMap) -> Result<i32, Error> {
    let path = config_path(env)?;
    ensure_provider_editable(&path)?;
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(i18n::t().err_provider_not_a_terminal.into()));
    }
    loop {
        let file = load_config(&path)?;
        let rows = provider_rows(&file);
        let active = trimmed_active(&file);
        let mut labels: Vec<String> = rows
            .iter()
            .map(|row| {
                if active.as_deref() == Some(row.name.as_str()) {
                    fill(i18n::t().provider_current, &[("name", &row.name)])
                } else {
                    row.name.clone()
                }
            })
            .collect();
        labels.push(i18n::t().provider_add.to_string());
        let default = active
            .as_ref()
            .and_then(|name| rows.iter().position(|row| row.name == *name))
            .unwrap_or(0);
        let Some(index) = select_index(i18n::t().provider_menu_title, &labels, default)? else {
            return Ok(0);
        };
        if index == rows.len() {
            add_provider(env, &path)?;
        } else {
            edit_provider_fields(env, &path, &rows[index].name)?;
        }
    }
}

fn add_provider(env: &EnvMap, path: &std::path::Path) -> Result<(), Error> {
    let file = load_config(path)?;
    let rows = provider_rows(&file);
    let had_active = trimmed_active(&file).is_some();
    let name = loop {
        let Some(raw) = prompt_line(i18n::t().prompt_provider_name)? else {
            return Ok(());
        };
        match normalize_provider(&raw) {
            Ok(name) if rows.iter().any(|row| row.name == name) => {
                eprintln!(
                    "cmdgen: {}",
                    fill(i18n::t().err_provider_exists, &[("name", &name)])
                );
            }
            Ok(name) => break name,
            Err(err) => eprintln!("cmdgen: {err}"),
        }
    };
    let Some(base_url) = prompt_new_base_url()? else {
        return Ok(());
    };
    let Some(model) = prompt_line(i18n::t().prompt_model)? else {
        return Ok(());
    };
    match create_provider(path, &name, &base_url, &model) {
        Ok(update) => {
            print_provider_update(&path.display().to_string(), &update);
            if had_active {
                println!(
                    "{}",
                    fill(i18n::t().added_provider_hint, &[("name", &update.name)])
                );
            } else {
                println!(
                    "{}",
                    fill(i18n::t().switched_provider, &[("name", &update.name)])
                );
            }
        }
        Err(err) => {
            eprintln!("cmdgen: {err}");
            return Ok(());
        }
    }
    prompt_optional_api_key(env, &name)
}

fn prompt_new_base_url() -> Result<Option<String>, Error> {
    loop {
        let Some(raw) = prompt_line(i18n::t().prompt_base_url)? else {
            return Ok(None);
        };
        match completions_endpoint(&raw) {
            Ok(endpoint) => {
                let base = endpoint
                    .trim_end_matches("/chat/completions")
                    .trim_end_matches('/')
                    .to_string();
                return Ok(Some(base));
            }
            Err(err) => eprintln!("cmdgen: {err}"),
        }
    }
}

fn prompt_optional_api_key(env: &EnvMap, name: &str) -> Result<(), Error> {
    let secret = match rpassword::prompt_password(i18n::t().prompt_api_key_optional) {
        Ok(secret) => secret,
        Err(_) => {
            eprintln!("cmdgen: {}", i18n::t().err_hidden_input);
            return Ok(());
        }
    };
    if secret.trim().is_empty() {
        return Ok(());
    }
    let file = match CredentialFile::from_env(env) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("cmdgen: {err}");
            return Ok(());
        }
    };
    match save_api_key(name, &secret, &KeyringStore, &file) {
        Ok(saved) => print_saved_key(name, saved),
        Err(err) => eprintln!("cmdgen: {err}"),
    }
    Ok(())
}

fn ensure_provider_editable(path: &std::path::Path) -> Result<(), Error> {
    let file = match load_config(path) {
        Ok(file) => file,
        Err(Error::ConfigNotFound { .. }) => {
            return Err(Error::Usage(i18n::t().err_no_config.into()));
        }
        Err(err) => return Err(err),
    };
    if provider_file_has_content(&file.provider) && file.providers.is_empty() {
        return Err(Error::invalid(path, i18n::t().err_legacy_only_edit));
    }
    Ok(())
}

fn provider_rows(file: &FileConfig) -> Vec<ProviderRow> {
    let mut rows = Vec::new();
    for (raw, profile) in &file.providers {
        let Ok(name) = normalize_provider(raw) else {
            continue;
        };
        if rows.iter().any(|row: &ProviderRow| row.name == name) {
            continue;
        }
        rows.push(ProviderRow {
            base_url: saved_or_preset(profile.base_url.as_deref(), preset_url(&name)),
            model: saved_or_preset(profile.model.as_deref(), preset_model_of(&name)),
            name,
        });
    }
    for (name, url, model) in [
        ("deepseek", DEEPSEEK_BASE_URL, DEEPSEEK_MODEL),
        ("bailian", BAILIAN_BASE_URL, BAILIAN_MODEL),
    ] {
        if rows.iter().any(|row| row.name == name) {
            continue;
        }
        rows.push(ProviderRow {
            name: name.to_string(),
            base_url: url.to_string(),
            model: model.to_string(),
        });
    }
    rows
}

fn saved_or_preset(saved: Option<&str>, preset: Option<&str>) -> String {
    saved
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(preset)
        .unwrap_or("")
        .to_string()
}

fn preset_url(name: &str) -> Option<&'static str> {
    match name {
        "deepseek" => Some(DEEPSEEK_BASE_URL),
        "bailian" => Some(BAILIAN_BASE_URL),
        _ => None,
    }
}

fn preset_model_of(name: &str) -> Option<&'static str> {
    match name {
        "deepseek" => Some(DEEPSEEK_MODEL),
        "bailian" => Some(BAILIAN_MODEL),
        _ => None,
    }
}

fn trimmed_active(file: &FileConfig) -> Option<String> {
    let raw = file.active.as_deref()?.trim();
    if raw.is_empty() {
        None
    } else {
        normalize_provider(raw).ok()
    }
}

fn api_key_env_name(file: &FileConfig, name: &str) -> Option<String> {
    let saved = file.providers.iter().find_map(|(key, profile)| {
        let Ok(normalized) = normalize_provider(key) else {
            return None;
        };
        (normalized == name).then(|| profile.api_key_env.clone())
    });
    if let Some(Some(value)) = saved {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    match name {
        "deepseek" => Some("DEEPSEEK_API_KEY".to_string()),
        "bailian" => Some("DASHSCOPE_API_KEY".to_string()),
        _ => None,
    }
}

fn edit_provider_fields(env: &EnvMap, path: &std::path::Path, name: &str) -> Result<(), Error> {
    loop {
        let file = load_config(path)?;
        let Some(row) = provider_rows(&file)
            .into_iter()
            .find(|row| row.name == name)
        else {
            return Ok(());
        };
        let key_env = api_key_env_name(&file, name);
        let key_status = key_status_label(env, name, key_env.as_deref());
        let messages = i18n::t();
        let items = vec![
            format!("{}  {}", messages.field_model, row.model),
            format!("{}  {}", messages.field_base_url, row.base_url),
            format!("{}  {key_status}", messages.field_set_key),
            messages.field_delete_key.to_string(),
            messages.field_back.to_string(),
        ];
        let Some(index) = select_index(name, &items, 0)? else {
            return Ok(());
        };
        match index {
            0 => edit_model(path, name)?,
            1 => edit_base_url(path, name)?,
            2 => edit_api_key(env, name)?,
            3 => delete_provider_key(env, name)?,
            _ => return Ok(()),
        }
    }
}

fn key_status_label(env: &EnvMap, name: &str, api_key_env: Option<&str>) -> String {
    let Ok(file) = CredentialFile::from_env(env) else {
        return i18n::t().status_unconfigured.into();
    };
    let resolved = resolve_api_key(name, api_key_env, env, &KeyringStore, &file);
    let status = key_status_from(resolved.as_ref());
    if status.configured {
        fill(
            i18n::t().credentials_redacted,
            &[("source", &status.source_label)],
        )
    } else {
        status.source_label
    }
}

fn edit_model(path: &std::path::Path, name: &str) -> Result<(), Error> {
    let Some(model) = prompt_line(i18n::t().prompt_model)? else {
        return Ok(());
    };
    match update_provider(path, Some(name), None, Some(&model)) {
        Ok(update) => print_provider_update(&path.display().to_string(), &update),
        Err(err) => eprintln!("cmdgen: {err}"),
    }
    Ok(())
}

fn edit_base_url(path: &std::path::Path, name: &str) -> Result<(), Error> {
    let Some(url) = prompt_line(i18n::t().prompt_base_url)? else {
        return Ok(());
    };
    match update_provider(path, Some(name), Some(&url), None) {
        Ok(update) => print_provider_update(&path.display().to_string(), &update),
        Err(err) => eprintln!("cmdgen: {err}"),
    }
    Ok(())
}

fn print_provider_update(path: &str, update: &crate::config::ProviderUpdate) {
    println!(
        "{}",
        fill(i18n::t().updated_provider, &[("name", &update.name)])
    );
    if let Some(url) = &update.base_url {
        println!("{}", fill(i18n::t().line_base_url, &[("url", url)]));
    }
    if let Some(model) = &update.model {
        println!("{}", fill(i18n::t().line_model, &[("model", model)]));
    }
    println!("{}", fill(i18n::t().config_path_line, &[("path", path)]));
}

fn edit_api_key(env: &EnvMap, name: &str) -> Result<(), Error> {
    let secret = match rpassword::prompt_password(i18n::t().prompt_api_key) {
        Ok(secret) => secret,
        Err(_) => {
            eprintln!("cmdgen: {}", i18n::t().err_hidden_input);
            return Ok(());
        }
    };
    let file = match CredentialFile::from_env(env) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("cmdgen: {err}");
            return Ok(());
        }
    };
    match save_api_key(name, &secret, &KeyringStore, &file) {
        Ok(saved) => print_saved_key(name, saved),
        Err(err) => eprintln!("cmdgen: {err}"),
    }
    Ok(())
}

fn print_saved_key(name: &str, saved: SavedKey) {
    match saved {
        SavedKey::Keyring { removed_file_entry } => {
            println!("{}", fill(i18n::t().saved_keyring, &[("account", name)]));
            if removed_file_entry {
                println!("{}", i18n::t().removed_file_key);
            }
        }
        SavedKey::File { path } => {
            println!(
                "{}",
                fill(
                    i18n::t().saved_file,
                    &[("path", &path.display().to_string())]
                )
            );
            println!("{}", i18n::t().saved_file_mode);
        }
    }
}

fn delete_provider_key(env: &EnvMap, name: &str) -> Result<(), Error> {
    if !confirm_delete(name)? {
        println!("{}", i18n::t().cancelled);
        return Ok(());
    }
    let file = match CredentialFile::from_env(env) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("cmdgen: {err}");
            return Ok(());
        }
    };
    match delete_api_key(name, &KeyringStore, &file) {
        Ok(deleted) => match (deleted.keyring, deleted.file) {
            (true, true) => {
                println!("{}", fill(i18n::t().deleted_both, &[("account", name)]));
            }
            (true, false) => {
                println!("{}", fill(i18n::t().deleted_keyring, &[("account", name)]));
            }
            (false, true) => println!(
                "{}",
                fill(
                    i18n::t().deleted_file,
                    &[("path", &file.path().display().to_string())]
                )
            ),
            (false, false) => unreachable!("任一侧未删除时 delete_api_key 会返回错误"),
        },
        Err(err) => eprintln!("cmdgen: {err}"),
    }
    Ok(())
}

fn prompt_line(prompt: &str) -> Result<Option<String>, Error> {
    eprint!("{prompt}");
    io::stderr()
        .flush()
        .map_err(|err| Error::io("stderr", err))?;
    match read_console_line()? {
        ConsoleLine::Submit(text) if !text.is_empty() => Ok(Some(text)),
        _ => Ok(None),
    }
}

enum ConsoleLine {
    Submit(String),
    Cancel,
}

/// 在 raw mode 里逐字节读一行，不用 `Stdin::read_line`。
///
/// Linux 上，规范模式的一次 `read` 会把 raw mode 期间已经排进队列的字节全部返回。
/// `read_line` 只消费第一行，剩下的留在 `Stdin` 的用户态缓冲里；
/// 菜单随后用 `read_byte` 直接读 fd，读不到这些字节，于是一直阻塞。
/// macOS 的规范模式按行返回，队列里的后续按键仍留给下一次读取。
fn read_console_line() -> Result<ConsoleLine, Error> {
    enable_raw_mode().map_err(|err| Error::io("stdin", err))?;
    let guard = TerminalGuard {
        lines: 0,
        raw: true,
    };
    let mut line = String::new();
    let mut err = io::stderr();
    let submitted = loop {
        match read_byte()? {
            b'\r' | b'\n' => break true,
            0x03 => break false,
            0x1b => {
                if let Some(KeyCode::Esc) = read_escape()? {
                    break false;
                }
            }
            0x7f | 0x08 => {
                if line.pop().is_some() {
                    write!(err, "\u{8} \u{8}").map_err(|err| Error::io("stderr", err))?;
                    err.flush().map_err(|err| Error::io("stderr", err))?;
                }
            }
            first => {
                let Some(ch) = decode_char(first)? else {
                    continue;
                };
                if ch.is_control() {
                    continue;
                }
                line.push(ch);
                write!(err, "{ch}").map_err(|err| Error::io("stderr", err))?;
                err.flush().map_err(|err| Error::io("stderr", err))?;
            }
        }
    };
    write!(err, "\r\n").map_err(|err| Error::io("stderr", err))?;
    err.flush().map_err(|err| Error::io("stderr", err))?;
    drop(guard);
    if submitted {
        Ok(ConsoleLine::Submit(line.trim().to_string()))
    } else {
        Ok(ConsoleLine::Cancel)
    }
}

fn decode_char(first: u8) -> Result<Option<char>, Error> {
    if first < 0x80 {
        return Ok(Some(first as char));
    }
    let width = match first {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => return Ok(None),
    };
    let mut buf = [first, 0, 0, 0];
    for slot in &mut buf[1..width] {
        let byte = read_byte()?;
        if byte & 0xC0 != 0x80 {
            push_byte(byte);
            return Ok(None);
        }
        *slot = byte;
    }
    Ok(std::str::from_utf8(&buf[..width])
        .ok()
        .and_then(|text| text.chars().next()))
}

/// 在 stderr 上画出菜单。`None` 表示 Esc 或 Ctrl-C。
fn select_index(title: &str, items: &[String], default: usize) -> Result<Option<usize>, Error> {
    if items.is_empty() {
        return Ok(None);
    }
    enable_raw_mode().map_err(|err| Error::io("stdin", err))?;
    let mut guard = TerminalGuard {
        lines: 0,
        raw: true,
    };
    execute!(io::stderr(), Hide).map_err(|err| Error::io("stderr", err))?;
    let mut index = default.min(items.len() - 1);
    loop {
        let lines = menu_lines(title, items, index);
        guard.draw(&lines)?;
        match read_key()? {
            KeyCode::Up => index = (index + items.len() - 1) % items.len(),
            KeyCode::Down => index = (index + 1) % items.len(),
            KeyCode::Enter => return Ok(Some(index)),
            KeyCode::Esc | KeyCode::Char('c') => return Ok(None),
            _ => {}
        }
    }
}

fn menu_lines(title: &str, items: &[String], index: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(items.len() + 2);
    lines.push(title.to_string());
    for (item, label) in items.iter().enumerate() {
        let marker = if item == index { '>' } else { ' ' };
        lines.push(format!("{marker} {label}"));
    }
    lines.push(i18n::t().lang_menu_hint.to_string());
    lines
}

/// 一次只从终端读一个按键，剩下的字节留在输入队列里，交给下一次 `read_byte`。
/// `Ctrl-C` 变成 `KeyCode::Char('c')`，调用方把它当作取消。
fn read_key() -> Result<KeyCode, Error> {
    loop {
        match read_byte()? {
            b'\r' | b'\n' => return Ok(KeyCode::Enter),
            0x03 => return Ok(KeyCode::Char('c')),
            0x1b => {
                if let Some(code) = read_escape()? {
                    return Ok(code);
                }
            }
            _ => {}
        }
    }
}

thread_local! {
    static PENDING_INPUT: RefCell<VecDeque<u8>> = const { RefCell::new(VecDeque::new()) };
}

fn read_byte() -> Result<u8, Error> {
    if let Some(byte) = PENDING_INPUT.with(|pending| pending.borrow_mut().pop_front()) {
        return Ok(byte);
    }
    let stdin = io::stdin();
    let mut buf = [0u8; 1];
    loop {
        match rustix::io::read(&stdin, &mut buf) {
            Ok(0) => {
                return Err(Error::io(
                    "stdin",
                    io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"),
                ));
            }
            Ok(_) => return Ok(buf[0]),
            Err(rustix::io::Errno::INTR) => continue,
            Err(err) => return Err(Error::io("stdin", err.into())),
        }
    }
}

fn push_byte(byte: u8) {
    PENDING_INPUT.with(|pending| pending.borrow_mut().push_front(byte));
}

fn input_ready(timeout: Duration) -> Result<bool, Error> {
    if PENDING_INPUT.with(|pending| !pending.borrow().is_empty()) {
        return Ok(true);
    }
    let stdin = io::stdin();
    let mut fds = [rustix::event::PollFd::new(
        &stdin,
        rustix::event::PollFlags::IN,
    )];
    let spec = rustix::event::Timespec {
        tv_sec: timeout.as_secs() as rustix::event::Secs,
        tv_nsec: timeout.subsec_nanos() as rustix::event::Nsecs,
    };
    match rustix::event::poll(&mut fds, Some(&spec)) {
        Ok(0) => Ok(false),
        Ok(_) => Ok(fds[0].revents().contains(rustix::event::PollFlags::IN)),
        Err(rustix::io::Errno::INTR) => Ok(false),
        Err(err) => Err(Error::io("stdin", err.into())),
    }
}

/// Esc 单独按下时返回取消。方向键是 `Esc [` 再加一个字母，已经在缓冲区里时立刻读完。
fn read_escape() -> Result<Option<KeyCode>, Error> {
    if !input_ready(Duration::from_millis(50))? {
        return Ok(Some(KeyCode::Esc));
    }
    let second = read_byte()?;
    if second != b'[' && second != b'O' {
        push_byte(second);
        return Ok(Some(KeyCode::Esc));
    }
    if !input_ready(Duration::from_millis(50))? {
        push_byte(second);
        return Ok(Some(KeyCode::Esc));
    }
    Ok(match read_byte()? {
        b'A' => Some(KeyCode::Up),
        b'B' => Some(KeyCode::Down),
        b'C' => Some(KeyCode::Right),
        b'D' => Some(KeyCode::Left),
        _ => None,
    })
}

struct TerminalGuard {
    lines: u16,
    raw: bool,
}

impl TerminalGuard {
    fn draw(&mut self, lines: &[String]) -> Result<(), Error> {
        let mut err = io::stderr();
        if self.lines > 0 {
            write!(err, "\x1b[{}A", self.lines).map_err(|err| Error::io("stderr", err))?;
        }
        for line in lines {
            write!(err, "\r\x1b[2K{line}\r\n").map_err(|err| Error::io("stderr", err))?;
        }
        err.flush().map_err(|err| Error::io("stderr", err))?;
        self.lines = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut err = io::stderr();
        if self.lines > 0 {
            let _ = write!(err, "\x1b[{}A\r\x1b[J", self.lines);
        }
        let _ = execute!(err, Show);
        if self.raw {
            let _ = disable_raw_mode();
        }
        let _ = err.flush();
    }
}

fn confirm_delete(name: &str) -> Result<bool, Error> {
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(i18n::t().err_confirm_delete.into()));
    }
    eprint!(
        "{}",
        fill(i18n::t().prompt_confirm_delete, &[("name", name)])
    );
    let _ = io::stderr().flush();
    Ok(matches!(
        read_console_line()?,
        ConsoleLine::Submit(text) if text == "yes"
    ))
}

fn reject_inline_secret(args: &[OsString]) -> Result<(), Error> {
    const VALUE_FLAGS: &[&str] = &[
        "--provider",
        "--model",
        "--base-url",
        "--shell",
        "--timeout",
        "--lang",
    ];
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let text = args[index].to_string_lossy();
        if text == "--" {
            if secret_command(&positionals) {
                return Err(Error::Usage(i18n::t().secret_arg.into()));
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
        if rejects_trailing_secret(&positionals) {
            return Err(Error::Usage(i18n::t().secret_arg.into()));
        }
        index += 1;
    }
    Ok(())
}

/// `config set-key` 或 `config provider` 本身，后面还没跟参数。
fn secret_command(positionals: &[String]) -> bool {
    positionals.len() >= 2
        && positionals[positionals.len() - 2] == "config"
        && matches!(
            positionals[positionals.len() - 1].as_str(),
            "set-key" | "provider"
        )
}

/// `config set-key <参数>` 和 `config provider <参数>` 都可能把密钥留在命令行里。
fn rejects_trailing_secret(positionals: &[String]) -> bool {
    positionals.len() >= 3 && secret_command(&positionals[..positionals.len() - 1])
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
    fn config_lang_is_a_subcommand() {
        let cli = Cli::try_parse_from(["cmdgen", "config", "lang"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Config {
                action: ConfigCommand::Lang
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
        for command in ["set-key", "provider"] {
            let err = reject_inline_secret(&[
                "cmdgen".into(),
                "config".into(),
                command.into(),
                "SENTINEL_SECRET_VALUE".into(),
            ])
            .unwrap_err();
            let text = err.to_string();
            assert!(!text.contains("SENTINEL_SECRET_VALUE"), "{text}");
            assert!(text.contains("config provider"), "{text}");
        }
    }

    #[test]
    fn config_provider_is_a_subcommand() {
        let cli = Cli::try_parse_from(["cmdgen", "config", "provider"]).unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Config {
                action: ConfigCommand::Provider
            })
        ));
    }
}
