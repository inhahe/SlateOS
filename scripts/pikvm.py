#!/usr/bin/env python3
"""Boot a kernel on the real-hardware test PC through its PiKVM, and watch it.

The test PC (`bare-metal-boot.md` section 9) is reached only through a PiKVM:
a network device that captures its screen, acts as its keyboard, mouse and a
USB drive, and works its power button. This script is how a session drives it:

    python scripts/pikvm.py status
    python scripts/pikvm.py stage <worktree> -o <img> [--cmdline "..."] [--add-cmdline "..."] [--no-linuxdrv] [--wifi FILE]
    python scripts/pikvm.py boot <img>          # upload, restart, pick it at F12
    python scripts/pikvm.py watch --seconds 90  # screenshots until the screen settles
    python scripts/pikvm.py snap <out.png> [--crop X0,Y0,X1,Y1]
    python scripts/pikvm.py power on|off|off-hard|reset-hard

`stage` builds the USB image from a worktree's built kernel: Limine, the kernel
(debug info stripped), the Linux driver processes -- built first, as the boot
test builds them, so that none older than the tree is staged -- as Limine
modules, and `limine.conf` with the command line on its first entry. The
default command line names the PC's spare disk as the only one the kernel may
write (`blk.writable=`, design-decisions 1575) and puts the kernel's log on the
screen (`log.screen=1`), which is the only log there is; `--add-cmdline` adds
to it, `--cmdline` replaces it. For the Linux e1000e run on the PC's I219-V,
add `linuxdrv.card=00:1f.6`; for Linux's mt7925e on its RZ717 WiFi card, which
scans and says the networks it finds, `linuxdrv.wifi=06:00.0` (and
`selftest.only=Linuxdrv` to run only those); `--no-linuxdrv` leaves the
driver processes off, and their self-test with them. The firmware the drivers
load (`linuxdrv/firmware.pin`) is staged with them. `--wifi ~/.slateos-wifi`
stages a WiFi network file -- an `ssid=` line and a `psk=` line, kept outside
the repository -- which mt7925e then joins after its scan, serving the
network to the netstack daemon (design-decisions 1596). The image then holds
the passphrase; nothing here prints it.

The PiKVM's address and login are read from `~/.pikvm` -- JSON with `url`,
`user`, `password` and, for `boot`, `boot_entry_xy`: where on the screen the
firmware's boot menu shows the PiKVM drive -- or from PIKVM_URL, PIKVM_USER and
PIKVM_PASSWORD. They never go into this repository.

Typing (`type`) translates text for the keyboard layout Windows uses on the
PC, US Dvorak (`--layout us` for QWERTY): the PiKVM sends physical keys.
"""
from __future__ import annotations

import argparse
import http.client
import io
import json
import os
import re
import shutil
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import linuxfw  # noqa: E402
import msysbash  # noqa: E402
import selftestflag  # noqa: E402

HERE = Path(__file__).resolve().parent
#: The capture's size; the PiKVM's absolute mouse coordinates span it.
WIDTH, HEIGHT = 1920, 1080
#: The command line `stage` gives the kernel by default.
DEFAULT_CMDLINE = "blk.writable=eui64:0025385181b1cd22 selftest.keep_going=1 log.screen=1"
#: The hello process's boot-medium firmware: its path under the ESP's
#: firmware/ (hello.c's HELLO_BOOT_FW), and the file staged there, which
#: boot-test.sh stages too (its text hello.c's HELLO_BOOT_FW_TEXT; the
#: self-test holds the two together).
HELLO_BOOT_FW = "slateos/hello.fw"
HELLO_BOOT_FW_FILE = Path("linuxdrv") / "hello" / "hello.fw"
#: The name the image is kept under on the PiKVM.
IMAGE_NAME = "slateos-pikvm.img"
#: The WiFi network file's path on the ESP, and its module's name: the
#: kernel places it under /.boot-wifi (linuxdrv::mount_boot_wifi; its
#: WIFI_MODULE and WIFI_NETWORK, which the self-test holds to this).
WIFI_NETWORK = "wifi/network"
#: The most a network file may hold: the driver reads no further
#: (lx_wifi.c's LX_WIFI_NETWORK_MAX).
WIFI_NETWORK_MAX = 512


def network_file_problem(data: bytes) -> str | None:
    """What is wrong with `data` as a WiFi network file, as the driver
    process reads one (linuxdrv/emul/lx_wifi.c, lx_wifi_network_parse):
    lines, a CR before each LF allowed, blank ones passed over, each else
    `ssid=<name>` -- 1 to 32 bytes -- or `psk=<passphrase>` -- 8 to 63
    printable ASCII characters -- each once. None when nothing is. Never
    quotes what it holds: the passphrase is in it."""
    if len(data) > WIFI_NETWORK_MAX:
        return f"it is longer than {WIFI_NETWORK_MAX} bytes"
    named = keyed = False
    for line in data.split(b"\n"):
        if line.endswith(b"\r"):
            line = line[:-1]
        if line.startswith(b"ssid=") and len(line) > 5:
            if named:
                return "it has two ssid= lines"
            if len(line) - 5 > 32:
                return "its ssid= is longer than 32 bytes"
            named = True
        elif line.startswith(b"psk=") and len(line) > 4:
            if keyed:
                return "it has two psk= lines"
            if not 8 <= len(line) - 4 <= 63:
                return "its psk= is not 8 to 63 characters long"
            if any(b < 0x20 or b > 0x7E for b in line[4:]):
                return "its psk= holds a character that is not printable ASCII"
            keyed = True
        elif line:
            return "it has a line that is neither ssid=<name> nor psk=<passphrase>"
    if not (named and keyed):
        return "it has no psk= line" if named else "it has no ssid= line"
    return None


def linux_drivers(worktree: Path) -> tuple[str, ...]:
    """The Linux driver processes `stage` builds and puts beside the kernel:
    boot-test.sh's LINUX_DRIVERS, read from the one list both use -- each a
    Limine module the kernel finds by its module_cmdline, linuxdrv/<name>."""
    text = (worktree / "scripts" / "boot-test.sh").read_text(encoding="utf-8")
    match = re.search(r"^LINUX_DRIVERS=\(([^)]*)\)", text, re.M)
    if not match or not match.group(1).split():
        raise ValueError("boot-test.sh has no LINUX_DRIVERS=(...) list")
    return tuple(match.group(1).split())

# The PiKVM reaches the network by WiFi, which drops now and then: a request
# that is safe to repeat, and that the network loses, is made again -- RETRIES
# times in all, RETRY_WAIT seconds apart; an upload cut off is started again
# from its beginning (the PiKVM keeps no half-written image to resume).
RETRIES = 4
RETRY_WAIT = 5
# The image is streamed from its file in pieces of this size, not read whole.
UPLOAD_CHUNK = 4 << 20
# How long `boot` presses the boot-menu key after power-on: a key pressed
# before the firmware listens is lost, and a cold start reaches the menu in
# 10-25 s.
MENU_KEY_SECONDS = 30
# A lost answer, not kvmd's: a connection refused, reset or timed out, or cut
# off partway through the answer. (urllib's HTTPError, kvmd's own answer, is an
# OSError too, and is told apart first wherever these are caught.)
NETWORK_ERRORS = (OSError, http.client.HTTPException)

