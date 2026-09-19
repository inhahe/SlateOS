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

WHAT IT LOOKS AT, which is the part that took three tries to get right. The
population is the app's own state struct -- the one behind
`impl <path::>App for X` -- **plus every struct it holds exactly one of**,
followed transitively. Not the whole crate: a sweep over every struct is
dominated by data whose booleans are immutable on purpose (a directory
entry's `is_directory`, a style's `bold`), and the first version of this
reported 197 fields that way, nearly all of them furniture.

Three corrections, each of which changed the answer by more than the whole
exercise was worth:

  * The trait regex wanted a bare `impl App for X`. Every app in this tree
    writes `impl oswindow::app::App for X`, so it matched none of them and 19
    of the 20 apps reported had silently fallen back to the whole-crate scan.
    That fallback is now *printed*, in its own section, never summed into the
    headline -- a docstring promising it was "visible in the count" is not the
    same thing as it being visible.
  * Scoping to the app struct alone dropped real settings that live one field
    away: `apps/lockscreen` keeps `show_clock_seconds` in a
    `LockScreenConfig`. Hence the transitive walk.
  * The walk then had to stop following data. A type the crate ever stores
    inside a `Vec`/`HashMap`/... describes one of many, so it is excluded
    wherever else it also appears -- `apps/chess` holds a `Move` in
    `last_move` *and* a `Vec<Move>` in its history, and `is_castling` is a
    fact about one move, not a setting.

HOW IT DECIDES, because the number is only worth what the method is. A field
counts as frozen when, **in live (non-test) code across every file of the
crate**:

  * it is declared `name: bool` in a struct;
  * `self.name` (or any `.name`) is read somewhere;
  * and no assignment to it appears anywhere -- no `.name =`, no `.name |=`,
    no `&mut ...name`.

Construction does not count as a write: `name: true,` in a struct literal is
the value it is stuck at, which is the whole complaint.

Restricted to `bool` and to **enums nothing can change in place**. A field of
a struct type can be changed by calling a method on it --
`self.viewport.scroll_by(..)` mutates the field's contents without ever
assigning the field -- so "never assigned" means nothing there. For a `bool`,
or for an enum with no `&mut self` method anywhere in its impl blocks, it
means exactly what it says.

That test used to be "every variant carries nothing", and it was wrong in
both directions. It excluded `apps/filesearch`'s `SizeFilter` -- seven
fieldless variants and one `Custom(u64, u64)` -- whose field was a frozen
filter strip exactly like the two beside it, so the tool reported those two
and not the third; it was found by reading, which is the work this exists to
replace. And it included fieldless enums that a `fn advance(&mut self) { *self
= ... }` moves without any assignment this survey can see. The shape of the
variants was never the question. Whether a value can change without being
assigned is, and the answer is in the type's impl blocks.

Enums were added at all after the bool-only version missed `apps/torrent`'s
`sort_column`: declared, constructed as `Added`, read once in the comparator
and assigned nowhere, so the list sorted by date-added descending for ever
and ten of its eleven comparator arms were unreachable. A survey that reports
the `bool` beside it and not that is reporting the smaller half of one defect.

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

Rows already looked at and found not to be defects live in
`scripts/frozen-flag-answered.txt`, one per line with the reason. They are
subtracted from the count and listed separately; a line there naming a field
this no longer reports is an error, because an answer about code that has
since changed reads as a decision about the code as it is now.

Run from anywhere: `python scripts/frozen-flag-survey.py [--all] [--answered]`.
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

#: `enum Name { ... }`, with the variant list captured.
#:
#: Nested braces would defeat `[^{}]*`, and a variant that carries a struct
#: body (`Foo { a: u8 }`) has them -- such an enum simply is not matched, and
#: so is not judged. That is the safe direction to fail in.
PLAIN_ENUM_RE = re.compile(
    r"\benum\s+([A-Z]\w*)\s*\{([^{}]*)\}",
    re.DOTALL,
)

#: `fn whatever(&mut self` inside a block. The question this answers is
#: whether a value of the type can be changed without being assigned.
MUT_METHOD_RE = re.compile(r"fn\s+\w+\s*(?:<[^>]*>)?\s*\(\s*&\s*mut\s+self\b")


