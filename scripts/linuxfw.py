"""The firmware the Linux driver processes load: fetched, and verified against its pin.

`linuxdrv/firmware.pin` names a linux-firmware release (its git tag) and each
file wanted from it with its sha256. `fetch` takes each from kernel.org at
that release into `build/firmware/<path>` of the worktree, refuses any whose
bytes are not the pinned ones, and leaves a file already there with the right
bytes alone. The files are not committed -- each is a binary under its own
licence, which linux-firmware's WHENCE names -- so they are fetched, as the
Linux tree is (`scripts/linux-tree.sh`). `scripts/pikvm.py stage` puts them
on the boot medium as modules `firmware/<path>`, which the kernel places
under /.boot-firmware (design-decisions 1592).

    python scripts/linuxfw.py fetch [--worktree DIR]   # fetch what is missing
    python scripts/linuxfw.py paths [--worktree DIR]   # where each file goes
    python scripts/linuxfw.py --self-test
"""
from __future__ import annotations

import argparse
import email.utils
import hashlib
import http.client
import re
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
PIN = Path("linuxdrv") / "firmware.pin"
OUT = Path("build") / "firmware"
URL = ("https://git.kernel.org/pub/scm/linux/kernel/git/firmware/linux-firmware.git/"
       "plain/{path}?h={release}")
# The largest file the pin may name: linux-firmware's largest are tens of MB;
# a response past this is not the file asked for.
MAX_BYTES = 256 << 20


class PinError(Exception):
    """The pin is not one: said, with the line."""


def read_pin(text: str) -> tuple[str, list[tuple[str, str]]]:
    """The release `text`, a pin's contents, names, and each file's path and
    sha256 in its order. PinError for a pin that is not exactly one
    `release <YYYYMMDD>` line and `file <path> <sha256>` lines -- a path
    relative, its components neither empty nor `.` nor `..`, named once."""
    release = None
    files: list[tuple[str, str]] = []
    for n, line in enumerate(text.splitlines(), 1):
        words = line.split("#", 1)[0].split()
        if not words:
            continue
        if words[0] == "release" and len(words) == 2:
            if release is not None:
                raise PinError(f"line {n}: a second release line")
            if not re.fullmatch(r"\d{8}", words[1]):
                raise PinError(f"line {n}: a release is a linux-firmware tag, YYYYMMDD: {words[1]!r}")
            release = words[1]
        elif words[0] == "file" and len(words) == 3:
            path, sha = words[1], words[2]
            if any(c in ("", ".", "..") for c in path.split("/")):
                raise PinError(f"line {n}: {path!r} is no relative path")
            if not re.fullmatch(r"[0-9a-f]{64}", sha):
                raise PinError(f"line {n}: {sha!r} is no sha256")
            if any(p == path for p, _ in files):
                raise PinError(f"line {n}: {path} named twice")
            files.append((path, sha))
        else:
            raise PinError(f"line {n}: neither 'release <tag>' nor 'file <path> <sha256>': {line.strip()!r}")
    if release is None:
        raise PinError("no release line")
    return release, files


def pinned(worktree: Path) -> tuple[str, list[tuple[str, str]]]:
    """`worktree`'s pin, read. SystemExit, saying why, for one that is not."""
    try:
        return read_pin((worktree / PIN).read_text(encoding="utf-8"))
    except OSError as e:
        sys.exit(f"linuxfw: cannot read {worktree / PIN}: {e}")
    except PinError as e:
        sys.exit(f"linuxfw: {worktree / PIN}: {e}")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


#: Seconds between tries, unless a 429 asks for another wait.
RETRY_WAIT_S = 3
#: The longest a 429's Retry-After is waited for.
RETRY_AFTER_MAX_S = 60


def retry_after(value: str, now: float) -> float | None:
    """The seconds a Retry-After header's `value` asks to wait from `now`
    (seconds since the epoch), at most RETRY_AFTER_MAX_S: given as seconds,
    or as an HTTP date (RFC 9110 allows either). None when it is neither."""
    # ASCII digits only: "²" is a digit to isdigit() and not to int().
    if value.isascii() and value.isdigit():
        return min(int(value), RETRY_AFTER_MAX_S)
    try:
        when = email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError, IndexError):
        return None
    if when.tzinfo is None:
        return None
    return min(max(when.timestamp() - now, 0.0), RETRY_AFTER_MAX_S)