# US Dvorak, as Windows on the test PC is set: physical key (its US position)
# -> (character, shifted character).
_DVORAK_KEYS = {
    "Backquote": ("`", "~"), "Digit1": ("1", "!"), "Digit2": ("2", "@"), "Digit3": ("3", "#"),
    "Digit4": ("4", "$"), "Digit5": ("5", "%"), "Digit6": ("6", "^"), "Digit7": ("7", "&"),
    "Digit8": ("8", "*"), "Digit9": ("9", "("), "Digit0": ("0", ")"), "Minus": ("[", "{"),
    "Equal": ("]", "}"),
    "KeyQ": ("'", '"'), "KeyW": (",", "<"), "KeyE": (".", ">"), "KeyR": ("p", "P"),
    "KeyT": ("y", "Y"), "KeyY": ("f", "F"), "KeyU": ("g", "G"), "KeyI": ("c", "C"),
    "KeyO": ("r", "R"), "KeyP": ("l", "L"), "BracketLeft": ("/", "?"), "BracketRight": ("=", "+"),
    "Backslash": (chr(92), "|"),
    "KeyA": ("a", "A"), "KeyS": ("o", "O"), "KeyD": ("e", "E"), "KeyF": ("u", "U"),
    "KeyG": ("i", "I"), "KeyH": ("d", "D"), "KeyJ": ("h", "H"), "KeyK": ("t", "T"),
    "KeyL": ("n", "N"), "Semicolon": ("s", "S"), "Quote": ("-", "_"),
    "KeyZ": (";", ":"), "KeyX": ("q", "Q"), "KeyC": ("j", "J"), "KeyV": ("k", "K"),
    "KeyB": ("x", "X"), "KeyN": ("b", "B"), "KeyM": ("m", "M"), "Comma": ("w", "W"),
    "Period": ("v", "V"), "Slash": ("z", "Z"), "Space": (" ", " "),
}
_US_ROWS = [("`1234567890-=", "~!@#$%^&*()_+"), ("qwertyuiop[]" + chr(92), "QWERTYUIOP{}|"),
            ("asdfghjkl;'", 'ASDFGHJKL:"'), ("zxcvbnm,./", "ZXCVBNM<>?")]
_US_CODES = [["Backquote"] + [f"Digit{d}" for d in "1234567890"] + ["Minus", "Equal"],
             [f"Key{c}" for c in "QWERTYUIOP"] + ["BracketLeft", "BracketRight", "Backslash"],
             [f"Key{c}" for c in "ASDFGHJKL"] + ["Semicolon", "Quote"],
             [f"Key{c}" for c in "ZXCVBNM"] + ["Comma", "Period", "Slash"]]


def layout_map(layout: str) -> dict[str, tuple[str, bool]]:
    """Character -> (physical key, shift) for `layout` ("dvorak" or "us")."""
    keys: dict[str, tuple[str, str]]
    if layout == "dvorak":
        keys = _DVORAK_KEYS
    else:
        keys = {"Space": (" ", " ")}
        for (plain, shifted), codes in zip(_US_ROWS, _US_CODES):
            for code, u, s in zip(codes, plain, shifted):
                keys[code] = (u, s)
    out: dict[str, tuple[str, bool]] = {}
    for code, (u, s) in keys.items():
        out.setdefault(u, (code, False))
        out.setdefault(s, (code, True))
    return out


def key_presses(text: str, layout: str) -> list[tuple[str, bool]]:
    """The key, and whether Shift is held, for each character of `text` on
    `layout`. ValueError for a character the layout cannot type, before any
    is typed, so that no half-typed command is left on the PC."""
    keys = layout_map(layout)
    keys["\n"] = ("Enter", False)
    missing = [ch for ch in text if ch not in keys]
    if missing:
        raise ValueError(f"cannot type {missing[0]!r} on the {layout} layout")
    return [keys[ch] for ch in text]


def screens_alike(last, img) -> bool:
    """Whether two greyscale screenshots show the same screen: the same size,
    and no pixel more than 40 levels apart. A change of size is the PC
    changing video mode, which is a change."""
    from PIL import ImageChops
    if last.size != img.size:
        return False
    return ImageChops.difference(last, img).point(lambda v: 255 if v > 40 else 0).getbbox() is None


def to_absolute(pixel: int, size: int) -> int:
    """A capture pixel as the PiKVM's absolute mouse coordinate (-32768..32767)."""
    return round(pixel / size * 65535) - 32768


def limine_conf(template: str, cmdline: str, modules: tuple[str, ...] = (),
                firmware: tuple[str, ...] = (), wifi: bool = False) -> str:
    """`template` (the repository's limine.conf) with `cmdline` on its first
    entry, the line after that entry's `kernel_path:` -- as boot-test.sh
    gives the test's command line -- and after it each Linux driver process
    of `modules` as a module, `linuxdrv/<name>.elf` named `linuxdrv/<name>`,
    then each firmware file of `firmware` (paths under the ESP's firmware/),
    `firmware/<path>` named so, which the kernel puts at <path> under
    /.boot-firmware, and with `wifi` the WiFi network file, `wifi/network`,
    which it puts under /.boot-wifi."""
    out, entry, done = [], 0, False
    for line in template.splitlines(keepends=True):
        if line.startswith("/"):
            entry += 1
        out.append(line)
        if entry == 1 and not done and line.split()[:1] == ["kernel_path:"]:
            out.append(f"    cmdline: {cmdline}\n")
            for name in modules:
                out.append(f"    module_path: boot():/linuxdrv/{name}.elf\n")
                out.append(f"    module_cmdline: linuxdrv/{name}\n")
            for path in firmware:
                out.append(f"    module_path: boot():/firmware/{path}\n")
                out.append(f"    module_cmdline: firmware/{path}\n")
            if wifi:
                out.append(f"    module_path: boot():/{WIFI_NETWORK}\n")
                out.append(f"    module_cmdline: {WIFI_NETWORK}\n")
            done = True
    if not done:
        raise ValueError("limine.conf has no first entry with a kernel_path")
    return "".join(out)


