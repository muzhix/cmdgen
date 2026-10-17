#![allow(dead_code)]

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct Sandbox {
    pub dir: TempDir,
    pub home: PathBuf,
    pub config_home: PathBuf,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let home = dir.path().join("home");
        let config_home = home.join(".config");
        std::fs::create_dir_all(&config_home).expect("config home");
        Self {
            dir,
            home,
            config_home,
        }
    }

    pub fn command(&self) -> Command {
        let mut cmd = Command::cargo_bin("cmdgen").expect("cmdgen binary");
        cmd.current_dir(self.dir.path());
        cmd.env("HOME", &self.home);
        cmd.env("XDG_CONFIG_HOME", &self.config_home);
        cmd.env("SHELL", "/bin/zsh");
        for key in [
            "CMDGEN_API_KEY",
            "DEEPSEEK_API_KEY",
            "OPENAI_API_KEY",
            "DASHSCOPE_API_KEY",
            "CMDGEN_BASE_URL",
            "CMDGEN_MODEL",
            "CMDGEN_PROVIDER",
            "CMDGEN_SHELL",
            "CMDGEN_TIMEOUT",
            "CMDGEN_DEBUG",
        ] {
            cmd.env_remove(key);
        }
        cmd.timeout(std::time::Duration::from_secs(20));
        cmd
    }

    pub fn config_dir(&self) -> PathBuf {
        self.config_home.join("cmdgen")
    }
}

pub fn assert_no_secret(output: &str, secret: &str) {
    assert!(!output.contains(secret), "输出泄露了凭据: {output}");
}

pub fn write_executable(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