def impl_bodies(code: str, ty: str) -> list[str]:
    """Every `impl ... <ty> ... { ... }` body in the crate, brace-matched."""
    out = []
    for m in re.finditer(r"\bimpl\b[^{;]*\b%s\b[^{;]*\{" % re.escape(ty), code):
        depth, i = 1, m.end()
        while i < len(code) and depth:
            if code[i] == "{":
                depth += 1
            elif code[i] == "}":
                depth -= 1
            i += 1
        out.append(code[m.end() : i])
    return out


def judgeable_enums(code: str) -> set[str]:
    """Enums whose values cannot be changed except by assigning them.

    Two things were wrong with the previous rule, which was "every variant
    carries nothing":

      * It excluded `apps/filesearch`'s `SizeFilter`, seven fieldless
        variants and one `Custom(u64, u64)`. That field was a frozen filter
        strip exactly like the two beside it, and the tool reported those two
        and not this one. It was found by reading, which is what this survey
        exists to replace.
      * It included fieldless enums that *can* change in place. `fn
        advance(&mut self) { *self = ... }` moves a field with no assignment
        to it anywhere, so `self.mode.advance()` is a writer this survey
        cannot see and would call frozen.

    Both are the same question -- can a value of this type change without
    being assigned? -- and the answer is in the type's own impl blocks, not in
    the shape of its variants. An enum with no `&mut self` method can only be
    changed by assignment, whatever its variants carry.
    """
    found = set()
    for m in PLAIN_ENUM_RE.finditer(code):
        name = m.group(1)
        if any(MUT_METHOD_RE.search(body) for body in impl_bodies(code, name)):
            continue
        found.add(name)
    return found

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


#: The one trait every windowed app implements, *however the crate spells it*.
#:
#: The first version of this required the bare name -- `impl App for X`.
#: Every app in this tree writes `impl oswindow::app::App for X`, so it
#: matched **none of them**: 19 of the 20 apps this survey reported on had
#: silently fallen back to scanning the whole crate, where a directory
#: entry's `is_directory` and a tree node's `has_children` count as frozen
#: settings. The headline figure was two-thirds furniture and said so
#: nowhere.
#:
#: That is the third checker in this tree to have its population defined by
#: a *name* it expected rather than by the thing it meant, so the fallback
#: is now reported in the output -- rather than described in a docstring as
#: "visible in the count" while being invisible.
APP_IMPL_OWNER_RE = re.compile(
    r"\bimpl\s+(?:[A-Za-z_]\w*\s*::\s*)*App\s+for\s+([A-Z]\w*)"
)


#: Types that hold *many* of something. A field of one is a per-datum field.
#:
#: This is the line between the two populations. `entries: Vec<FileEntry>`
#: makes `FileEntry::is_directory` a property of a directory entry -- read
#: everywhere, written never, and correct that way. `config: LockScreenConfig`
#: makes `LockScreenConfig::show_clock_seconds` a setting of the one running
#: lock screen, and a setting nothing writes is a setting nobody has.
#: Identical Rust; opposite meanings; the container is what tells them apart.
MANY_OF = ("Vec", "VecDeque", "HashMap", "BTreeMap", "HashSet", "BTreeSet", "Slab")

#: Wrappers that still hold at most one, and are therefore followed through.
ONE_OF = ("Option", "Box", "Rc", "Arc", "RefCell", "Cell", "Mutex", "RwLock")

#: `name: Type,` in a struct body, with the type captured whole.
TYPED_FIELD_RE = re.compile(r"[a-z_][a-z0-9_]*\s*:\s*([A-Za-z_][A-Za-z0-9_:<>, ]*?)\s*,")


def struct_body(code: str, name: str) -> str | None:
    """The brace-matched body of `struct <name> { ... }`, or `None`."""
    decl = re.search(r"struct\s+%s\b[^{;]*\{" % re.escape(name), code)
    if decl is None:
        return None
    depth, i = 1, decl.end()
    while i < len(code) and depth:
        if code[i] == "{":
            depth += 1
        elif code[i] == "}":
            depth -= 1
        i += 1
    return code[decl.end() : i]


