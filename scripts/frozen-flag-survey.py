#!/usr/bin/env python3
"""Boolean fields an app reads and can never change.

WHY THIS EXISTS. `apps/mindmap`'s `show_sidebar` was `true` at construction and
had no writer anywhere, so the panel could not be closed. `apps/pdfviewer`'s
`dark_mode` was `true` with no writer, so every page of every document was
drawn in rgb(40,42,54) and the "reading mode" was one mode.
`apps/calendar`'s `week_starts_monday` was `true` with no writer, so every
month grid began on Monday for everybody -- a question with no universally
right answer, answered once at compile time. Eight of these were fixed by hand
on 2026-09-18 and they were found by reading, one app at a time.

The defect is not dead code and no compiler warns about it. The field is read,
the renderer draws from it, the program behaves *consistently* -- it simply has
one behaviour where it offers two. A user looking at the window sees a setting
that does not exist rather than a setting that is broken, which is why nobody
reports it.

WHAT THIS IS NOT. Two neighbouring gates already exist and this is neither:

  * `check-fields-written-never-read.py` -- work the program does and throws
    away. The mirror image: written, never read.
  * `check-unreachable-mutators.py` -- a mutator nothing calls. Scoped to
    `kernel/src/fs`, and it finds the case where the *operation* exists and is
    unwired. Here there is usually no operation at all.

HOW IT DECIDES, because the number is only worth what the method is. A field
counts as frozen when, **in live (non-test) code across every file of the
crate**:

  * it is declared `name: bool` in a struct;
  * `self.name` (or any `.name`) is read somewhere;
  * and no assignment to it appears anywhere -- no `.name =`, no `.name |=`,
    no `&mut ...name`.

Construction does not count as a write: `name: true,` in a struct literal is
the value it is stuck at, which is the whole complaint.

Restricted to `bool` and to **fieldless enums**. A field of a struct type can
be changed by calling a method on it -- `self.viewport.scroll_by(..)` mutates
the field's contents without ever assigning the field -- so "never assigned"
means nothing there. For a `bool`, or an enum whose variants carry nothing, it
means exactly what it says.

Enums were added after the bool-only version missed `apps/torrent`'s
`sort_column`: declared, constructed as `Added`, read once in the comparator
and assigned nowhere, so the list sorts by date-added descending for ever and
ten of its eleven comparator arms are unreachable. A survey that reports the
`bool` beside it and not that is reporting the smaller half of one defect.

KNOWN LIMITS, in the tool's own voice rather than a reader's:

  * A field written only through `..Default::default()` or a destructuring
    assignment is not seen as written, and would be reported wrongly.
  * A field name that repeats across two structs in one crate is treated as
    one name; a write to either exonerates both.
  * Test-only writers are deliberately ignored, because a flag only a test can
    move is frozen for every real user. That is a decision, not an oversight.
  * **A field built from parsed configuration reads as frozen and is not.**
    `apps/installer`'s `wipe` and `auto_reboot` come from an answer file --
    `disk.get("wipe")`, `root.get("auto_reboot")` -- and are set by
    constructing the struct, which this deliberately does not count as a
    write. They are settable by editing a document; nothing assigns them
    afterwards. The tell is a `get(` or a deserialize near the construction
    site, and it is worth looking for before believing any row in an app that
    reads a config file.

    This is the same evidence as `apps/lockscreen`, with the opposite answer:
    there `main` passes `LockScreenConfig::default()` and nothing parses
    anything, so the flags really are fixed at compile time. Construction from
    a *parser* and construction from a *literal* look identical to this tool
    and mean opposite things.

Run from anywhere: `python scripts/frozen-flag-survey.py [--all]`.
"""

from __future__ import annotations

import io
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import rustlex  # noqa: E402

ROOT = HERE.parent

#: A fieldless `enum Name { A, B, C }` -- no payloads on any variant.
#:
#: Those are safe to judge exactly as a `bool` is: nothing can change one in
#: place, so "never assigned" means "never changed". An enum with payloads, or
#: any struct type, can be mutated through a method without the field ever
#: appearing on the left of an `=`, which is why they are left alone.
PLAIN_ENUM_RE = re.compile(
    r"\benum\s+([A-Z]\w*)\s*\{([^{}]*)\}",
    re.DOTALL,
)

#: `show_sidebar: bool,` in a struct declaration.
FIELD_RE = re.compile(r"\b(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*bool\s*,")

#: Any assignment to a field of that name, however it is reached.
def written_re(name: str) -> re.Pattern[str]:
    return re.compile(
        r"\.%s\s*(?:=[^=]|[|&^+-]=)" % re.escape(name)
        + r"|&\s*mut\s+[A-Za-z_][A-Za-z0-9_.]*\.%s\b" % re.escape(name)
    )


