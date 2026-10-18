use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::fsutil;
use crate::i18n::{self, fill};

pub const MANAGED_BEGIN: &str = "# >>> cmdgen managed integration >>>";
pub const MANAGED_END: &str = "# <<< cmdgen managed integration <<<";

const ZSH_TEMPLATE: &str = include_str!("../../shell/cmdgen.zsh");
const BASH_TEMPLATE: &str = include_str!("../../shell/cmdgen.bash");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationShell {
    Zsh,
    Bash,
}

impl IntegrationShell {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
        }
    }

    fn rc_name(self) -> &'static str {
        match self {
            Self::Zsh => ".zshrc",
            Self::Bash => ".bashrc",
        }
    }

    fn script_name(self) -> &'static str {
        match self {
            Self::Zsh => "integration.zsh",
            Self::Bash => "integration.bash",
        }
    }

    fn template(self) -> &'static str {
        match self {
            Self::Zsh => ZSH_TEMPLATE,
            Self::Bash => BASH_TEMPLATE,
        }
    }

    fn all() -> [Self; 2] {
        [Self::Zsh, Self::Bash]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub canonical: String,
    pub zsh: String,
    pub bash: String,
    pub display: String,
}

pub fn parse_keybinding(input: &str) -> Result<KeyBinding, Error> {
    let normalized = input.trim().to_ascii_lowercase().replace('_', "-");
    let letter = normalized
        .strip_prefix("ctrl-")
        .or_else(|| normalized.strip_prefix("control-"))
        .or_else(|| normalized.strip_prefix("c-"));
    let Some(letter) = letter else {
        return Err(invalid_keybinding(input));
    };
    let mut chars = letter.chars();
    let Some(ch) = chars.next() else {
        return Err(invalid_keybinding(input));
    };
    if chars.next().is_some() || !ch.is_ascii_alphabetic() {
        return Err(invalid_keybinding(input));
    }
    let upper = ch.to_ascii_uppercase();
    let lower = ch.to_ascii_lowercase();
    Ok(KeyBinding {
        canonical: format!("ctrl-{lower}"),
        zsh: format!("^{upper}"),
        bash: format!("\\C-{lower}"),
        display: format!("Ctrl+{upper}"),
    })
}

fn invalid_keybinding(input: &str) -> Error {
    Error::InvalidConfig {
        path: "shell.keybinding".into(),
        reason: fill(i18n::t().err_keybinding, &[("input", input)]),
    }
}

#[derive(Debug)]
pub struct InstallReport {
    pub shell: IntegrationShell,
    pub script_path: PathBuf,
    pub rc_path: PathBuf,
    pub reload: String,
    pub changed: bool,
    pub created_backup: bool,
    pub binding_warning: Option<String>,
    pub display_binding: String,
}

pub fn install(
    shell: IntegrationShell,
    home: &Path,
    config_dir: &Path,
    keybinding: &str,
) -> Result<InstallReport, Error> {
    let binding = parse_keybinding(keybinding)?;
    fsutil::ensure_dir(config_dir, 0o700)?;
    let script_path = config_dir.join(shell.script_name());
    let script = render_script(shell, &binding)?;
    let script_changed = rewrite_if_changed(&script_path, &script, 0o600)?;

    let rc_path = home.join(shell.rc_name());
    let original = read_optional(&rc_path)?;
    let source = source_line(home, &script_path);
    let updated = upsert_managed_block(&original, &source)?;
    let outside = remove_managed_block(&original)?.unwrap_or(original.clone());
    let warning = foreign_binding_warning(&outside, shell, &binding);
    let rc_changed = updated != original;
    let created_backup = if rc_changed {
        write_rc(&rc_path, &updated)?
    } else {
        false
    };

    Ok(InstallReport {
        shell,
        script_path,
        rc_path: rc_path.clone(),
        reload: reload_command(home, &rc_path),
        changed: script_changed || rc_changed,
        created_backup,
        binding_warning: warning,
        display_binding: binding.display,
    })
}

