# cmdgen：AI Shell Command Generator --- 开发交接文档

> 文档类型：Implementation Handoff / 开发规格\
> 项目代号：`cmdgen`\
> 目标用户：macOS / Linux 上使用 POSIX sh、Bash、Zsh 的开发者\
> 首期核心：Shell 命令生成 CLI + Shell 输入行快捷键集成\
> 首选技术栈：Rust\
> 默认模型服务：DeepSeek 官方 API\
> 更新时间：2026-10-17

## 1. 项目目标

开发一个轻量、快速、易安装的独立命令行工具 `cmdgen`，只聚焦 ShellGPT
最有价值的两个功能：

1.  **Shell Command**：把自然语言转换为目标操作系统和 Shell
    对应的命令，并将命令打印到标准输出。
2.  **Shell Integration**：用户在 Zsh/Bash
    输入行中写下自然语言，按快捷键后将当前输入行交给
    `cmdgen`，再把生成结果填回输入行。用户可以检查、编辑，最后自行按
    Enter 执行。

典型用法：

``` bash
cmdgen "列出所有 docker 容器，包括已经停止的"
```

预期标准输出：

``` bash
docker ps -a
```

终端集成示例：

1.  用户在任意终端模拟器的 Zsh 输入行键入 `列出所有 docker 容器`。
2.  按默认快捷键 `Ctrl+G`。
3.  `cmdgen` 请求模型生成命令。
4.  输入行被替换为 `docker ps -a`。
5.  用户检查或编辑后自行按 Enter 执行。

终端模拟器只负责显示终端界面；输入行操作由当前 Shell 的行编辑机制实现。不得依赖任何特定终端模拟器的私有 API，也不依赖终端厂商提供的 AI 额度。

## 2. 项目名称与描述

项目名称：`cmdgen`

项目描述：

> cmdgen — AI-powered shell command generation for your terminal.

`cmdgen` 是一个独立、轻量、BYOK 的 AI Shell 命令生成工具。用户输入自然语言，工具生成适用于当前操作系统和 Shell 的命令；用户检查、编辑后自行执行。

正式发布前仍需核验 GitHub 仓库名、crates.io 包名、Homebrew Formula 名称及相关商标的可用性。
## 3. 核心原则

-   **单一职责**：只生成 Shell
    命令，不实现通用聊天、代码生成、REPL、Agent、MCP、工具调用或命令自动执行。
-   **BYOK**：用户自行配置 API Key。默认 DeepSeek，可配置其他
    OpenAI-compatible Chat Completions 接口。
-   **输出契约严格**：stdout 只输出命令；诊断和错误输出到
    stderr；失败时返回非零退出码。
-   **绝不自动执行**：程序只生成、打印或将命令填回 Shell
    输入行。不得通过 `eval`、`sh -c`、`system()` 等执行模型输出。
-   **Shell 原生集成**：Zsh 使用 ZLE；Bash 使用 Readline。Rust
    进程不负责模拟终端输入缓冲区。
-   **安装简单**：正式发布提供预编译二进制，不要求用户安装 Rust、Python
    或 Node.js。
-   **隐私优先**：默认只向模型发送必要的 OS、Shell
    和用户描述，不发送完整命令历史、环境变量或文件内容。
-   **不复制不必要的 ShellGPT
    功能**：参考其设计和公开行为，独立实现核心模块。若复制或改编其代码，必须遵守
    MIT License 的版权与许可要求。

## 4. 首期范围

### 4.1 P0：必须交付

