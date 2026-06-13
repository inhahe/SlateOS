#!/usr/bin/env python3
"""Display-name pass: SlateOS -> "Slate OS" in user-facing text of .rs files.

The mechanical OuRoS->SlateOS rename produced the identifier form "SlateOS"
everywhere, including user-facing text. The operator wants human-readable text
to read "Slate OS" (with a space), while code identifiers stay "SlateOS".

Discriminator: human-readable display text lives inside string literals and
(for developer-facing prose) inside comments / module docs. Code identifiers
live in code regions. So we rewrite a standalone "SlateOS" -> "Slate OS" ONLY
inside the targeted region kind, and only when "SlateOS" is a standalone word
(not adjacent to . / _ - :). That protects app-ids ("SlateOS.installer"),
PRODID machine fields ("...//SlateOS//..."), path/type qualifiers
("SlateOS::Foo"), concatenated identifiers ("SlateOSConfig"), usernames
("SlateOSUser") and protocol tags ("SlateOS_1.0").

  * STRINGS mode (default): transform inside string literals.
  * COMMENTS mode (--comments): transform inside line/block comments.

A single tokenizer classifies every byte of the file into one region kind
(code / string / char-literal / line-comment / block-comment) so that quotes
embedded in comments or char literals (e.g. `// 2: "` or `'"'`) cannot desync
string detection. This was a real bug in an earlier line-by-line scanner:
a lone `"` in a comment started a bogus "string region" that swallowed the
rest of the file and caused real string literals to be skipped.

Inserting a space inside an existing string literal or comment cannot break
Rust syntax; key/assertion consistency is preserved because every targeted
occurrence is transformed identically.

Usage: python scripts/display_rename.py [--comments] file1.rs file2.rs ...
Prints one line per file actually changed, then a total.
"""
import re
import sys

# Standalone "SlateOS": not preceded/followed by a word char, and not adjacent
# to . / _ - : (those signal an identifier/app-id/path/PRODID, not display text).
WORD = re.compile(r'(?<![\w./:\-])SlateOS(?![\w./:\-])')

# Region scanners, tried in priority order at each position. The optional `b`
# prefix covers byte strings/chars (b"...", br"...", b'x'); the optional `r`
# (with matching hashes) covers raw strings, whose contents may contain
# unescaped quotes and backslashes and so must be matched whole.
RAW = re.compile(r'b?r(#*)"(?:.*?)"\1', re.DOTALL)
NORMAL = re.compile(r'b?"(?:[^"\\]|\\.)*"', re.DOTALL)
CHAR = re.compile(r"b?'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f]+\}|.)|[^'\\\n])'")


def transform_region(s: str) -> str:
    return WORD.sub('Slate OS', s)


def process(text: str, *, comments: bool) -> str:
    """Tokenize `text` and transform SlateOS only inside the targeted regions.

    If `comments` is False (STRINGS mode) we transform string literals; if True
    (COMMENTS mode) we transform line/block comments. All other regions (code,
    char literals, and the non-targeted text kind) are emitted verbatim.
    """
    out = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]

        # Raw / normal string literal (may carry a b/r prefix).
        m = None
        if c in 'br"':
            m = RAW.match(text, i) or NORMAL.match(text, i)
        if m is not None:
            seg = m.group(0)
            out.append(transform_region(seg) if not comments else seg)
            i = m.end()
            continue

        # Char / byte literal. Must be consumed so an embedded quote (e.g. '"')
        # does not look like the start of a string. A bare ' that doesn't form
        # a char literal is a lifetime/label; emit it as a single code char.
        if c in "b'":
            m = CHAR.match(text, i)
            if m is not None:
                out.append(m.group(0))  # never transform char literals
                i = m.end()
                continue
            if c == "'":
                out.append(c)
                i += 1
                continue

        # Line comment: // ... up to (not including) newline.
        if c == '/' and i + 1 < n and text[i + 1] == '/':
            j = text.find('\n', i)
            if j == -1:
                j = n
            seg = text[i:j]
            out.append(transform_region(seg) if comments else seg)
            i = j
            continue

        # Block comment: /* ... */ with Rust nesting.
        if c == '/' and i + 1 < n and text[i + 1] == '*':
            depth = 1
            j = i + 2
            while j < n and depth > 0:
                if text[j] == '/' and j + 1 < n and text[j + 1] == '*':
                    depth += 1
                    j += 2
                elif text[j] == '*' and j + 1 < n and text[j + 1] == '/':
                    depth -= 1
                    j += 2
                else:
                    j += 1
            seg = text[i:j]
            out.append(transform_region(seg) if comments else seg)
            i = j
            continue

        out.append(c)
        i += 1
    return ''.join(out)


def main(argv):
    comments = False
    if argv and argv[0] == '--comments':
        comments = True
        argv = argv[1:]
    changed = 0
    for path in argv:
        try:
            with open(path, 'r', encoding='utf-8') as f:
                original = f.read()
        except (OSError, UnicodeDecodeError) as e:
            print(f"SKIP {path}: {e}", file=sys.stderr)
            continue
        updated = process(original, comments=comments)
        if updated != original:
            with open(path, 'w', encoding='utf-8', newline='') as f:
                f.write(updated)
            changed += 1
            print(f"changed {path}")
    print(f"=== {changed} files changed ===")


if __name__ == '__main__':
    main(sys.argv[1:])
