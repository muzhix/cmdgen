<p align="center">
  <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <img src="docs/logo.svg" width="88" alt="cmdgen">
</p>

<h1 align="center">cmdgen</h1>

<p align="center">
  <strong>Turn a sentence into a command for the current shell.</strong><br>
  AI-powered shell command generation for your terminal.
</p>

<p align="center">
  <a href="https://github.com/muzhix/cmdgen/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/muzhix/cmdgen/ci.yml?branch=main&style=flat-square&label=ci"></a>
  &nbsp;
  <a href="https://github.com/muzhix/cmdgen/releases/latest"><img alt="Release" src="https://img.shields.io/github/v/release/muzhix/cmdgen?style=flat-square&color=C9842A&label=release"></a>
  &nbsp;
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/github/license/muzhix/cmdgen?style=flat-square&color=5C6B7A"></a>
  &nbsp;
  <img alt="Platform" src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-3E4C59?style=flat-square">
  &nbsp;
  <img alt="Rust 1.85+" src="https://img.shields.io/badge/rust-1.85%2B-C9842A?style=flat-square&logo=rust&logoColor=white">
</p>

<p align="center">
  <img src="docs/demo-shell.gif" width="928" alt="In Zsh, type a request and press Ctrl+G. The suggestion appears as gray ghost text. Tab accepts it. A destructive suggestion is shown in red with a warning and is not executed.">
</p>

`cmdgen` turns a sentence of natural language into a command the current operating system and shell can run. It is a standalone, lightweight, bring-your-own-key tool: it calls DeepSeek by default, and can be pointed at any OpenAI-compatible Chat Completions endpoint.

The result is printed to standard output, or written back into the Zsh / Bash input line. Read it, edit it if you need to, then press Enter yourself. cmdgen does not run the generated command.

The interface language defaults to English. Errors and diagnostics go to standard error and follow the current interface language.

## Language

These choices apply only to this run and are not written back to the config:

```bash
cmdgen --lang zh "list every docker container"
cmdgen --lang en --help
```

`en-US` is treated as `en`. `zh-CN` and `zh-Hans` are treated as `zh`. The `CMDGEN_LANG` environment variable works the same way. A persistent default lives in `language` in `config.toml`. Priority:

1. `--lang`
2. `CMDGEN_LANG`
3. `language`
4. built-in `en`

Ctrl+G invokes cmdgen without `--lang`, so the shortcut path uses `language` from the config file. `cmdgen config init` writes `language`: the value of `--lang` or `CMDGEN_LANG` when one of those is set, otherwise `en`. An unknown language code is reported in English, with the supported codes listed.

## What it does

- Turns a description into one command. Standard output is only that command, so it can be piped.
- Zsh shows the suggestion as ghost text. `Tab` or Right accepts it. Bash replaces the current input line on success. The default shortcut is `Ctrl+G`.
- You keep the API key. It does not go into `config.toml` or command-line arguments.
- Prebuilt binaries for macOS and Linux. Rust is not required.

It does not chat, remember a conversation, generate code, or act as an agent, and it does not do tool calling or MCP. It does not read command history, environment variables, or files in the directory. An input-line shortcut for Fish or pure POSIX sh is out of scope.

## Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
```

The script picks the archive for the current system from the [GitHub Release](https://github.com/muzhix/cmdgen/releases/latest), checks sha256 when the release includes `SHA256SUMS`, and installs into a directory that is already on `PATH` and writable by the current user, preferring `~/.local/bin`. It does not edit `~/.zshrc` or `~/.bashrc`.

Pin a version or directory:

```bash
curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --version v0.1.2
CMDGEN_BIN_DIR="$HOME/.local/bin" curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
```

Without `--version`, it installs the latest release. If the install directory is not on `PATH`, the script prints the line to add to a startup file. It does not write that line itself.

Remove only the binary. Config and credentials stay:

```bash
curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash -s -- --uninstall
```

If a startup file still has the shell integration, uninstall stops and tells you to run `cmdgen shell uninstall` first.

You can also download the archive for your platform from the release and put the `cmdgen` binary on `PATH` yourself:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Windows is not in the release set. See [DEV.md](DEV.md) to build from source.

## Quick start

```bash
cmdgen config init
cmdgen config set-key
cmdgen "list every docker container, including stopped ones"
```

`set-key` hides input. When the system credential store is available, the key is saved under the service name `cmdgen` with the current provider name as the account. macOS uses the login keychain. Linux uses Secret Service. When the credential store is unavailable, it writes `credentials.toml` in the same directory (mode `0600`). The key remains after you close the terminal. Other processes of the same user can read that file. Containers and CI should inject `CMDGEN_API_KEY` on each start and not depend on this file.

On success, standard output is only the command, for example `docker ps -a`.

To use it on the input line, install the integration:

```bash
cmdgen shell install zsh
source ~/.zshrc
```

Then type natural language and press `Ctrl+G`.

## Generate directly

<p align="center">
  <img src="docs/demo-cli.gif" width="863" alt="Three cmdgen prompts. stdout is only docker ps -a, find . -type f -size +100M, and docker logs --tail 200 nginx.">
</p>

```bash
cmdgen "list all docker containers, including stopped ones"
cmdgen "find files larger than 100MB"
cmdgen "show the last 200 lines of the nginx logs"
```

The commands in the recording above are example results. Actual output depends on the model. In an interactive terminal, standard error shows a spinner while waiting, then clears it on success or failure. Redirecting or piping standard error hides the spinner. On success, standard error is still empty.

Options go before the description:

```bash
cmdgen --shell zsh --model deepseek-flash "show git status"
cmdgen -- --config
cmdgen -- "-n list the current directory"
```

Everything after `--` is the description. `cmdgen -- config` generates a command, while `cmdgen config` is the config subcommand.

One-shot overrides: `--provider`, `--model`, `--base-url`, `--shell`, `--timeout`, `--verbose`. Except for `--model` and `--base-url` saved by `config set`, these affect only this run. `--verbose` prints the model, endpoint, and elapsed time to stderr. It does not print the API key or the full prompt.

## Commands

| Command | Effect |
| --- | --- |
| `cmdgen "<description>"` | Generate a command and print it to stdout |
| `cmdgen config init` | Create the default config file |
| `cmdgen config show` | Show the effective config, with the key redacted |
| `cmdgen config providers` | List saved providers and mark the active one |
| `cmdgen config use <name>` | Write the active provider to the config file |
| `cmdgen config set` | Write `--model` or `--base-url` into the config |
| `cmdgen config set-key` | Read an API key without echoing it |
| `cmdgen config delete-key` | Delete the current provider's saved credential |
| `cmdgen shell install zsh\|bash` | Install input-line integration |
| `cmdgen shell uninstall [zsh\|bash]` | Remove the integration cmdgen manages |
| `cmdgen doctor` | Check config, key source, network, and integration |
| `cmdgen --version` / `--help` | Version and help |

Non-interactive deletion needs an explicit confirmation: `cmdgen config delete-key --yes`.

`config set` can take just `--model` or just `--base-url`. When `--provider` is omitted, it updates the current `active` entry and does not switch the active provider. `CMDGEN_MODEL` and `CMDGEN_BASE_URL` are not written.

Save a key for one provider: `cmdgen config set-key --provider bailian`. A one-shot switch does not edit the file: `cmdgen --provider bailian "list large files"`. Options such as `--provider` go after `config`; if they come first, `config` is treated as the description.

## Configuration

The config file is:

- `$XDG_CONFIG_HOME/cmdgen/config.toml` when `XDG_CONFIG_HOME` is set
- otherwise `~/.config/cmdgen/config.toml`

The directory mode is `0700` when possible, and the config file is `0600`. The API key is not written to this file.

Priority for non-secret settings:

1. command-line flags
2. environment variables
3. TOML
4. built-in defaults

| Variable | Effect |
| --- | --- |
| `CMDGEN_API_KEY` | Generic API key. It wins over the credential sources below |
| `CMDGEN_PROVIDER` | provider name |
| `CMDGEN_BASE_URL` | API prefix; do not include `/chat/completions` |
| `CMDGEN_MODEL` | model id |
| `CMDGEN_SHELL` | target shell. The integration script sets this to `zsh` or `bash` |
| `CMDGEN_TIMEOUT` | timeout in seconds |
| `CMDGEN_LANG` | interface language, `en` or `zh` |
| `DEEPSEEK_API_KEY` | DeepSeek key |
| `DASHSCOPE_API_KEY` | Alibaba Cloud Bailian key |

Shell resolution order: `--shell` > `CMDGEN_SHELL` > `shell.default` in the config > the filename of `$SHELL` > `sh`. `auto` means keep resolving. Detection does not start a new shell process.

Which provider is selected:

1. `--provider`
2. `CMDGEN_PROVIDER`
3. `active` in the config; a legacy single `[provider]` uses its `name`
4. built-in `deepseek`

After selection, that profile's own `base_url` and `model` are used. `--base-url`, `CMDGEN_BASE_URL`, `--model`, and `CMDGEN_MODEL` can still override those fields for this run. The built-in presets are `deepseek` and `bailian`; their address and model can be omitted. Any other name must set both fields in `[providers.<name>]`, or both must be given on the command line. With no config file, the default is `deepseek`.

The API key uses a separate order:

1. `CMDGEN_API_KEY`
2. that profile's `api_key_env`. When unset, `deepseek` uses `DEEPSEEK_API_KEY` and `bailian` uses `DASHSCOPE_API_KEY`. A new `[providers.<name>]` does not fall through to `OPENAI_API_KEY`
3. the system credential store entry whose account equals the provider name
4. the same provider's entry in the local credential file `credentials.toml`

A legacy single `[provider]` whose name is not `deepseek` still uses `OPENAI_API_KEY` at step 2.

Default config written by `cmdgen config init`:

```toml
language = "en"
active = "deepseek"