def singleton_field_types(body: str) -> list[str]:
    """Crate types this struct holds exactly one of, unwrapped of `Option` etc."""
    out = []
    for m in TYPED_FIELD_RE.finditer(body):
        ty = m.group(1).strip()
        if any(re.search(r"\b%s\s*<" % c, ty) for c in MANY_OF):
            continue
        for wrapper in ONE_OF:
            inner = re.fullmatch(r"%s\s*<(.+)>" % wrapper, ty)
            if inner:
                ty = inner.group(1).strip()
        ty = ty.split("::")[-1].strip()
        if re.fullmatch(r"[A-Z]\w*", ty):
            out.append(ty)
    return out


def held_by_the_many(code: str) -> set[str]:
    """Every type the crate ever puts inside a collection.

    A type can be *both* a singleton field and a collection element, and when
    it is, it is data. `apps/chess` keeps a `Move` in `last_move` and a
    `Vec<Move>` in its history; following the singleton made `is_castling`
    look like a frozen setting, when it is a fact about one move that is
    written at construction and true forever after. Same for `apps/weather`'s
    forecast row and `apps/email`'s message.

    So the collection wins wherever both appear: one `Vec<T>` anywhere in the
    crate is proof that `T` describes one of many, whatever else holds it.
    """
    found = set()
    for m in re.finditer(r"\b(?:%s)\s*<([^<>]*)>" % "|".join(MANY_OF), code):
        for part in re.split(r"[,()\[\]]", m.group(1)):
            ty = part.split("::")[-1].strip()
            if re.fullmatch(r"[A-Z]\w*", ty):
                found.add(ty)
    return found


def owned_state(code: str) -> tuple[str, bool]:
    """The app's own struct **and the singleton structs it owns**, joined.

    Scoping to the app struct alone was too narrow in exactly the way that
    matters: `apps/lockscreen` keeps its settings in a `LockScreenConfig` held
    by one field, so `show_clock_seconds` -- a real frozen setting, filed as
    one -- vanished from the survey the moment the scope became accurate. A
    settings struct is app state that happens to have a name.

    What is deliberately *not* followed is anything held by the many: a field
    of a `Vec<FileEntry>` element is a property of a file, not a setting of the
    program. Nor does this catch a struct only ever built as an argument --
    `explorer`'s `OperationPlan` is constructed fresh per operation, so its
    uniformly-`Rename` `ConflictPolicy` is a defect in every *call site*
    rather than a frozen field, and wants a different tool than this one.
    """
    owner = re.search(APP_IMPL_OWNER_RE, code)
    if owner is None:
        return code, False
    root = struct_body(code, owner.group(1))
    if root is None:
        return code, False
    # A type the crate stores by the many is data, not state, wherever
    # else it also appears -- so it is pre-seeded into the visited set.
    seen = held_by_the_many(code) | {owner.group(1)}
    bodies = [root]
    queue = list(singleton_field_types(root))
    while queue:
        ty = queue.pop()
        if ty in seen:
            continue
        seen.add(ty)
        sub_body = struct_body(code, ty)
        if sub_body is None:
            continue
        bodies.append(sub_body)
        queue.extend(singleton_field_types(sub_body))
    return "\n".join(bodies), True




def survey(crate: Path) -> tuple[list[str], int, bool]:
    """`(frozen names, fields scanned, whether the app struct was found)`.

    The third value is not a detail. With it a row means "a setting of this
    app that nothing can change"; without it a row means "some boolean
    somewhere in this crate", which is mostly data that is immutable by
    design. Summing the two is how this survey came to report 197, then 87,
    when the answer it meant to give was neither.
    """
    code = crate_live_code(crate)
    if not code.strip():
        return [], 0, True
    body, scoped = owned_state(code)
    # Declared in the app's own struct; written (or not) anywhere in the crate.
    names = set(FIELD_RE.findall(body))
    for enum in judgeable_enums(code):
        # `Option<Enum>` as well as `Enum`. That shape is how an app spells
        # "no filter, or this one", and it is exactly what a filter strip with
        # a selection highlight is built on -- `apps/regextester`'s
        # `library_category_filter` is `Option<PatternCategory>`, `None` at
        # construction, read to highlight a chip and to filter the list, and
        # written nowhere. A survey that covers `Enum` and not `Option<Enum>`
        # misses the filters, which are the ones a user can see.
        wrapped = re.compile(
            r"\b(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*Option<\s*%s\s*>\s*,"
            % re.escape(enum)
        )
        names.update(wrapped.findall(body))
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
    return frozen, len(names), scoped


