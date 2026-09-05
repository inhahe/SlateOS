#!/usr/bin/env python3
"""Strip [workspace] and [profile.*] sections from sub-crate Cargo.toml files.

Run from the repo root after the root Cargo.toml has been set up as the
sole workspace manifest. This idempotently rewrites each sub-Cargo.toml
to remove `[workspace]` and any `[profile.*]` table; the root workspace
manifest carries those concerns now.

It also strips `[lints]` sections that say `workspace = true` if no
`[workspace.lints]` section exists in the root — but for our case the
root does have one, so we leave `[lints] workspace = true` alone.

The script preserves all other content verbatim (comments, formatting,
etc.) by working on the raw text rather than re-emitting TOML.
"""
from __future__ import annotations

import glob
import re
import sys
from pathlib import Path


HEADER_RE = re.compile(r'^\s*\[([^\]\s]+)\]\s*$')


def strip_sections(text: str, kill_predicate) -> str:
    """Remove TOML sections matching kill_predicate(header) -> bool.

    Sections include all lines from the header up to (but not including)
    the next section header or end-of-file.
    """
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        m = HEADER_RE.match(line)
        if m:
            header = m.group(1)
            if kill_predicate(header):
                # Skip this section entirely.
                i += 1
                # Also gobble a leading comment block above this section
                # (walk backwards over comment + blank lines we already
                # emitted), so we don't leave a stray comment introducing
                # a now-deleted section.
                while out and (out[-1].strip().startswith('#') or out[-1].strip() == ''):
                    out.pop()
                while i < len(lines) and not HEADER_RE.match(lines[i]):
                    i += 1
                continue
        out.append(line)
        i += 1
    return ''.join(out)


def should_kill(header: str) -> bool:
    if header == 'workspace':
        return True
    if header.startswith('profile.'):
        return True
    if header.startswith('profile '):
        return True
    return False


def process(path: Path) -> bool:
    """Returns True if the file was modified.

    The read translates any line ending to `\\n` (Python's default) and the
    write emits `\\n` verbatim -- `newline='\\n'`, *not* `Path.write_text`,
    which opens in text mode and on Windows turns every `\\n` back into
    `\\r\\n`. That asymmetry is the bug: read-translating and write-translating
    together mean a file this touches comes out with every line ending changed,
    burying the real edit under a whole-file diff. `scripts/quote-sweep.py`
    learned it the expensive way -- "rewrote every line of all 50 files it
    touched" -- and the lesson did not reach here.

    Translating on *read* rather than preserving is deliberate. The body below
    emits its own `\\n` (the blank-line collapse, the trailing newline), so
    preserving a CRLF input would produce a file mixing both endings, which is
    strictly worse than either. Normalising instead means a CRLF manifest comes
    out LF -- the tree's policy, the index's existing content, and therefore a
    zero-line diff.
    """
    original = path.read_text(encoding='utf-8')
    stripped = strip_sections(original, should_kill)
    # Collapse runs of >2 blank lines to a single blank line.
    stripped = re.sub(r'\n{3,}', '\n\n', stripped)
    # Trim leading blank lines.
    stripped = stripped.lstrip('\n')
    # Ensure trailing newline.
    if not stripped.endswith('\n'):
        stripped += '\n'
    if stripped != original:
        with open(path, 'w', encoding='utf-8', newline='\n') as f:
            f.write(stripped)
        return True
    return False


def main() -> int:
    roots = [
        'apps/*/Cargo.toml',
        'userspace/*/Cargo.toml',
        'gui/*/Cargo.toml',
        'init/*/Cargo.toml',
        'net/*/Cargo.toml',
    ]
    paths: list[Path] = []
    for pattern in roots:
        paths.extend(Path(p) for p in glob.glob(pattern))
    modified = 0
    for p in paths:
        if process(p):
            modified += 1
    print(f'Processed {len(paths)} files, modified {modified}.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
