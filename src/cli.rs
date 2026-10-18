use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use crate::config::{
    activate_provider, config_dir, config_path, delete_api_key, home_dir, init_config,
    key_status_from, load_config, load_effective, render_providers, resolve_api_key, save_api_key,
    set_language, update_provider, CredentialFile, EnvMap, InitOutcome, KeyringStore, Overrides,
    SavedKey,
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
    /// Write --model or --base-url to the config file. With no --provider, update the current active provider
    Set,
    /// Read an API key without echoing it. Prefer the system credential store; otherwise write the local credential file
    SetKey,
    /// Delete the current provider's API key from the system credential store and the local credential file
    DeleteKey {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
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
        let config = set_about(config, "set", m.help_config_set);
        let config = set_about(config, "set-key", m.help_config_set_key);
        let config = set_about(config, "delete-key", m.help_config_delete_key);
        let config = config.mut_subcommand("use", |use_cmd| {
            set_arg_help(use_cmd, "name", m.help_config_use_name)
        });
        config.mut_subcommand("delete-key", |delete| {
            set_arg_help(delete, "yes", m.help_delete_key_yes)
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
        ConfigCommand::Set => {
            let path = config_path(&env)?;
            match update_provider(
                &path,
                overrides.provider.as_deref(),
                overrides.base_url.as_deref(),
                overrides.model.as_deref(),
            ) {
                Ok(update) => {
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
        ConfigCommand::SetKey => {
            let config = load_effective(&env, overrides)?;
            let secret = rpassword::prompt_password(i18n::t().prompt_api_key)
                .map_err(|_| Error::Usage(i18n::t().err_hidden_input.into()))?;
            let file = CredentialFile::from_env(&env)?;
            match save_api_key(&config.provider_name, &secret, &KeyringStore, &file)? {
                SavedKey::Keyring { removed_file_entry } => {
                    println!(
                        "{}",
                        fill(
                            i18n::t().saved_keyring,
                            &[("account", &config.provider_name)]
                        )
                    );
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
            Ok(0)
        }
        ConfigCommand::DeleteKey { yes } => {
            let config = load_effective(&env, overrides)?;
            if !yes && !confirm_delete()? {
                println!("{}", i18n::t().cancelled);
                return Ok(0);
            }
            let file = CredentialFile::from_env(&env)?;
            let deleted = delete_api_key(&config.provider_name, &KeyringStore, &file)?;
            match (deleted.keyring, deleted.file) {
                (true, true) => println!(
                    "{}",
                    fill(
                        i18n::t().deleted_both,
                        &[("account", &config.provider_name)]
                    )
                ),
                (true, false) => println!(
                    "{}",
                    fill(
                        i18n::t().deleted_keyring,
                        &[("account", &config.provider_name)]
                    )
                ),
                (false, true) => println!(
                    "{}",
                    fill(
                        i18n::t().deleted_file,
                        &[("path", &file.path().display().to_string())]
                    )
                ),
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
    enable_raw_mode().map_err(|err| Error::io("stdin", err))?;
    let mut guard = TerminalGuard {
        lines: 0,
        raw: true,
    };
    execute!(io::stderr(), Hide).map_err(|err| Error::io("stderr", err))?;
    let mut index = LANGUAGE_CHOICES
        .iter()
        .position(|(lang, _)| *lang == current)
        .unwrap_or(0);
    loop {
        let lines = language_menu_lines(index);
        guard.draw(&lines)?;
        match read_key()? {
            KeyCode::Up => index = (index + LANGUAGE_CHOICES.len() - 1) % LANGUAGE_CHOICES.len(),
            KeyCode::Down => index = (index + 1) % LANGUAGE_CHOICES.len(),
            KeyCode::Enter => return Ok(Some(LANGUAGE_CHOICES[index].0)),
            KeyCode::Esc => return Ok(None),
            KeyCode::Char('c') => return Ok(None),
            _ => {}
        }
    }
}

fn language_menu_lines(index: usize) -> Vec<String> {
    let messages = i18n::t();
    let mut lines = Vec::with_capacity(LANGUAGE_CHOICES.len() + 2);
    lines.push(messages.lang_menu_title.to_string());
    for (item, (_, label)) in LANGUAGE_CHOICES.iter().enumerate() {
        let marker = if item == index { '>' } else { ' ' };
        lines.push(format!("{marker} {label}"));
    }
    lines.push(messages.lang_menu_hint.to_string());
    lines
}

/// 只处理按下和按住。`Ctrl-C` 在这里变成 `KeyCode::Char('c')`，调用方把它当作取消。
fn read_key() -> Result<KeyCode, Error> {
    loop {
        let event = match event::read() {
            Ok(event) => event,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(Error::io("stdin", err)),
        };
        let Event::Key(key) = event else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        if key.code == KeyCode::Char('c') && !key.modifiers.contains(KeyModifiers::CONTROL) {
            continue;
        }
        return Ok(key.code);
    }
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

fn confirm_delete() -> Result<bool, Error> {
    if !io::stdin().is_terminal() {
        return Err(Error::Usage(i18n::t().err_confirm_delete.into()));
    }
    eprint!("{}", i18n::t().prompt_confirm_delete);
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
        "--lang",
    ];
    let mut positionals = Vec::new();
    let mut index = 1;
    while index < args.len() {
        let text = args[index].to_string_lossy();
        if text == "--" {
            if positionals.last().map(String::as_str) == Some("set-key")
                && positionals.iter().rev().nth(1).map(String::as_str) == Some("config")
            {
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
        if positionals.len() >= 3
            && positionals[positionals.len() - 3] == "config"
            && positionals[positionals.len() - 2] == "set-key"
        {
            return Err(Error::Usage(i18n::t().secret_arg.into()));
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
