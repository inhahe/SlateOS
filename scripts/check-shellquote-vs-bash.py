#!/usr/bin/env python3
"""Cross-check kernel/src/shellquote.rs's rules against real bash.

Ports the Rust scanner byte-for-byte, then asks bash what it actually does
with the same input.  A disagreement means the Rust is wrong, not bash.

Requires WSL; see `bashprobe.py` for why that keeps it out of the boot test.

**The port is the weak point, and is guarded.**  A hand copy of the scanner
can drift away from the Rust it claims to model, and a drifted copy reports
"0 disagreements" about a scanner that no longer exists -- worse than no
checker, because it looks like evidence.  `assert_port_matches_rust()` reads
`shellquote.rs` and refuses to run if the one table that is easy to get
silently wrong -- the set of characters a backslash may escape inside
double quotes -- differs from the set below.  It cannot prove the whole port
is faithful; it can and does stop the failure mode that actually happens,
which is a rule being changed in the Rust and not here.
"""
import pathlib
import re
import sys

import bashprobe

UNQ, SGL, DBL = "U", "S", "D"
DQ_ESCAPABLE = set(b'"\\$`\n')

RUST = pathlib.Path(__file__).resolve().parent.parent / "kernel" / "src" / "shellquote.rs"


def assert_port_matches_rust():
    """Refuse to run if the Rust's escape alphabet is not the one ported."""
    try:
        src = RUST.read_text(encoding="utf-8")
    except OSError as e:
        raise SystemExit(f"cannot read {RUST}: {e}") from e
    m = re.search(r"const DQ_ESCAPABLE: \[u8; \d+\] = \[([^\]]*)\];", src)
    if not m:
        raise SystemExit(
            "DQ_ESCAPABLE was renamed or reshaped in shellquote.rs.\n"
            "  This checker's port can no longer be shown to match it, so its\n"
            "  verdict would be about a scanner that is not the one shipping."
        )
    # `b'"'`, `b'\\'`, `b'$'`, `b'`'`, `b'\n'` -> the bytes themselves.
    escapes = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", "'": "'"}
    theirs = set()
    for lit in re.findall(r"b'((?:\\.|[^'])+)'", m.group(1)):
        theirs.add(ord(escapes[lit[1]] if lit.startswith("\\") else lit))
    if theirs != DQ_ESCAPABLE:
        raise SystemExit(
            "PORT HAS DRIFTED -- every result below would be about the wrong "
            "scanner.\n"
            f"  shellquote.rs: {sorted(theirs)}\n"
            f"  this file    : {sorted(DQ_ESCAPABLE)}"
        )



def scan(bs: bytes):
    """Yield (off, byte, ctx, escaped, structural) -- the Rust `Tok`."""
    i = 0
    ctx = UNQ
    pending = False
    n = len(bs)
    while i < n:
        b = bs[i]
        off = i
        i += 1
        if pending:
            pending = False
            yield (off, b, ctx, True, False)
            continue
        if ctx == SGL:
            structural = b == ord("'")
            if structural:
                ctx = UNQ
            yield (off, b, SGL, False, structural)
        elif ctx == DBL:
            if b == ord("\\") and i < n and bs[i] in DQ_ESCAPABLE:
                pending = True
                yield (off, b, DBL, False, True)
            else:
                structural = b == ord('"')
                if structural:
                    ctx = UNQ
                yield (off, b, DBL, False, structural)
        else:
            if b == ord("\\") and i < n:
                pending = True
                yield (off, b, UNQ, False, True)
            elif b == ord("'"):
                ctx = SGL
                yield (off, b, SGL, False, True)
            elif b == ord('"'):
                ctx = DBL
                yield (off, b, DBL, False, True)
            else:
                yield (off, b, UNQ, False, False)


def is_bare(t):
    return t[2] == UNQ and not t[3] and not t[4]


def strip_quotes(bs):
    return bytes(t[1] for t in scan(bs) if not t[4])


def split_bare_words(bs):
    out, start, quoted = [], None, False
    for off, b, ctx, esc, st in scan(bs):
        if b in (32, 9) and is_bare((off, b, ctx, esc, st)):
            if start is not None:
                out.append((start, off, quoted))
                start = None
            quoted = False
        else:
            if start is None:
                start = off
            if st:
                quoted = True
    if start is not None:
        out.append((start, len(bs), quoted))
    return out


def find_bare(bs, needle):
    for t in scan(bs):
        if t[1] == needle and is_bare(t):
            return t[0]
    return None