#: Any read at all.
def read_re(name: str) -> re.Pattern[str]:
    return re.compile(r"\.%s\b" % re.escape(name))


def crate_live_code(crate: Path) -> str:
    """Every `.rs` file in the crate, comments and literals blanked, tests cut.

    All files, not just `main.rs`: 12 of 141 apps have more than one, and a
    sweep over `main.rs` alone reports a writer missing that is one file away.
    """
    out = []
    for path in sorted((crate / "src").rglob("*.rs")):
        try:
            src = io.open(path, encoding="utf-8").read()
        except (OSError, UnicodeDecodeError):
            continue
        live, _test = rustlex.live_code(src)
        out.append(rustlex.strip_noise(live, keep_literals=False))
    return "\n".join(out)


#: `impl App for FooApp` / `impl FooApp {` containing the key handler.
APP_IMPL_RE = re.compile(r"\bimpl\s+(?:\w+\s+for\s+)?([A-Z]\w*)\b")


def app_struct_body(code: str) -> str:
    """Just the app's own state struct, brace-matched.

    Without this the survey is dominated by *data* whose booleans are
    immutable on purpose: a directory entry's `is_directory`, a tree node's
    `expandable`, a style's `bold`. Those are read and never written because
    that is what they are, and reporting them buries the handful of fields
    that really are a setting the user cannot reach. The first run of this
    said 197 in 67 apps and most of it was furniture.

    The struct is found through `impl App for X` -- the one trait every app in
    this tree implements to receive events -- rather than by walking impl
    bodies looking for a key handler. The first version did the latter and
    silently fell back to the whole crate for every app, because a brace walk
    over blanked source finds a closing brace the source does not have. One
    regex naming the thing outright cannot fail that way.

    Falls back to the whole crate when the trait impl is absent, because a
    survey that silently reports nothing is worse than one that reports too
    much -- but that fallback is now visible in the count, not hidden.
    """
    owner = re.search(r"impl\s+App\s+for\s+(\w+)", code)
    if owner is None:
        return code
    decl = re.search(r"struct\s+%s\b[^{]*\{" % re.escape(owner.group(1)), code)
    if decl is None:
        return code
    start = decl.end()
    depth, i = 1, start
    while i < len(code) and depth:
        if code[i] == "{":
            depth += 1
        elif code[i] == "}":
            depth -= 1
        i += 1
    return code[start:i]


def plain_enums(code: str) -> set[str]:
    """Every enum in the crate whose variants carry nothing."""
    found = set()
    for m in PLAIN_ENUM_RE.finditer(code):
        name, body = m.group(1), m.group(2)
        variants = [v.strip() for v in body.split(",")]
        if all(
            v == "" or re.fullmatch(r"(?:#\[[^\]]*\]\s*)?[A-Z]\w*", v)
            for v in variants
        ):
            found.add(name)
    return found


def survey(crate: Path) -> tuple[list[str], int]:
    code = crate_live_code(crate)
    if not code.strip():
        return [], 0
    body = app_struct_body(code)
    # Declared in the app's own struct; written (or not) anywhere in the crate.
    names = set(FIELD_RE.findall(body))
    for enum in plain_enums(code):
        field_of_enum = re.compile(
            r"\b(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*%s\s*,"
            % re.escape(enum)
        )
        names.update(field_of_enum.findall(body))
    names = sorted(names)
    frozen = [
        name
        for name in names
        if read_re(name).search(code) and not written_re(name).search(code)
    ]
    return frozen, len(names)


def main(argv: list[str]) -> int:
    show_all = "--all" in argv
    rows: list[tuple[int, str, list[str]]] = []
    total_fields = 0
    for crate in sorted((ROOT / "apps").iterdir()):
        if not crate.is_dir():
            continue
        frozen, count = survey(crate)
        total_fields += count
        if frozen:
            rows.append((len(frozen), crate.name, frozen))

    rows.sort(key=lambda r: (-r[0], r[1]))
    stuck = sum(r[0] for r in rows)
    print(
        f"{stuck} field(s) in {len(rows)} app(s) are read and never written, "
        f"out of {total_fields} bool and plain-enum fields scanned"
    )
    print("(live code only; construction is not a write; see this file's docstring)\n")
    limit = len(rows) if show_all else 25
    for n, name, frozen in rows[:limit]:
        shown = ", ".join(frozen[:6]) + ("..." if len(frozen) > 6 else "")
        print(f"{name:<20}{n:>3}  {shown}")
    if not show_all and len(rows) > limit:
        print(f"\n...and {len(rows) - limit} more apps; --all for the rest")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