class PiKvm:
    """The PiKVM's HTTP API, with the login from `~/.pikvm` or the environment."""

    def __init__(self) -> None:
        cfg: dict = {}
        path = Path.home() / ".pikvm"
        if path.exists():
            cfg = json.loads(path.read_text(encoding="utf-8"))
        self.url = os.environ.get("PIKVM_URL", cfg.get("url", ""))
        user = os.environ.get("PIKVM_USER", cfg.get("user", ""))
        password = os.environ.get("PIKVM_PASSWORD", cfg.get("password", ""))
        if not (self.url and user and password):
            sys.exit(f"pikvm: no address or login: write {path} (url, user, password) "
                     "or set PIKVM_URL, PIKVM_USER, PIKVM_PASSWORD")
        self.headers = {"X-KVMD-User": user, "X-KVMD-Passwd": password}
        self.boot_entry_xy = cfg.get("boot_entry_xy")
        # The PiKVM's certificate is its own, self-signed.
        self.ctx = ssl.create_default_context()
        self.ctx.check_hostname = False
        self.ctx.verify_mode = ssl.CERT_NONE

    def call(self, method: str, path: str, body=None, timeout: float = 30,
             repeatable: bool = False) -> bytes:
        """One request to kvmd: its answer's body. An answer kvmd gives, an
        error status included, is final (HTTPError). A request the network
        loses is made again (RETRIES in all) only when `repeatable`: when
        doing it twice does no more than doing it once -- a read, a key or
        button sent with the state it is to be in, a move to a position, a
        power action (kvmd presses only what changes the state). Anything else
        -- a key pressed and released, a drive connected -- may have been
        carried out before its answer was lost, and is not repeated."""
        tries = RETRIES if repeatable else 1
        for attempt in range(1, tries + 1):
            req = urllib.request.Request(self.url + path, data=body, method=method, headers=self.headers)
            try:
                with urllib.request.urlopen(req, context=self.ctx, timeout=timeout) as r:
                    return r.read()
            except urllib.error.HTTPError:
                raise
            except NETWORK_ERRORS as e:
                if attempt == tries:
                    raise
                print(f"pikvm: {method} {path.split('?')[0]}: {e}; again in {RETRY_WAIT} s",
                      file=sys.stderr)
                time.sleep(RETRY_WAIT)
        raise AssertionError("unreachable")

    def json(self, method: str, path: str, **kw) -> dict:
        kw.setdefault("repeatable", method == "GET")
        return json.loads(self.call(method, path, **kw))["result"]

    def key(self, code: str, state: bool | None = None) -> None:
        """Press and release `code`, or with `state` hold it down (True) or
        let it go (False) -- which bears repeating, as a press and release
        does not."""
        q = f"key={code}" + ("" if state is None else f"&state={'true' if state else 'false'}")
        self.call("POST", f"/api/hid/events/send_key?{q}", repeatable=state is not None)

    def move(self, x: int, y: int) -> None:
        self.call("POST", f"/api/hid/events/send_mouse_move?to_x={to_absolute(x, WIDTH)}"
                          f"&to_y={to_absolute(y, HEIGHT)}", repeatable=True)

    def click(self, button: str = "left") -> None:
        self.call("POST", f"/api/hid/events/send_mouse_button?button={button}&state=true",
                  repeatable=True)
        time.sleep(0.08)
        self.call("POST", f"/api/hid/events/send_mouse_button?button={button}&state=false",
                  repeatable=True)

    def type_text(self, text: str, layout: str) -> None:
        """Type `text`, each key held down and let go as two events, so that
        one repeated after a lost answer types nothing twice."""
        for code, shift in key_presses(text, layout):
            if shift:
                self.key("ShiftLeft", True)
            self.key(code, True)
            self.key(code, False)
            if shift:
                self.key("ShiftLeft", False)

    def snapshot(self) -> bytes:
        return self.call("GET", "/api/streamer/snapshot", repeatable=True)

    def leds(self) -> dict:
        return self.json("GET", "/api/atx")["leds"]

    def power(self, action: str) -> None:
        """Press the PC's power button as `action` says (`on`, `off`,
        `off_hard`, `reset_hard`), returning once the press is over
        (`wait=1`): `off_hard` holds the button for seconds, and kvmd refuses
        another press meanwhile (409) -- the power light goes out before the
        hold ends. A refusal is tried again for a few seconds all the same."""
        for attempt in range(1, 11):
            try:
                self.call("POST", f"/api/atx/power?action={action}&wait=1", timeout=60,
                          repeatable=True)
                return
            except urllib.error.HTTPError as e:
                if e.code != 409 or attempt == 10:
                    raise
                time.sleep(1)

    def set_connected(self, connected: bool) -> None:
        """Connect the virtual drive to the PC, or disconnect it, unless it is
        so already -- kvmd refuses either then (400). Not repeated on a lost
        answer, which may have been carried out: the drive's state is read
        again instead, and the request made again only if it did not take."""
        for attempt in range(1, RETRIES + 1):
            if self.json("GET", "/api/msd")["drive"]["connected"] == connected:
                return
            try:
                self.call("POST", f"/api/msd/set_connected?connected={int(connected)}")
                return
            except urllib.error.HTTPError:
                raise
            except NETWORK_ERRORS as e:
                if attempt == RETRIES:
                    raise
                print(f"pikvm: set_connected: {e}; looking again in {RETRY_WAIT} s", file=sys.stderr)
                time.sleep(RETRY_WAIT)

    def wait_idle(self, seconds: float = 120) -> None:
        """Wait while the virtual drive's storage is busy -- as it stays for a
        while after an upload cut off, until kvmd sees the connection gone."""
        end = time.monotonic() + seconds
        while self.json("GET", "/api/msd").get("busy") and time.monotonic() < end:
            time.sleep(2)

    def upload(self, image: Path) -> None:
        """Put `image` on the virtual drive as a read-only flash drive, connected."""
        # Read first: a missing or unreadable image fails here, before the
        # PiKVM's drive is touched, not as a "cut off" upload.
        size = image.stat().st_size
        with image.open("rb"):
            pass
        self.set_connected(False)
        for attempt in range(1, RETRIES + 1):
            self.wait_idle()
            try:
                self.call("POST", f"/api/msd/remove?image={IMAGE_NAME}", repeatable=True)
            except urllib.error.HTTPError:
                pass  # No earlier image of that name: nothing to remove.
            try:
                self.write_image(image, size)
                break
            except urllib.error.HTTPError as e:
                # Its body, to tell one refusal from another; a body the
                # network loses is said as nothing, not raised from here.
                try:
                    said = e.read()[:300]
                except NETWORK_ERRORS:
                    said = b""
                if e.code == 409 and attempt < RETRIES:
                    print(f"pikvm: the drive is busy; trying again in {RETRY_WAIT} s", file=sys.stderr)
                    time.sleep(RETRY_WAIT)
                    continue
                # An image of this script's own name kvmd still keeps -- one
                # an upload left as the network dropped, which the removal
                # above found busy: removed, as the loop's next turn does.
                if b"MsdImageExistsError" in said and attempt < RETRIES:
                    print(f"pikvm: {IMAGE_NAME} is still stored; removing it and starting again",
                          file=sys.stderr)
                    continue
                sys.exit(f"pikvm: the PiKVM refused the image: {e.code} {said!r}")
            except NETWORK_ERRORS as e:
                if attempt == RETRIES:
                    raise
                print(f"pikvm: the upload was cut off ({e}); starting it again", file=sys.stderr)
                time.sleep(RETRY_WAIT)
        self.call("POST", f"/api/msd/set_params?image={IMAGE_NAME}&cdrom=0&rw=0", repeatable=True)
        self.set_connected(True)
        for _ in range(60):
            if self.json("GET", "/api/msd")["drive"]["image"]["complete"]:
                return
            time.sleep(1)
        sys.exit("pikvm: the image never showed as complete")

    def write_image(self, image: Path, size: int) -> None:
        """Send `image`, `size` bytes, to the virtual drive's storage as
        IMAGE_NAME, streamed from the file, saying how far it has got. kvmd's
        refusal raises HTTPError; a connection lost, one of NETWORK_ERRORS."""
        start = time.monotonic()
        headers = {**self.headers, "Content-Length": str(size),
                   "Content-Type": "application/octet-stream"}
        with image.open("rb") as fh:
            req = urllib.request.Request(self.url + f"/api/msd/write?image={IMAGE_NAME}",
                                         data=Progress(fh, size), method="POST", headers=headers)
            with urllib.request.urlopen(req, context=self.ctx, timeout=120) as r:
                r.read()
        print(f"pikvm: {size >> 20} MiB sent in {time.monotonic() - start:.0f} s", flush=True)