-   Rust CLI，支持 `cmdgen "自然语言描述"`，可从 POSIX sh、Bash、Zsh 等 Shell 调用。
-   DeepSeek 官方 API 预设。
-   通用 OpenAI-compatible Chat Completions API 配置。
-   TOML 配置文件。
-   API Key 从环境变量或系统凭据存储读取。
-   交互式设置 API Key，不将 Key 写入普通 TOML 文件。
-   OS / Shell 类型识别。
-   专用 Shell Command system prompt。
-   纯命令输出、错误处理、超时和退出码。
-   Zsh 集成：快捷键替换当前输入行。
-   Bash 集成：快捷键替换当前输入行。
-   Shell 集成安装、卸载、重复安装保护。
-   `doctor` 诊断命令。
-   macOS Apple Silicon / Intel 和 Linux x86_64 / ARM64 预编译发布。
-   基本单元测试、集成测试、安装说明和安全说明。

### 4.2 P1：首期稳定后考虑

-   Fish 集成。
-   请求缓存（必须考虑 OS、Shell、模型、prompt
    和配置作为缓存键，默认可关闭）。
-   剪贴板快捷操作。
-   更多 Provider 预设。
-   可选地把当前工作目录作为上下文发送给模型。
-   模型响应时间统计。
-   更丰富的终端 UI。

### 4.3 明确不做

-   通用问答和聊天历史。
-   多轮对话 / REPL。
-   代码生成模式。
-   Agent Runtime、工具调用、MCP。
-   自动执行模型生成的命令。
-   将终端历史、环境变量、目录文件自动上传到模型。
-   不依赖任何特定终端模拟器。
-   强制要求用户购买任何特定终端或 AI 订阅。

## 5. 技术选型

使用 Rust stable，避免过度架构。

  组件                          建议用途
  ----------------------------- -----------------------------------
  `clap`                        CLI 参数与子命令
  `reqwest` + `rustls`          HTTPS 请求，首期采用 blocking API
  `serde` / `serde_json`        请求和响应 JSON
  `toml`                        用户配置
  `directories`                 跨平台配置目录定位
  `keyring`                     macOS Keychain 等系统凭据存储
  `anyhow` / `thiserror`        错误上下文与错误类型
  `assert_cmd` / `predicates`   CLI 集成测试（可选）
  `wiremock` 或等价库           HTTP API 测试 mock（可选）

实现时检查 crates.io 的当前稳定版本与
API，不要机械复制过时示例。首期不需要 Tokio、复杂异步架构或通用 LLM
SDK；一次短请求使用 `reqwest::blocking` 即可。

如果 `keyring` 在某些 Linux
桌面环境中不可用，必须提供明确提示，并确保环境变量方案仍然可用。Keychain
不可用不能阻止用户使用 `CMDGEN_API_KEY`。

## 6. 建议的项目结构

``` text
cmdgen/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── CHANGELOG.md
├── .github/
│   └── workflows/
│       ├── ci.yml
│       └── release.yml
├── src/
│   ├── main.rs
│   ├── cli.rs
│   ├── config/
│   │   ├── mod.rs
│   │   ├── file.rs
│   │   └── secrets.rs
│   ├── generator/
│   │   ├── mod.rs
│   │   ├── prompt.rs
│   │   ├── client.rs
│   │   └── output.rs
│   ├── shell/
│   │   ├── mod.rs
│   │   ├── detect.rs
│   │   └── integration.rs
│   └── doctor.rs
├── shell/
│   ├── cmdgen.zsh
│   └── cmdgen.bash
└── tests/
    ├── cli.rs
    ├── config.rs
    ├── generator.rs
    └── integration_install.rs
```

可以按实际实现精简文件数量，但必须保持以下职责边界：

-   `cli`：参数解析和命令分发。
-   `config`：合并配置、环境变量和凭据存储。
-   `generator`：Prompt 构造、HTTP 请求、响应解析与输出规范化。
-   `shell`：OS/Shell 检测及 Shell Integration 安装管理。
-   `doctor`：检查配置、凭据、API 连通性和 Shell 集成状态。

## 7. CLI 设计

### 7.1 核心用法

``` bash
cmdgen "列出所有 docker 容器"
cmdgen "查找当前目录下超过 100MB 的文件"
cmdgen "查看最近 200 行 nginx 容器日志"
cmdgen "查找当前目录下所有 Markdown 文件"
```