#: Rows already looked at and found not to be defects.
#:
#: See `frozen-flag-answered.txt` for the rules. The important one is that a
#: line naming a field the survey no longer reports is an *error*: an answer
#: about code that has since changed reads as a decision somebody made about
#: the code as it is now, and it was not.
ANSWERED = HERE / "frozen-flag-answered.txt"


def answered() -> dict[tuple[str, str], str]:
    """`{(crate, field): reason}` from the answers file."""
    out: dict[tuple[str, str], str] = {}
    try:
        text = io.open(ANSWERED, encoding="utf-8").read()
    except OSError:
        return out
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        out[(parts[0], parts[1])] = parts[2]
    return out


def main(argv: list[str]) -> int:
    show_all = "--all" in argv
    show_answered = "--answered" in argv
    known = answered()
    rows: list[tuple[int, str, list[str], bool]] = []
    answered_rows: list[tuple[str, str, str]] = []
    total_fields = 0
    for crate in sorted((ROOT / "apps").iterdir()):
        if not crate.is_dir():
            continue
        frozen, count, scoped = survey(crate)
        total_fields += count
        open_fields = []
        for field in frozen:
            reason = known.get((crate.name, field))
            if reason is None:
                open_fields.append(field)
            else:
                answered_rows.append((crate.name, field, reason))
        if open_fields:
            rows.append((len(open_fields), crate.name, open_fields, scoped))

    rows.sort(key=lambda r: (-r[0], r[1]))
    scoped_rows = [r for r in rows if r[3]]
    loose_rows = [r for r in rows if not r[3]]
    stuck = sum(r[0] for r in scoped_rows)
    print(
        f"{stuck} field(s) in {len(scoped_rows)} app(s) are read and never "
        f"written, "
        f"out of {total_fields} bool and plain-enum fields scanned"
    )
    print("(live code only; construction is not a write; see this file's docstring)\n")
    limit = len(scoped_rows) if show_all else 25
    for n, name, frozen, _ in scoped_rows[:limit]:
        shown = ", ".join(frozen[:6]) + ("..." if len(frozen) > 6 else "")
        print(f"{name:<20}{n:>3}  {shown}")
    if not show_all and len(scoped_rows) > limit:
        print(
            f"\n...and {len(scoped_rows) - limit} more apps; --all for the rest"
        )

    if loose_rows:
        # Counted separately and never added in, because these rows are a
        # different claim: no `impl <path::>App for X` was found, so the scan
        # covered every struct in the crate, and most of what that finds is
        # data which is immutable on purpose. Worth printing -- an app with
        # no App impl may be one that needs one -- but not worth summing.
        print(
            f"\nplus {sum(r[0] for r in loose_rows)} in "
            f"{len(loose_rows)} app(s) with no `App` impl to scope the scan; "
            f"whole-crate results, mostly data rather than settings:"
        )
        shown_loose = loose_rows if show_all else loose_rows[:8]
        for n, name, frozen, _ in shown_loose:
            shown = ", ".join(frozen[:6]) + ("..." if len(frozen) > 6 else "")
            print(f"  {name:<18}{n:>3}  {shown}")

    if answered_rows:
        print(f"\n{len(answered_rows)} already answered (--answered for why)")
        if show_answered:
            for crate_name, field, reason in sorted(answered_rows):
                print(f"  {crate_name}.{field}: {reason}")

    # An answer about a field the survey no longer reports is not harmless.
    # It reads as a decision somebody made about the code as it is now, and
    # the code has moved -- the field may have been fixed, renamed, or
    # deleted, and each of those wants the line gone or rewritten.
    stale = sorted(set(known) - {(c, f) for c, f, _ in answered_rows})
    if stale:
        print(
            f"\n{len(stale)} line(s) in {ANSWERED.name} name a field this "
            f"survey no longer reports:"
        )
        for crate_name, field in stale:
            print(f"  {crate_name}.{field}")
        print("Remove or rewrite them: an answer about code that has changed "
              "is not an answer.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