class Progress:
    """A file being sent, read in UPLOAD_CHUNK pieces whatever the reader
    asks for, saying every 10 s how far it has got."""

    def __init__(self, fh, size: int) -> None:
        self.fh, self.size, self.sent = fh, size, 0
        self.said = time.monotonic()

    def read(self, _n: int = -1) -> bytes:
        block = self.fh.read(UPLOAD_CHUNK)
        self.sent += len(block)
        if time.monotonic() - self.said >= 10:
            print(f"pikvm: {self.sent >> 20} of {self.size >> 20} MiB sent", flush=True)
            self.said = time.monotonic()
        return block


def build_linux_drivers(worktree: Path, names: tuple[str, ...]) -> None:
    """The layer's Rust half, then each driver process of `names`, built in
    `worktree` as boot-test.sh's build_linux_drivers builds them -- under
    Git's MSYS bash, as boot-test.sh runs, not whatever bash PATH finds
    first (WSL's, on this machine) -- so that none older than the tree is
    staged. SystemExit, saying which, for one that does not build, or a host
    that cannot build them at all, which `--no-linuxdrv` stages without."""
    cannot = "pikvm: this host cannot build the Linux driver processes ({}); --no-linuxdrv stages without them"
    try:
        kit = subprocess.run(["cargo", "build", "--release", "--quiet"],
                             cwd=worktree / "linuxdrv" / "kit").returncode
        bash = msysbash.bash()
    except (OSError, RuntimeError) as e:
        sys.exit(cannot.format(e))
    if kit:
        sys.exit("pikvm: the Linux driver layer's Rust half (linuxdrv/kit) did not build (above)")
    for name in names:
        rc = subprocess.run([bash, "scripts/linux-tree.sh", "driver", name], cwd=worktree).returncode
        if rc == 3:
            sys.exit(cannot.format("above"))
        if rc:
            sys.exit(f"pikvm: the Linux driver process {name} did not build (above)")


def stage(worktree: Path, out: Path, cmdline: str, linuxdrv: bool = True,
          wifi: Path | None = None) -> None:
    """Build the USB image of `worktree`'s built kernel at `out` -- its Linux
    driver processes built first, unless `linuxdrv` is false -- from an ESP
    tree laid out in a scratch directory that is gone afterwards; with `wifi`,
    the WiFi network file there, checked first (network_file_problem), which
    the image then carries -- the passphrase in it, said nowhere
    (design-decisions 1596)."""
    names = linux_drivers(worktree) if linuxdrv else ()
    network = None
    if wifi is not None:
        if not linuxdrv:
            sys.exit("pikvm: --wifi is for the WiFi driver process, and --no-linuxdrv leaves it off")
        # Without a card named, no join runs and the image would carry the
        # passphrase for nothing.
        if "linuxdrv.wifi=" not in cmdline:
            sys.exit("pikvm: --wifi is joined by the card linuxdrv.wifi= names, and the command "
                     "line names none (add it with --add-cmdline)")
        wifi = wifi.expanduser()
        try:
            network = wifi.read_bytes()
        except OSError as e:
            sys.exit(f"pikvm: the WiFi network file {wifi} could not be read: {e.strerror}")
        problem = network_file_problem(network)
        if problem:
            sys.exit(f"pikvm: the WiFi network file {wifi} is not one: {problem}")
    for option in ("linuxdrv.card=", "linuxdrv.wifi="):
        if not linuxdrv and option in cmdline:
            sys.exit(f"pikvm: {option[:-1]} asks for a Linux driver run, and --no-linuxdrv "
                     "leaves the driver processes off")
    # What is staged besides the drivers, there before minutes of building.
    for needed in (kernel_path(worktree), limine_path(worktree), worktree / "limine.conf"):
        if not needed.exists():
            sys.exit(f"pikvm: nothing to stage at {needed}")
    firmware: tuple[str, ...] = ()
    if names:
        # What linuxdrv/firmware.pin names, fetched and checked against it --
        # before minutes of building, as what is staged beside them is.
        firmware = tuple(p.as_posix() for p in linuxfw.fetch(worktree))
        build_linux_drivers(worktree, names)
    with tempfile.TemporaryDirectory(prefix="pikvm-esp-") as scratch:
        stage_esp(worktree, Path(scratch), cmdline, names, firmware=firmware, wifi=network)
        subprocess.run([sys.executable, str(HERE / "build-usb-image.py"), "--source", scratch,
                        "--output", str(out)], check=True)


def kernel_path(worktree: Path) -> Path:
    """`worktree`'s built kernel, as `stage` stages it."""
    return worktree / "target" / "x86_64-unknown-none" / "debug" / "kernel"


def limine_path(worktree: Path) -> Path:
    """The Limine `stage` puts on the ESP: `worktree`'s, or else os-lane-a's
    beside it (a fresh worktree has none until provisioned)."""
    limine = worktree / "limine" / "BOOTX64.EFI"
    if not limine.exists():
        limine = worktree.parent / "os-lane-a" / "limine" / "BOOTX64.EFI"
    return limine


def stage_esp(worktree: Path, esp: Path, cmdline: str, modules: tuple[str, ...],
              strip_kernel: bool = True, firmware: tuple[str, ...] = (),
              wifi: bytes | None = None) -> None:
    """Lay out an ESP in `esp`: Limine, `worktree`'s built kernel (stripped of
    its debug sections when `strip_kernel` and llvm-strip is there), the
    Linux driver processes `modules` names from build/linuxdrv/ -- each
    required -- the firmware files `firmware` names from build/firmware/
    (linuxfw.py's; each required) under firmware/, with hello's own test
    file there when hello is staged, as boot-test.sh stages it, `wifi` as
    the WiFi network file (WIFI_NETWORK) when given, and its limine.conf with
    `cmdline` on the first entry, the driver processes, the firmware and the
    network file as its modules."""
    (esp / "EFI" / "BOOT").mkdir(parents=True)
    (esp / "boot").mkdir()
    shutil.copy(limine_path(worktree), esp / "EFI" / "BOOT" / "BOOTX64.EFI")
    kernel = kernel_path(worktree)
    strip = (Path.home() / ".rustup" / "toolchains" / "stable-x86_64-pc-windows-gnu" / "lib"
             / "rustlib" / "x86_64-pc-windows-gnu" / "bin" / "llvm-strip.exe")
    if strip_kernel and strip.exists():
        subprocess.run([str(strip), "--strip-debug", str(kernel), "-o", str(esp / "boot" / "kernel")],
                       check=True)
    else:
        shutil.copy(kernel, esp / "boot" / "kernel")
    built = worktree / "build" / "linuxdrv"
    missing = [name for name in modules if not (built / f"{name}.elf").exists()]
    if missing:
        raise FileNotFoundError(f"no build/linuxdrv/{missing[0]}.elf to stage")
    if modules:
        (esp / "linuxdrv").mkdir()
        for name in modules:
            shutil.copy(built / f"{name}.elf", esp / "linuxdrv" / f"{name}.elf")
    fetched = worktree / "build" / "firmware"
    missing = [path for path in firmware if not (fetched / path).is_file()]
    if missing:
        raise FileNotFoundError(f"no build/firmware/{missing[0]} to stage")
    staged = list(firmware)
    for path in firmware:
        (esp / "firmware" / path).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(fetched / path, esp / "firmware" / path)
    if "hello" in modules and HELLO_BOOT_FW not in staged:
        (esp / "firmware" / HELLO_BOOT_FW).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(worktree / HELLO_BOOT_FW_FILE, esp / "firmware" / HELLO_BOOT_FW)
        staged.append(HELLO_BOOT_FW)
    if wifi is not None:
        (esp / WIFI_NETWORK).parent.mkdir(parents=True, exist_ok=True)
        (esp / WIFI_NETWORK).write_bytes(wifi)
    template = (worktree / "limine.conf").read_text(encoding="utf-8")
    (esp / "limine.conf").write_text(
        limine_conf(template, cmdline, modules, tuple(staged), wifi=wifi is not None),
        encoding="utf-8", newline="\n")


