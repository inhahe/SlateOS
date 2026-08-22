#!/usr/bin/env python3
r"""Measure GNU's `c_maybe_quoting_style`, in both the plain form and the form
`quotearg_n_style_colon` produces, and write the recording that
`userspace/coreutils/tests/c-maybe.rs` replays.

Two oracles, because the two forms are reachable from different programs:

  * **`c-maybe`** — `ls --quoting-style=c-maybe -d -- NAME`, run in a scratch
    directory holding a file of that name. `ls` names the style directly, so
    this is the style with nothing added to it.
  * **`c-maybe-colon`** — `paste -d 'ARG\'`, whose diagnostic

        paste: delimiter list ends with an unescaped backslash: <quoted>

    comes from `quotearg_n_style_colon (0, c_maybe_quoting_style, delim_arg)`
    and quotes the **raw** argument, before escape collapsing. Appending a
    lone backslash to any byte string therefore turns `paste` into an oracle.

Wrinkles each oracle imposes, and why the union of the two is still not
everything:

  * A raw backslash in a `paste` payload would eat the trailing one — `a\` +
    `\` collapses to `a\` with the final backslash escaped and no error — so
    the probe doubles payload backslashes and reports the doubled text, which
    is the raw argument and is what got quoted.
  * A file name cannot hold `/` or NUL, and cannot be empty; an argument
    cannot hold NUL. So NUL and the empty string are measured by neither, and
    `/` only by `paste`. Those three are asserted from `quotearg.c` in unit
    tests instead, and flagged there as unmeasured.

`LC_ALL=C.UTF-8` is forced throughout, matching `scripts/quote-probe.py`. It is
not an arbitrary choice: gnulib decodes multibyte sequences only in a UTF-8
locale, so under plain `C` a valid character comes back escaped byte by byte and
the recording cannot tell a *character* test from a *byte* test. Since
`design-decisions.md` §351 osh's string layer is UTF-8 in every locale, so `C`
is the setting in which the reference would be measuring the wrong thing.

Run:

    python3 scripts/c-maybe-probe.py userspace/coreutils/tests/c-maybe-gnu.txt

Rows are `<style> <input-hex> <output-hex>`, hex because the subject of the
measurement is itself a quoting form and a fixture written in one would have
to be trusted to be read.
"""

import os
import subprocess
import sys
import tempfile

ENV = {"LC_ALL": "C.UTF-8", "LANG": "C.UTF-8", "PATH": "/usr/bin:/bin"}

# Sequences that are *valid* UTF-8, one per reason the answer could differ —
# the same set `scripts/quote-probe.py` uses, because the printability question
# is the same one and the two recordings should be comparable row for row.
MULTIBYTE = [
    "\u00e9",      # two bytes, printable (LATIN SMALL LETTER E WITH ACUTE)
    "\u20ac",      # three bytes, printable (EURO SIGN)
    "\U0001f600",  # four bytes, printable (GRINNING FACE)
    "\u4e00",      # three bytes, printable, double-width (CJK IDEOGRAPH ONE)
    "\u00a0",      # NO-BREAK SPACE — a space that is not `isspace`-ish here
    "\u0301",      # COMBINING ACUTE ACCENT
    "\u2018",      # quote()'s own opening mark
    "\u2019",      # quote()'s own closing mark
    "\u0080",      # C1 control, decodes, not printable
    "\u009f",      # C1 control, the other end of the range
    "\u00ad",      # SOFT HYPHEN (Cf)
    "\u200b",      # ZERO WIDTH SPACE (Cf)
    "\ufeff",      # ZERO WIDTH NO-BREAK SPACE (Cf)
    "\u2028",      # LINE SEPARATOR (Zl)
    "\u2029",      # PARAGRAPH SEPARATOR (Zp)
    "\u0378",      # unassigned
    "\ue000",      # private use
]

# Sequences that are *not* valid UTF-8, one per way a decode can fail. These
# stay byte-escaped whatever the printability test decides, and they are what
# pins the boundary between "decoded a character" and "did not".
UNDECODABLE = [
    b"\xc3",              # two-byte lead, continuation missing
    b"\xc3z",             # two-byte lead, followed by a non-continuation
    b"\xe2\x82",          # three-byte sequence, truncated
    b"\xf0\x9f\x98",      # four-byte sequence, truncated
    b"\xc0\xaf",          # overlong encoding of `/`
    b"\xed\xa0\x80",      # a surrogate, which UTF-8 may not encode
    b"\xf4\x90\x80\x80",  # above U+10FFFF
    b"\x80",              # a continuation byte with no lead
    b"\xff",              # not a lead byte at all
]
PASTE_PREFIX = b"paste: delimiter list ends with an unescaped backslash: "


def gnu_version() -> str:
    out = subprocess.run(
        [b"paste", b"--version"], capture_output=True, env=ENV
    ).stdout
    return out.split(b"\n")[0].decode("ascii", "replace")