def download(url: str, opener=urllib.request.urlopen, sleep=time.sleep, tries: int = 3,
             clock=time.time) -> bytes:
    """`url`'s whole body. Tried up to `tries` times, `sleep`ing
    RETRY_WAIT_S between -- or, after a 429, as long as its Retry-After asks
    (retry_after, by `clock`), up to a minute -- for what another try may
    mend: a connection lost or refused, a body cut short (fewer bytes than
    its Content-Length, or http.client's IncompleteRead), a server's error
    (5xx), a "not now" (408, 429). OSError, saying why, for the last of
    those, or at once for what no try mends: any other 4xx (the pin names a
    file or release kernel.org does not have) or a body past MAX_BYTES."""
    # kernel.org answers urllib's own User-Agent with 403 Forbidden.
    request = urllib.request.Request(url, headers={"User-Agent": "slateos-linuxfw/1"})
    why = "no attempt made"
    wait: float = RETRY_WAIT_S
    for attempt in range(tries):
        if attempt:
            sleep(wait)
        wait = RETRY_WAIT_S
        try:
            with opener(request, timeout=60) as response:
                body = response.read(MAX_BYTES + 1)
                promised = response.headers.get("Content-Length")
        except urllib.error.HTTPError as e:
            # 408 and 429 are the server's "not now": tried again, after the
            # wait a 429 asks for when it asks one.
            if e.code < 500 and e.code not in (408, 429):
                raise OSError(f"HTTP {e.code} {e.reason}") from e
            why = f"HTTP {e.code} {e.reason}"
            asked = (e.headers or {}).get("Retry-After", "") if e.code == 429 else ""
            asked_s = retry_after(asked, clock()) if asked else None
            if asked_s is not None:
                wait = asked_s
            continue
        except (OSError, http.client.HTTPException) as e:
            why = f"{type(e).__name__}: {e}"
            continue
        if len(body) > MAX_BYTES:
            raise OSError(f"more than {MAX_BYTES} bytes")
        if promised is not None and promised.isdigit() and int(promised) != len(body):
            why = f"cut short: {len(body)} of the {promised} bytes promised"
            continue
        return body
    raise OSError(f"{why} (tried {tries} times)")


def fetch(worktree: Path, opener=urllib.request.urlopen, say=print, sleep=time.sleep) -> list[Path]:
    """Every file `worktree`'s pin names, in `build/firmware/<path>` with its
    pinned bytes: those already right left alone, the rest fetched. The
    paths, relative to `build/firmware`, in the pin's order. SystemExit,
    saying which, for a file that cannot be fetched or whose bytes are not
    the pin's -- nothing is left at its path but a right file."""
    release, files = pinned(worktree)
    out = worktree / OUT
    got: list[Path] = []
    for path, want in files:
        dest = out / path
        if dest.is_file():
            if sha256(dest.read_bytes()) == want:
                got.append(Path(path))
                continue
            # Not the pin's bytes (an older release's, or damaged): gone
            # before anything else, so that a fetch that fails leaves nothing.
            dest.unlink()
        url = URL.format(path=path, release=release)
        say(f"linuxfw: fetching {path} at linux-firmware {release}")
        try:
            body = download(url, opener, sleep)
        except OSError as e:
            sys.exit(f"linuxfw: could not fetch {url}: {e}")
        if sha256(body) != want:
            sys.exit(f"linuxfw: {url} is not the file the pin names (sha256 {sha256(body)}, "
                     f"pinned {want}); refusing it")
        dest.parent.mkdir(parents=True, exist_ok=True)
        part = dest.with_name(dest.name + ".part")
        part.write_bytes(body)
        part.replace(dest)
        got.append(Path(path))
    return got