#[derive(Debug)]
pub struct UninstallReport {
    pub shell: IntegrationShell,
    pub rc_path: PathBuf,
    pub removed_block: bool,
    pub removed_script: bool,
}

pub fn uninstall(
    shells: &[IntegrationShell],
    home: &Path,
    config_dir: &Path,
) -> Result<Vec<UninstallReport>, Error> {
    let selected: Vec<IntegrationShell> = if shells.is_empty() {
        IntegrationShell::all().to_vec()
    } else {
        shells.to_vec()
    };
    let mut reports = Vec::new();
    for shell in selected {
        let rc_path = home.join(shell.rc_name());
        let original = read_optional(&rc_path)?;
        let removed_block = match remove_managed_block(&original)? {
            Some(updated) if updated != original => {
                write_rc(&rc_path, &updated)?;
                true
            }
            Some(_) => false,
            None => false,
        };
        let script_path = config_dir.join(shell.script_name());
        let removed_script = script_path.exists();
        fsutil::remove_if_exists(&script_path)?;
        reports.push(UninstallReport {
            shell,
            rc_path,
            removed_block,
            removed_script,
        });
    }
    Ok(reports)
}

pub fn upsert_managed_block(original: &str, source_line: &str) -> Result<String, Error> {
    if source_line.contains('\n') {
        return Err(Error::ShellIntegrationError(
            i18n::t().err_source_newline.into(),
        ));
    }
    let block = format!("{MANAGED_BEGIN}\n{source_line}\n{MANAGED_END}\n");
    match block_span(original)? {
        None => {
            if original.is_empty() {
                Ok(block)
            } else if original.ends_with('\n') {
                Ok(format!("{original}{block}"))
            } else {
                Ok(format!("{original}\n{block}"))
            }
        }
        Some((start, end)) => {
            let mut result = String::new();
            result.push_str(&original[..start]);
            result.push_str(&block);
            let rest = original[end..]
                .strip_prefix('\n')
                .unwrap_or(&original[end..]);
            result.push_str(rest);
            Ok(result)
        }
    }
}

pub fn remove_managed_block(original: &str) -> Result<Option<String>, Error> {
    let Some((start, end)) = block_span(original)? else {
        return Ok(None);
    };
    let mut result = String::new();
    result.push_str(&original[..start]);
    let rest = original[end..]
        .strip_prefix('\n')
        .unwrap_or(&original[end..]);
    result.push_str(rest);
    Ok(Some(result))
}

pub fn integration_state(home: &Path, config_dir: &Path, shell: IntegrationShell) -> String {
    let rc_path = home.join(shell.rc_name());
    let script_path = config_dir.join(shell.script_name());
    let rc = read_optional(&rc_path).unwrap_or_default();
    let marked = rc.contains(MANAGED_BEGIN) && rc.contains(MANAGED_END);
    match (marked, script_path.exists()) {
        (true, true) => fill(
            i18n::t().state_installed,
            &[
                ("shell", shell.as_str()),
                ("path", &rc_path.display().to_string()),
            ],
        ),
        (true, false) => fill(
            i18n::t().state_mark_no_script,
            &[
                ("shell", shell.as_str()),
                ("path", &script_path.display().to_string()),
            ],
        ),
        (false, true) => fill(
            i18n::t().state_script_no_mark,
            &[
                ("shell", shell.as_str()),
                ("path", &rc_path.display().to_string()),
            ],
        ),
        (false, false) => fill(i18n::t().state_absent, &[("shell", shell.as_str())]),
    }
}

pub fn integration_is_broken(home: &Path, config_dir: &Path, shell: IntegrationShell) -> bool {
    let rc = read_optional(&home.join(shell.rc_name())).unwrap_or_default();
    let marked = rc.contains(MANAGED_BEGIN) && rc.contains(MANAGED_END);
    let script_exists = config_dir.join(shell.script_name()).exists();
    marked != script_exists
}

