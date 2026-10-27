#!/usr/bin/env python3
"""Boot a kernel on the real-hardware test PC through its PiKVM, and watch it.

The test PC (`bare-metal-boot.md` section 9) is reached only through a PiKVM:
a network device that captures its screen, acts as its keyboard, mouse and a
USB drive, and works its power button. This script is how a session drives it:

    python scripts/pikvm.py status
    python scripts/pikvm.py stage <worktree> -o <img> [--cmdline "..."]
    python scripts/pikvm.py boot <img>          # upload, restart, pick it at F12
    python scripts/pikvm.py watch --seconds 90  # screenshots until the screen settles
    python scripts/pikvm.py snap <out.png> [--crop X0,Y0,X1,Y1]
    python scripts/pikvm.py power on|off|off-hard|reset-hard

`stage` builds the USB image from a worktree's built kernel: Limine, the kernel
(debug info stripped), and `limine.conf` with the command line on its first
entry. The default command line names the PC's spare disk as the only one the
kernel may write (`blk.writable=`, design-decisions 1575) and puts the kernel's
log on the screen (`log.screen=1`), which is the only log there is.

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
import selftestflag  # noqa: E402

HERE = Path(__file__).resolve().parent
#: The capture's size; the PiKVM's absolute mouse coordinates span it.
WIDTH, HEIGHT = 1920, 1080
#: The command line `stage` gives the kernel by default.
DEFAULT_CMDLINE = "blk.writable=eui64:0025385181b1cd22 selftest.keep_going=1 log.screen=1"
#: The name the image is kept under on the PiKVM.
IMAGE_NAME = "slateos-pikvm.img"

# The PiKVM reaches the network by WiFi, which drops now and then: a request
# the network loses is made again, RETRIES times in all, RETRY_WAIT seconds
# apart; an upload cut off is started again from its beginning (the PiKVM
# keeps no half-written image to resume).
RETRIES = 4
RETRY_WAIT = 5
# The image goes up in pieces of this size, streamed from the file: sent as
# one write of half a gigabyte it was cut off every time.
UPLOAD_CHUNK = 4 << 20
# How long `boot` presses the boot-menu key after power-on: this board takes
# 10-25 s from a cold start to its menu, and a key pressed before it is up is
# lost.
MENU_KEY_SECONDS = 30

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


def limine_conf(template: str, cmdline: str) -> str:
    """`template` (the repository's limine.conf) with `cmdline` on its first
    entry, the line after that entry's `kernel_path:` -- as boot-test.sh
    gives the test's command line."""
    out, entry, done = [], 0, False
    for line in template.splitlines(keepends=True):
        if line.startswith("/"):
            entry += 1
        out.append(line)
        if entry == 1 and not done and line.split()[:1] == ["kernel_path:"]:
            out.append(f"    cmdline: {cmdline}\n")
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

    def call(self, method: str, path: str, body: bytes | None = None, timeout: float = 30) -> bytes:
        """One request to kvmd: its answer's body. One the network loses is
        made again (RETRIES in all); an answer, an error status included, is
        final. A dropped request may have been carried out before it was
        lost: the actions here (a key, a click, a power button) bear one
        repeat."""
        for attempt in range(1, RETRIES + 1):
            req = urllib.request.Request(self.url + path, data=body, method=method, headers=self.headers)
            try:
                with urllib.request.urlopen(req, context=self.ctx, timeout=timeout) as r:
                    return r.read()
            except urllib.error.HTTPError:
                raise
            except OSError as e:  # URLError, a reset or a timeout: the network
                if attempt == RETRIES:
                    raise
                print(f"pikvm: {method} {path.split('?')[0]}: {e}; again in {RETRY_WAIT} s",
                      file=sys.stderr)
                time.sleep(RETRY_WAIT)
        raise AssertionError("unreachable")

    def json(self, method: str, path: str, **kw) -> dict:
        return json.loads(self.call(method, path, **kw))["result"]

    def key(self, code: str, state: bool | None = None) -> None:
        q = f"key={code}" + ("" if state is None else f"&state={'true' if state else 'false'}")
        self.call("POST", f"/api/hid/events/send_key?{q}")

    def move(self, x: int, y: int) -> None:
        self.call("POST", f"/api/hid/events/send_mouse_move?to_x={to_absolute(x, WIDTH)}"
                          f"&to_y={to_absolute(y, HEIGHT)}")

    def click(self, button: str = "left") -> None:
        self.call("POST", f"/api/hid/events/send_mouse_button?button={button}&state=true")
        time.sleep(0.08)
        self.call("POST", f"/api/hid/events/send_mouse_button?button={button}&state=false")

    def type_text(self, text: str, layout: str) -> None:
        for code, shift in key_presses(text, layout):
            if shift:
                self.key("ShiftLeft", True)
            self.key(code)
            if shift:
                self.key("ShiftLeft", False)

    def snapshot(self) -> bytes:
        return self.call("GET", "/api/streamer/snapshot")

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
                self.call("POST", f"/api/atx/power?action={action}&wait=1", timeout=60)
                return
            except urllib.error.HTTPError as e:
                if e.code != 409 or attempt == 10:
                    raise
                time.sleep(1)

    def upload(self, image: Path) -> None:
        """Put `image` on the virtual drive as a read-only flash drive, connected."""
        # Disconnecting a drive that is not connected is refused (HTTP 400).
        if self.json("GET", "/api/msd")["drive"]["connected"]:
            self.call("POST", "/api/msd/set_connected?connected=0")
        for attempt in range(1, RETRIES + 1):
            try:
                self.call("POST", f"/api/msd/remove?image={IMAGE_NAME}")
            except urllib.error.HTTPError:
                pass  # No earlier image of that name, or none left: nothing to remove.
            try:
                self.write_image(image)
                break
            except (OSError, http.client.HTTPException) as e:
                if attempt == RETRIES:
                    raise
                print(f"pikvm: the upload was cut off ({e}); starting it again", file=sys.stderr)
                time.sleep(RETRY_WAIT)
        self.call("POST", f"/api/msd/set_params?image={IMAGE_NAME}&cdrom=0&rw=0")
        self.call("POST", "/api/msd/set_connected?connected=1")
        for _ in range(60):
            if self.json("GET", "/api/msd")["drive"]["image"]["complete"]:
                return
            time.sleep(1)
        sys.exit("pikvm: the image never showed as complete")

    def write_image(self, image: Path) -> None:
        """Send `image` to the virtual drive's storage as IMAGE_NAME, streamed
        from the file in UPLOAD_CHUNK pieces, saying how far it has got. A
        refusal from kvmd ends the script; a connection lost raises."""
        parts = urllib.parse.urlsplit(self.url)
        conn = (http.client.HTTPSConnection(parts.netloc, context=self.ctx, timeout=120)
                if parts.scheme == "https" else http.client.HTTPConnection(parts.netloc, timeout=120))
        size = image.stat().st_size
        try:
            conn.putrequest("POST", f"/api/msd/write?image={IMAGE_NAME}")
            for key, value in self.headers.items():
                conn.putheader(key, value)
            conn.putheader("Content-Length", str(size))
            conn.putheader("Content-Type", "application/octet-stream")
            conn.endheaders()
            sent, start = 0, time.monotonic()
            said = start
            with image.open("rb") as fh:
                while block := fh.read(UPLOAD_CHUNK):
                    conn.send(block)
                    sent += len(block)
                    if time.monotonic() - said >= 10:
                        print(f"pikvm: {sent >> 20} of {size >> 20} MiB sent", flush=True)
                        said = time.monotonic()
            answer = conn.getresponse()
            body = answer.read()
            if answer.status != 200:
                sys.exit(f"pikvm: the PiKVM refused the image: {answer.status} {body[:300]!r}")
            print(f"pikvm: {size >> 20} MiB sent in {time.monotonic() - start:.0f} s", flush=True)
        finally:
            conn.close()


def stage(worktree: Path, out: Path, cmdline: str) -> None:
    """Build the USB image of `worktree`'s built kernel at `out`, from an ESP
    tree laid out in a scratch directory that is gone afterwards."""
    with tempfile.TemporaryDirectory(prefix="pikvm-esp-") as scratch:
        stage_esp(worktree, Path(scratch), cmdline)
        subprocess.run([sys.executable, str(HERE / "build-usb-image.py"), "--source", scratch,
                        "--output", str(out)], check=True)


def stage_esp(worktree: Path, esp: Path, cmdline: str) -> None:
    """Lay out an ESP in `esp`: Limine, `worktree`'s built kernel (stripped of
    its debug sections when llvm-strip is there), and its limine.conf with
    `cmdline` on the first entry."""
    (esp / "EFI" / "BOOT").mkdir(parents=True)
    (esp / "boot").mkdir()
    limine = worktree / "limine" / "BOOTX64.EFI"
    if not limine.exists():
        limine = worktree.parent / "os-lane-a" / "limine" / "BOOTX64.EFI"
    shutil.copy(limine, esp / "EFI" / "BOOT" / "BOOTX64.EFI")
    kernel = worktree / "target" / "x86_64-unknown-none" / "debug" / "kernel"
    strip = (Path.home() / ".rustup" / "toolchains" / "stable-x86_64-pc-windows-gnu" / "lib"
             / "rustlib" / "x86_64-pc-windows-gnu" / "bin" / "llvm-strip.exe")
    if strip.exists():
        subprocess.run([str(strip), "--strip-debug", str(kernel), "-o", str(esp / "boot" / "kernel")],
                       check=True)
    else:
        shutil.copy(kernel, esp / "boot" / "kernel")
    template = (worktree / "limine.conf").read_text(encoding="utf-8")
    (esp / "limine.conf").write_text(limine_conf(template, cmdline), encoding="utf-8", newline="\n")


def boot(kvm: PiKvm, image: Path, menu_key: str) -> None:
    """Upload `image`, restart the PC, and pick the PiKVM drive in its boot menu."""
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
    # The menu waits for a choice: once the screen is still, it is up.
    menu = wait_still(kvm, 30)
    x, y = kvm.boot_entry_xy
    for _ in range(3):
        kvm.move(x, y)
        time.sleep(0.3)
        kvm.click()
        if menu is None or changed_from(kvm, menu, 10):
            return
        print("pikvm: the boot menu is still up; clicking the entry again", flush=True)
    sys.exit("pikvm: the boot menu did not take the click")


def snapshot_image(kvm: PiKvm):
    """The screen, greyscale; None when no picture can be had."""
    from PIL import Image
    try:
        return Image.open(io.BytesIO(kvm.snapshot())).convert("L")
    except (urllib.error.URLError, OSError):
        return None


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
    from PIL import Image
    out_dir.mkdir(parents=True, exist_ok=True)
    last, still, n = None, 0.0, 0
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        time.sleep(every)
        try:
            data = kvm.snapshot()
        except urllib.error.HTTPError:
            print("pikvm: no picture (the PC is off or between modes)")
            continue
        except OSError as e:  # the network, past call's retries
            print(f"pikvm: no picture ({e})")
            continue
        n += 1
        shot = out_dir / f"watch-{n:03}.jpg"
        shot.write_bytes(data)
        img = Image.open(io.BytesIO(data)).convert("L")
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
    try:
        limine_conf("timeout: 3\n", "x")
        check("limine.conf without an entry is refused", "taken", "refused")
    except ValueError:
        pass
    for f in failures:
        print("FAIL", f)
    print("pikvm self-test:", "FAILED" if failures else "passed")
    return 1 if failures else 0


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
        stage(args.worktree, args.output, args.cmdline)
        return 0
    kvm = PiKvm()
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
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
