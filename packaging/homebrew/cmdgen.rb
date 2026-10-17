# Homebrew formula 模板。
# 首次 GitHub Release 之后，把 url / sha256 换成对应资产再放入 tap。
# 不要把未填的占位符直接发布给用户安装。

class Cmdgen < Formula
  desc "AI-powered shell command generation for your terminal"
  homepage "REPLACE_WITH_REPOSITORY_URL"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "REPLACE_WITH_RELEASE_URL/cmdgen-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME"
    end
    on_intel do
      url "REPLACE_WITH_RELEASE_URL/cmdgen-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME"
    end
  end

  on_linux do
    on_intel do
      url "REPLACE_WITH_RELEASE_URL/cmdgen-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ME"
    end
    on_arm do
      url "REPLACE_WITH_RELEASE_URL/cmdgen-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ME"
    end
  end

  def install
    bin.install "cmdgen"
  end

  def caveats
    <<~EOS
      Shell 集成不会在安装时自动修改启动文件。需要时再运行：
        cmdgen integration install zsh
        cmdgen integration install bash
    EOS
  end

  test do
    assert_match "cmdgen", shell_output("#{bin}/cmdgen --version")
  end
end