def self_test() -> int:
    failures = 0

    def check(what, got, want):
        nonlocal failures
        ok = got == want
        failures += not ok
        print(f"  {'ok  ' if ok else 'FAIL'} {what}" + ("" if ok else f": got {got!r}, want {want!r}"))

    def pin_error(text):
        try:
            read_pin(text)
        except PinError as e:
            return str(e)
        return None

    a, b = b"first file", b"second file"
    good = (f"# a comment\nrelease 20261019\nfile x/a.bin {sha256(a)}  # trailing\n\n"
            f"file b.bin {sha256(b)}\n")
    check("a pin read: its release and files, in order",
          read_pin(good), ("20261019", [("x/a.bin", sha256(a)), ("b.bin", sha256(b))]))
    check("no release refused", pin_error(f"file b.bin {sha256(b)}\n"), "no release line")
    check("two releases refused", (pin_error("release 20261019\nrelease 20261020\n") or "").endswith(
        "a second release line"), True)
    check("a release not a tag refused", "YYYYMMDD" in (pin_error("release main\n") or ""), True)
    for bad in ("../a.bin", "/a.bin", "a//b.bin", "a/./b.bin", "a/"):
        check(f"path {bad!r} refused", "no relative path" in (pin_error(
            f"release 20261019\nfile {bad} {sha256(a)}\n") or ""), True)
    check("a short hash refused", "no sha256" in (pin_error("release 20261019\nfile a.bin abc\n") or ""), True)
    check("a file named twice refused", "named twice" in (pin_error(
        f"release 20261019\nfile a.bin {sha256(a)}\nfile a.bin {sha256(a)}\n") or ""), True)
    check("an unknown line refused", "neither" in (pin_error("release 20261019\nfetch everything\n") or ""), True)

    class Answer:
        """A response: `body`, and the length its header promised."""

        def __init__(self, body, promised=None):
            self.body = body
            self.headers = {"Content-Length": str(len(body) if promised is None else promised)}

        def __enter__(self):
            return self

        def __exit__(self, *exc):
            return False

        def read(self, n=-1):
            return self.body if n < 0 else self.body[:n]

    asked: list[str] = []

    def kernel_org(files, lose=0, faults=()):
        """kernel.org serving `files` at release 20261019 -- the first `lose`
        requests lost, then each of `faults` spoiling the next answer:
        "cut" (IncompleteRead), "short" (fewer bytes than promised),
        "busy" (503) -- and 404 for any other file."""
        lost = [lose]
        spoil = list(faults)

        def opener(request, timeout=None):
            url = request.full_url
            asked.append(url)
            # Without one of its own, urllib sends its own, which kernel.org refuses.
            if not request.get_header("User-agent"):
                raise urllib.error.HTTPError(url, 403, "Forbidden", None, None)
            if lost[0]:
                lost[0] -= 1
                raise OSError("connection reset")
            body = next((b for p, b in files.items()
                         if url == URL.format(path=p, release="20261019")), None)
            if body is None:
                raise urllib.error.HTTPError(url, 404, "Not Found", None, None)
            fault = spoil.pop(0) if spoil else None
            if fault == "cut":
                raise http.client.IncompleteRead(body[:3], len(body) - 3)
            if fault == "short":
                return Answer(body[:3], promised=len(body))
            if fault == "busy":
                raise urllib.error.HTTPError(url, 503, "Service Unavailable", None, None)
            if fault == "slow":
                raise urllib.error.HTTPError(url, 429, "Too Many Requests", {"Retry-After": "2"}, None)
            if fault == "odd":
                # A Retry-After of byte 0xB2, as latin-1 decodes it: a digit to
                # isdigit() and not to int().
                raise urllib.error.HTTPError(url, 429, "Too Many Requests",
                                             {"Retry-After": "²"}, None)
            if fault == "dated":
                # Retry-After as an HTTP date, half a minute from now.
                when = email.utils.formatdate(time.time() + 30, usegmt=True)
                raise urllib.error.HTTPError(url, 429, "Too Many Requests",
                                             {"Retry-After": when}, None)
            return Answer(body)
        return opener

    with tempfile.TemporaryDirectory() as tmp:
        wt = Path(tmp)
        (wt / PIN).parent.mkdir(parents=True)
        (wt / PIN).write_text(good, encoding="utf-8", newline="")
        quiet = lambda _line: None  # noqa: E731
        nap = lambda _s: None  # noqa: E731
        got = fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}, lose=1), quiet, nap)
        check("fetched: each file at its path, in the pin's order",
              (got, (wt / OUT / "x/a.bin").read_bytes(), (wt / OUT / "b.bin").read_bytes()),
              ([Path("x/a.bin"), Path("b.bin")], a, b))
        check("a lost answer tried again", len(asked), 3)
        asked.clear()
        fetch(wt, kernel_org({}), quiet, nap)
        check("files already right not fetched again", asked, [])
        (wt / OUT / "b.bin").write_bytes(b"stale")
        fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}), quiet, nap)
        check("a file not right fetched again", ((wt / OUT / "b.bin").read_bytes(), len(asked)), (b, 1))
        (wt / OUT / "b.bin").unlink()
        try:
            fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b"tampered"}), quiet, nap)
            refused = None
        except SystemExit as e:
            refused = str(e)
        check("bytes not the pin's refused, and not left",
              ("refusing it" in (refused or ""), (wt / OUT / "b.bin").exists(),
               (wt / OUT / "b.bin.part").exists()), (True, False, False))
        try:
            fetch(wt, kernel_org({}, lose=5), quiet, nap)
            unreachable = None
        except SystemExit as e:
            unreachable = str(e)
        check("an unreachable file said", "could not fetch" in (unreachable or ""), True)
        for fault in ("cut", "short", "busy"):
            asked.clear()
            fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}, faults=(fault,)), quiet, nap)
            check(f"an answer {fault} tried again, and taken whole",
                  ((wt / OUT / "b.bin").read_bytes(), len(asked)), (b, 2))
            (wt / OUT / "b.bin").unlink()
        asked.clear()
        slept: list[float] = []
        fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}, faults=("slow",)), quiet, slept.append)
        check("a 429 tried again, after the Retry-After it asked, and only that",
              ((wt / OUT / "b.bin").read_bytes(), len(asked), slept), (b, 2, [2]))
        (wt / OUT / "b.bin").unlink()
        asked.clear()
        slept.clear()
        fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}, faults=("odd",)), quiet, slept.append)
        check("a 429 whose Retry-After is no number tried again all the same, after the usual wait",
              ((wt / OUT / "b.bin").read_bytes(), len(asked), slept), (b, 2, [RETRY_WAIT_S]))
        (wt / OUT / "b.bin").unlink()
        asked.clear()
        slept.clear()
        fetch(wt, kernel_org({"x/a.bin": a, "b.bin": b}, faults=("dated",)), quiet, slept.append)
        check("a 429 whose Retry-After is a date tried again, after waiting until then",
              ((wt / OUT / "b.bin").read_bytes(), len(asked), len(slept),
               all(25 <= s <= 30 for s in slept)), (b, 2, 1, True))
        (wt / OUT / "b.bin").unlink()
        now = 1_000_000_000.0
        for value, want in [("7", 7), ("600", RETRY_AFTER_MAX_S), ("", None), ("soon", None),
                            (email.utils.formatdate(now + 12, usegmt=True), 12),
                            (email.utils.formatdate(now - 12, usegmt=True), 0),
                            (email.utils.formatdate(now + 3600, usegmt=True), RETRY_AFTER_MAX_S)]:
            check(f"Retry-After {value!r} read as {want}", retry_after(value, now), want)
        asked.clear()
        try:
            fetch(wt, kernel_org({"x/a.bin": a}), quiet, nap)
            missing = None
        except SystemExit as e:
            missing = str(e)
        check("a file kernel.org does not have said at once, not tried again",
              ("HTTP 404" in (missing or ""), len(asked)), (True, 1))
        (wt / OUT / "b.bin").write_bytes(b"an older release's")
        try:
            fetch(wt, kernel_org({"x/a.bin": a}), quiet, nap)
        except SystemExit:
            pass
        check("a stale file is gone once its fetch fails", (wt / OUT / "b.bin").exists(), False)
    check("the repository's own pin reads", bool(read_pin((REPO / PIN).read_text(encoding="utf-8"))[1]), True)
    print(f"linuxfw self-test: {'passed' if not failures else f'{failures} FAILED'}")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cmd", choices=("fetch", "paths"))
    ap.add_argument("--worktree", type=Path, default=REPO)
    args = ap.parse_args(argv)
    if args.cmd == "fetch":
        for path in fetch(args.worktree):
            print(args.worktree / OUT / path)
    else:
        for path, _sha in pinned(args.worktree)[1]:
            print(args.worktree / OUT / path)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