fn block_span(original: &str) -> Result<Option<(usize, usize)>, Error> {
    let start = original.find(MANAGED_BEGIN);
    let end_rel = start.and_then(|start| original[start + MANAGED_BEGIN.len()..].find(MANAGED_END));
    match (start, end_rel) {
        (None, None) => Ok(None),
        (Some(start), Some(rel)) => Ok(Some((
            start,
            start + MANAGED_BEGIN.len() + rel + MANAGED_END.len(),
        ))),
        _ => Err(Error::ShellIntegrationError(
            i18n::t().err_marker_incomplete.into(),
        )),
    }
}

fn render_script(shell: IntegrationShell, binding: &KeyBinding) -> Result<String, Error> {
    let sequence = match shell {
        IntegrationShell::Zsh => binding.zsh.as_str(),
        IntegrationShell::Bash => binding.bash.as_str(),
    };
    let placeholder = match shell {
        IntegrationShell::Zsh => "__CMDGEN_ZSH_BINDING__",
        IntegrationShell::Bash => "__CMDGEN_BASH_BINDING__",
    };
    let script = shell
        .template()
        .replace("__CMDGEN_KEYBINDING_NAME__", &binding.canonical)
        .replace(placeholder, sequence);
    if script.contains("__CMDGEN_") {
        return Err(Error::ShellIntegrationError(
            i18n::t().err_template_incomplete.into(),
        ));
    }
    Ok(script)
}

fn rewrite_if_changed(path: &Path, contents: &str, mode: u32) -> Result<bool, Error> {
    if path.exists() && fsutil::read_to_string(path).ok().as_deref() == Some(contents) {
        return Ok(false);
    }
    fsutil::atomic_write(path, contents, mode)?;
    Ok(true)
}

fn read_optional(path: &Path) -> Result<String, Error> {
    if !path.exists() {
        return Ok(String::new());
    }
    fsutil::read_to_string(path).map_err(|err| {
        Error::ShellIntegrationError(fill(
            i18n::t().err_cannot_read,
            &[
                ("path", &path.display().to_string()),
                ("detail", &err.to_string()),
            ],
        ))
    })
}

fn write_rc(path: &Path, updated: &str) -> Result<bool, Error> {
    let created_backup = if path.exists() {
        let backup = backup_path(path);
        if !backup.exists() {
            std::fs::copy(path, &backup)
                .map_err(|err| Error::io(backup.display().to_string(), err))?;
            true
        } else {
            false
        }
    } else {
        false
    };
    fsutil::atomic_write(path, updated, 0o644)?;
    Ok(created_backup)
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rc".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{name}.cmdgen.bak"))
}

fn source_line(home: &Path, script: &Path) -> String {
    if let Ok(relative) = script.strip_prefix(home) {
        let relative = relative.to_string_lossy().replace('\\', "/");
        let safe = relative
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'));
        if safe {
            return format!("source \"$HOME/{relative}\"");
        }
    }
    format!(
        "source {}",
        shell_single_quote(&script.display().to_string())
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn reload_command(home: &Path, rc_path: &Path) -> String {
    if let Ok(relative) = rc_path.strip_prefix(home) {
        let relative = relative.to_string_lossy().replace('\\', "/");
        if relative.starts_with('.') && !relative.contains(' ') {
            return format!("source ~/{relative}");
        }
    }
    format!(
        "source {}",
        shell_single_quote(&rc_path.display().to_string())
    )
}

fn foreign_binding_warning(
    outside: &str,
    shell: IntegrationShell,
    binding: &KeyBinding,
) -> Option<String> {
    let needle = match shell {
        IntegrationShell::Zsh => binding.zsh.as_str(),
        IntegrationShell::Bash => binding.bash.as_str(),
    };
    let conflict = outside.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            return false;
        }
        match shell {
            IntegrationShell::Zsh => trimmed.contains("bindkey") && trimmed.contains(needle),
            IntegrationShell::Bash => trimmed.contains("bind") && trimmed.contains(needle),
        }
    });
    if conflict {
        Some(fill(
            i18n::t().binding_warning,
            &[("path", shell.rc_name()), ("binding", &binding.display)],
        ))
    } else {
        None
    }
}