def boot(kvm: PiKvm, image: Path, menu_key: str) -> None:
    """Upload `image`, restart the PC, and pick the PiKVM drive in its boot
    menu. The click is checked as far as pictures allow: one the menu does
    not take (the screen unchanged) is made again. A screen that changes is
    taken for the boot starting -- which it may not be, if what settled was a
    logo before the menu; `watch` or `snap` shows which."""
    if not kvm.boot_entry_xy:
        sys.exit("pikvm: boot needs boot_entry_xy in ~/.pikvm: where the boot menu shows the PiKVM drive")
    kvm.upload(image)
    if kvm.leds()["power"]:
        kvm.power("off_hard")
        for _ in range(15):
            time.sleep(1)
            if not kvm.leds()["power"]:
                break
    time.sleep(2)
    kvm.power("on")
    for _ in range(MENU_KEY_SECONDS * 4):
        kvm.key(menu_key)
        time.sleep(0.25)
    click_entry(kvm, tuple(kvm.boot_entry_xy))


def click_entry(kvm: PiKvm, xy: tuple) -> None:
    """Click the boot menu's entry at `xy` once the screen has settled -- the
    menu waits for a choice -- and again if the screen does not change."""
    if wait_still(kvm, 30) is None:
        print("pikvm: no settled screen to check the click against (it kept "
              "changing, or no picture can be had): clicking once, unchecked", flush=True)
        kvm.move(*xy)
        time.sleep(0.3)
        kvm.click()
        return
    for _ in range(3):
        kvm.move(*xy)
        time.sleep(0.5)
        # Taken after the move: a pointer drawn on the entry, or the entry
        # lit up under it, is part of the picture, not a sign the click took.
        before = snapshot_image(kvm)
        kvm.click()
        if before is None or changed_from(kvm, before, 10):
            return
        print("pikvm: the boot menu is still up; clicking the entry again", flush=True)
    sys.exit("pikvm: the boot menu did not take the click")


def picture(kvm: PiKvm):
    """The screen: (the JPEG kvmd sent, the picture in greyscale), or a
    string saying why there is none -- no Pillow, the PC off or between
    modes, the network, a picture cut short."""
    try:
        from PIL import Image
    except ImportError:
        return "Pillow is not installed"
    try:
        data = kvm.snapshot()
    except urllib.error.HTTPError:
        return "the PC is off or between modes"
    except NETWORK_ERRORS as e:
        return str(e)
    try:
        return data, Image.open(io.BytesIO(data)).convert("L")
    except OSError as e:  # PIL's UnidentifiedImageError: a picture cut short
        return f"a picture that does not decode ({e})"


def snapshot_image(kvm: PiKvm):
    """The screen in greyscale; None when no picture can be had."""
    got = picture(kvm)
    return None if isinstance(got, str) else got[1]


def wait_still(kvm: PiKvm, seconds: float):
    """The screen once two pictures a second apart show it the same, within
    `seconds`; None if it never settles, or no picture can be had."""
    last = None
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        img = snapshot_image(kvm)
        if img is not None and last is not None and screens_alike(last, img):
            return img
        last = img
        time.sleep(1)
    return None


def changed_from(kvm: PiKvm, before, seconds: float) -> bool:
    """Whether the screen comes to show something other than `before`
    within `seconds`."""
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        img = snapshot_image(kvm)
        if img is not None and not screens_alike(before, img):
            return True
        time.sleep(1)
    return False


def watch(kvm: PiKvm, seconds: float, every: float, out_dir: Path) -> None:
    """Screenshot every `every` seconds for up to `seconds`; say when the
    screen stops changing (a hang, or the end of a boot)."""
    out_dir.mkdir(parents=True, exist_ok=True)
    last, still, n = None, 0.0, 0
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        time.sleep(every)
        got = picture(kvm)
        if isinstance(got, str):
            print(f"pikvm: no picture ({got})")
            continue
        data, img = got
        n += 1
        shot = out_dir / f"watch-{n:03}.jpg"
        shot.write_bytes(data)
        if last is not None and screens_alike(last, img):
            still += every
        else:
            still = 0.0
        last = img
        print(f"{shot}  {'unchanged ' + str(int(still)) + ' s' if still else 'changing'}")
    print(f"pikvm: last screenshot {out_dir / f'watch-{n:03}.jpg'}")


