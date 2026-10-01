#!/usr/bin/env python3
"""Generate `userspace/file/src/isomedia_table.rs` from file 5.45's magic.

The ISO base media (`ftyp`) rules of `magic/Magdir/animation` from file 5.45
(git tag FILE5_45), as a table `userspace/file/src/isomedia.rs` evaluates the
way libmagic does: sibling tests in order, every match printed, children
tested only under a matching parent, and the first `!:mime` met is the type.

    curl -sSfLO https://raw.githubusercontent.com/file/file/FILE5_45/magic/Magdir/animation
    python scripts/file-isomedia-gen.py animation userspace/file/src/isomedia_table.rs

The input is pinned by SHA-256: a different `animation` is refused rather than
silently producing a different table. The tests the section uses are the only
ones accepted (`string`, `string/W`, `byte`, `beshort`, with `x` or a value;
`%d` and `%.4s` in descriptions); anything else stops the generator, so a new
kind of rule cannot be dropped without notice.
"""

import hashlib
import re
import sys

PINNED_SHA256 = "d5b7d238de8d66fcadf313ab3e5e5f4d0cadcd4f5362ea169709a0bdc39e5cb0"
URL = "https://raw.githubusercontent.com/file/file/FILE5_45/magic/Magdir/animation"

# file's COPYING, whose condition 1 asks that the notice be kept "immediately at
# the beginning of the file, without modification" -- so the generated table,
# which is derived from file's source, opens with it.
NOTICE = """\
$File: COPYING,v 1.2 2018/09/09 20:33:28 christos Exp $
Copyright (c) Ian F. Darwin 1986, 1987, 1989, 1990, 1991, 1992, 1994, 1995.
Software written by Ian F. Darwin and others;
maintained 1994- Christos Zoulas.

This software is not subject to any export provision of the United States
Department of Commerce, and may be exported to any country or planet.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice immediately at the beginning of the file, without modification,
   this list of conditions, and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
SUCH DAMAGE."""


def magic_string(s):
    """A magic(5) string value with its backslash escapes decoded."""
    out = bytearray()
    i = 0
    simple = {"n": 10, "t": 9, "r": 13, "b": 8, "f": 12, "v": 11, "a": 7}
    while i < len(s):
        c = s[i]
        if c != "\\":
            out += c.encode("latin-1")
            i += 1
            continue
        i += 1
        if i >= len(s):
            out += b"\\"
            break
        c = s[i]
        if c in "01234567":
            j = i
            while j < len(s) and j < i + 3 and s[j] in "01234567":
                j += 1
            out.append(int(s[i:j], 8) & 0xFF)
            i = j
        elif c == "x":
            j = i + 1
            while j < len(s) and j < i + 3 and s[j] in "0123456789abcdefABCDEF":
                j += 1
            out.append(int(s[i + 1:j], 16) & 0xFF)
            i = j
        elif c in simple:
            out.append(simple[c])
            i += 1
        else:
            # `\ ` is a space; any other escaped character is itself.
            out += c.encode("latin-1")
            i += 1
    return bytes(out)


def parse_rule(line):
    """(level, offset, type, value, description) of one rule line."""
    m = re.match(r"(>*)(\d+)[ \t]+(\S+)[ \t]+((?:\\.|\S)+)[ \t]*(.*)$", line)
    if not m:
        sys.exit(f"cannot parse: {line!r}")
    return len(m.group(1)), int(m.group(2)), m.group(3), m.group(4), m.group(5)


class Rule:
    def __init__(self, level, offset, typ, value, desc):
        self.level = level
        self.offset = offset
        self.typ = typ
        self.value = value
        self.desc = desc
        self.mime = None
        self.children = []


def test_of(rule):
    """The Rust `Test` expression for a rule."""
    t, v = rule.typ, rule.value
    if t in ("string", "string/W"):
        if v == "x":
            return "Test::AnyString"
        b = magic_string(v)
        kind = "StringW" if t == "string/W" else "String"
        if kind == "StringW" and b" " in b:
            sys.exit(f"string/W with whitespace is not supported: {v!r}")
        return f"Test::{kind}({rs_bytes(b)})"
    if t == "byte":
        if v == "x":
            sys.exit("byte x is not used here")
        n = int(v, 0)
        return f"Test::Byte({n})"
    if t == "beshort":
        if v != "x":
            sys.exit(f"beshort with a value is not used here: {v!r}")
        return "Test::AnyBeShort"
    sys.exit(f"unsupported test type {t!r}")


