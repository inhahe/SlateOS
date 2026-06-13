#!/usr/bin/env python3
"""One-shot display-name pass: SlateOS -> "Slate OS" inside .rs string literals.

The mechanical OuRoS->SlateOS rename produced the identifier form "SlateOS"
everywhere, including user-facing text. The operator wants human-readable text
to read "Slate OS" (with a space), while code identifiers stay "SlateOS".

Discriminator: in Rust, display text lives inside string literals; code
identifiers live outside them. So we rewrite "SlateOS" -> "Slate OS" ONLY
inside string-literal regions, and only when "SlateOS" is a standalone word
(not adjacent to . / _ - :). That protects app-ids ("SlateOS.installer"),
PRODID machine fields ("...//SlateOS//..."), path/type qualifiers
("SlateOS::Foo"), and concatenated identifiers ("SlateOSConfig").

Inserting a space inside an existing string literal cannot break Rust syntax;
key/assertion consistency is preserved because every literal occurrence is
transformed identically.

Usage: python scripts/display_rename.py file1.rs file2.rs ...
Prints one line per file actually changed, then a total.
"""
import re
import sys

# Standalone "SlateOS": not preceded/followed by a word char, and not adjacent
# to . / _ - : (those signal an identifier/app-id/path/PRODID, not display text).
WORD = re.compile(r'(?<![\w./:\-])SlateOS(?![\w./:\-])')

# String-literal scanner: raw strings r#*"..."#* first (so their contents,
# which may contain unescaped quotes/backslashes, are matched whole), then
# normal "..." strings honoring \" escapes.
RAW = re.compile(r'r(#*)"(?:.*?)"\1', re.DOTALL)
NORMAL = re.compile(r'"(?:[^"\\]|\\.)*"', re.DOTALL)


def transform_region(s: str) -> str:
    return WORD.sub('Slate OS', s)


def process(text: str) -> str:
    # Walk the text, find string literals (raw or normal), transform only inside.
    out = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        # Try raw string at this position.
        m = None
        if c == 'r':
            m = RAW.match(text, i)
        if m is None and c == '"':
            m = NORMAL.match(text, i)
        if m is not None:
            out.append(transform_region(m.group(0)))
            i = m.end()
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def main(argv):
    changed = 0
    for path in argv:
        try:
            with open(path, 'r', encoding='utf-8') as f:
                original = f.read()
        except (OSError, UnicodeDecodeError) as e:
            print(f"SKIP {path}: {e}", file=sys.stderr)
            continue
        updated = process(original)
        if updated != original:
            with open(path, 'w', encoding='utf-8', newline='') as f:
                f.write(updated)
            changed += 1
            print(f"changed {path}")
    print(f"=== {changed} files changed ===")


if __name__ == '__main__':
    main(sys.argv[1:])
