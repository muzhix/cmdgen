# cmdgen

cmdgen — AI-powered shell command generation for your terminal.

`cmdgen` 把自然语言转换成当前操作系统和 Shell 可用的命令。它是一个独立、轻量、BYOK 的命令行工具：默认调用 DeepSeek 官方 API，也可以改成任何 OpenAI-compatible Chat Completions 接口。

生成结果只打印到标准输出，或在 Zsh/Bash 里填回输入行。**cmdgen 不会执行生成的命令。** 请先阅读、按需修改，再自己按 Enter。

错误和诊断使用中文，写到标准错误。

## 安装

本地开发：

```bash
cargo install --path .
```

正式发布提供这些预编译目标：

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

GitHub Release 会在打上 `v*` 标签后由 `.github/workflows/release.yml` 构建。macOS 也可以用 `packaging/homebrew/cmdgen.rb` 作为 Homebrew tap 模板；发布前需要填入真实的下载地址和 sha256。安装二进制不会自动修改 `~/.zshrc` 或 `~/.bashrc`。

## 配置 API Key

API Key 不会写入 `config.toml`，也不会通过命令行参数传入。

```bash
cmdgen config init
cmdgen config set-key
```

`set-key` 会隐藏输入。系统凭据存储可用时，Key 保存到服务名 `cmdgen`、账号为当前 provider 名称的条目。macOS 使用登录钥匙串；Linux 使用 Secret Service。凭据存储不可用时，`set-key` 写入 `credentials.toml`（与 `config.toml` 同目录，权限 `0600`）。关掉终端后再打开仍然有效。同一用户下的其他进程可以读取该文件。容器和 CI 不依赖这个文件，请在每次启动时注入 `CMDGEN_API_KEY`。

删除已保存的凭据：

```bash
cmdgen config delete-key
```

非交互环境需要显式确认：`cmdgen config delete-key --yes`。

也可以不运行 `set-key`，直接导出环境变量，见下文优先级。

## 生成命令

```bash
cmdgen "列出所有 docker 容器，包括已经停止的"
cmdgen "查找当前目录下超过 100MB 的文件"
```

成功时标准输出只有命令，例如：

```bash
docker ps -a
```

选项要写在描述前面：

```bash
cmdgen --shell zsh --model deepseek-flash "查看 git 状态"
cmdgen -- --config
cmdgen -- "-n 列出当前目录"
```

`--` 后面的内容全部当作描述。因此 `cmdgen -- config` 会生成命令，而 `cmdgen config` 是配置子命令。

其他命令：

```bash
cmdgen config show
cmdgen doctor
cmdgen --version
cmdgen --help
```

单次覆盖参数：`--provider`、`--model`、`--base-url`、`--shell`、`--timeout`、`--verbose`。`--verbose` 只向 stderr 输出模型、接口和耗时，不输出 API Key，也不输出完整 prompt。

## 配置优先级

配置文件位于：

- 设置了 `XDG_CONFIG_HOME` 时：`$XDG_CONFIG_HOME/cmdgen/config.toml`
- 否则：`~/.config/cmdgen/config.toml`

目录权限尽量为 `0700`，配置文件为 `0600`。

非敏感配置的优先级：

1. 命令行参数
2. 环境变量
3. TOML
4. 内置默认值

环境变量：

| 变量 | 作用 |
| --- | --- |
| `CMDGEN_PROVIDER` | provider 名称 |
| `CMDGEN_BASE_URL` | API 前缀，不要包含 `/chat/completions` |
| `CMDGEN_MODEL` | 模型 ID |
| `CMDGEN_SHELL` | 目标 shell。集成脚本会设置为 `zsh` 或 `bash` |
| `CMDGEN_TIMEOUT` | 超时秒数 |

Shell 识别顺序：`--shell` > `CMDGEN_SHELL` > 配置里的 `shell.default` > `$SHELL` 的文件名 > `sh`。`auto` 表示继续向下识别。检测不会启动新的 Shell 进程。

API Key 使用单独的顺序：

1. `CMDGEN_API_KEY`
2. DeepSeek 使用 `DEEPSEEK_API_KEY`；其他 provider 使用 `OPENAI_API_KEY`
3. 系统凭据存储中、账号等于 provider 名称的凭据
4. 本地凭据文件 `credentials.toml` 中、同一 provider 的凭据

默认配置：

```toml
[provider]
name = "deepseek"
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[generation]
temperature = 0.0
max_tokens = 256
timeout_secs = 20

[shell]
default = "auto"
keybinding = "ctrl-g"
include_cwd = false
```

`deepseek-flash` 于 2026-10-17 对照 DeepSeek 官方文档核对。它不是永久有效的常量；如果官方更换模型名，请修改配置和这里的说明。DeepSeek 默认开启思考模式，cmdgen 在 provider 为 `deepseek` 时发送 `thinking.type=disabled`，以便直接返回命令。

`base_url` 是 API 前缀。客户端只拼接一次 `/chat/completions`，不会重复追加 `/v1`。非本机地址必须使用 HTTPS，且不能关闭证书校验。`127.0.0.1` 和 `localhost` 允许 HTTP，便于连接本机兼容服务。

`shell.include_cwd` 默认为 `false`。只有设为 `true` 时，请求才会附带当前工作目录路径，不会上传目录内容、历史或环境变量。