def colon_style(raw: bytes) -> bytes:
    """What `quotearg_n_style_colon (0, c_maybe_quoting_style, raw)` printed."""
    proc = subprocess.run([b"paste", b"-d", raw], capture_output=True, env=ENV)
    err = proc.stderr.rstrip(b"\n")
    if not err.startswith(PASTE_PREFIX):
        sys.exit(f"unexpected paste diagnostic for {raw!r}: {err!r}")
    return err[len(PASTE_PREFIX):]


def plain_style(name: bytes, scratch: bytes) -> bytes:
    """What `--quoting-style=c-maybe` printed for a file of this name."""
    path = scratch + b"/" + name
    with open(path, "wb"):
        pass
    try:
        proc = subprocess.run(
            [b"ls", b"--quoting-style=c-maybe", b"-d", b"--", name],
            capture_output=True,
            env=ENV,
            cwd=scratch,
        )
        if proc.returncode != 0:
            sys.exit(f"ls failed for {name!r}: {proc.stderr!r}")
        return proc.stdout.rstrip(b"\n")
    finally:
        os.unlink(path)


def payloads() -> list[bytes]:
    cases: list[bytes] = []
    # Every byte that can appear at all, alone and embedded. Alone matters
    # separately because gnulib treats `{`, `}`, `#` and `~` by position.
    for b in range(1, 256):
        cases.append(bytes([b]))
        cases.append(b"a" + bytes([b]) + b"z")
        cases.append(bytes([b]) + b"z")
    # Shapes that exercise the interactions between rules.
    cases += [
        b"a",
        b"abc",
        b"a b",
        b"a'b",
        b'a"b',
        b'"',
        b'""',
        b"a:b",
        b":",
        b"::",
        b"a:b\tc",
        b"a\tb",
        b"a\\b",
        b"\\",
        b"\\\\",
        b"\\t",
        b"a\\\tb",
        b"?",
        b"??!",
        b"??/",
        b"??(",
        b"a??=b",
        b"{",
        b"}",
        b"{}",
        b"#a",
        b"a#",
        b"~a",
        b"a~",
        b"\x7f",
        b"\xc3\xa9",
        b"\xff\xfe",
        b"a\x01b",
        b"a\x012b",
        b"a\x01" + b"9",
        b"%+,-./09:AZ]_az",
        b"!$&()*;<=>[^`|",
        b"a b\\",
        b"a'b\\",
        b'a"b\\',
        b"a:b\\",
    ]
    # A whole character, alone and with ASCII on both sides, so a row that
    # decodes is distinguishable from one that does not. `\` is excluded from
    # the neighbours: the `paste` oracle doubles payload backslashes, which
    # would change what is being asked.
    for ch in MULTIBYTE:
        enc = ch.encode()
        cases += [enc, b"a" + enc + b"z", enc + b"z", b'"' + enc, enc + b'"']
    # The same shapes over sequences that do not decode, plus each one beside a
    # character: this is the pair of rows that shows whether an escape swallows
    # its printable neighbour or stops at it.
    for seq in UNDECODABLE:
        cases += [seq, b"a" + seq + b"z", seq + b"z"]
        cases += [seq + "\u00e9".encode(), "\u00e9".encode() + seq]
    seen: set[bytes] = set()
    unique = []
    for c in cases:
        if c not in seen:
            seen.add(c)
            unique.append(c)
    return unique


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(f"usage: {sys.argv[0]} <output-file>")
    rows: list[str] = []
    with tempfile.TemporaryDirectory() as scratch_str:
        scratch = scratch_str.encode()
        for payload in payloads():
            # `paste` sees the payload as the raw argument with a lone
            # backslash appended, so a payload backslash must be doubled to
            # survive escape collapsing. What GNU quoted is the doubled text.
            raw = payload.replace(b"\\", b"\\\\") + b"\\"
            rows.append(f"c-maybe-colon {raw.hex()} {colon_style(raw).hex()}")
            # `/`, `.` and `..` are not names a file can have, so the `ls`
            # oracle simply cannot see them. `paste` covers all three.
            if b"/" not in payload and payload not in (b".", b".."):
                rows.append(
                    f"c-maybe {payload.hex()} {plain_style(payload, scratch).hex()}"
                )

    with open(sys.argv[1], "w", encoding="ascii", newline="\n") as out:
        out.write("# c_maybe_quoting_style, measured from GNU coreutils.\n")
        out.write(f"# {gnu_version()}\n")
        out.write("# Generated by scripts/c-maybe-probe.py under LC_ALL=C.UTF-8.\n")
        out.write("# <style> <input-hex> <output-hex>\n")
        for row in rows:
            out.write(row + "\n")
    print(f"{len(rows)} rows -> {sys.argv[1]}", file=sys.stderr)


main()