标准输出示例：

``` bash
docker ps -a
find . -type f -size +100M
docker logs --tail=200 nginx
find . -type f -name '*.md'
```

以上是示例结果，实际输出由模型决定。

### 7.2 子命令

  -------------------------------------------------------------------------------
  命令                                        说明
  ------------------------------------------- -----------------------------------
  `cmdgen "<prompt>"`                          生成命令并打印到 stdout

  `cmdgen config init`                         创建默认配置文件

  `cmdgen config show`                         显示有效配置，Key 必须脱敏

  `cmdgen config set-key`                      隐藏输入 API
                                              Key，并保存到系统凭据存储

  `cmdgen config delete-key`                   删除当前 Provider
                                              的已保存凭据，执行前确认

  `cmdgen integration install zsh`             安装 Zsh 集成

  `cmdgen integration install bash`            安装 Bash 集成

  `cmdgen integration uninstall [zsh\|bash]`   删除由 cmdgen 管理的集成代码

  `cmdgen doctor`                              检查配置、Key 来源、网络和集成状态

  `cmdgen --version`                           打印版本

  `cmdgen --help`                              显示帮助
  -------------------------------------------------------------------------------

可以补充 `--provider`、`--model`、`--base-url`、`--shell`、`--timeout`
等单次覆盖参数，但不要为了这些参数增加复杂的 Provider 插件系统。

CLI 必须支持通过 `--` 传入以 `-` 开头的自然语言描述。Shell 集成应使用
`command cmdgen -- "$prompt"`，避免调用同名 Shell 函数并规避参数歧义。

如果 Clap 的子命令解析和默认 prompt
参数产生冲突，应优先保证文档中的常见用法 `cmdgen "..."` 稳定可用，并测试
prompt 恰好为 `config`、`doctor` 等字符串时的行为。

## 8. 配置与 BYOK

### 8.1 配置文件

建议路径：

-   macOS / Linux：`~/.config/cmdgen/config.toml`。
-   使用 `directories` crate 确定平台配置目录；如采用 XDG 规范，应尊重
    `XDG_CONFIG_HOME`。

示例：

``` toml
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

**模型 ID 必须在实现时核对 DeepSeek 官方最新文档。** 上面的
`deepseek-flash`
是可编辑的示例默认值，不得假设它永远有效；如果已弃用，应更新默认值和
README。Provider、Base URL、模型 ID 必须允许用户自行配置。

配置文件不保存 API Key。配置目录权限应尽可能设为 `0700`，配置文件权限为
`0600`（平台允许时）。

### 8.2 API Key 获取顺序

推荐顺序：

1.  `CMDGEN_API_KEY`（通用显式覆盖）。
2.  对应 Provider 的环境变量，例如 DeepSeek 使用
    `DEEPSEEK_API_KEY`，OpenAI-compatible 服务可使用 `OPENAI_API_KEY`。
3.  系统凭据存储中按 Provider 命名空间保存的 Key。
4.  若均不存在，输出清晰的设置指引并返回非零状态。

不要将 API Key 写入 `config.toml`。`cmdgen config set-key`
使用隐藏输入读取 Key，保存到系统凭据存储。不得把 Key
放进命令行参数、Shell History、普通日志或错误消息。

系统凭据存储服务名建议使用 `cmdgen`，账号键使用 Provider 名称，例如
`deepseek`。对兼容接口，可允许用户指定凭据名称；首期也可简单地使用当前
`provider.name` 作为凭据账号。

### 8.3 环境变量

至少支持：

-   `CMDGEN_API_KEY`
-   `DEEPSEEK_API_KEY`
-   `OPENAI_API_KEY`
-   `CMDGEN_BASE_URL`
-   `CMDGEN_MODEL`
-   `CMDGEN_PROVIDER`
-   `CMDGEN_SHELL`
-   `CMDGEN_TIMEOUT`

环境变量优先级必须在 README 中明确。建议非敏感配置采用：CLI 参数 \>
环境变量 \> TOML \> 默认值；API Key 采用上面独立的凭据优先级。

## 9. LLM API Client

### 9.1 协议

首期统一使用 OpenAI-compatible **Chat Completions**：

``` http
POST {base_url}/chat/completions
Content-Type: application/json
Authorization: Bearer {api_key}
```

请求示例：

``` json
{
  "model": "deepseek-flash",
  "temperature": 0,
  "max_tokens": 256,
  "stream": false,
  "messages": [
    {
      "role": "system",
      "content": "You are a shell command generator..."
    },
    {
      "role": "user",
      "content": "列出所有 docker 容器"
    }
  ]
}
```

请求字段要依据实际 Provider
的当前文档确认。若某个兼容服务不接受可选参数，应提供清晰错误或最小兼容策略，不要在首期引入复杂的参数适配层。

响应主要读取 `choices[0].message.content`。处理缺失 choices、空
content、HTTP 非 2xx、超时、无效 JSON 和 API 错误对象等情况。

### 9.2 Base URL 规范

-   `base_url` 表示 API 前缀，而不是完整的 `/chat/completions` URL。
-   例如 DeepSeek 使用 `https://api.deepseek.com`，客户端拼接
    `/chat/completions`。
