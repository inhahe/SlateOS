#!/usr/bin/env python3
"""Regenerate `userspace/oils/src/bind_tables.rs` from the reference bash.

osh has no line editor, but bash without one still answers every `bind`
listing — so the listings are testable, and the only way to answer them is to
carry readline's compiled-in defaults. This script captures them rather than
having anyone transcribe them, so the provenance is a command instead of a
claim.

Two things make the capture subtle, both recorded in known-issues
TD-OILS-NO-BIND-BUILTIN:

  * `INPUTRC=/dev/null` is mandatory. With this machine's `/etc/inputrc`
    loaded the emacs keymap has 493 lines rather than 488 and `bind -s` is
    non-empty — capturing that would bake one machine's configuration in as if
    it were readline's default.
  * bash is run non-interactively, which is exactly osh's permanent condition,
    so it warns `line editing not enabled` on stderr. That warning is discarded
    here; the tables it precedes are the real ones.

Usage:
    python scripts/gen-oils-bind-tables.py [--bash PATH]
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

DEFAULT_BASH = "C:/Program Files/Git/usr/bin/bash.exe"


def run_bash(bash: str, script: str) -> str:
    """Run a snippet under a pristine, non-interactive reference bash."""
    env = dict(os.environ, INPUTRC="/dev/null")
    proc = subprocess.run(
        [bash, "-c", script],
        env=env,
        capture_output=True,
        text=True,
        check=True,
    )
    return proc.stdout


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

    version = run_bash(args.bash, "echo $BASH_VERSION").strip()
    print(f"capturing from {args.bash} (bash {version})")

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

    body: list[str] = []
    body.append(f'''//! Readline's built-in tables, as osh reports them.
//!
//! These are readline's *compiled-in* defaults, captured from the reference
//! bash (here bash {version}) under `INPUTRC=/dev/null` so that no
//! `/etc/inputrc` or `~/.inputrc` on the capturing machine is baked in as if it
//! were a default — see known-issues TD-OILS-NO-BIND-BUILTIN, which records
//! that the two differ: 493 emacs bindings with `/etc/inputrc` loaded versus
//! 488 without.
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
    body.append("/// readline's variables and their default values, in `bind -v` order.")
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