def rs_bytes(b):
    return 'b"' + "".join(chr(c) if 32 <= c < 127 and c not in (34, 92) else f"\\x{c:02x}" for c in b) + '"'


def rs_str(s):
    """A Rust string literal for `s`, with a control character written as an
    escape rather than raw: file 5.45 has a literal TAB inside one
    description (the `caqv` rule), which is real data and must survive, but
    should not be invisible in the generated source."""
    out = []
    for c in s:
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif c == "\t":
            out.append("\\t")
        elif ord(c) < 32 or ord(c) == 127:
            out.append(f"\\u{{{ord(c):x}}}")
        else:
            out.append(c)
    return '"' + "".join(out) + '"'


def desc_parts(desc):
    """(backspace, text): whether the description begins with `\\b` (print
    with no separator), and the rest. Only `%d` and `%.4s` are accepted."""
    backspace = desc.startswith("\\b")
    text = desc[2:] if backspace else desc
    for spec in re.findall(r"%[^%]*?[a-zA-Z]", text):
        if spec not in ("%d", "%.4s"):
            sys.exit(f"unsupported format {spec!r} in {desc!r}")
    return backspace, text


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: file-isomedia-gen.py ANIMATION_MAGIC OUTPUT.rs")
    path, out_path = sys.argv[1], sys.argv[2]
    lines_out = []

    def emit(line=""):
        lines_out.append(line)

    data = open(path, "rb").read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != PINNED_SHA256:
        sys.exit(f"{path}: sha256 {digest}, expected {PINNED_SHA256} ({URL})")
    lines = data.decode("latin-1").splitlines()

    start = next(i for i, l in enumerate(lines) if re.match(r"4\s+string\s+ftyp\s+ISO Media", l))
    top = []
    last = None
    parents = {}
    for line in lines[start + 1:]:
        if not line or line.startswith("#"):
            continue
        if line.startswith("!:mime"):
            if last is None:
                sys.exit("a !:mime before any rule")
            last.mime = line.split(None, 1)[1].strip()
            continue
        if line.startswith("!:"):
            continue  # !:ext, !:strength -- not part of the output
        if not line.startswith(">"):
            break  # the next level-0 rule: the ftyp section is over
        level, offset, typ, value, desc = parse_rule(line)
        rule = Rule(level, offset, typ, value, desc)
        if level == 1:
            top.append(rule)
        elif level == 2:
            parents[1].children.append(rule)
        else:
            sys.exit(f"level {level} is not used here: {line!r}")
        parents[level] = rule
        last = rule

    def emit_rule(r, indent):
        bs, text = desc_parts(r.desc)
        mime = f"Some({rs_str(r.mime)})" if r.mime else "None"
        pad = " " * indent
        out = [f"{pad}Rule {{",
               f"{pad}    offset: {r.offset},",
               f"{pad}    test: {test_of(r)},",
               f"{pad}    backspace: {'true' if bs else 'false'},",
               f"{pad}    text: {rs_str(text)},",
               f"{pad}    mime: {mime},"]
        if r.children:
            out.append(f"{pad}    children: &[")
            for c in r.children:
                out.extend(emit_rule(c, indent + 8))
            out.append(f"{pad}    ],")
        else:
            out.append(f"{pad}    children: &[],")
        out.append(f"{pad}}},")
        return out

    emit("// " + NOTICE.replace("\n", "\n// ").replace("// \n", "//\n"))
    emit("//")
    emit("// @generated by scripts/file-isomedia-gen.py from file 5.45's")
    emit(f"// magic/Magdir/animation (sha256 {PINNED_SHA256}):")
    emit("// the rules under `4 string ftyp ISO Media`, in the file's order. Do not")
    emit("// edit by hand; regenerate.")
    emit()
    emit("use crate::isomedia::{Rule, Test};")
    emit()
    n_children = sum(len(r.children) for r in top)
    emit(f"/// The {len(top)} brand rules at offset 8, and the {n_children} under them.")
    emit("///")
    emit("/// Laid out by the generator, not rustfmt: regenerating must reproduce")
    emit("/// this file byte for byte.")
    emit("#[rustfmt::skip]")
    emit("pub(crate) const RULES: &[Rule] = &[")
    for r in top:
        emit("\n".join(emit_rule(r, 4)))
    emit("];")
    # Written here, not through stdout: on Windows a redirected stdout turns
    # every newline into CRLF.
    with open(out_path, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines_out) + "\n")


if __name__ == "__main__":
    main()
