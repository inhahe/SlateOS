#!/usr/bin/env python3
"""Regenerate `userspace/oils/src/bind_tables.rs` from the reference bash.

osh has no line editor, but bash without one still answers every `bind`
listing — so the listings are testable, and the only way to answer them is to
carry readline's compiled-in defaults. This script captures them rather than
having anyone transcribe them, so the provenance is a command instead of a
claim.

Two things make the capture subtle, both recorded in known-issues
TD-OILS-NO-BIND-BUILTIN:

  * `INPUTRC=/dev/null` is mandatory. With the reference system's
    `/etc/inputrc` loaded the emacs keymap has 494 lines rather than 487 —
    capturing that would bake one machine's configuration in as if it were
    readline's default.
  * `LC_ALL` is mandatory for the same reason: it decides `convert-meta`, which
    decides whether the escape prefix is written `\\M-b` or `\\eb`. See LOCALE.
  * bash is run non-interactively, which is exactly osh's permanent condition,
    so it warns `line editing not enabled` on stderr. That warning is discarded
    here; the tables it precedes are the real ones.

The reference must be a **glibc** bash, and this script refuses any other.
readline is configured per platform, so the table is not the same table
everywhere: a Cygwin readline knows `paste-from-clipboard`, which is a Windows
clipboard call, and a Linux one does not. This file's first capture was taken
from the MSYS bash that happened to be on the developer's PATH, and that one
extra name is what osh then answered `bind -l` with — 174 where every Linux
bash says 173. A generator that will capture from anything makes that mistake
silently and permanently, since the output is committed and nobody re-derives
it; so the check below is a refusal rather than a warning.

Usage — the glibc bash lives inside WSL, so run the script there too:
    wsl -e python3 scripts/gen-oils-bind-tables.py [--bash PATH]
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys

# readline's `keymap_names` table. `bind -m` accepts these and nothing else,
# and several are aliases for the same map — which the capture below confirms
# rather than assumes.
KEYMAP_ALIASES: list[list[str]] = [
    ["emacs", "emacs-standard"],
    ["emacs-meta"],
    ["emacs-ctlx"],
    ["vi", "vi-move", "vi-command"],
    ["vi-insert"],
]

DEFAULT_BASH = "/usr/bin/bash"

# The locale is as load-bearing as `INPUTRC` and for the same reason: it silently
# changes what is captured. readline's `_rl_init_eightbit` asks `LC_CTYPE` and
# takes the eight-bit branch for any locale that is not exactly `C` or `POSIX`
# (nls.c:168-186), which turns `convert-meta` **off** -- and `convert-meta` is
# what decides whether a listing names the escape sub-map after the modifier it
# stands for (`\M-b`) or writes the byte as itself (`\eb`).
#
# So the same bash captures two different tables depending on the environment
# the generator happened to inherit, and an unpinned capture is not
# reproducible. It is not a platform difference: measured four ways, MSYS bash
# and glibc bash agree with each other and disagree with themselves across the
# two locales.
#
# `C.UTF-8` is the eight-bit choice because osh is UTF-8-only (design-decisions
# §104), so it is the locale osh actually runs in, and it is present on every
# glibc system without needing to be generated.
LOCALE = "C.UTF-8"


def run_bash(bash: str, script: str) -> str:
    """Run a snippet under a pristine, non-interactive reference bash."""
    env = dict(os.environ, INPUTRC="/dev/null", LC_ALL=LOCALE, LANG=LOCALE)
    proc = subprocess.run(
        [bash, "-c", script],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return proc.stdout


def require_glibc(bash: str) -> str:
    """Refuse to capture from anything but a Linux bash; return its `$MACHTYPE`.

    Asked of the shell rather than inferred from its path, because the path is
    a coincidence and the answer is a fact: `$MACHTYPE` is the triple bash's own
    configure script recorded when it was built.
    """
    triple = run_bash(bash, "printf %s \"$MACHTYPE\"").strip()
    if "linux" not in triple:
        raise SystemExit(
            f"{bash} is a {triple or 'non-Linux'} bash, and its readline is not\n"
            "the one SlateOS targets -- a Cygwin build carries the extra\n"
            "`paste-from-clipboard` function, which is how this file came to\n"
            "claim 174 function names where Linux has 173.\n"
            "Capture from a glibc bash: `wsl -e python3 "
            "scripts/gen-oils-bind-tables.py`."
        )
    return triple


def split_quoted(line: str) -> tuple[str, str] | None:
    """Split a `bind -p` line `"KEYSEQ": function` into its two halves.

    The key sequence is double-quoted and a backslash escapes the next
    character, so `"\\"": self-insert` is the quote character bound to
    self-insert — not an empty sequence followed by junk. Lines that are not
    bindings (the leading blank, and `# name (not bound)`) return None.
    """
    if not line.startswith('"'):
        return None
    i = 1
    while i < len(line):
        if line[i] == "\\":
            i += 2
            continue
        if line[i] == '"':
            break
        i += 1
    else:
        raise ValueError(f"unterminated key sequence: {line!r}")
    seq = line[1:i]
    rest = line[i + 1 :]
    if not rest.startswith(": "):
        raise ValueError(f"no `: ' after key sequence: {line!r}")
    return seq, rest[2:]


def rust_str(s: str) -> str:
    """A Rust string literal for `s`.

    JSON's escaping is a subset of Rust's for the ASCII these tables contain,
    and unlike `repr` it produces double quotes.
    """
    return json.dumps(s)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--bash", default=DEFAULT_BASH, help="reference bash to capture from")
    ap.add_argument(
        "--out",
        default=None,
        help="output path (default: userspace/oils/src/bind_tables.rs beside this script)",
    )
    args = ap.parse_args()

    root = pathlib.Path(__file__).resolve().parent.parent
    out = pathlib.Path(args.out) if args.out else root / "userspace/oils/src/bind_tables.rs"

    triple = require_glibc(args.bash)
    version = run_bash(args.bash, "echo $BASH_VERSION").strip()
    print(f"capturing from {args.bash} (bash {version}, {triple})")

    functions = run_bash(args.bash, "bind -l").split()
    print(f"  {len(functions)} function names")

    keymaps: list[tuple[list[str], list[tuple[str, str]]]] = []
    for names in KEYMAP_ALIASES:
        dumps = {n: run_bash(args.bash, f"bind -m {n} -p") for n in names}
        first = dumps[names[0]]
        for n, d in dumps.items():
            if d != first:
                raise SystemExit(f"{n} is not an alias of {names[0]} after all")
        bindings = []
        for line in first.splitlines():
            pair = split_quoted(line)
            if pair is not None:
                bindings.append(pair)
        print(f"  {'/'.join(names)}: {len(bindings)} bindings")
        keymaps.append((names, bindings))

    variables: list[tuple[str, str]] = []
    for line in run_bash(args.bash, "bind -v").splitlines():
        if not line.startswith("set "):
            raise ValueError(f"unexpected `bind -v' line: {line!r}")
        name, _, value = line[4:].partition(" ")
        variables.append((name, value))
    print(f"  {len(variables)} variables")

    # Pinning `LC_ALL` above only *asks* for the eight-bit locale. If the
    # reference system has no `C.UTF-8`, `setlocale` quietly falls back to `C`
    # and readline captures the other table -- the same silent-wrong-capture
    # failure the locale was pinned to prevent. `convert-meta` is that choice
    # made visible, so read it back rather than trusting the request.
    meta = dict(variables).get("convert-meta")
    if meta != "off":
        raise SystemExit(
            f"`convert-meta' came back {meta!r}, so {LOCALE} did not take effect and\n"
            "readline captured its C-locale tables: every escape prefix would be\n"
            "written `\\M-b' instead of `\\eb'. Install the locale on the reference\n"
            "system (`locale-gen C.UTF-8') and capture again."
        )

    body: list[str] = []
    body.append(f'''//! Readline's built-in tables, as osh reports them.
//!
//! These are readline's *compiled-in* defaults, captured from bash {version}
//! built for {triple}, under `LC_ALL={LOCALE}` and
//! `INPUTRC=/dev/null` so that no `/etc/inputrc` or `~/.inputrc` on the
//! capturing machine is baked in as if it were a default — see known-issues
//! TD-OILS-NO-BIND-BUILTIN, which records that the two differ: 494 emacs
//! bindings with the reference system's `/etc/inputrc` loaded versus 487
//! without.
//!
//! Both of those conditions are part of what this file *is*, not notes about
//! where it came from, and the generator refuses a capture that does not meet
//! them:
//!
//!   * **The triple.** readline is configured per platform, so a Cygwin capture
//!     carries an extra `paste-from-clipboard` function — a Windows clipboard
//!     call — and answers `bind -l` with 174 names where every Linux bash says
//!     173. That is the one genuine platform difference in these tables.
//!   * **The locale.** It decides `convert-meta`, and `convert-meta` decides
//!     whether a listing names the escape sub-map after the modifier it stands
//!     for (`\\M-b`) or writes the byte as itself (`\\eb`). This is *not* a
//!     platform difference, though it was once recorded as one: measured four
//!     ways, MSYS bash and glibc bash agree with each other in each locale and
//!     disagree with themselves across the two.
//!
//! They live in their own module rather than in `interp.rs` because that file
//! is already one translation unit big enough to run rustc out of memory when
//! built with `--test`.
//!
//! **Generated — do not edit by hand.** Run `scripts/gen-oils-bind-tables.py`
//! against a reference bash to rebuild it.

/// One of readline's keymaps: the names `bind -m` accepts for it, and every
/// key sequence bound in it paired with the function it runs.
///
/// The bindings are in `bind -p` order, which is the funmap order the listings
/// are expected to keep — grouped by function name, and by key sequence within
/// a function.
pub struct Keymap {{
    /// Every name `bind -m` accepts for this map; several are aliases.
    pub names: &'static [&'static str],
    /// `(key sequence, function name)`, in `bind -p` order.
    pub bindings: &'static [(&'static str, &'static str)],
}}

/// Every function name readline knows, in the order `bind -l` prints them
/// (sorted, which is the order the listing is expected to keep).
pub const FUNCTION_NAMES: [&str; {len(functions)}] = [''')
    for f in functions:
        body.append(f"    {rust_str(f)},")
    body.append("];")
    body.append("")
    body.append("/// readline's keymaps, in no particular order — `bind -m` looks up by name.")
    body.append(f"pub const KEYMAPS: [Keymap; {len(keymaps)}] = [")
    for names, bindings in keymaps:
        body.append("    Keymap {")
        body.append("        names: &[" + ", ".join(rust_str(n) for n in names) + "],")
        body.append("        bindings: &[")
        for seq, fn in bindings:
            body.append(f"            ({rust_str(seq)}, {rust_str(fn)}),")
        body.append("        ],")
        body.append("    },")
    body.append("];")
    body.append("")
    body.append('''/// readline's variables and their default values, in `bind -v` order.
///
/// The four meta variables are the *eight-bit* defaults, not the ones the C
/// declarations give. readline picks between two sets at startup:
/// `_rl_init_eightbit` asks `LC_CTYPE` and hands it to `_rl_set_localevars`
/// (nls.c:168-186), which goes eight-bit for **any** locale that is not exactly
/// `C` or `POSIX` —
///
/// ```c
/// if (localestr && *localestr && (localestr[0] != 'C' || localestr[1]) && (STREQ (localestr, "POSIX") == 0))
///   {
///     _rl_meta_flag = 1;                    /* input-meta, meta-flag  on  */
///     _rl_convert_meta_chars_to_ascii = 0;  /* convert-meta           off */
///     _rl_output_meta_chars = 1;            /* output-meta            on  */
/// ```
///
/// so `C.UTF-8` takes that branch on the `localestr[1]` clause. The C
/// declarations (readline.c:300 and neighbours) are only what survives when the
/// locale really is `C`/`POSIX`, and osh is never in that locale: see
/// design-decisions.md §104, which settles that osh is UTF-8-only. Encoding the
/// C-locale set here made every meta binding land in the escape sub-map —
/// `"\\M-\\C-e": yank` came back as `\\M-\\C-e` where bash, with `convert-meta`
/// off, binds the single byte `0x85` and lists it `\\205`.
///
/// This block is why the generator now pins `LC_ALL` and then reads
/// `convert-meta` back: these values were once hand-corrected here while the
/// key sequences above were left as captured under the C locale, leaving the
/// file describing one locale in its variables and the other in its tables.
/// Capturing everything in one pinned locale is what keeps the two halves
/// talking about the same shell.
///
/// This is the one place the locale reaches the tables. `byte-oriented` is
/// *not* a second instance: it tracks `MB_CUR_MAX`, and readline reports it
/// `off` in both locales.''')
    body.append(f"pub const VARIABLES: [(&str, &str); {len(variables)}] = [")
    for name, value in variables:
        body.append(f"    ({rust_str(name)}, {rust_str(value)}),")
    body.append("];")
    body.append("")

    out.write_text("\n".join(body), encoding="utf-8", newline="\n")
    print(f"wrote {out} ({len(body)} lines)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