-   常规 OpenAI-compatible 服务可以使用
    `https://host.example/v1`，客户端拼接 `/chat/completions`。
-   去除末尾多余 `/`，防止拼接产生双斜线。
-   不要重复追加 `/v1`。
-   如果用户配置了看起来已经包含 `/chat/completions`
    的地址，应给出明确的配置错误提示，而不是静默生成错误 URL。

实现前核对 DeepSeek 官方最新 API
文档和模型名称。不得在文档或测试中把未经验证的模型 ID
当作永远有效的常量。

### 9.3 请求性能

-   首期使用非流式请求：收到完整命令后再输出或替换输入行。
-   默认超时 20 秒，可通过配置覆盖。
-   输出 token 上限保持较小，例如 256，避免不必要的长回复。
-   温度默认 0。
-   设置合理的 HTTP 超时，区分连接超时与整体请求超时。
-   不做多次自动重试，避免重复请求、额外费用和不可预期延迟。可仅对明确安全的瞬时连接错误进行一次可选重试，但首期建议不实现。
-   不在每次请求中执行额外的网络探测。
-   不将网络耗时误认为 Rust
    运行时性能；性能目标要分别测量冷启动与完整模型请求。

## 10. Prompt Builder 与输出规范化

### 10.1 Prompt 原则

System prompt 至少包含：

-   角色：Shell command generator。
-   当前 OS。
-   当前 Shell 类型。
-   只输出命令，不输出 Markdown、代码围栏或解释。
-   缺少细节时采用最合理的安全解释，不追问。
-   优先选择安全、非破坏性方案。
-   对删除、覆盖、权限变更等高风险操作，除非用户明确要求，否则优先给出只读检查或
    dry-run 命令。
-   多步骤操作可以组合为合法的多行命令或 `&&`
    链，但不要为了压缩而牺牲可读性。
-   永远不执行命令。

建议初始 Prompt：

``` text
You are a shell command generator.

Target operating system: {os}
Target shell: {shell}

Convert the user's request into a valid command for the target shell.

Rules:
- Output only the shell command.
- Do not include Markdown, code fences, or explanations.
- Do not ask follow-up questions. Choose the most reasonable interpretation.
- Prefer safe, non-destructive commands.
- For potentially destructive operations, prefer inspection or dry-run commands unless the user explicitly requests the destructive action.
- Never execute the generated command.
```

Prompt 应集中在一个模块中，方便测试与迭代。不要把整份 README
或其他不必要上下文发送给模型。

