#!/usr/bin/env python3
"""Cut one shell function out of a script, by name, so a test runs the real one.

Several suites drive a function of `scripts/boot-test.sh`, `scripts/run-checker.sh`
or `scripts/hooks/pre-push` by lifting it out of the shipped file rather than
restating it: a restated copy goes on passing after the original changes. They
rely on the one layout those files keep: a function opens with `name() {` at
column 0 and closes with a `}` alone on its line at column 0, every brace inside
it being indented.

Everything wrong is an error, never a partial answer, because a test that runs a
truncated or mistaken function passes the assertions made about it: the name not
defined at column 0, defined twice (which copy runs is a guess), or no closing
brace.
"""

from __future__ import annotations

import re
from pathlib import Path


class ShellFunctionError(RuntimeError):
    """The function is not there as the layout above has it, or is there twice."""


def shell_function(text: str, name: str, where: str = "the script") -> str:
    """The definition of shell function `name` in the script source `text`, from
    `name() {` through its closing brace, with that line's newline.

    `where` names the script in an error, e.g. its file name.
    """
    opener = re.compile(rf"^{re.escape(name)}\(\)[ \t]*\{{", re.MULTILINE)
    starts = [m.start() for m in opener.finditer(text)]
    if not starts:
        raise ShellFunctionError(
            f"{where} no longer defines `{name}() {{` at column 0. If it was "
            f"renamed or restructured, update the suite that lifts it; do not "
            f"delete the test.")
    if len(starts) > 1:
        raise ShellFunctionError(
            f"{where} defines `{name}` {len(starts)} times; which one a test "
            f"would run is a guess.")
    end = re.compile(r"^\}$", re.MULTILINE).search(text, starts[0])
    if end is None:
        raise ShellFunctionError(f"`{name}` in {where} has no closing `}}` at column 0.")
    return text[starts[0]:end.end()] + "\n"


def shell_function_in(path: str | Path, name: str) -> str:
    """`shell_function` of the script file at `path`, read as UTF-8."""
    path = Path(path)
    return shell_function(path.read_text(encoding="utf-8"), name, path.name)