def self_test() -> int:
    failures = []

    def check(what, got, want):
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    dv = layout_map("dvorak")
    check("dvorak: p is the US R key", dv["p"], ("KeyR", False))
    check("dvorak: a backslash is unshifted", dv[chr(92)], ("Backslash", False))
    check("dvorak: { is shifted [", dv["{"], ("Minus", True))
    us = layout_map("us")
    check("us: p is P", us["p"], ("KeyP", False))
    check("us: ? is shifted /", us["?"], ("Slash", True))
    check("every printable character types in both layouts",
          [c for c in map(chr, range(32, 127)) if c not in dv or c not in us], [])
    check("coordinates: the corners", (to_absolute(0, WIDTH), to_absolute(WIDTH, WIDTH)), (-32768, 32767))
    check("typing: a line, its Enter", key_presses("ls\n", "us"),
          [("KeyL", False), ("KeyS", False), ("Enter", False)])
    try:
        key_presses("ls\tx", "us")
        check("typing: a tab is refused before anything is typed", "typed", "refused")
    except ValueError:
        pass
    try:
        from PIL import Image
    except ImportError:
        print("pikvm self-test: no PIL, so watch's screen comparison is not checked")
    else:
        black = Image.new("L", (4, 4))
        check("screens: one picture is alike itself", screens_alike(black, black.copy()), True)
        check("screens: a changed pixel is a change",
              screens_alike(black, Image.new("L", (4, 4), 200)), False)
        check("screens: a new video mode is a change, not an error",
              screens_alike(black, Image.new("L", (8, 4))), False)
    template = "timeout: 3\n\n/OS Kernel\n    protocol: limine\n    kernel_path: boot():/boot/kernel\n\n/OS Kernel (recovery)\n    kernel_path: boot():/boot/kernel\n    cmdline: single recovery\n"
    conf = limine_conf(template, "a=1 b")
    check("limine.conf: the first entry gets the line", conf.count("cmdline: a=1 b"), 1)
    check("limine.conf: right after its kernel_path",
          "kernel_path: boot():/boot/kernel\n    cmdline: a=1 b\n" in conf, True)
    check("limine.conf: in the first entry, not the recovery one",
          conf.find("cmdline: a=1 b") < conf.find("/OS Kernel (recovery)"), True)
    check("limine.conf: the recovery entry keeps its own", conf.count("cmdline: single recovery"), 1)
    check("limine.conf: no driver process, no module", "module_path" in conf, False)
    conf = limine_conf(template, "a=1 b", ("hello", "e1000e"))
    check("limine.conf: each driver process a module after the cmdline, named as the kernel finds it",
          "    cmdline: a=1 b\n"
          "    module_path: boot():/linuxdrv/hello.elf\n    module_cmdline: linuxdrv/hello\n"
          "    module_path: boot():/linuxdrv/e1000e.elf\n    module_cmdline: linuxdrv/e1000e\n" in conf, True)
    check("limine.conf: the modules in the first entry only",
          conf.rfind("module_path") < conf.find("/OS Kernel (recovery)"), True)
    check("the driver processes are boot-test.sh's list",
          {"hello", "e1000e"} <= set(linux_drivers(HERE.parent)), True)
    hello_c = (HERE.parent / "linuxdrv" / "hello" / "hello.c").read_text(encoding="utf-8")
    boot_test = (HERE / "boot-test.sh").read_text(encoding="utf-8")
    text = re.search(r'^#define HELLO_BOOT_FW_TEXT "((?:[^"\\]|\\.)*)"$', hello_c, re.M)
    check("hello's boot-medium firmware: its path hello.c's, its file's bytes the text hello.c "
          "checks, and boot-test.sh staging that file there",
          (f'#define HELLO_BOOT_FW "{HELLO_BOOT_FW}"' in hello_c,
           text is not None and (HERE.parent / HELLO_BOOT_FW_FILE).read_bytes()
           == text.group(1).encode().decode("unicode_escape").encode(),
           f'"$PROJECT_ROOT/{HELLO_BOOT_FW_FILE.as_posix()}" "$ESP_DIR/firmware/{HELLO_BOOT_FW}"'
           in boot_test),
          (True, True, True))
    with tempfile.TemporaryDirectory(prefix="pikvm-selftest-") as tmp:
        wt = Path(tmp) / "wt"
        for rel, data in [("limine/BOOTX64.EFI", b"efi"),
                          ("target/x86_64-unknown-none/debug/kernel", b"kernel"),
                          ("limine.conf", template.encode()),
                          ("build/linuxdrv/hello.elf", b"hello"),
                          ("build/linuxdrv/e1000e.elf", b"e1000e"),
                          (HELLO_BOOT_FW_FILE.as_posix(), b"hello's firmware\n")]:
            (wt / rel).parent.mkdir(parents=True, exist_ok=True)
            (wt / rel).write_bytes(data)
        esp = Path(tmp) / "esp"
        stage_esp(wt, esp, "a=1", ("hello", "e1000e"), strip_kernel=False)
        conf = (esp / "limine.conf").read_text(encoding="utf-8")
        check("stage: each driver process copied beside the kernel",
              [(esp / "linuxdrv" / f"{n}.elf").read_bytes() for n in ("hello", "e1000e")],
              [b"hello", b"e1000e"])
        check("stage: each a module of the first entry",
              conf.count("module_cmdline: linuxdrv/hello") + conf.count("module_cmdline: linuxdrv/e1000e"), 2)
        check("stage: hello's boot-medium firmware beside it, a module named by its path",
              ((esp / "firmware" / HELLO_BOOT_FW).read_bytes(),
               "    module_path: boot():/firmware/slateos/hello.fw\n"
               "    module_cmdline: firmware/slateos/hello.fw\n" in conf),
              (b"hello's firmware\n", True))
        fw = "mediatek/mt7925/x.bin"
        (wt / "build/firmware" / fw).parent.mkdir(parents=True)
        (wt / "build/firmware" / fw).write_bytes(b"fw")
        esp4 = Path(tmp) / "esp4"
        stage_esp(wt, esp4, "a=1", ("hello",), strip_kernel=False, firmware=(fw,))
        conf = (esp4 / "limine.conf").read_text(encoding="utf-8")
        check("stage: the pinned firmware copied under firmware/, each a module after the drivers",
              ((esp4 / "firmware" / fw).read_bytes(),
               conf.find("module_cmdline: linuxdrv/hello") < conf.find(f"module_cmdline: firmware/{fw}"),
               conf.count("module_cmdline: firmware/")),
              (b"fw", True, 2))
        try:
            stage_esp(wt, Path(tmp) / "esp5", "a=1", (), strip_kernel=False, firmware=("nonesuch.bin",))
            check("stage: firmware asked for and not fetched is refused", "staged", "refused")
        except FileNotFoundError:
            pass
        network = b"ssid=home\r\npsk=v+r%a8pqq\r\n"
        esp6 = Path(tmp) / "esp6"
        stage_esp(wt, esp6, "a=1", ("hello",), strip_kernel=False, firmware=(fw,), wifi=network)
        conf = (esp6 / "limine.conf").read_text(encoding="utf-8")
        check("stage: the WiFi network file copied as wifi/network, a module after the firmware",
              ((esp6 / "wifi" / "network").read_bytes(),
               "    module_path: boot():/wifi/network\n    module_cmdline: wifi/network\n" in conf,
               conf.find(f"module_cmdline: firmware/{fw}") < conf.find("module_cmdline: wifi/network"),
               conf.rfind("module_path") < conf.find("/OS Kernel (recovery)")),
              (network, True, True, True))
        check("stage: no WiFi network file asked for, none staged",
              ((esp4 / "wifi").exists(), "wifi/network" in (esp4 / "limine.conf").read_text(encoding="utf-8")),
              (False, False))
        esp2 = Path(tmp) / "esp2"
        stage_esp(wt, esp2, "a=1", (), strip_kernel=False)
        check("stage: none asked for, none staged",
              ((esp2 / "linuxdrv").exists(), (esp2 / "firmware").exists(),
               "module_path" in (esp2 / "limine.conf").read_text(encoding="utf-8")),
              (False, False, False))
        (wt / "build/linuxdrv/e1000e.elf").unlink()
        try:
            stage_esp(wt, Path(tmp) / "esp3", "a=1", ("hello", "e1000e"), strip_kernel=False)
            check("stage: a driver process asked for and not built is refused", "staged", "refused")
        except FileNotFoundError:
            pass
    for label, data, want in [
        ("a name and a passphrase", b"ssid=home\npsk=12345678\n", None),
        ("CRLF lines, blank ones, either order", b"\r\npsk=a b%c+d!\r\n\r\nssid=x\r\n", None),
        ("no passphrase", b"ssid=home\n", "it has no psk= line"),
        ("no name", b"psk=12345678\n", "it has no ssid= line"),
        ("a passphrase of 7", b"ssid=h\npsk=1234567\n", "its psk= is not 8 to 63 characters long"),
        ("a passphrase of 64", b"ssid=h\npsk=" + b"1" * 64 + b"\n", "its psk= is not 8 to 63 characters long"),
        ("a tab in the passphrase", b"ssid=h\npsk=1234\t5678\n",
         "its psk= holds a character that is not printable ASCII"),
        ("a name of 33 bytes", b"ssid=" + b"n" * 33 + b"\npsk=12345678\n", "its ssid= is longer than 32 bytes"),
        ("two names", b"ssid=a\nssid=b\npsk=12345678\n", "it has two ssid= lines"),
        ("an empty name", b"ssid=\npsk=12345678\n",
         "it has a line that is neither ssid=<name> nor psk=<passphrase>"),
        ("another line", b"ssid=h\npsk=12345678\nhidden=1\n",
         "it has a line that is neither ssid=<name> nor psk=<passphrase>"),
        ("good lines past 512 bytes", b"ssid=h\npsk=12345678\n" + b"\n" * 500,
         "it is longer than 512 bytes"),
        ("good lines of exactly 512 bytes", b"ssid=h\npsk=12345678\n" + b"\n" * 492, None),
    ]:
        check(f"WiFi network file: {label}", network_file_problem(data), want)
    lx_wifi = (HERE.parent / "linuxdrv" / "emul" / "lx_wifi.c").read_text(encoding="utf-8")
    check("WiFi network file: each refusal is the driver's own words, so the two read alike",
          all(f'"{why}"' in lx_wifi for why in (
              "it has no psk= line", "it has no ssid= line", "it has two ssid= lines",
              "it has two psk= lines", "its ssid= is longer than 32 bytes",
              "its psk= is not 8 to 63 characters long",
              "its psk= holds a character that is not printable ASCII",
              "it has a line that is neither ssid=<name> nor psk=<passphrase>",
              f"it is longer than {WIFI_NETWORK_MAX} bytes")), True)
    check("WiFi network file: its most is the driver's",
          f"#define LX_WIFI_NETWORK_MAX {WIFI_NETWORK_MAX}" in lx_wifi, True)
    linuxdrv_rs = (HERE.parent / "kernel" / "src" / "linuxdrv.rs").read_text(encoding="utf-8")
    prefix, name = WIFI_NETWORK.split("/")
    check("WiFi network file: its module's name is the kernel's prefix and file name",
          (f'const WIFI_MODULE: &[u8] = b"{prefix}/";' in linuxdrv_rs,
           f'const WIFI_NETWORK: &str = "{name}";' in linuxdrv_rs), (True, True))
    for label, cmdline, flags in [
        ("without a card named, --wifi is refused", "a=1", {}),
        ("with --no-linuxdrv, --wifi is refused", "a=1 linuxdrv.wifi=06:00.0", {"linuxdrv": False}),
    ]:
        try:
            stage(HERE.parent, Path("nonesuch.img"), cmdline, wifi=Path("nonesuch"), **flags)
            check(f"stage: {label}", "staged", "refused")
        except SystemExit as e:
            check(f"stage: {label}", "--wifi" in str(e), True)
    try:
        limine_conf("timeout: 3\n", "x")
        check("limine.conf without an entry is refused", "taken", "refused")
    except ValueError:
        pass
    self_test_requests(check)
    self_test_screens(check)
    for f in failures:
        print("FAIL", f)
    print("pikvm self-test:", "FAILED" if failures else "passed")
    return 1 if failures else 0


