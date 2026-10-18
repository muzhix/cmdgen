# AGENTS.md

Rust CLI (`cmdgen`) that turns natural language into shell commands via an LLM (default DeepSeek, any OpenAI-compatible Chat Completions API, BYOK). Also installs Zsh/Bash input-line integrations (Ctrl+G). macOS/Linux only; Windows is out of scope. Read `DEV.md` (maintenance) and `README.md` (behavior contracts) before changing config, secrets, or shell integration; `cmdgen_handoff.md` is the original design spec.

## Commands

```bash
cargo fmt --all                      # rustfmt: edition 2021, max_width 100 (rustfmt.toml)
cargo clippy --all-targets -- -D warnings
cargo test --all-targets            # all mocks are local (wiremock); no real API key needed
bash tests/install_smoke.sh          # installer smoke test, offline
```

CI (`.github/workflows/ci.yml`) runs all four on ubuntu + macos. On Linux, tests need `zsh`, `pkg-config`, `libdbus-1-dev` installed (keyring dependency).

## Layout

- `src/cli.rs` — clap parsing and dispatch; `src/main.rs`/`lib.rs` — entry, `#![forbid(unsafe_code)]`, errors printed as `cmdgen: {err}` to stderr
- `src/config/` — `file.rs` (TOML config, provider selection/priority), `secrets.rs` (keyring + `credentials.toml` fallback, `EnvMap` abstraction)
- `src/generator/` — `client.rs` (HTTP), `prompt.rs`, `output.rs` (response validation)
- `src/shell/` — `detect.rs` (shell detection), `integration.rs` (install/uninstall managed blocks)
- `shell/cmdgen.zsh`, `shell/cmdgen.bash` — integration scripts embedded via `include_str!` in `src/shell/integration.rs`; edits there change the binary
- `tests/` — integration tests; `common/mod.rs` `Sandbox` isolates HOME/`XDG_CONFIG_HOME` and strips all `CMDGEN_*` env vars; PTY tests driven by `python3` helpers (`tty_loading.py`, `zsh_redraw.py`)
- `install.sh` — curl-to-bash installer (binary only, never edits shell startup files); `packaging/homebrew/cmdgen.rb` — Homebrew template, needs real URL+sha256 filled before release

## Non-negotiable invariants

- Never execute model output; there is no `--execute` and none may be added.
- API keys never appear in `config.toml`, logs, error messages, or shell history. `cmdgen config provider` takes no argument; a positional secret is rejected without being echoed. Tests use `assert_no_secret` to enforce this — keep it that way.
- Exit contract: success → stdout contains only the command, stderr empty, exit 0; any failure → empty stdout, Chinese message on stderr, non-zero exit.
- All user-facing errors/diagnostics are written in Chinese (to stderr). Debug stack traces only when `CMDGEN_DEBUG` is set.
- `base_url` is an API prefix; the client appends `/chat/completions` exactly once. HTTPS required for non-localhost; never disable cert validation.
- Shell integration never `eval`s or auto-presses Enter; failure must preserve the user's original input.

## Gotchas

- Built-in preset model names (`deepseek-flash`, `qwen3.8-flash`, verified 2026-10-17) are not permanent. If the vendor renames a model, update the constants in `src/config/mod.rs`, `README.md`, and `DEV.md` together.
- Provider priority: CLI args > env (`CMDGEN_*`) > TOML > built-in defaults; built-in presets are only `deepseek` and `bailian`. The legacy single `[provider]` table and `[providers]` are mutually exclusive and both must keep parsing.
- DeepSeek thinking mode is disabled via `thinking.type=disabled` when provider is `deepseek` — keep this or output parsing breaks.
- Zsh integration uses ZLE + ghost text; Bash uses `bind -x` and requires Bash ≥ 4 (macOS ships 3.2). PTY-driven tests in `tests/integration_install.rs` and `tests/cli.rs` require `python3`.
- Release: push a `v*` tag → `.github/workflows/release.yml` builds 4 targets with `SHA256SUMS`. Update `CHANGELOG.md` when releasing.