### 10.2 OS / Shell 识别

-   OS 使用 Rust 标准库平台常量，必要时提供人类可读名称。
-   Shell 自动识别优先读取 `CMDGEN_SHELL`，其次读取 `$SHELL` 的
    basename。
-   Shell Integration 调用时必须显式设置 `CMDGEN_SHELL=zsh` 或
    `CMDGEN_SHELL=bash`，因为 `$SHELL` 通常表示登录
    Shell，不一定是当前正在运行的 Shell。
-   支持 `--shell zsh` / `--shell bash` 覆盖识别结果。
-   不应为检测 Shell 而启动一个新的 Shell 进程。

### 10.3 输出处理

-   stdout 只包含命令文本，不包含 `Generated command:` 等前缀。
-   stderr 用于错误、诊断和可选的状态提示。
-   去掉响应首尾空白。
-   如果整个响应被单层 Markdown code fence 包围，可以去掉 fence
    与可选语言标记。
-   保留合法多行命令，不得只保留第一行。
-   不要通过简单的正则表达式删除所有注释或分号；这些可能是合法 Shell
    语法。
-   空响应必须报错。
-   对明显包含"Here is the
    command"等解释文本的响应，首期应报格式错误并提示重试，而不是静默猜测或拼接。
-   不能保证模型生成的命令在语义上正确；README
    必须提醒用户在执行前检查。

### 10.4 stdout / stderr / exit code 契约

  情况                 stdout   stderr                          退出码
  -------------------- -------- ------------------------------- --------
  成功生成             仅命令   默认无输出                      `0`
  缺少 Key             空       设置 Key 的明确说明             非零
  网络超时             空       超时信息                        非零
  HTTP/API 错误        空       Provider 错误摘要，不包含 Key   非零
  无效 JSON / 空响应   空       响应解析错误                    非零
  配置错误             空       指明错误配置项                  非零

不得将调试信息写到 stdout，因为用户可能会使用管道将输出传给其他命令。

## 11. Shell Integration 设计

### 11.1 总体行为

CLI 命令可从 POSIX sh、Bash、Zsh 等 Shell 调用；首期快捷键输入行集成支持 Bash 和 Zsh。POSIX sh 本身没有标准的交互式行编辑接口，因此不承诺在纯 sh 中实现输入行快捷键替换。


-   快捷键默认 `Ctrl+G`，必须允许修改。
-   当前输入为空或只有空白时，不发起请求。
-   请求期间可以显示轻量状态，但不能污染生成结果。
-   仅在命令生成成功且非空时替换输入缓冲区。
-   请求失败时保留原始输入。
-   替换后光标移动到命令末尾。
-   用户必须自行按 Enter 执行。
-   不允许 Integration 自动执行、自动接受或通过 `eval` 运行模型输出。

### 11.2 Zsh

使用 ZLE Widget：

-   从 `BUFFER` 获取原始输入。
-   使用 `command cmdgen -- "$prompt"` 调用 CLI。
-   调用时设置 `CMDGEN_SHELL=zsh`。
-   捕获 stdout 作为候选命令，捕获退出码。
-   成功且输出非空时更新 `BUFFER`，设置 `CURSOR=${#BUFFER}`。
-   失败时保持原 `BUFFER` 不变，并通过 stderr 显示简短错误。
-   正确处理 ZLE redraw 和终端状态，避免状态提示残留。
-   使用 `zle -N` 注册 Widget，并使用 `bindkey` 绑定快捷键。

### 11.3 Bash

使用 Readline：

-   从 `READLINE_LINE` 读取原始输入。
-   使用 `command cmdgen -- "$prompt"` 调用 CLI，并设置
    `CMDGEN_SHELL=bash`。
-   成功时写回 `READLINE_LINE`，设置
    `READLINE_POINT=${#READLINE_LINE}`。
