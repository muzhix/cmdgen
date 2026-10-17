"""Drive the Zsh widget in a PTY and print the final redrawn line."""

import os
import pty
import select
import time


def read_for(fd, seconds):
    data = b""
    end = time.time() + seconds
    while time.time() < end:
        ready, _, _ = select.select([fd], [], [], 0.05)
        if fd not in ready:
            continue
        chunk = os.read(fd, 8192)
        if not chunk:
            break
        data += chunk
    return data


def final_line(raw):
    text = raw.decode("utf-8", "replace")
    needle = "hanbd@mbp cmdgen % docker ps -a"
    index = text.rfind(needle)
    if index < 0:
        return ""
    end = index + len(needle)
    return text[index:end]


def main():
    widget = os.environ["CMDGEN_WIDGET"]
    bindir = os.environ["CMDGEN_BIN"]
    master, slave = pty.openpty()
    pid = os.fork()
    if pid == 0:
        os.setsid()
        os.dup2(slave, 0)
        os.dup2(slave, 1)
        os.dup2(slave, 2)
        os.close(master)
        os.close(slave)
        env = os.environ.copy()
        env["TERM"] = "xterm-256color"
        env["PATH"] = bindir + os.pathsep + env.get("PATH", "")
        env["PS1"] = "hanbd@mbp cmdgen %% "
        os.execvp("zsh", ["zsh", "-f", "-i"])

    os.close(slave)
    try:
        read_for(master, 0.4)
        os.write(master, b"PS1='hanbd@mbp cmdgen %% '\r")
        read_for(master, 0.3)
        os.write(master, f"source {widget}\r".encode())
        read_for(master, 0.3)
        os.write(master, "列出所有docker容器".encode())
        read_for(master, 0.2)
        os.write(master, b"\x07")
        raw = read_for(master, 1.2)
        os.write(master, b"\x04")
        read_for(master, 0.2)
    finally:
        os.close(master)
        os.waitpid(pid, 0)

    line = final_line(raw)
    if not line:
        raise SystemExit("no redraw: " + raw.decode("utf-8", "replace"))
    print("LINE=" + line)


if __name__ == "__main__":
    main()