def self_test_requests(check) -> None:
    """call()'s retries, set_connected(), type_text() and upload()'s first
    step, against a stubbed urlopen and a stubbed PiKVM: no network."""
    from unittest import mock
    global RETRY_WAIT
    saved_wait, RETRY_WAIT = RETRY_WAIT, 0
    try:
        kvm = PiKvm.__new__(PiKvm)
        kvm.url, kvm.headers, kvm.ctx = "https://pikvm.test", {}, None

        class Answer:
            def __init__(self, body):
                self.body = body

            def __enter__(self):
                return self

            def __exit__(self, *exc):
                return False

            def read(self):
                return self.body

        def lost_then(n, body=b"ok", error=None):
            """An urlopen whose first `n` calls lose the network."""
            calls = []

            def urlopen(req, **_kw):
                calls.append(req.full_url)
                if len(calls) <= n:
                    raise error or urllib.error.URLError("network down")
                return Answer(body)
            return urlopen, calls

        urlopen, calls = lost_then(2)
        with mock.patch.object(urllib.request, "urlopen", urlopen):
            got = kvm.call("GET", "/api/x", repeatable=True)
        check("call: a repeatable request lost twice is made a third time", (got, len(calls)), (b"ok", 3))
        urlopen, calls = lost_then(1)
        with mock.patch.object(urllib.request, "urlopen", urlopen):
            try:
                kvm.call("POST", "/api/hid/events/send_key?key=KeyA")
                check("call: a lost press-and-release is not made again", "made again", "raised")
            except urllib.error.URLError:
                check("call: a lost press-and-release is not made again", len(calls), 1)
        urlopen, calls = lost_then(1, error=http.client.IncompleteRead(b"half"))
        with mock.patch.object(urllib.request, "urlopen", urlopen):
            got = kvm.call("GET", "/api/streamer/snapshot", repeatable=True)
        check("call: an answer cut off partway is a lost answer", (got, len(calls)), (b"ok", 2))
        urlopen, calls = lost_then(9)
        with mock.patch.object(urllib.request, "urlopen", urlopen):
            try:
                kvm.call("GET", "/api/x", repeatable=True)
                check("call: gives up after RETRIES", "answered", "raised")
            except urllib.error.URLError:
                check("call: gives up after RETRIES", len(calls), RETRIES)

        class Stub:
            """A PiKVM that records what it is asked."""

            def __init__(self, states, lose_post=False):
                self.states, self.lose_post, self.log = list(states), lose_post, []

            def json(self, method, path, **_kw):
                connected = self.states.pop(0) if len(self.states) > 1 else self.states[0]
                return {"drive": {"connected": connected}, "busy": False}

            def call(self, method, path, **_kw):
                self.log.append(path)
                if self.lose_post:
                    self.lose_post = False
                    raise urllib.error.URLError("answer lost")
                return b""

            def key(self, code, state=None):
                self.log.append((code, state))

        stub = Stub([True])
        PiKvm.set_connected(stub, True)
        check("set_connected: a drive connected already is left alone", stub.log, [])
        stub = Stub([False, True], lose_post=True)
        PiKvm.set_connected(stub, True)
        check("set_connected: a lost answer that took is not made again",
              stub.log, ["/api/msd/set_connected?connected=1"])
        stub = Stub([True])
        PiKvm.type_text(stub, "Ab", "us")
        check("type: each key held and let go as two events",
              stub.log, [("ShiftLeft", True), ("KeyA", True), ("KeyA", False),
                         ("ShiftLeft", False), ("KeyB", True), ("KeyB", False)])
        stub = Stub([True])
        try:
            PiKvm.upload(stub, Path("no-such-image.img"))
            check("upload: a missing image fails", "uploaded", "raised")
        except FileNotFoundError:
            check("upload: a missing image fails before the drive is touched", stub.log, [])

        class UploadStub(Stub):
            """A PiKVM whose first write is refused: the image is stored still."""

            def __init__(self):
                super().__init__([True])
                self.writes = 0

            def set_connected(self, connected):
                self.log.append(f"connected={int(connected)}")

            def wait_idle(self, seconds=120):
                pass

            def write_image(self, image, size):
                self.writes += 1
                if self.writes == 1:
                    raise urllib.error.HTTPError(
                        self.url if hasattr(self, "url") else "https://pikvm.test", 400, "Bad Request",
                        None, io.BytesIO(b'{"result": {"error": "MsdImageExistsError"}}'))
                self.log.append("written")

            def json(self, method, path, **_kw):
                return {"drive": {"image": {"complete": True}}}

        with tempfile.TemporaryDirectory(prefix="pikvm-selftest-") as tmp:
            image = Path(tmp) / "x.img"
            image.write_bytes(b"image")
            stub = UploadStub()
            PiKvm.upload(stub, image)
            removes = [p for p in stub.log if isinstance(p, str) and p.startswith("/api/msd/remove")]
            check("upload: an image of its name still stored is removed and the upload made again",
                  (stub.writes, len(removes), "written" in stub.log, stub.log[-1]),
                  (2, 2, True, "connected=1"))
    finally:
        RETRY_WAIT = saved_wait