[providers.deepseek]
base_url = "https://api.deepseek.com"
model = "deepseek-flash"

[providers.bailian]
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen3.8-flash"
api_key_env = "DASHSCOPE_API_KEY"

[generation]
temperature = 0.0
max_tokens = 256
timeout_secs = 20

[shell]
default = "auto"
keybinding = "ctrl-g"
include_cwd = false
```

Switch to Bailian:

```bash
cmdgen config set-key --provider bailian
cmdgen config use bailian
```

`api_key_env` is an environment variable name, not a secret. The Bailian preset uses the shared North China 2 (Beijing) endpoint and does not include a workspace id. To use a workspace-specific endpoint, run `cmdgen config set --provider bailian --base-url <url>`. For one run only, still use `--base-url`.

A legacy `[provider]` table can still be read; it cannot appear together with `[providers]`. Other OpenAI-compatible services can be written as `[providers.<name>]`, or created with `cmdgen config set --provider <name> --base-url <url> --model <model>`.

`deepseek-flash` is not a permanent model name. If the vendor changes it, update `model` in the config. DeepSeek enables thinking mode by default; when the provider is `deepseek`, cmdgen sends `thinking.type=disabled` so the response is a command.

`base_url` is the API prefix. The client appends `/chat/completions` once and does not append `/v1` again. Non-local addresses must use HTTPS, and certificate checks cannot be disabled. `127.0.0.1` and `localhost` may use HTTP so a local compatible service can be reached.

`shell.include_cwd` defaults to `false`. Only when it is `true` does the request include the current working directory path. Directory contents, history, and environment variables are not uploaded.

## Shell integration

The default shortcut is `Ctrl+G`. To use another key, set `shell.keybinding = "ctrl-x"` in the config, then install again. Only `ctrl-<letter>` is supported.

```bash
cmdgen shell install zsh
source ~/.zshrc

