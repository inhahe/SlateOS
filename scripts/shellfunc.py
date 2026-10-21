#!/usr/bin/env python3
"""Cut one shell function out of a script, by name, so a test runs the real one.

Several suites drive a function of `scripts/boot-test.sh`, `scripts/run-checker.sh`
or `scripts/hooks/pre-push` by lifting it out of the shipped file rather than
restating it: a restated copy goes on passing after the original changes. They
rely on the one layout those files keep: a function opens with `name() {` alone
on its line at column 0 (a trailing comment allowed) and closes with a `}` alone
on its line at column 0, every brace inside it being indented.

Everything else is an error, never a partial answer, because a test that runs a
truncated or overlong function passes the assertions made about it: the name not
defined at column 0, defined twice (which copy runs is a guess), defined on one
line, or closed by anything but a bare `}` -- which would carry the lift on into
the code and the functions after it.
"""

from __future__ import annotations

import re
from pathlib import Path

#: A function's closing line.
_CLOSER = re.compile(r"^\}$", re.MULTILINE)
#: What may not appear at column 0 inside a function: a function or brace group
#: opening there means the lift has run past the function's own end.
_TOP_LEVEL_OPEN = re.compile(r"^(?:[A-Za-z_][\w-]*\(\)[ \t]*\{|\{)", re.MULTILINE)


class ShellFunctionError(RuntimeError):
    """The function is not there as the layout above has it, or is there twice."""


def shell_function(text: str, name: str, where: str = "the script") -> str:
    """The definition of shell function `name` in the script source `text`, from
    `name() {` through its closing brace, with that line's newline. CRLF line
    endings are read as LF, so the answer does not depend on the checkout.

    `where` names the script in an error, e.g. its file name.
    """
    text = text.replace("\r\n", "\n")
    opener = re.compile(rf"^{re.escape(name)}\(\)[ \t]*\{{(.*)$", re.MULTILINE)
    found = list(opener.finditer(text))
    if not found:
        raise ShellFunctionError(
            f"{where} no longer defines `{name}() {{` at column 0. If it was "
            f"renamed or restructured, update the suite that lifts it; do not "
            f"delete the test.")
    if len(found) > 1:
        raise ShellFunctionError(
            f"{where} defines `{name}` {len(found)} times; which one a test "
            f"would run is a guess.")
    start = found[0]
    rest = start.group(1).strip()
    if rest and not rest.startswith("#"):
        raise ShellFunctionError(
            f"`{name}` in {where} is defined on one line; lifting it needs its "
            f"body below `{name}() {{` and its `}}` on a line of its own.")
    end = _CLOSER.search(text, start.end())
    if end is None:
        raise ShellFunctionError(f"`{name}` in {where} has no closing `}}` at column 0.")
    if _TOP_LEVEL_OPEN.search(text, start.end(), end.start()):
        raise ShellFunctionError(
            f"`{name}` in {where} runs into another function or block before a "
            f"`}}` alone at column 0: its own closing line is not a bare `}}`.")
    return text[start.start():end.end()] + "\n"


def shell_function_in(path: str | Path, name: str) -> str:
    """`shell_function` of the script file at `path`, read as UTF-8; errors name
    the file."""
    path = Path(path)
    return shell_function(path.read_text(encoding="utf-8"), name, path.name)
