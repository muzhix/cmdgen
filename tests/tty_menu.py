"""Run argv on a PTY. After CMDGEN_TTY_MARKER appears, write CMDGEN_TTY_KEYS (hex)."""

import os
import pty
import select
import sys


def drain(master, data):
    while True:
        ready, _, _ = select.select([master], [], [], 0.05)
        if master not in ready:
            return data
        try:
            chunk = os.read(master, 8192)
        except OSError:
            return data
        if not chunk:
            return data
        data += chunk


def main():
    if len(sys.argv) < 2:
        sys.exit(2)
    marker = os.environ.get("CMDGEN_TTY_MARKER", "English (en)").encode()
    keys = bytes.fromhex(os.environ.get("CMDGEN_TTY_KEYS", ""))
    master, slave = pty.openpty()
    pid = os.fork()
    if pid == 0:
        os.setsid()
        os.dup2(slave, 0)
        os.dup2(slave, 1)
        os.dup2(slave, 2)
        os.close(master)
        os.close(slave)
        os.execvpe(sys.argv[1], sys.argv[1:], os.environ)
        os._exit(127)

    os.close(slave)
    data = b""
    sent = False
    status = None
    waits = 0
    while True:
        ready, _, _ = select.select([master], [], [], 0.05)
        if master in ready:
            try:
                chunk = os.read(master, 8192)
            except OSError:
                chunk = b""
            if not chunk:
                break
            data += chunk
            if not sent and marker in data:
                os.write(master, keys)
                sent = True
        if not sent:
            waits += 1
            if waits > 100:
                os.kill(pid, 15)
                break
        child, waited = os.waitpid(pid, os.WNOHANG)
        if child:
            status = waited
            data = drain(master, data)
            break
    if status is None:
        _, status = os.waitpid(pid, 0)
        data = drain(master, data)
    sys.stdout.buffer.write(data)
    if os.WIFEXITED(status):
        sys.exit(os.WEXITSTATUS(status))
    sys.exit(1)


if __name__ == "__main__":
    main()