cmdgen shell install bash
source ~/.bashrc
```

Installing again does not append a second managed block. Uninstall removes only cmdgen's marker block and its own script:

```bash
cmdgen shell uninstall zsh
cmdgen shell uninstall bash
cmdgen shell uninstall
```

The marker looks like:

```bash
# >>> cmdgen managed integration >>>
source "$HOME/.config/cmdgen/integration.zsh"
# <<< cmdgen managed integration <<<
```

Integration behavior:

- An empty or whitespace-only input line does not send a request.
- A spinner runs at the end of the input line while generating. Pressing any key cancels the request and keeps the original input. That key is not inserted into the line.
- When the Zsh result is a single-line command, typed text is left as-is and the suggestion is shown as ghost text. `Tab` or Right accepts it and moves the cursor to the end. Typing away from the suggestion clears the ghost. Pressing the shortcut again while a suggestion is showing regenerates it. Enter runs the text you typed, not the ghost. Destructive commands such as `rm` and `kill` are shown in red with `⚠` and are not inserted until you accept them. Incomplete suggestions that would leave the shell waiting for more input are discarded. Multi-line results are written into the input line directly.
- On success with a non-empty result, Bash replaces the current input line and moves the cursor to the end. There is no ghost text.
- On failure, the original input is kept.
- Nothing is `eval`'d, and Enter is not pressed for you.

To use the word `shell` as a description, write `cmdgen -- shell`. Colors can be overridden with `CMDGEN_HIGHLIGHT` and `CMDGEN_HIGHLIGHT_DESTRUCTIVE`, using the same format as Zsh `region_highlight`.

The Bash integration uses `bind -x` and needs Bash 4 or newer. The Bash 3.2 that ships with macOS cannot bind the shortcut; install a newer Bash with Homebrew. Zsh uses ZLE and does not depend on a particular terminal emulator.

## Diagnostics

```bash
cmdgen doctor
```

It checks the config, key source, credential store, API reachability, and integration files. Keys in the output are redacted. The exit code is 1 when any check fails. The network check only reaches `base_url`. It does not send the API key and does not start a completion.

## Exit contract

| Case | stdout | stderr | Exit |
| --- | --- | --- | --- |
| Success | the command only | no output by default | 0 |
| Missing key, timeout, HTTP/API error, unparseable response, config error | empty | an explanation in the interface language, with no key | non-zero |

A debug stack appears only when `CMDGEN_DEBUG` is set.

## Security

- Model output is not executed. There is no `--execute`.
- The key is not written to `config.toml`, logs, error messages, or shell history. Do not run `cmdgen config set-key <secret>`. When the system credential store is unavailable, the key is written only to `credentials.toml` with mode `0600`.
- Prompts, responses, and command history are not recorded by default. Environment variables and file contents are not uploaded.
- Only HTTPS is used. Remote addresses cannot disable TLS certificate checks.
- The prompt asks the model to prefer non-destructive commands. That does not remove the risk. Check the command yourself before running it.
- Commands that delete, overwrite, change permissions, fetch remote scripts, or affect production need a human check.

## Troubleshooting

- **API key not found**: run `cmdgen config set-key`, or export `CMDGEN_API_KEY`. Without a keychain or Secret Service, `set-key` writes the key to `credentials.toml`. To use a key only in the current terminal, export the environment variable; it disappears when the terminal closes.
- **Invalid base_url**: drop a trailing `/chat/completions`, and make sure a remote address is `https://`.
- **Request timed out**: raise `generation.timeout_secs` or set `CMDGEN_TIMEOUT`. The default is 20 seconds, with no automatic retry.
- **Explanation text came back**: cmdgen rejects obvious prose. Make the request more specific and try again.
- **Bash does nothing**: confirm `bash --version` is at least 4, and that you have run `source ~/.bashrc`.
- **Shortcut conflict**: install warns when a binding already exists. Change `shell.keybinding` to another `ctrl-<letter>` and install again.

## Known limits

- No general chat, multi-turn conversation, code generation, agent, tool calling, or MCP.
- No Fish, and no promise of an input-line shortcut for pure POSIX sh.
- No request cache and no resident daemon.
- Windows is not in this version's release set.
- Model output is a suggestion. Both syntax and meaning can be wrong.

## Development

See [DEV.md](DEV.md) for working on this repository, running tests, and re-recording the terminal demos above.

## License

[MIT](LICENSE)
