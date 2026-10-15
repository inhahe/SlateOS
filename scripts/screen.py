#!/usr/bin/env python3
"""A guest's screen from the host: capture it, type at it, read its text back.

    python scripts/screen.py capture (--hmp PORT | --qmp PORT) OUT.ppm|OUT.png [--device ID]
    python scripts/screen.py type (--hmp PORT | --qmp PORT) TEXT     # `\\n` in TEXT is Enter
    python scripts/screen.py text SCREEN.ppm                         # the console's text
    python scripts/screen.py probe (--hmp PORT | --qmp PORT) [--timeout 120] [--keep DIR]
    python scripts/screen.py --self-test

Design decision 1392 makes "a boot test that boots to the desktop and checks
it, with QEMU's screen capture and simulated input" lane A's step on the path
to a desktop a person can use. This is that test's host half. The desktop
does not start yet, so what it checks today is the stand-in on the screen
instead: the kernel's framebuffer console with init's shell on it. `probe`
types `echo <token>` there and waits for the token to be drawn on a line of
its own -- keyboard, kernel, init's shell, console, framebuffer and QEMU's
screen capture, end to end, which nothing else checks: BOOT_OK proves only
the serial port. When the desktop starts, the probe's expectation changes;
the capture, the typing and the reading stay.

The monitor is QEMU's own: HMP, the text one `boot-test.sh` already attaches
(`-monitor tcp:`), or QMP, the JSON one `guest.py start --qmp` attaches.

Which screen: QEMU's console 0 unless `--device` names another -- the one
its window shows a person. On the boot test's machine that is the `-vga std`
adapter, the framebuffer Limine hands the kernel and the console draws on;
the virtio-gpu the kernel's DRM layer makes primary is a second console,
black until something draws on it.

Reading the text: the console draws the 8x16 VGA font of `kernel/src/font.rs`
on a grid from the top-left corner, each glyph in one colour on its cell's
background (`kernel/src/console.rs`), so a cell's pixels that differ from its
own commonest colour are exactly its glyph's bits. The font is read from that
file, so the two cannot drift apart. A cell that matches no glyph -- the
mouse pointer over it, an underline -- reads as `?`.

Exit status: 0 done (for `probe`: the token was drawn), 1 the check failed,
2 a usage or connection error.
"""
from __future__ import annotations

import argparse
import json
import os
import random
import re
import socket
import struct
import sys
import threading
import time
import zlib
from pathlib import Path
from typing import NamedTuple

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
FONT_FILE = ROOT / "kernel" / "src" / "font.rs"
GLYPH_W, GLYPH_H = 8, 16

# ---------------------------------------------------------------------------
# Screens: PPM in (what QEMU's screendump writes), PNG out (what a person opens)
# ---------------------------------------------------------------------------


class Screen(NamedTuple):
    """A captured screen: `pixels` holds `width * height` RGB triples, rows
    top to bottom."""

    width: int
    height: int
    pixels: bytes


def parse_ppm(data: bytes) -> Screen:
    """A binary PPM (`P6`, 8-bit samples) -- QEMU's screendump format."""
    tokens: list[bytes] = []
    i = 0
    while len(tokens) < 4:
        while i < len(data) and data[i : i + 1].isspace():
            i += 1
        if data[i : i + 1] == b"#":
            while i < len(data) and data[i : i + 1] != b"\n":
                i += 1
            continue
        start = i
        while i < len(data) and not data[i : i + 1].isspace():
            i += 1
        if start == i:
            raise ValueError("a PPM header that ends early")
        tokens.append(data[start:i])
    if tokens[0] != b"P6":
        raise ValueError(f"not a binary PPM (magic {tokens[0]!r})")
    width, height, maxval = (int(t) for t in tokens[1:])
    if maxval != 255:
        raise ValueError(f"a PPM with {maxval} as its largest sample, not 255")
    body = data[i + 1 :]  # one whitespace byte ends the header
    if len(body) < width * height * 3:
        raise ValueError(f"a {width}x{height} PPM with {len(body)} bytes of pixels")
    return Screen(width, height, body[: width * height * 3])


def read_screen(path: str | Path) -> Screen:
    return parse_ppm(Path(path).read_bytes())


def ppm_bytes(screen: Screen) -> bytes:
    return b"P6\n%d %d\n255\n" % (screen.width, screen.height) + screen.pixels