def self_test_screens(check) -> None:
    """wait_still(), changed_from() and click_entry() against a stubbed
    screen, with time passing only as they sleep."""
    try:
        from PIL import Image
    except ImportError:
        print("pikvm self-test: no PIL, so the click checks are not exercised")
        return
    from unittest import mock

    def jpeg(level):
        out = io.BytesIO()
        Image.new("L", (8, 8), level).save(out, format="JPEG")
        return out.getvalue()

    class Screen:
        """A PiKVM whose screen shows `frames` in turn, then the last for
        good; a click advances it by `on_click` frames."""

        def __init__(self, frames, on_click=0):
            self.frames, self.on_click, self.clicks, self.moves = list(frames), on_click, 0, 0

        def snapshot(self):
            return jpeg(self.frames.pop(0) if len(self.frames) > 1 else self.frames[0])

        def move(self, x, y):
            self.moves += 1

        def click(self):
            self.clicks += 1
            del self.frames[:min(self.on_click, len(self.frames) - 1)]

    clock = [0.0]
    with mock.patch.object(time, "monotonic", lambda: clock[0]), \
            mock.patch.object(time, "sleep", lambda t: clock.__setitem__(0, clock[0] + t)):
        check("wait_still: settles on the first picture seen twice",
              wait_still(Screen([0, 200, 200]), 30) is not None, True)
        check("wait_still: a screen that never settles is None",
              wait_still(Screen([0, 200, 0, 200, 0, 200] * 20), 10), None)
        black = Image.new("L", (8, 8), 0)
        check("changed_from: a screen that changes", changed_from(Screen([0, 0, 200]), black, 10), True)
        check("changed_from: one that does not", changed_from(Screen([0]), black, 10), False)
        def clicks(screen):
            """How many clicks click_entry made; -1 if it gave up."""
            try:
                click_entry(screen, (5, 5))
            except SystemExit:
                return -1
            return screen.clicks

        # The menu (0) settles; the first click is lost, the second takes (200).
        check("click_entry: a click the menu did not take is made again",
              clicks(Screen([0] * 14 + [200])), 2)
        # The menu settles, then the click takes it away at once.
        check("click_entry: a click that takes is made once",
              clicks(Screen([0, 0, 0, 0, 200], on_click=1)), 1)
        check("click_entry: a menu that never takes a click is given up on",
              clicks(Screen([0])), -1)



def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("status")
    p = sub.add_parser("stage")
    p.add_argument("worktree", type=Path)
    p.add_argument("-o", "--output", type=Path, required=True)
    p.add_argument("--cmdline", default=DEFAULT_CMDLINE)
    p.add_argument("--add-cmdline", default="",
                   help="words added to the command line (the default's, or --cmdline's)")
    p.add_argument("--no-linuxdrv", action="store_true",
                   help="leave the Linux driver processes off, and their self-test with them")
    p.add_argument("--wifi", type=Path, default=None,
                   help="a WiFi network file -- ssid= and psk= lines, kept outside the "
                        "repository, as ~/.slateos-wifi -- for the WiFi driver process to join "
                        "the network it names, with linuxdrv.wifi=")
    p = sub.add_parser("boot")
    p.add_argument("image", type=Path)
    p.add_argument("--menu-key", default="F12")
    p = sub.add_parser("watch")
    p.add_argument("--seconds", type=float, default=90)
    p.add_argument("--every", type=float, default=6)
    p.add_argument("--dir", type=Path, default=Path("build") / "pikvm-watch")
    p = sub.add_parser("snap")
    p.add_argument("out", type=Path)
    p.add_argument("--crop")
    p = sub.add_parser("power")
    p.add_argument("action", choices=["on", "off", "off-hard", "reset-hard"])
    p = sub.add_parser("type")
    p.add_argument("text")
    p.add_argument("--layout", choices=["dvorak", "us"], default="dvorak")
    p = sub.add_parser("key")
    p.add_argument("code")
    args = ap.parse_args(argv)

    if args.cmd == "stage":
        cmdline = " ".join(w for w in (args.cmdline, args.add_cmdline) if w)
        stage(args.worktree, args.output, cmdline, linuxdrv=not args.no_linuxdrv,
              wifi=args.wifi)
        return 0
    kvm = PiKvm()
    try:
        run_command(kvm, args)
    except urllib.error.HTTPError as e:
        sys.exit(f"pikvm: the PiKVM refused a request: {e.code} {e.reason}")
    # The network's failures only -- not every OSError, which a missing
    # image or an unwritable directory is too, and says itself better. TLS
    # cut off mid-answer (ssl.SSLEOFError) is one of them.
    except (urllib.error.URLError, http.client.HTTPException, ConnectionError,
            TimeoutError, ssl.SSLError) as e:
        sys.exit(f"pikvm: the PiKVM at {kvm.url} does not answer ({e}): is it on the network?")
    return 0


def run_command(kvm: PiKvm, args) -> None:
    """The command `args` names, on `kvm`."""
    if args.cmd == "status":
        print("leds", kvm.leds())
        source = kvm.json("GET", "/api/streamer")["streamer"]["source"]
        print("video", source["online"], source["resolution"])
        print("drive", kvm.json("GET", "/api/msd")["drive"])
    elif args.cmd == "boot":
        boot(kvm, args.image, args.menu_key)
    elif args.cmd == "watch":
        watch(kvm, args.seconds, args.every, args.dir)
    elif args.cmd == "snap":
        data = kvm.snapshot()
        from PIL import Image
        img = Image.open(io.BytesIO(data))
        if args.crop:
            img = img.crop(tuple(int(v) for v in args.crop.split(",")))
        img.save(args.out)
    elif args.cmd == "power":
        kvm.power(args.action.replace("-", "_"))
    elif args.cmd == "type":
        kvm.type_text(args.text, args.layout)
    elif args.cmd == "key":
        kvm.key(args.code)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
