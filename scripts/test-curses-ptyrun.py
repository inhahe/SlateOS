#!/usr/bin/env python3
"""Regression tests for `scripts/curses-ptyrun.py`'s `keys:` escapes.

Run: `python scripts/test-curses-ptyrun.py` (exit 0 = pass, 1 = fail).
No pytest dependency, like the other suites here.

What is tested is `unescape` and `check`: the bytes a `keys:` action types,
and the steps refused before anything runs. A step the runner cannot type as
written must be refused, never typed some other way, because both sides of a
comparison would then agree on keys the case does not name. The pty itself
is the harnesses' to exercise (curses-diff.sh and the rest), in WSL.

The script imports `fcntl`, `pty` and `termios` at the top, which Windows'
Python has not, and this suite runs on the Windows host too: where they are
missing, empty stand-ins are registered before the script is loaded. Nothing
tested here touches them.
"""

from __future__ import annotations

import importlib.util
import sys
import types
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "curses-ptyrun.py"

_FAILURES: list[str] = []


def load():
    """curses-ptyrun.py as a module, with stand-ins for what this host lacks."""
    for name in ("fcntl", "pty", "termios"):
        try:
            importlib.import_module(name)
        except ImportError:
            sys.modules[name] = types.ModuleType(name)
    spec = importlib.util.spec_from_file_location("curses_ptyrun", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
        return
    print(f"FAIL  {label}")
    print(f"        got:  {got!r}")
    print(f"        want: {want!r}")
    _FAILURES.append(label)


def refused(fn, arg) -> bool:
    """Whether `fn(arg)` raises ValueError."""
    try:
        fn(arg)
    except ValueError:
        return True
    return False


def main() -> int:
    m = load()
    # Every escape the runner knows, and plain text.
    check("plain text", m.unescape("abc"), b"abc")
    check("\\r is a carriage return", m.unescape("a\\r"), b"a\r")
    check("\\n, \\t, \\e", m.unescape("\\n\\t\\e"), b"\n\t\x1b")
    check("\\\\ is one backslash", m.unescape("a\\\\b"), b"a\\b")
    check("\\x and two hex digits", m.unescape("\\x7f\\x41"), b"\x7fA")
    check("\\x at the very end", m.unescape("ab\\x41"), b"abA")
    check("UTF-8 passes as its bytes", m.unescape("é"), "é".encode())
    # Refused rather than typed some other way.
    check("an unknown escape is refused", refused(m.unescape, "a\\q"), True)
    check("a lone trailing backslash is refused", refused(m.unescape, "abc\\"), True)
    check("a backslash alone is refused", refused(m.unescape, "\\"), True)
    check("\\x cut short is refused", refused(m.unescape, "ab\\x4"), True)
    check("\\x with no hex digits is refused", refused(m.unescape, "\\xzz"), True)
    # check() refuses the step before the program starts.
    check("check refuses a step with a trailing backslash",
          refused(m.check, "keys:l;keys:abc\\"), True)
    check("check refuses an unknown action", refused(m.check, "type:abc"), True)
    check("check accepts the actions it knows",
          refused(m.check, "keys:a\\r;resize:24x80;signal:INT;sleep:0.1;copy:a:b"), False)
    if _FAILURES:
        print(f"\n{len(_FAILURES)} failed: " + ", ".join(_FAILURES))
        return 1
    print("\nall passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