def png_bytes(screen: Screen) -> bytes:
    """`screen` as an 8-bit RGB PNG, every row unfiltered."""
    stride = screen.width * 3
    raw = b"".join(
        b"\x00" + screen.pixels[y * stride : (y + 1) * stride] for y in range(screen.height)
    )

    def chunk(kind: bytes, body: bytes) -> bytes:
        return (struct.pack(">I", len(body)) + kind + body
                + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF))

    header = struct.pack(">IIBBBBB", screen.width, screen.height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def write_screen(screen: Screen, path: str | Path) -> None:
    """Save `screen` as PNG or PPM, by `path`'s suffix."""
    p = Path(path)
    p.write_bytes(png_bytes(screen) if p.suffix.lower() == ".png" else ppm_bytes(screen))


def colours(screen: Screen, step: int = 7) -> int:
    """How many colours a sample of every `step`th pixel holds: 1 is a blank
    screen."""
    px = screen.pixels
    return len({px[i : i + 3] for i in range(0, len(px), 3 * step)})


# ---------------------------------------------------------------------------
# Reading the console's text
# ---------------------------------------------------------------------------


def load_font(path: Path = FONT_FILE) -> dict[bytes, str]:
    """The console's glyphs, as {16 row bytes: the character}, from the
    kernel's own font table. Blank glyphs are left out; a blank cell reads as a
    space before any lookup."""
    text = path.read_text(encoding="utf-8")
    declared = re.search(r"\bstatic\s+FONT_DATA\b", text)
    if not declared:
        raise ValueError(f"{path}: no `static FONT_DATA`")
    # The table runs from the `=` to the `];` alone on its line: the type
    # before it, `[[u8; 16]; 128]`, has a `];` of its own.
    start = text.index("=", declared.end())
    end = text.index("\n];", start)
    body = re.sub(r"//[^\n]*", "", text[start:end])
    values = [int(v, 16) for v in re.findall(r"0x([0-9A-Fa-f]{2})", body)]
    if len(values) != 128 * GLYPH_H:
        raise ValueError(f"{path}: FONT_DATA has {len(values)} bytes, not {128 * GLYPH_H}")
    font: dict[bytes, str] = {}
    for code in range(32, 127):
        rows = bytes(values[code * GLYPH_H : (code + 1) * GLYPH_H])
        if any(rows):
            font.setdefault(rows, chr(code))
    return font


def console_text(screen: Screen, font: dict[bytes, str]) -> list[str]:
    """The text on `screen`, one string per row of 8x16 cells, right-trimmed."""
    stride = screen.width * 3
    px = screen.pixels
    cols, rows = screen.width // GLYPH_W, screen.height // GLYPH_H
    lines = []
    for row in range(rows):
        chars = []
        for col in range(cols):
            x0 = col * GLYPH_W * 3
            slices = [
                px[(row * GLYPH_H + r) * stride + x0 : (row * GLYPH_H + r) * stride + x0 + 24]
                for r in range(GLYPH_H)
            ]
            first = slices[0][:3]
            if all(s == first * GLYPH_W for s in slices):
                chars.append(" ")
                continue
            counts: dict[bytes, int] = {}
            for s in slices:
                for k in range(0, 24, 3):
                    c = s[k : k + 3]
                    counts[c] = counts.get(c, 0) + 1
            background = max(counts, key=counts.__getitem__)
            mask = bytes(
                sum(0x80 >> (k // 3) for k in range(0, 24, 3) if s[k : k + 3] != background)
                for s in slices
            )
            chars.append(font.get(mask, "?"))
        lines.append("".join(chars).rstrip())
    return lines


def render(lines: list[str], font: dict[bytes, str], width: int, height: int,
           fg: bytes = b"\xcc\xcc\xcc", bg: bytes = b"\x00\x00\x00") -> Screen:
    """`lines` drawn as the console draws them -- for the self-test and the
    fake monitors in it."""
    by_char = {c: rows for rows, c in font.items()}
    out = bytearray(bg * (width * height))
    stride = width * 3
    for row, line in enumerate(lines[: height // GLYPH_H]):
        for col, ch in enumerate(line[: width // GLYPH_W]):
            for r, bits in enumerate(by_char.get(ch, bytes(GLYPH_H))):
                for b in range(GLYPH_W):
                    if bits & (0x80 >> b):
                        at = (row * GLYPH_H + r) * stride + (col * GLYPH_W + b) * 3
                        out[at : at + 3] = fg
    return Screen(width, height, bytes(out))


# ---------------------------------------------------------------------------
# Keys
# ---------------------------------------------------------------------------

#: QEMU's key names (`QKeyCode`) for the characters that are not their own.
_KEY_NAMES = {
    " ": "spc", "\n": "ret", "\t": "tab", "-": "minus", "=": "equal", ".": "dot",
    ",": "comma", "/": "slash", ";": "semicolon", "'": "apostrophe",
    "[": "bracket_left", "]": "bracket_right", "\\": "backslash", "`": "grave_accent",
}
#: The shifted characters, and the key each is on (US layout, as QEMU's is).
_SHIFTED = {
    "_": "-", "+": "=", ":": ";", '"': "'", "<": ",", ">": ".", "?": "/",
    "{": "[", "}": "]", "|": "\\", "~": "`", "!": "1", "@": "2", "#": "3",
    "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9", ")": "0",
}


def keys_for(text: str) -> list[list[str]]:
    """The key presses that type `text`: each a list of QEMU key names pressed
    together (`["shift", "a"]` for `A`)."""
    out = []
    for ch in text:
        if ch.islower() and ch.isascii() or ch.isdigit() and ch.isascii():
            out.append([ch])
        elif ch.isupper() and ch.isascii():
            out.append(["shift", ch.lower()])
        elif ch in _KEY_NAMES:
            out.append([_KEY_NAMES[ch]])
        elif ch in _SHIFTED:
            base = _SHIFTED[ch]
            out.append(["shift", _KEY_NAMES.get(base, base)])
        else:
            raise ValueError(f"no key types {ch!r}")
    return out


# ---------------------------------------------------------------------------
# Monitors
# ---------------------------------------------------------------------------


class MonitorError(Exception):
    """The monitor refused a command, or could not be reached."""


_ESCAPE = re.compile(r"\x1b\[[0-9;?]*[A-Za-z]")
_HMP_PROMPT = b"(qemu) "


def hmp_output(raw: bytes) -> str:
    """What an HMP command printed, from all the monitor sent back for it:
    the command's own echo -- redrawn a key at a time, with cursor movement --
    ends at the first line break; the prompt ends the rest."""
    text = _ESCAPE.sub("", raw.decode("utf-8", "replace"))
    _, _, rest = text.partition("\r\n")
    if rest.endswith(_HMP_PROMPT.decode()):
        rest = rest[: -len(_HMP_PROMPT)]
    return rest.strip()


def hmp_quote(path: str) -> str:
    """A path as one HMP argument: quoted, so a space does not end it, with
    `/` for `\\`, which HMP's quoting would read as an escape."""
    return '"' + path.replace("\\", "/").replace('"', '\\"') + '"'


class Monitor:
    """One connection to a QEMU monitor on 127.0.0.1: `capture` the screen,
    `press` keys. HMP or QMP, chosen by `kind`."""

    def __init__(self, kind: str, port: int, timeout: float = 30.0):
        if kind not in ("hmp", "qmp"):
            raise ValueError(f"no monitor kind {kind!r}")
        self.kind = kind
        self.timeout = timeout
        try:
            self.sock = socket.create_connection(("127.0.0.1", port), timeout=timeout)
        except OSError as e:
            raise MonitorError(f"no {kind.upper()} monitor on 127.0.0.1:{port}: {e}") from e
        self.buf = b""
        if kind == "hmp":
            self._until(_HMP_PROMPT)
        else:
            self._qmp_message()  # the greeting
            self._qmp("qmp_capabilities")

    def close(self) -> None:
        self.sock.close()

    def __enter__(self) -> "Monitor":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()

    def _until(self, end: bytes) -> bytes:
        deadline = time.monotonic() + self.timeout
        while end not in self.buf:
            if time.monotonic() > deadline:
                raise MonitorError(f"the monitor sent no {end!r} in {self.timeout:.0f}s")
            try:
                chunk = self.sock.recv(65536)
            except OSError as e:
                raise MonitorError(f"the monitor connection failed: {e}") from e
            if not chunk:
                raise MonitorError("the monitor closed the connection")
            self.buf += chunk
        at = self.buf.index(end) + len(end)
        out, self.buf = self.buf[:at], self.buf[at:]
        return out

    def hmp(self, command: str) -> str:
        """Run one HMP command; answer what it printed."""
        self.sock.sendall(command.encode("utf-8") + b"\n")
        return hmp_output(self._until(_HMP_PROMPT))

    def _qmp_message(self) -> dict:
        while True:
            line = self._until(b"\n").strip()
            if not line:
                continue
            message = json.loads(line)
            if "event" not in message:  # events arrive whenever they happen
                return message

    def _qmp(self, command: str, arguments: dict | None = None) -> dict:
        request: dict = {"execute": command}
        if arguments:
            request["arguments"] = arguments
        self.sock.sendall(json.dumps(request).encode("utf-8") + b"\n")
        reply = self._qmp_message()
        if "error" in reply:
            raise MonitorError(f"{command}: {reply['error'].get('desc', reply['error'])}")
        return reply.get("return", {})

    def capture(self, path: str | Path, device: str | None = None) -> Screen:
        """The screen QEMU shows -- console 0, or `device`'s -- written to
        `path` (a PPM) and read back."""
        path = str(Path(path).resolve())
        if self.kind == "hmp":
            said = self.hmp(f"screendump {hmp_quote(path)}" + (f" {device}" if device else ""))
            if said:
                raise MonitorError(f"screendump: {said}")
        else:
            arguments = {"filename": path}
            if device:
                arguments["device"] = device
            self._qmp("screendump", arguments)
        return read_screen(path)

    def press(self, keys: list[str], hold_ms: int = 100) -> None:
        """Press `keys` together, hold them `hold_ms`, let go."""
        if self.kind == "hmp":
            said = self.hmp(f"sendkey {'-'.join(keys)} {hold_ms}")
            if said:
                raise MonitorError(f"sendkey {'-'.join(keys)}: {said}")
        else:
            self._qmp("send-key", {"keys": [{"type": "qcode", "data": k} for k in keys],
                                   "hold-time": hold_ms})
        # QEMU lets go on a timer of its own; the next press waits for it, so
        # two presses of one key are two, not one long one.
        time.sleep(hold_ms / 1000 + 0.05)

    def type_text(self, text: str, hold_ms: int = 100) -> None:
        for keys in keys_for(text):
            self.press(keys, hold_ms)


# ---------------------------------------------------------------------------
# The probe
# ---------------------------------------------------------------------------


class ProbeResult(NamedTuple):
    ok: bool
    #: What was found, in a sentence; on failure, which stage of the path.
    message: str
    #: The last screen captured, and its text.
    screen: Screen | None
    text: list[str]


def probe(mon: Monitor, scratch: Path, timeout: float = 120.0, poll: float = 2.0,
          device: str | None = None, font: dict[bytes, str] | None = None,
          token: str | None = None) -> ProbeResult:
    """Type `echo <token>` and Enter at the guest, and wait up to `timeout`
    seconds for a line holding only the token to be drawn. `scratch` is the
    file each capture is written to."""
    font = font if font is not None else load_font()
    token = token or "screenprobe%06d" % random.randrange(10**6)
    command = f"echo {token}"
    mon.type_text(command + "\n")
    deadline = time.monotonic() + timeout
    screen, text = None, []
    while True:
        screen = mon.capture(scratch, device)
        text = console_text(screen, font)
        if any(line.strip() == token for line in text):
            return ProbeResult(True, f"typed `{command}`; the answer `{token}` was drawn",
                               screen, text)
        if time.monotonic() >= deadline:
            break
        time.sleep(poll)
    if colours(screen) == 1:
        why = "nothing is drawn on the screen: it is one colour"
    elif not any(sum(c.isalnum() for c in line) >= 3 for line in text):
        why = ("the screen holds no text in the console's font -- it is not the kernel "
               "console (a desktop, or a picture this probe does not know)")
    elif any(command in line for line in text):
        why = (f"the keys reached the screen (`{command}` is drawn) but no line `{token}` "
               "followed: init's shell did not run it, or its answer was not drawn")
    else:
        why = "the screen shows the console, but no key typed reached it"
    return ProbeResult(False, f"after {timeout:.0f}s, {why}", screen, text)


# ---------------------------------------------------------------------------
# Self-test: fake monitors, a fake shell, and the readers against known pixels
# ---------------------------------------------------------------------------


class _FakeGuest:
    """A console a fake monitor captures, with a shell that answers `echo`
    when `answers`; keys reach it when `keys_arrive`."""

    def __init__(self, font, lines, keys_arrive=True, answers=True, blank=False):
        self.font, self.lines = font, list(lines)
        self.keys_arrive, self.answers, self.blank = keys_arrive, answers, blank
        self.typing = ""
        self.lock = threading.Lock()

    def key(self, names: list[str]) -> None:
        if not self.keys_arrive:
            return
        by_name = {v: k for k, v in _KEY_NAMES.items()}
        name = names[-1]
        ch = by_name.get(name, name)
        if names[0] == "shift":
            ch = {v: k for k, v in _SHIFTED.items()}.get(ch, ch.upper())
        with self.lock:
            if ch == "\n":
                self.lines.append("user> " + self.typing)
                if self.answers and self.typing.startswith("echo "):
                    self.lines.append(self.typing[5:])
                self.typing = ""
            else:
                self.typing += ch

    def screen(self) -> Screen:
        with self.lock:
            if self.blank:
                return Screen(320, 96, bytes(320 * 96 * 3))
            shown = self.lines + ["user> " + self.typing]
            return render(shown[-6:], self.font, 320, 96)


def _serve(kind: str, guest: _FakeGuest) -> tuple[int, threading.Thread]:
    """A one-connection fake HMP or QMP monitor for `guest`, on a free
    loopback port."""
    srv = socket.socket()
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    port = srv.getsockname()[1]

    def run() -> None:
        conn, _ = srv.accept()
        srv.close()
        f = conn.makefile("rb")
        if kind == "hmp":
            conn.sendall(b"QEMU 11.0.93 monitor - type 'help' for more information\r\n(qemu) ")
        else:
            conn.sendall(b'{"QMP": {"version": {}, "capabilities": []}}\r\n')
        for raw in f:
            line = raw.decode().strip()
            if kind == "hmp":
                # The echo as QEMU's readline sends it: redrawn a key at a time.
                echo = "".join(line[: i + 1] + "\x1b[K" + "\x1b[D" * (i + 1)
                               for i in range(len(line)))
                words = re.findall(r'"(?:[^"\\]|\\.)*"|\S+', line)
                out = ""
                if words[0] == "screendump":
                    Path(words[1].strip('"')).write_bytes(ppm_bytes(guest.screen()))
                elif words[0] == "sendkey":
                    guest.key(words[1].split("-"))
                else:
                    out = f"unknown command: '{words[0]}'\r\n"
                conn.sendall((echo + "\r\n" + out + "(qemu) ").encode())
            else:
                request = json.loads(line)
                command, arguments = request["execute"], request.get("arguments", {})
                if command == "screendump":
                    Path(arguments["filename"]).write_bytes(ppm_bytes(guest.screen()))
                elif command == "send-key":
                    guest.key([k["data"] for k in arguments["keys"]])
                reply = {"return": {}} if command in (
                    "qmp_capabilities", "screendump", "send-key") else {
                    "error": {"class": "CommandNotFound", "desc": f"no {command}"}}
                # An event between the command and its answer, as QEMU may send.
                conn.sendall(b'{"event": "VNC_CONNECTED", "data": {}}\r\n'
                             + json.dumps(reply).encode() + b"\r\n")
        conn.close()

    t = threading.Thread(target=run, daemon=True)
    t.start()
    return port, t


def self_test() -> int:
    failures: list[str] = []

    def expect(what: str, got, want) -> None:
        ok = got == want
        print(f"  {'ok  ' if ok else 'FAIL'}  {what}")
        if not ok:
            failures.append(what)
            print(f"        got  {got!r}\n        want {want!r}")

    font = load_font()
    expect("the font: every printable glyph but the space, each different", len(font), 94)

    # Reading text off pixels drawn the console's way.
    lines = ["user> echo screenprobe042", "screenprobe042", "", "  [+] Memory manager"]
    shot = render(lines, font, 640, 96)
    expect("text drawn in the font reads back", console_text(shot, font)[:4], lines)
    shot2 = render(["Mixed CASE {}|~ 0123456789"], font, 640, 32, fg=b"\xff\xff\xff",
                   bg=b"\x00\x00\xaa")
    expect("any colours, every kind of character",
           console_text(shot2, font)[0], "Mixed CASE {}|~ 0123456789")
    # A pointer drawn over one cell spoils that cell and no other.
    px = bytearray(shot.pixels)
    for y in range(16, 32):
        at = (y * 640 + 8 * 3) * 3
        px[at : at + 6] = b"\xff\xff\xff" * 2
    read = console_text(Screen(640, 96, bytes(px)), font)
    expect("a cell under the pointer reads `?`, its neighbours stay", read[1], "scr?enprobe042")
    expect("a blank screen is one colour", colours(Screen(64, 32, bytes(64 * 32 * 3))), 1)

    # Formats.
    expect("PPM round trip", parse_ppm(ppm_bytes(shot)), shot)
    expect("a PPM header comment is skipped",
           parse_ppm(b"P6\n# by hand\n2 1\n255\n" + b"\x01" * 6), Screen(2, 1, b"\x01" * 6))
    for bad, why in ((b"P5\n1 1\n255\n\x00", "a grey PPM"), (b"P6\n2 2\n255\n\x00", "short"),
                     (b"P6\n1 1\n65535\n" + b"\x00" * 6, "16-bit samples"), (b"P6\n1", "cut")):
        try:
            parse_ppm(bad)
            expect(f"refused: {why}", "accepted", "refused")
        except ValueError:
            expect(f"refused: {why}", "refused", "refused")
    png = png_bytes(Screen(2, 2, bytes(range(12))))
    ihdr_len = struct.unpack(">I", png[8:12])[0]
    idat_at = 8 + 12 + ihdr_len
    idat_len = struct.unpack(">I", png[idat_at : idat_at + 4])[0]
    expect("PNG: signature, header, rows each led by filter 0",
           (png[:8], png[12:16], zlib.decompress(png[idat_at + 8 : idat_at + 8 + idat_len])),
           (b"\x89PNG\r\n\x1a\n", b"IHDR", b"\x00" + bytes(range(6)) + b"\x00" + bytes(range(6, 12))))
    expect("PNG: every chunk's CRC holds",
           all(struct.unpack(">I", png[at + 8 + n : at + 12 + n])[0]
               == zlib.crc32(png[at + 4 : at + 8 + n]) & 0xFFFFFFFF
               for at, n in ((8, ihdr_len), (idat_at, idat_len))), True)

    # Keys and monitor text.
    expect("keys: letters, capitals, digits, space, symbols, Enter",
           keys_for("eA 1_?\n"),
           [["e"], ["shift", "a"], ["spc"], ["1"], ["shift", "minus"], ["shift", "slash"], ["ret"]])
    try:
        keys_for("é")
        expect("a character no key types is refused", "typed", "refused")
    except ValueError:
        expect("a character no key types is refused", "refused", "refused")
    expect("HMP: the echo and the prompt are not output",
           hmp_output(b"s\x1b[K\x1b[Dsc\x1b[K\x1b[D\x1b[Dscreendump x\r\n(qemu) "), "")
    expect("HMP: an error is output",
           hmp_output(b"sen\x1b[K\x1b[Dsendkey zz\r\ninvalid parameter: zz\r\n(qemu) "),
           "invalid parameter: zz")
    expect("HMP: a path with a space is one argument, and `\\` becomes `/`",
           hmp_quote("E:\\visual studio\\a.ppm"), '"E:/visual studio/a.ppm"')

    # The probe against fake guests, through each kind of monitor.
    scratch_dir = Path(os.environ.get("TEMP", "/tmp")) / f"screen-selftest-{os.getpid()}"
    scratch_dir.mkdir(parents=True, exist_ok=True)
    scratch = scratch_dir / "with space.ppm"
    try:
        cases = (
            ("hmp", {}, True, "drawn"),
            ("qmp", {}, True, "drawn"),
            ("hmp", {"answers": False}, False, "init's shell did not run it"),
            ("qmp", {"keys_arrive": False}, False, "no key typed reached it"),
            ("hmp", {"blank": True}, False, "nothing is drawn"),
        )
        for kind, how, want_ok, said in cases:
            guest = _FakeGuest(font, ["Type 'help' for available commands."], **how)
            port, t = _serve(kind, guest)
            with Monitor(kind, port, timeout=10) as mon:
                r = probe(mon, scratch, timeout=1.0, poll=0.2, font=font, token="screenprobe7")
                label = f"probe over {kind.upper()}, {how or 'a working guest'}"
                expect(f"{label}: {'passes' if want_ok else 'fails'}", r.ok, want_ok)
                expect(f"{label}: says `{said}`", said in r.message, True)
            t.join(5)
        # A refusal comes back as an error, not as a capture of nothing.
        guest = _FakeGuest(font, [])
        port, t = _serve("qmp", guest)
        with Monitor("qmp", port, timeout=10) as mon:
            try:
                mon._qmp("no-such-command")
                expect("QMP: an error reply raises", "returned", "raised")
            except MonitorError:
                expect("QMP: an error reply raises", "raised", "raised")
        t.join(5)
    finally:
        for f in scratch_dir.iterdir():
            f.unlink()
        scratch_dir.rmdir()
    try:
        Monitor("hmp", _free_port(), timeout=2)
        expect("no monitor on the port: an error", "connected", "MonitorError")
    except MonitorError:
        expect("no monitor on the port: an error", "MonitorError", "MonitorError")

    print(f"screen: self-test {'FAILED' if failures else 'passed'} ({len(failures)} failure(s))")
    return 1 if failures else 0


def _free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


# ---------------------------------------------------------------------------


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="verb", required=True)

    def monitor_args(p: argparse.ArgumentParser) -> None:
        g = p.add_mutually_exclusive_group(required=True)
        g.add_argument("--hmp", type=int, metavar="PORT", help="QEMU's text monitor (-monitor tcp:)")
        g.add_argument("--qmp", type=int, metavar="PORT", help="QEMU's JSON monitor (-qmp tcp:)")
        p.add_argument("--device", help="the display device's id (default: console 0)")

    p = sub.add_parser("capture", help="save the screen as a PPM or PNG")
    monitor_args(p)
    p.add_argument("out")
    p = sub.add_parser("type", help="type TEXT at the guest")
    monitor_args(p)
    p.add_argument("text")
    p = sub.add_parser("text", help="the console's text on a captured screen")
    p.add_argument("screen")
    p = sub.add_parser("probe", help="type `echo <token>` and wait for the token to be drawn")
    monitor_args(p)
    p.add_argument("--timeout", type=float, default=120.0)
    p.add_argument("--keep", metavar="DIR", help="keep the last screen there, as probe.png "
                   "and probe.txt")
    args = ap.parse_args(argv)

    if args.verb == "text":
        try:
            print("\n".join(console_text(read_screen(args.screen), load_font())).rstrip("\n"))
        except (OSError, ValueError) as e:
            print(f"screen: {args.screen}: {e}", file=sys.stderr)
            return 2
        return 0

    kind, port = ("hmp", args.hmp) if args.hmp is not None else ("qmp", args.qmp)
    try:
        with Monitor(kind, port) as mon:
            if args.verb == "capture":
                out = Path(args.out)
                if out.suffix.lower() == ".png":
                    shot = mon.capture(out.with_suffix(".ppm"), args.device)
                    write_screen(shot, out)
                    out.with_suffix(".ppm").unlink()
                else:
                    shot = mon.capture(out, args.device)
                print(f"screen: {out}: {shot.width}x{shot.height}")
                return 0
            if args.verb == "type":
                mon.type_text(args.text.replace("\\n", "\n"))
                return 0
            keep = Path(args.keep) if args.keep else None
            scratch_dir = keep or Path(os.environ.get("TEMP", "/tmp"))
            scratch_dir.mkdir(parents=True, exist_ok=True)
            scratch = scratch_dir / f"probe-{os.getpid()}.ppm"
            try:
                result = probe(mon, scratch, timeout=args.timeout, device=args.device)
            finally:
                if scratch.exists():
                    scratch.unlink()
    except MonitorError as e:
        print(f"screen: {e}", file=sys.stderr)
        return 2
    except (OSError, ValueError) as e:
        print(f"screen: {e}", file=sys.stderr)
        return 2
    if keep and result.screen is not None:
        write_screen(result.screen, keep / "probe.png")
        (keep / "probe.txt").write_text("\n".join(result.text).rstrip("\n") + "\n",
                                        encoding="utf-8", newline="\n")
    print(f"screen probe: {'ok' if result.ok else 'FAILED'} -- {result.message}")
    if not result.ok:
        shown = [line for line in result.text if line.strip()][-12:]
        print("  the screen's last lines:\n" + "\n".join("    | " + line for line in shown))
    return 0 if result.ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
