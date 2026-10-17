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


def read_until(fd, seconds, needle):
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
        if needle in data:
            break
    return data


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
    before = b""
    after = b""
    try:
        read_for(master, 0.4)
        os.write(master, b"PS1='hanbd@mbp cmdgen %% '\r")
        read_for(master, 0.3)
        os.write(master, f"source {widget}\r".encode())
        read_for(master, 0.3)
        os.write(master, "列出所有docker容器".encode())
        typed = read_for(master, 0.3)
        os.write(master, b"\x07")
        ghost = read_until(master, 2.0, "docker ps -a".encode())
        before = typed + ghost
        # Let the widget leave its cancel read before Tab accepts the ghost.
        time.sleep(0.3)
        os.write(master, b"\t")
        after = read_for(master, 1.2)
        os.write(master, b"\x04")
        read_for(master, 0.2)
    finally:
        os.close(master)
        os.waitpid(pid, 0)

    before_text = before.decode("utf-8", "replace")
    print("BEFORE_INPUT=" + str("列出所有docker容器" in before_text))
    print("BEFORE_GHOST=" + str("docker ps -a" in before_text))
    print("BEFORE_SMASHED=" + str("列出所有docker容器docker ps -a" in before_text))
    line = final_line(after)
    if not line:
        raise SystemExit(
            "no redraw: before="
            + before_text
            + " after="
            + after.decode("utf-8", "replace")
        )
    print("LINE=" + line)


if __name__ == "__main__":
    main()