-   失败时保留原输入行。
-   使用 `bind -x` 绑定快捷键。
-   处理 stderr 输出与 Readline 重绘，避免提示文本破坏终端显示。

### 11.4 安装器要求

推荐安装布局：

``` text
~/.config/cmdgen/
├── config.toml
├── integration.zsh
└── integration.bash
```

安装时：

1.  把由 cmdgen 管理的集成脚本写入 `~/.config/cmdgen/`。
2.  在 `~/.zshrc` 或 `~/.bashrc` 中增加带有唯一标记的 source 区块。
3.  重复执行安装时不得重复追加区块。
4.  如果文件已存在，先安全读取并原子写入；不要覆盖用户其他配置。
5.  卸载只删除 cmdgen
    自己的标记区块和管理的集成脚本，不得删除用户文件中其他内容。
6.  如需备份，使用明确的备份命名并避免每次重复安装生成大量备份。
7.  安装完成后打印需要执行的 reload 命令，例如 `source ~/.zshrc`。
8.  如果检测到快捷键已有绑定，应给出警告，并允许用户配置其他快捷键。
9.  所有文件操作都应处理权限错误、文件不存在和部分写入等异常。

建议使用标记：

``` bash
# >>> cmdgen managed integration >>>
source "$HOME/.config/cmdgen/integration.zsh"
# <<< cmdgen managed integration <<<
```

Bash 使用对应的 `integration.bash`。

实际脚本必须经真实 Zsh / Bash
测试；以上为安装设计约定，不是可以不经验证直接发布的完整实现。

## 12. 安全与隐私要求

-   默认永远不执行生成的命令。
-   禁止对生成结果使用 `eval`、`sh -c`、`bash -c`、`system()`
    等执行路径。
-   不提供默认开启的 `--execute` 功能。
-   API Key 不得写入日志、错误信息、终端状态行或普通配置文件。
-   隐藏输入 Key；不通过 `cmdgen config set-key <secret>` 这种容易写入
    Shell History 的方式设置。
-   默认不记录用户 prompt、生成结果或 API 响应到文件。
-   默认不上传当前目录、命令历史、环境变量或文件内容。
-   仅使用 HTTPS；不得提供关闭 TLS 证书验证的选项。
-   错误信息可显示 HTTP 状态码与 Provider 返回的安全摘要，但必须过滤
    Authorization Header 和 Key。
-   配置与凭据文件尽量采用最小文件权限。
-   命令生成结果只是建议，不保证正确；README 明确要求用户在执行前审查。
-   对可能删除数据、覆盖文件、修改权限、执行远程脚本或影响生产环境的命令，应在
    Prompt 中强调安全优先，但不要宣称 Prompt 可以完全消除风险。

## 13. 错误处理

错误类型至少包括：

-   `ConfigNotFound` / `InvalidConfig`
-   `MissingApiKey`
-   `CredentialStoreUnavailable`
-   `UnsupportedProvider`
-   `InvalidBaseUrl`
-   `NetworkError`
-   `Timeout`
-   `HttpError`
-   `ApiError`
-   `InvalidResponse`
-   `EmptyCommand`
-   `ShellIntegrationError`

要求：

-   错误提示用清晰中文或简洁英文，首期统一中文也可以，但 README 需说明。
-   不输出 Rust backtrace，除非用户显式开启 debug。
-   `--verbose` 可以输出请求目标、耗时、模型 ID 等诊断信息，但不得输出
    Key；默认不得打印完整 prompt。
-   退出码非零。
-   不对 API 错误无限重试。

## 14. 测试策略

### 14.1 单元测试

至少覆盖：

-   默认配置构造与 TOML 解析。
-   CLI 参数、`--` 和自然语言 prompt 解析。
-   API Key 优先级。
-   Base URL 规范化和 endpoint 拼接。
-   Prompt 中 OS / Shell 注入正确。
-   空响应、围栏响应、多行命令输出处理。
-   API 错误与无效 JSON 处理。
-   stderr / stdout 分离。
-   配置脱敏。