OpenAI 预设只是一个可改的起点（`https://api.openai.com/v1`，模型 `gpt-4o-mini`）。自定义服务只要写上 `name`、`base_url` 和 `model` 即可，不需要插件系统。

## Shell 集成

默认快捷键是 `Ctrl+G`，可在配置里用 `shell.keybinding = "ctrl-x"` 修改，然后重新安装。

```bash
cmdgen shell install zsh
source ~/.zshrc

cmdgen shell install bash
source ~/.bashrc
```

重复安装不会追加第二段托管代码。卸载只删除 cmdgen 的标记区块和它自己的脚本：

```bash
cmdgen shell uninstall zsh
cmdgen shell uninstall bash
cmdgen shell uninstall
```

标记形如：

```bash
# >>> cmdgen managed integration >>>
source "$HOME/.config/cmdgen/integration.zsh"
# <<< cmdgen managed integration <<<
```

集成行为：

- 输入为空或只有空白时不发请求。
- 生成期间在输入行末尾转圈。此时再按任意键会取消请求，并保留原始输入；这个按键不会插入到输入行。
- Zsh 在结果是单行命令时不改已输入的文字，把建议显示成 ghost text。`Tab` 或右方向键接受，光标移到末尾。继续输入并偏离建议时 ghost 消失。建议还在时再按一次快捷键会重新生成。Enter 执行的是已经打出来的文字，不是 ghost。`rm`、`kill` 这类破坏性命令会标成红色并带 `⚠`，接受前不会进入输入行。不完整、会让 Shell 继续等待输入的建议会被丢弃。多行结果直接写入输入行。
- Bash 在成功且结果非空时替换当前输入行，光标移到末尾。没有 ghost text。
- 失败时保留原始输入。
- 不会 `eval`，也不会自动按 Enter。

要把单词 `shell` 当作描述，请写成 `cmdgen -- shell`。颜色可以用 `CMDGEN_HIGHLIGHT` 和 `CMDGEN_HIGHLIGHT_DESTRUCTIVE` 覆盖，格式与 Zsh 的 `region_highlight` 相同。

Bash 集成使用 `bind -x`，需要 Bash 4 或更高版本。macOS 自带的 Bash 3.2 不能绑定快捷键；可以用 Homebrew 安装新版 Bash。Zsh 使用 ZLE，不依赖特定终端模拟器。

## 诊断

```bash
cmdgen doctor
```

它检查配置、Key 来源、凭据存储、API 连通性和集成文件。输出里的 Key 会脱敏。存在失败项时退出码为 1。网络检查只访问 `base_url`，不发送 API Key，也不发起补全请求。

## 退出契约

| 情况 | stdout | stderr | 退出码 |
| --- | --- | --- | --- |
| 成功 | 只有命令 | 默认无输出 | 0 |
| 缺少 Key、超时、HTTP/API 错误、响应无法解析、配置错误 | 空 | 中文说明，不含 Key | 非零 |

调试栈信息只在设置 `CMDGEN_DEBUG` 时出现。

## 安全

- 不执行模型输出，没有 `--execute`。
- 不把 Key 写入 `config.toml`、日志、错误信息或 Shell 历史。不要运行 `cmdgen config set-key <密钥>`。系统凭据存储不可用时，Key 只写入权限为 `0600` 的 `credentials.toml`。
- 默认不记录 prompt、响应或命令历史，也不上传环境变量和文件内容。
- 只使用 HTTPS。远程地址不能关闭 TLS 证书校验。
- 提示词会要求模型优先给出非破坏性命令。这不能消除风险，执行前必须自己检查。
- 删除、覆盖、改权限、拉取远程脚本或影响生产环境的命令尤其要人工确认。

## 故障排查

- **未找到 API Key**：运行 `cmdgen config set-key`，或导出 `CMDGEN_API_KEY`。没有钥匙串或 Secret Service 时，`set-key` 会把 Key 写入 `credentials.toml`。只在当前终端临时使用时，导出环境变量即可，关闭终端后失效。
- **base_url 无效**：去掉末尾的 `/chat/completions`，并确认远程地址是 `https://`。
- **请求超时**：增大 `generation.timeout_secs` 或设置 `CMDGEN_TIMEOUT`。默认 20 秒，不会自动重试。
- **返回解释文字**：cmdgen 会拒绝明显的说明文本。把需求写得更具体后重试。
- **Bash 没有反应**：确认 `bash --version` 至少是 4，并且已经 `source ~/.bashrc`。
- **快捷键冲突**：安装时如果发现已有绑定会警告。把 `shell.keybinding` 改成其他 `ctrl-<字母>` 后重新安装。

## 开发

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

测试使用本地 mock，不需要真实的 DeepSeek Key。

手工验收时建议覆盖：直接命令、中文输入、快捷键替换、失败后保留输入、编辑后再执行、环境变量和钥匙串两种 Key、无 Key、错误 Key、断网、超时、HTTP 429/5xx、重复安装和卸载。Apple Silicon、Intel、Linux x86_64 和 Linux ARM64 都要分别看二进制能否启动。

## 已知限制

- 不做通用聊天、多轮对话、代码生成、Agent、工具调用或 MCP。
- 不支持 Fish，也不承诺纯 POSIX sh 的输入行快捷键。
- 不缓存请求，也不提供常驻守护进程。
- Windows 不在本版本的发布范围内。
- 模型输出只是建议，语法和语义都可能是错的。
