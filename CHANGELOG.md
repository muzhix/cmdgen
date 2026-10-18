# Changelog

## Unreleased

- README 增加 logo、发布与平台徽章，以及用真实二进制和 Zsh 集成录制的英文终端演示。
- 提供 `install.sh`，可以用 `curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash` 安装预编译二进制。
- 配置文件可以保存多家 OpenAI 兼容 provider，并用 `cmdgen config use` 或 `--provider` 切换。
- 内置预设为 DeepSeek 和阿里云百炼，默认启用 DeepSeek。
- `cmdgen config set` 可以把 provider 的 `model` 和 `base_url` 写入配置文件。

## 0.1.0 - 2026-10-17

- 提供 `cmdgen "<描述>"`，把自然语言转成当前操作系统和 Shell 的命令，并只打印到 stdout。
- 默认使用 DeepSeek 官方 Chat Completions（模型 `deepseek-flash`），也可配置其他 OpenAI-compatible 接口。
- 支持 TOML 配置、环境变量和系统凭据存储。API Key 不写入 `config.toml`；凭据存储不可用时写入权限为 0600 的 `credentials.toml`。Linux 只使用 Secret Service。
- 提供 `config`、`doctor`、Zsh/Bash 输入行集成的安装与卸载。
- 程序不会执行模型返回的命令。
