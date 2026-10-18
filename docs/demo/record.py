#!/usr/bin/env python3
"""Record the README terminal demos from the real cmdgen binary.

The model is a local OpenAI-compatible stub so the recording is deterministic
and does not need an API key. The shell, the spinner, stdout, and the Zsh
widget are the real program.

    cargo build --release
    python3 docs/demo/record.py
"""

from __future__ import annotations

import os
import pty
import select
import shutil
import struct
import subprocess
import tempfile
import termios
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pyte
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs"
BINARY = ROOT / "target" / "release" / "cmdgen"
JB = Path("/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf")
JB_BOLD = Path("/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Bold.ttf")

COLS = 96
ROWS = 16
FONT_PX = 22
FPS = 14
MOCK_DELAY = 0.72

BG = (23, 27, 34)
TITLE_BG = (16, 19, 24)
FG = (228, 230, 227)
BORDER = (44, 51, 64)
AMBER = (242, 193, 78)
IVORY = (244, 241, 234)
MIST = (154, 163, 178)
DOTS = ((224, 108, 117), (230, 180, 80), (152, 195, 121))

NAMED = {
    "black": (26, 29, 36),
    "red": (224, 108, 117),
    "green": (152, 195, 121),
    "brown": (230, 180, 80),
    "blue": (97, 175, 239),
    "magenta": (198, 120, 221),
    "cyan": (86, 182, 194),
    "white": FG,
    "brightblack": (140, 148, 160),
    "brightred": (224, 108, 117),
    "brightgreen": (181, 212, 163),
    "brightbrown": (240, 198, 116),
    "brightblue": (124, 184, 245),
    "brightmagenta": (215, 164, 232),
    "brightcyan": (127, 211, 220),
    "brightwhite": (255, 255, 255),
}

RESPONSES = (
    ("list all docker containers, including stopped ones", "docker ps -a"),
    ("find files larger than 100MB", "find . -type f -size +100M"),
    ("show the last 200 lines of the nginx logs", "docker logs --tail 200 nginx"),
    ("delete temporary files in this directory", "rm -i -- *.tmp"),
)

# The binary's status line is Chinese. The README GIF is English, so the
# recorder rewrites that one label while keeping the real spinner glyphs.
STATUS_ZH = "正在生成"
STATUS_EN = "generating"


class MockHandler(BaseHTTPRequestHandler):
    def do_POST(self) -> None:  # noqa: N802
        length = int(self.headers.get("Content-Length", "0"))
        raw = self.rfile.read(length).decode("utf-8", "replace")
        command = "echo unmatched"
        for needle, output in RESPONSES:
            if needle in raw:
                command = output
                break
        time.sleep(MOCK_DELAY)
        body = (
            '{"choices":[{"message":{"role":"assistant","content":'
            + json_string(command)
            + "}}]}"
        ).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args) -> None:
        return


def json_string(text: str) -> str:
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