def bash_words(line: bytes):
    """The exact word list bash produces for `line`.

    Delegated to bashprobe.  The original version of this function passed the
    script as an argv element to `bash -c` and read `printf '%s\\n'` output,
    and BOTH halves of that were wrong in ways that quietly weakened this
    file's verdict:

      * the argv round trip through Windows/wsl.exe ate backslashes, so every
        backslash case below was compared against a *different input* than
        the one written down -- in a file whose whole subject is backslashes;
      * `printf` reruns its format at least once, so zero words and one empty
        word both printed a single blank line.

    Both are why this file once reported 0 failures while the transport was
    silently mangling its own test data.  bashprobe delivers the bytes on
    stdin and counts words with `set --`/`$#`, and proves the transport is
    faithful before any case runs.
    """
    return bashprobe.words(line.decode("latin-1"), setup="")


def ours_words(line: bytes):
    return [strip_quotes(line[s:e]) for s, e, _q in split_bare_words(line)]


CASES = [
    # the two known-issues bugs
    b'"it\'s fine"',
    b'"don\'t.txt"',
    # backslash per context
    b"a\\ b",
    b"a\\'b",
    b"a\\>b",
    b'"C:\\dir"',
    b'"say \\"hi\\""',
    b"'it\\'",
    b"a\\\\",
    # quoting basics
    b"'a > b'",
    b"''",
    b"'' x",
    b'"" x',
    b"a'b'c",
    b'a"b"c',
    b"'a'\\''b'",
    # NOTE: no `$`-bearing cases here.  bash expands before quote removal and
    # this harness only does quote removal, so any `$` case compares apples to
    # oranges.  The `$` question -- "which context is it in?" -- is asked
    # separately below, which is the only part kshell's expander needs.
    b"a b  c",
    b"  lead",
    b'"a b" c',
    b"x'y z'w",
    b'"a\\tb"',
    b'"a\\\\b"',
    b"\\\\",
    b"a\\",
]

assert_port_matches_rust()
print("port verified against shellquote.rs")
bashprobe.assert_transport_is_faithful()
print("transport verified faithful\n")

fails = 0
for line in CASES:
    theirs = bash_words(line)
    if theirs is None:
        print(f"SKIP (bash rejected): {line!r}")
        continue
    mine = ours_words(line)
    ok = mine == theirs
    if not ok:
        fails += 1
    print(f"{'ok  ' if ok else 'FAIL'} {line!r}\n      bash={theirs!r}\n      ours={mine!r}")

# Delimiter visibility: the redirect bug, asked directly.
DELIM = [
    (b'echo "it\'s fine" > out', ord(">"), 17),
    (b"cat < \"don't.txt\"", ord("<"), 4),
    (b"echo 'a > b'", ord(">"), None),
    (b'echo "a" > b', ord(">"), 9),
    (b"echo a\\>b", ord(">"), None),
]
print("\n--- bare-delimiter offsets ---")
for line, needle, want in DELIM:
    got = find_bare(line, needle)
    ok = got == want
    if not ok:
        fails += 1
    print(f"{'ok  ' if ok else 'FAIL'} {line!r} {chr(needle)} -> {got} (want {want})")

# Which context is a `$` in?  This is the whole of what kshell's expander needs
# from the scanner, and getting it wrong is bug #2 in known-issues.md:
# `echo "it's $HOME"` prints $HOME literally because the expander tracks only
# `'` and treats the apostrophe inside the double quotes as opening a region.
CTX = [
    (b'echo "it\'s $HOME"', DBL),   # expands  -- the bug
    (b"echo 'it \"is\" $HOME'", SGL),   # does not expand
    (b"echo $HOME", UNQ),           # expands
    (b'echo "\\$HOME"', DBL),       # inside "..."; `escaped` is what stops it
    (b"echo \\' $HOME", UNQ),       # `\'` must not flip quoting for the line
]
print("\n--- context of the `$` ---")
for line, want in CTX:
    toks = [t for t in scan(line) if t[1] == ord("$")]
    got = toks[0][2] if toks else None
    esc = toks[0][3] if toks else None
    # For the escaped case the `$` is data, which the expander sees as "escaped"
    # rather than "quoted"; ctx is Unquoted either way.
    ok = got == want
    if not ok:
        fails += 1
    print(f"{'ok  ' if ok else 'FAIL'} {line!r} -> ctx={got} escaped={esc} (want {want})")

print(f"\n{fails} failure(s)")
sys.exit(1 if fails else 0)
