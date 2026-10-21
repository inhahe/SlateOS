#!/usr/bin/env python3
"""Regression tests for `scripts/shellfunc.py`.

Run: `python scripts/test-shellfunc.py` (0 = pass, 1 = fail).
"""

from __future__ import annotations

import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shellfunc  # noqa: E402

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}")
        failures.append(label)


def error_of(text: str, name: str) -> str:
    try:
        shellfunc.shell_function(text, name, "fixture.sh")
    except shellfunc.ShellFunctionError as exc:
        return str(exc)
    return ""


SCRIPT = (
    "set -u\n"
    "before() {\n    echo before\n}\n"
    "target() {\n"
    "    if true; then\n        echo {inner}\n    fi\n"
    "    case x in\n    x) echo x ;;\n    esac\n"
    "}\n"
    "after() {\n    echo after\n}\n"
)


def main() -> int:
    got = shellfunc.shell_function(SCRIPT, "target")
    check("the function is cut from its opener through its own closing brace",
          got, "target() {\n    if true; then\n        echo {inner}\n    fi\n"
               "    case x in\n    x) echo x ;;\n    esac\n}\n")
    check("an indented brace inside it does not end it", "case x in" in got, True)
    check("a name that only ends another's is not matched",
          error_of("pretarget() {\n}\n", "target") != "", True)
    check("a name used inside another function's body is not taken for a definition",
          error_of("other() {\n    target() { :; }\n}\n", "target") != "", True)
    check("a missing function is an error naming it and the script",
          ("`target() {`" in error_of("x() {\n}\n", "target"),
           "fixture.sh" in error_of("x() {\n}\n", "target")), (True, True))
    check("a function defined twice is an error, not the first one",
          "2 times" in error_of("target() {\n}\ntarget() {\n}\n", "target"), True)
    check("a function with no closing brace at column 0 is an error",
          "no closing" in error_of("target() {\n    echo\n  }\n", "target"), True)
    check("a name with regex metacharacters is matched literally",
          error_of("aXb() {\n}\n", "a.b") != "", True)

    # The overrun: a closing line that is not a bare `}` must not carry the
    # lift on through the code and the function after it.
    for closer in ("  }", "} >&2", "}  # end"):
        check(f"a function closed by {closer!r} is an error, not a lift into the next one",
              "runs into another function" in error_of(
                  f"target() {{\n    echo\n{closer}\necho top-level\nafter() {{\n    echo\n}}\n",
                  "target"), True)
    check("...and so is one followed by a brace group rather than a function",
          "runs into" in error_of("target() {\n    echo\n  }\n{\n    echo\n}\n", "target"), True)
    nested = ("target() {\n    helper() {\n        echo\n    }\n"
              "    { echo a; echo b\n    }\n}\nafter() {\n}\n")
    check("an indented closing brace -- a nested function, a brace group -- does not end it",
          shellfunc.shell_function(nested, "target"),
          "target() {\n    helper() {\n        echo\n    }\n    { echo a; echo b\n    }\n}\n")
    check("a one-line definition is an error, not a lift to a later brace",
          "on one line" in error_of("target() { echo; }\nafter() {\n}\n", "target"), True)
    check("a comment after the opening brace is allowed",
          shellfunc.shell_function("target() {  # why\n    echo\n}\n", "target"),
          "target() {  # why\n    echo\n}\n")
    check("CRLF text is lifted as LF",
          shellfunc.shell_function("x=1\r\ntarget() {\r\n    echo\r\n}\r\n", "target"),
          "target() {\n    echo\n}\n")

    # shell_function_in: reads the file (CRLF too) and names it in an error.
    d = tempfile.mkdtemp(prefix="shellfunc-")
    try:
        p = os.path.join(d, "some-script.sh")
        with open(p, "wb") as fh:
            fh.write(b"target() {\r\n    echo\r\n}\r\n")
        check("shell_function_in lifts from a file, CRLF read as LF",
              shellfunc.shell_function_in(p, "target"), "target() {\n    echo\n}\n")
        try:
            shellfunc.shell_function_in(p, "missing")
            named = False
        except shellfunc.ShellFunctionError as exc:
            named = "some-script.sh" in str(exc)
        check("...and its error names the file", named, True)
    finally:
        shutil.rmtree(d, ignore_errors=True)
    print()
    if failures:
        print(f"{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("all shellfunc tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