### 14.2 集成测试

-   使用 mock HTTP 服务验证请求 URL、Header、JSON Body 和错误处理。
-   不需要真实 DeepSeek Key 的 CI 测试。
-   CLI 成功时 stdout 只有命令，退出码为 0。
-   API 失败时 stdout 为空、stderr 有诊断信息、退出码非零。
-   Shell 集成安装重复运行不会产生重复区块。
-   卸载只移除 cmdgen 管理的区块。
-   Zsh/Bash 集成请求失败时不会覆盖原始输入。
-   确认生成的命令永远不会被工具执行。

### 14.3 手工验收

macOS：

-   Apple Silicon 和 Intel（如果可用）分别测试。
-   任意终端环境 +
    Zsh：直接命令、中文输入行、快捷键替换、失败后保留输入、手工编辑后执行。
-   Bash：同样测试。
-   API Key 从 Keychain 和环境变量读取。
-   无 Key、错误 Key、网络断开、超时、HTTP 429、HTTP 5xx。
-   配置路径和文件权限。
-   安装、重复安装、卸载与重新安装。

Linux：

-   x86_64 与 ARM64 二进制启动。
-   Zsh/Bash 集成。
-   没有 Secret Service 时，环境变量方式仍可使用。
-   XDG 配置目录生效。

## 15. 性能目标

性能指标必须区分本地程序开销与模型请求耗时：

  -----------------------------------------------------------------------
  指标                                初始目标
  ----------------------------------- -----------------------------------
  本地冷启动（不发请求）              macOS P95 ≤ 50 ms，CI
                                      可做基准但不作跨机器绝对保证

  请求方式                            非流式，拿到完整结果后再替换输入

  默认超时                            20 秒，可配置

  默认输出上限                        256 tokens，可配置

  重复请求缓存                        P1，默认不启用

  正常模型请求延迟                    记录实测数据；受网络和 Provider
                                      影响，不承诺固定 SLA
  -----------------------------------------------------------------------

首期不应为降低几十毫秒而引入守护进程。优先保持架构简单、可预测。只有真实测量表明进程启动成本明显影响体验后，再考虑
daemon 或本地 socket。

## 16. 发布与安装

### 16.1 发布产物

首期目标：

-   `aarch64-apple-darwin`
-   `x86_64-apple-darwin`
-   `x86_64-unknown-linux-gnu`
-   `aarch64-unknown-linux-gnu`

发布前确认所选 TLS、Keychain 和其他依赖在目标平台可编译。对 Linux ARM64
/ x86_64 需确认所需 glibc 版本；如果目标是更广泛的 Linux
兼容性，单独评估 musl 构建。

Windows 暂不列入首期发布范围。

### 16.2 安装方式

-   本地开发：`cargo install --path .`
-   正式用户：GitHub Releases 预编译二进制。
-   macOS：提供 Homebrew Formula；初期可先通过 tap 分发。
-   安装 CLI 与安装 Shell Integration 分离，避免用户安装程序后自动修改
    `.zshrc` / `.bashrc`。

README 应提供从零开始的安装、API Key 设置、命令生成、Shell
集成、卸载和故障排查步骤。

## 17. 开发里程碑

### Milestone 1：CLI 最小闭环

-   创建 Rust 项目和 CI。
-   完成默认配置与环境变量。
-   完成 DeepSeek / OpenAI-compatible Chat Completions。
-   实现 `cmdgen "<prompt>"`。
-   stdout / stderr / exit code 契约稳定。
-   加入 mock API 测试。

**验收：** 在 macOS 的任意终端环境中运行
`cmdgen "列出所有 docker 容器"`，返回纯命令；不配置 Key
时有清晰提示；任何情况下都不执行输出。

### Milestone 2：配置与凭据

