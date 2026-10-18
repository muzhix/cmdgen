# Changelog

## Unreleased

## 0.1.5 - 2026-10-18

- `cmdgen config provider` 菜单最后一项可以新增 provider。依次输入名称、base_url 和 model，API Key 可以留空跳过，并且不会写入配置文件。
- `cmdgen config provider` 取代 `config set`、`config set-key` 和 `config delete-key`。在终端里选择 provider 后编辑 model、base_url 或 API Key，改完可以继续编辑其他项。
- `cmdgen config lang` 用上下键选择界面语言，确认后把 `language` 写入配置文件。
- 界面语言默认改为英文。可用 `--lang`、`CMDGEN_LANG` 或配置文件里的 `language` 在中英文之间切换。
- README 增加 logo、发布与平台徽章，以及用真实二进制和 Zsh 集成录制的英文终端演示。
- 提供 `install.sh`，可以用 `curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash` 安装预编译二进制。
- 配置文件可以保存多家 OpenAI 兼容 provider，并用 `cmdgen config use` 或 `--provider` 切换。
- 内置预设为 DeepSeek 和阿里云百炼，默认启用 DeepSeek。

## 0.1.0 - 2026-10-17

- 提供 `cmdgen "<描述>"`，把自然语言转成当前操作系统和 Shell 的命令，并只打印到 stdout。
- 默认使用 DeepSeek 官方 Chat Completions（模型 `deepseek-flash`），也可配置其他 OpenAI-compatible 接口。
- 支持 TOML 配置、环境变量和系统凭据存储。API Key 不写入 `config.toml`；凭据存储不可用时写入权限为 0600 的 `credentials.toml`。Linux 只使用 Secret Service。
- 提供 `config`、`doctor`、Zsh/Bash 输入行集成的安装与卸载。
- 程序不会执行模型返回的命令。