class Recorder:
    def __init__(self, argv: list[str], env: dict[str, str], cols: int, rows: int) -> None:
        self.argv = argv
        self.env = env
        self.cols = cols
        self.rows = rows
        self.events: list[tuple[float, bytes]] = []
        self._start = 0.0
        self._stop = threading.Event()
        self._fd = -1
        self._thread: threading.Thread | None = None

    def __enter__(self) -> "Recorder":
        master, slave = pty.openpty()
        winsize = struct.pack("HHHH", self.rows, self.cols, 0, 0)
        import fcntl

        fcntl.ioctl(slave, termios.TIOCSWINSZ, winsize)
        pid = os.fork()
        if pid == 0:
            os.setsid()
            os.dup2(slave, 0)
            os.dup2(slave, 1)
            os.dup2(slave, 2)
            os.close(master)
            os.close(slave)
            os.execvpe(self.argv[0], self.argv, self.env)
        os.close(slave)
        self._pid = pid
        self._fd = master
        self._start = time.perf_counter()
        self._thread = threading.Thread(target=self._read, daemon=True)
        self._thread.start()
        return self

    def __exit__(self, *_) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=1)
        if self._fd >= 0:
            os.close(self._fd)
        try:
            os.kill(self._pid, 15)
        except OSError:
            pass
        os.waitpid(self._pid, 0)

    def _read(self) -> None:
        while not self._stop.is_set():
            ready, _, _ = select.select([self._fd], [], [], 0.05)
            if not ready:
                continue
            try:
                chunk = os.read(self._fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            self.events.append((time.perf_counter() - self._start, chunk))

    def now(self) -> float:
        return time.perf_counter() - self._start

    def send(self, data: bytes) -> None:
        os.write(self._fd, data)

    def wait(self, seconds: float) -> None:
        time.sleep(seconds)

    def type_text(self, text: str, delay: float = 0.026) -> None:
        for char in text:
            self.send(char.encode())
            self.wait(delay)


def screen_text(screen: pyte.Screen) -> str:
    return "\n".join(line.rstrip() for line in screen.display).rstrip()


def replay(events: list[tuple[float, bytes]], start: float, end: float, cols: int, rows: int):
    screen = pyte.Screen(cols, rows)
    stream = pyte.ByteStream(screen)
    frames = []
    cursor = 0
    t = start
    step = 1 / FPS
    while t <= end + 1e-6:
        while cursor < len(events) and events[cursor][0] <= t:
            stream.feed(events[cursor][1])
            cursor += 1
        frames.append(snapshot(screen))
        t += step
    return frames


def snapshot(screen: pyte.Screen):
    rows = []
    cursor_x = screen.cursor.x
    cursor_y = screen.cursor.y
    for y in range(screen.lines):
        row = []
        for x in range(screen.columns):
            char = screen.buffer[y][x]
            row.append((char.data, char.fg, char.bg, char.bold, char.reverse))
        row, shift, status_end = rewrite_status(row)
        rows.append(row)
        if y == cursor_y and cursor_x >= status_end:
            cursor_x += shift
    return rows, (min(cursor_x, screen.columns - 1), cursor_y, screen.cursor.hidden)


def rewrite_status(row: list) -> tuple[list, int, int]:
    text = "".join(data for data, *_rest in row if data)
    if STATUS_ZH not in text:
        return row, 0, 0
    start = next(index for index, cell in enumerate(row) if cell[0] == STATUS_ZH[0])
    seen = 0
    end = start
    while end < len(row) and seen < len(STATUS_ZH):
        if row[end][0]:
            seen += 1
        end += 1
    style = row[start][1:]
    updated = list(row)
    for offset, char in enumerate(STATUS_EN):
        if start + offset < len(updated):
            updated[start + offset] = (char, *style)
    return updated, len(STATUS_EN) - (end - start), end


def used_rows(frames) -> int:
    last = 1
    for rows, (cx, cy, hidden) in frames:
        del cx
        if not hidden:
            last = max(last, cy + 1)
        for y, row in enumerate(rows):
            if any(data not in ("", " ") for data, *_ in row):
                last = max(last, y + 1)
    return min(last + 1, ROWS)


def used_cols(frames) -> int:
    last = 24
    for rows, (cx, cy, hidden) in frames:
        del cy
        if not hidden:
            last = max(last, cx + 2)
        for row in rows:
            for x, (data, *_) in enumerate(row):
                if data not in ("", " "):
                    last = max(last, x + 2)
    return min(last, COLS)


def parse_color(value: str, default: tuple[int, int, int]) -> tuple[int, int, int]:
    if value in ("default", "", None):
        return default
    if value in NAMED:
        return NAMED[value]
    if len(value) == 6:
        try:
            return tuple(int(value[i : i + 2], 16) for i in (0, 2, 4))  # type: ignore[return-value]
        except ValueError:
            return default
    return default


class Renderer:
    """Draw the terminal in JetBrains Mono so the GIF reads as code."""

    def __init__(self) -> None:
        self.latin = ImageFont.truetype(str(JB), FONT_PX)
        self.latin_bold = ImageFont.truetype(str(JB_BOLD), FONT_PX)
        self.cell_w = max(1, round(self.latin.getlength("M")))
        self.title_font = ImageFont.truetype(str(JB), 15)
        ascent, descent = self.latin.getmetrics()
        self.cell_h = ascent + descent + 4
        self.baseline = 2 + ascent

    def font_for(self, char: str, bold: bool):
        del char
        return self.latin_bold if bold else self.latin

    def render(self, frame, title: str, rows: int, cols: int) -> Image.Image:
        cells, (cursor_x, cursor_y, cursor_hidden) = frame
        pad_x = 22
        pad_y = 18
        title_h = 42
        width = pad_x * 2 + cols * self.cell_w
        height = title_h + pad_y * 2 + rows * self.cell_h
        image = Image.new("RGB", (width, height), BG)
        draw = ImageDraw.Draw(image)
        draw.rectangle((0, 0, width - 1, height - 1), outline=BORDER)
        draw.rectangle((1, 1, width - 2, title_h), fill=TITLE_BG)
        draw.line((0, title_h, width, title_h), fill=BORDER)
        for index, color in enumerate(DOTS):
            cx = 18 + index * 18
            draw.ellipse((cx, 16, cx + 10, 26), fill=color)
        tw = draw.textlength(title, font=self.title_font)
        draw.text(((width - tw) / 2, 12), title, font=self.title_font, fill=MIST)
        origin_y = title_h + pad_y
        for y in range(rows):
            x = 0
            while x < cols:
                data, fg, bg, bold, reverse = cells[y][x]
                if data == "":
                    x += 1
                    continue
                wide = x + 1 < COLS and cells[y][x + 1][0] == "" and data not in ("", " ")
                span = 2 if wide else 1
                fg_c = parse_color(fg, FG)
                bg_c = parse_color(bg, BG)
                if reverse:
                    fg_c, bg_c = bg_c, fg_c
                px = pad_x + x * self.cell_w
                py = origin_y + y * self.cell_h
                if bg_c != BG:
                    draw.rectangle(
                        (px, py, px + span * self.cell_w, py + self.cell_h),
                        fill=bg_c,
                    )
                if data not in ("", " "):
                    code = ord(data[0])
                    if 0x2800 <= code <= 0x28FF:
                        self.draw_braille(draw, px, py, fg_c, code)
                    else:
                        font = self.font_for(data, bold)
                        draw.text(
                            (px, py + self.baseline),
                            data,
                            font=font,
                            fill=fg_c,
                            anchor="ls",
                        )
                x += span
        if not cursor_hidden and cursor_y < rows and cursor_x < cols:
            bar_x = pad_x + cursor_x * self.cell_w
            bar_y = origin_y + cursor_y * self.cell_h + 4
            draw.rectangle(
                (bar_x, bar_y, bar_x + 2, bar_y + self.cell_h - 8),
                fill=AMBER,
            )
        return image

    def draw_braille(self, draw, px: int, py: int, color, code: int) -> None:
        bits = code - 0x2800
        # Unicode braille dots run down the left column, then the right.
        positions = ((0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3))
        grid_w = self.cell_w * 0.78
        grid_h = min(self.cell_h * 0.72, self.cell_w * 1.7)
        radius = max(2, round(self.cell_w * 0.16))
        origin_x = px + (self.cell_w - grid_w) / 2
        origin_y = py + (self.cell_h - grid_h) / 2
        step_x = grid_w
        step_y = grid_h / 3
        for index, (col, row) in enumerate(positions):
            if bits & (1 << index):
                cx = origin_x + col * step_x
                cy = origin_y + row * step_y
                draw.ellipse((cx - radius, cy - radius, cx + radius, cy + radius), fill=color)


def write_gif(frames: list[Image.Image], path: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        folder = Path(tmp)
        for index, frame in enumerate(frames):
            frame.save(folder / f"f{index:04d}.png")
        palette = folder / "palette.png"
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-framerate",
                str(FPS),
                "-i",
                str(folder / "f%04d.png"),
                "-vf",
                "palettegen=max_colors=192:reserve_transparent=0:stats_mode=diff",
                str(palette),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-framerate",
                str(FPS),
                "-i",
                str(folder / "f%04d.png"),
                "-i",
                str(palette),
                "-lavfi",
                "paletteuse=dither=none",
                "-loop",
                "0",
                str(path),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )


def contact_sheet(frames: list[Image.Image], path: Path) -> None:
    sample = frames[:: max(1, len(frames) // 8)]
    if frames[-1] is not sample[-1]:
        sample.append(frames[-1])
    width = sample[0].width
    height = sum(frame.height for frame in sample)
    sheet = Image.new("RGB", (width, height), BG)
    y = 0
    for frame in sample:
        sheet.paste(frame, (0, y))
        y += frame.height
    sheet.save(path)


def assert_text(frames_text: str, needles: list[str], label: str) -> None:
    missing = [needle for needle in needles if needle not in frames_text]
    if missing:
        raise SystemExit(f"{label} missing {missing}\n---\n{frames_text[-2000:]}")


def base_env(home: Path, port: int) -> dict[str, str]:
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["XDG_CONFIG_HOME"] = str(home / ".config")
    env["TERM"] = "xterm-256color"
    env["LANG"] = "C.UTF-8"
    env["LC_ALL"] = "C.UTF-8"
    env["CMDGEN_API_KEY"] = "demo-not-a-secret"
    env["PATH"] = str(BINARY.parent) + os.pathsep + env.get("PATH", "")
    env.pop("CMDGEN_BASE_URL", None)
    env.pop("CMDGEN_MODEL", None)
    env.pop("CMDGEN_PROVIDER", None)
    config = home / ".config" / "cmdgen"
    config.mkdir(parents=True, exist_ok=True)
    (config / "config.toml").write_text(
        "\n".join(
            [
                'active = "local"',
                "",
                "[providers.local]",
                f'base_url = "http://127.0.0.1:{port}"',
                'model = "demo"',
                "",
                "[shell]",
                'default = "auto"',
                'keybinding = "ctrl-g"',
                "include_cwd = false",
                "",
            ]
        ),
        encoding="utf-8",
    )
    os.chmod(config / "config.toml", 0o600)
    return env


def record_cli(env: dict[str, str]) -> tuple[list, float, float, list]:
    child_env = env.copy()
    child_env["PS1"] = r"\[\e[38;5;179m\]❯\[\e[0m\] "
    with Recorder(["bash", "--noprofile", "--norc", "-i"], child_env, COLS, ROWS) as rec:
        rec.wait(0.35)
        rec.send(b"clear\r")
        rec.wait(0.25)
        start = rec.now()
        rec.wait(0.35)
        for prompt in (
            'cmdgen "list all docker containers, including stopped ones"',
            'cmdgen "find files larger than 100MB"',
            'cmdgen "show the last 200 lines of the nginx logs"',
        ):
            rec.type_text(prompt)
            rec.wait(0.18)
            rec.send(b"\r")
            rec.wait(MOCK_DELAY + 0.55)
            rec.wait(0.55)
        rec.wait(0.9)
        end = rec.now()
        rec.wait(0.2)
        events = list(rec.events)
    frames = replay(events, start, end, COLS, ROWS)
    text = "\n".join(screen_text_from_cells(frame) for frame in frames)
    assert_text(
        text,
        ["docker ps -a", "find . -type f -size +100M", "docker logs --tail 200 nginx", "generating"],
        "cli",
    )
    assert_english(text, "cli")
    return frames, start, end, events


def assert_english(text: str, label: str) -> None:
    allowed = set("❯⇥⚠")
    foreign = sorted({char for char in text if ord(char) > 127 and char not in allowed and not (0x2800 <= ord(char) <= 0x28FF)})
    if foreign or STATUS_ZH in text:
        raise SystemExit(f"{label} is not English: {foreign!r}\n{text[-1500:]}")


def record_shell(env: dict[str, str], home: Path) -> tuple[list, float, float]:
    install = subprocess.run(
        [str(BINARY), "shell", "install", "zsh"],
        env=env,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    script = home / ".config" / "cmdgen" / "integration.zsh"
    if not script.exists():
        raise SystemExit(install.stdout + install.stderr)
    child_env = env.copy()
    child_env["PROMPT"] = "%# "
    with Recorder(["zsh", "-f", "-i"], child_env, COLS, ROWS) as rec:
        rec.wait(0.4)
        rec.send(b"unset zle_bracketed_paste\r")
        rec.wait(0.15)
        rec.send("PROMPT='%F{179}❯%f '\r".encode())
        rec.wait(0.15)
        rec.send(f"source {script}\r".encode())
        rec.wait(0.25)
        rec.send(b"clear\r")
        rec.wait(0.3)
        start = rec.now()
        rec.wait(0.35)
        rec.type_text("list all docker containers, including stopped ones")
        rec.wait(0.25)
        rec.send(b"\x07")
        rec.wait(MOCK_DELAY + 0.85)
        rec.wait(0.2)
        rec.send(b"\t")
        rec.wait(1.35)
        rec.send(b"\x15")  # Ctrl+U, clear the accepted command without running it
        rec.wait(0.4)
        rec.type_text("delete temporary files in this directory")
        rec.wait(0.28)
        rec.send(b"\x07")
        rec.wait(MOCK_DELAY + 1.7)
        end = rec.now()
        rec.send(b"\x15")
        rec.wait(0.1)
        rec.send(b"\x04")
        rec.wait(0.2)
        events = list(rec.events)
    frames = replay(events, start, end, COLS, ROWS)
    text = "\n".join(screen_text_from_cells(frame) for frame in frames)
    assert_text(
        text,
        ["docker ps -a", "rm -i -- *.tmp", "list all docker containers", "⚠"],
        "shell",
    )
    assert_english(text, "shell")
    return frames, start, end


def screen_text_from_cells(frame) -> str:
    rows, _cursor = frame
    lines = []
    for row in rows:
        chars = []
        for data, *_rest in row:
            chars.append(data if data else "")
        lines.append("".join(chars).rstrip())
    return "\n".join(lines).rstrip()


def render_demo(frames, title: str, renderer: Renderer, path: Path, sheet: Path) -> None:
    height = used_rows(frames)
    cols = used_cols(frames)
    images = [renderer.render(frame, title, height, cols) for frame in frames]
    write_gif(images, path)
    contact_sheet(images, sheet)
    stills = {
        "docker ps -a": path.with_name(path.stem + "-still-command.png"),
        "rm -i -- *.tmp": path.with_name(path.stem + "-still-warn.png"),
        "docker logs --tail 200 nginx": path.with_name(path.stem + "-still-final.png"),
    }
    for needle, dest in stills.items():
        for frame, image in zip(frames, images):
            if needle in screen_text_from_cells(frame):
                image.save("/tmp/" + dest.name)


def main() -> None:
    if not BINARY.exists():
        raise SystemExit(f"missing binary: {BINARY}")
    server = ThreadingHTTPServer(("127.0.0.1", 0), MockHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    home = Path(tempfile.mkdtemp(prefix="cmdgen-demo-"))
    try:
        env = base_env(home, port)
        renderer = Renderer()
        cli_frames, _, _, _ = record_cli(env)
        shell_frames, _, _ = record_shell(env, home)
        render_demo(cli_frames, "bash", renderer, DOCS / "demo-cli.gif", Path("/tmp/cmdgen-demo-cli.png"))
        render_demo(
            shell_frames,
            "zsh",
            renderer,
            DOCS / "demo-shell.gif",
            Path("/tmp/cmdgen-demo-shell.png"),
        )
        print(f"wrote {DOCS / 'demo-cli.gif'} ({(DOCS / 'demo-cli.gif').stat().st_size} bytes)")
        print(f"wrote {DOCS / 'demo-shell.gif'} ({(DOCS / 'demo-shell.gif').stat().st_size} bytes)")
    finally:
        server.shutdown()
        shutil.rmtree(home, ignore_errors=True)


if __name__ == "__main__":
    main()