-   TOML 配置。
-   Keychain / keyring 集成。
-   `config init`、`config show`、`config set-key`、`config delete-key`。
-   环境变量优先级。
-   `doctor`。

**验收：** Key 不出现在 TOML、Shell History
或日志中；环境变量方式不依赖系统凭据服务。

### Milestone 3：Zsh Integration

-   ZLE Widget。
-   可配置快捷键，默认 `Ctrl+G`。
-   成功替换输入行，失败保留原输入。
-   安装 / 卸载 / 幂等管理。

**验收：** 在 macOS 的任意终端模拟器中使用 Zsh，可从自然语言输入行触发命令生成；用户必须手工确认执行。

### Milestone 4：Bash Integration

-   Readline Widget。
-   安装 / 卸载 / 幂等管理。
-   与 Zsh 共用 CLI 和配置。

**验收：** Linux/macOS Bash 下同样可用，失败不破坏输入行。

### Milestone 5：发行与文档

-   多平台 CI 构建。
-   GitHub Releases。
-   Homebrew Formula（可作为首期后半段）。
-   完整 README、故障排查、安全说明。
-   手工验收矩阵。

## 18. 编码 Agent 的实施要求

请按以下原则推进开发：

1.  先检查仓库现状；若仓库为空，初始化 Rust
    项目。不要假设已有技术栈或文件。
2.  开发前核对当前 DeepSeek 官方 API 文档与可用模型 ID，并核对所选 Rust
    crate 的当前 API。
3.  按里程碑实现，优先完成 Milestone 1
    的端到端可运行闭环，不要一开始就实现所有 P1 功能。
4.  每个里程碑都要添加测试，并运行格式化、静态检查、单元测试与集成测试。
5.  不要将 API Key、真实用户目录或个人配置写入测试、日志和仓库。
6.  任何模型输出都不得由程序自动执行。
7.  Shell Integration
    安装器只能编辑带有明确管理标记的代码区块，不得覆盖用户原有 Shell
    配置。
8.  对不确定的 API 兼容性、跨平台凭据行为和 Shell Readline/ZLE
    行为，先验证再实现，不要猜测。
9.  README 中所有命令必须与实际 CLI 参数保持一致。
10. 每个里程碑结束时，给出完成项、测试结果、已知限制和下一步计划。

## 19. 官方参考

-   ShellGPT
    README：<https://github.com/TheR1D/shell_gpt/blob/main/README.md>
-   ShellGPT Shell
    Integration：<https://github.com/TheR1D/shell_gpt/blob/main/sgpt/integration.py>
-   ShellGPT Shell Command
    Prompt：<https://github.com/TheR1D/shell_gpt/blob/main/sgpt/role.py>
-   ShellGPT
    CLI：<https://github.com/TheR1D/shell_gpt/blob/main/sgpt/app.py>
-   ShellGPT MIT
    License：<https://github.com/TheR1D/shell_gpt/blob/main/LICENSE>
-   DeepSeek API 文档：<https://api-docs.deepseek.com/>
-   Rust：<https://www.rust-lang.org/>
-   Clap：<https://docs.rs/clap/latest/clap/>
-   Reqwest：<https://docs.rs/reqwest/latest/reqwest/>
-   Keyring：<https://docs.rs/keyring/latest/keyring/>

------------------------------------------------------------------------

## 最终产品定义

`cmdgen` 是一个独立、轻量、BYOK 的 AI Shell 命令生成器：

-   `cmdgen "自然语言"` 输出 Shell 命令。
-   Zsh/Bash 快捷键将当前输入行替换为生成结果。
-   默认 DeepSeek，兼容 OpenAI Chat Completions。
-   Rust 原生二进制，易安装。
-   不执行命令，不依赖终端厂商提供的 AI 额度，不包含无关 Agent 功能。

以这四项作为首期产品边界，优先交付可靠、快速、可安装的闭环。
