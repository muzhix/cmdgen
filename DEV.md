# 开发

维护 cmdgen 仓库时看这份说明。使用方法见 [README.md](README.md)。

## 本地安装

```bash
cargo install --path .
```

## 检查

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

测试使用本地 mock，不需要真实的 DeepSeek Key。

## 手工验收

手工验收时建议覆盖：直接命令、中文输入、快捷键替换、失败后保留输入、编辑后再执行、环境变量和钥匙串两种 Key、无 Key、错误 Key、断网、超时、HTTP 429/5xx、重复安装和卸载。Apple Silicon、Intel、Linux x86_64 和 Linux ARM64 都要分别看二进制能否启动。

## 发布

正式发布提供这些预编译目标：

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

GitHub Release 会在打上 `v*` 标签后由 `.github/workflows/release.yml` 构建，并附带 `SHA256SUMS`。用户侧一键安装是仓库根目录的 `install.sh`：

```bash
curl -fsSL https://raw.githubusercontent.com/muzhix/cmdgen/main/install.sh | bash
```

脚本只放置二进制，不修改 shell 启动文件。`bash tests/install_smoke.sh` 用本地压缩包覆盖安装、校验失败、卸载和管道执行，不访问网络。macOS 也可以用 `packaging/homebrew/cmdgen.rb` 作为 Homebrew tap 模板；发布前需要填入真实的下载地址和 sha256。

## README 演示

`docs/demo-shell.gif` 和 `docs/demo-cli.gif` 用本机的 release 二进制录制，Shell 集成也是安装器写出的脚本。模型是 `docs/demo/record.py` 里的本地兼容接口，按固定文案返回命令，这样画面可以复现，也不需要 API Key。

```bash
cargo build --release
python3 docs/demo/record.py
```

画面里的描述是英文，字体是 JetBrains Mono。二进制状态行里的「正在生成」会在录制时写成 `generating`。需要 `zsh`、`ffmpeg`、Python 包 `pillow` 和 `pyte`，以及系统里的 JetBrains Mono。

## 内置模型名

`deepseek-flash` 于 2026-10-17 对照 DeepSeek 官方文档核对。`qwen3.8-flash` 于 2026-10-17 对照百炼文本生成文档核对。它们不是永久有效的常量。如果官方更换模型名，请同时修改内置预设、README 和这里的说明。
