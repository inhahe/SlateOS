#!/usr/bin/env python3
"""Refuse an intra-doc link whose target does not exist anywhere in its crate.

WHY THIS EXISTS
---------------
On 2026-09-01 a sweep of `coreutils` found 35 broken `cargo doc` intra-doc
links. They fell into four kinds, and only one of them rots on its own:

    prose eaten as a link       `set[ug]id` -> `[ug]` is not an item
    the path does not name it   `get_header` is really `Df::get_header`
    THE ITEM DOES NOT EXIST     `destination_is_older` is now
                                `destination_is_up_to_date`
    test-only target            `#[cfg(test)]`, so no doc build resolves it

The third kind is what a *rename* leaves behind. Every one of the eight found
named a real function under a name that was no longer real, because renaming a
function does not rename the prose that points at it. That class will come back
the next time anything is renamed, so it is the class worth a gate.

The other three do not need one: rustdoc reports them the moment anyone runs
`cargo doc`, and they do not appear spontaneously.

WHY NOT JUST RUN `cargo doc`
----------------------------
Because no single `cargo doc` invocation sees the whole truth, which is the
other thing that sweep established:

    --target x86_64-pc-windows-gnu      misses everything inside `#[cfg(unix)]`
                                        AND reports five cfg(unix) items as
                                        missing that are perfectly fine
    default (no --document-private-items)
                                        does not resolve links on private
                                        items at all -- a private item's doc
                                        comment is never read

So the honest rustdoc check is three builds on two targets, one of which
(`x86_64-unknown-linux-gnu`) is not otherwise built here and whose `target/`
subtree is pure cost. This checker is a text scan: it needs no toolchain, no
target installed, and no build, and it is blind to `cfg` by construction --
which is exactly right for this class, since a renamed function is missing
under every `cfg`.

WHAT IT DELIBERATELY DOES NOT DO
--------------------------------
It does not resolve paths. `[\\`get_header\\`]` when the item is
`Df::get_header` is a real defect and this will not flag it, because deciding
that needs name resolution and a wrong answer here is worse than no answer: a
gate that cries wolf gets bypassed, and a bypassed gate is not a gate. The rule
is deliberately one-sided -- flag only when the final path segment appears
NOWHERE in the crate as any kind of definition. If a name exists at all, this
stays quiet and leaves the judgement to rustdoc.

Consequently every finding is actionable: the name is simply gone.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# Roots scanned. Lane B's trees; a crate outside these is another lane's to
# gate, and a gate scoped wider than its owner can fix is a gate that blocks
# people who cannot act on it.
ROOTS = ("userspace", "services", "init", "posix")

# --------------------------------------------------------------------------
# Definitions. Anything that can legitimately be the target of an intra-doc
# link and can be spotted without parsing Rust.
#
# Generous on purpose: a name matched here silences the checker, and silence is
# the safe direction. Missing a definition form would produce a false positive,
# which is the failure mode that gets a gate switched off.
# --------------------------------------------------------------------------
# Type definitions specifically, and only by keyword. This is the set a link's
# *head* is tested against, and it is deliberately much narrower than the one
# below.
#
# The distinction is the difference between a working gate and a noisy one.
# `Style::quote_with` is a real method of `quoting::Style`, which `coreutils`
# re-exports as `coreutils::quote::Style`; `Ch::U` is a real variant of
# `ere::ch::Ch`, which `oils` re-exports. Neither type is *defined* in the crate
# that links to it, so neither one's members can be found by reading that
# crate -- but both names still land in the general set below, because some
# unrelated enum has a variant spelled `Ch(Ch)` and some struct has a field
# spelled `style:`. Requiring the head to be keyword-defined here is what tells
# "a type we can read" from "a name that merely occurs".
# One pass, not sixteen. The `ty` group is the narrow type set; every other
# group feeds the general "this name exists" set. Scanning `oils/src/interp.rs`
# -- 100k lines -- once per pattern took the whole scan to 61 seconds, which is
# far too slow for something that runs on every push; merged, it is about four.
#
# The last two branches catch enum variants and struct fields: a bare `Name,` /
# `name: Type,` at the head of an indented line. Both are linkable and neither
# is declared by a keyword, so they have to be caught by shape -- and that
# looseness is exactly why they are kept out of `ty`.
DEF_RE = re.compile(
    r"\b(?:struct|enum|trait|union|type)\s+(?P<ty>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\b(?:fn|mod)\s+(?P<it>[A-Za-z_][A-Za-z0-9_]*)"
    # `const fn hex_len` must yield `hex_len`, not `fn`. A merged alternation
    # matches at the FIRST keyword and then resumes past what it consumed, so a
    # branch that stops at `const` swallows the `fn` and hides the real name --
    # which silently un-defined every `const fn` in the tree and invented ten
    # findings. Separate patterns did not have this problem because each rescanned
    # the whole text.
    r"|\bconst\s+(?:fn\s+)?(?P<cn>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\bstatic\s+(?:mut\s+)?(?P<st>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\bmacro_rules!\s*(?P<mc>[A-Za-z_][A-Za-z0-9_]*)"
    r"|^[ \t]+(?P<va>[A-Z][A-Za-z0-9_]*)\s*[,({]"
    r"|^[ \t]+(?:pub\s+)?(?P<fl>[a-z_][A-Za-z0-9_]*)\s*:",
    re.M,
)

# Everything a `use` brings into scope. Rustdoc resolves against the importing
# module's scope, so `use quoting::escape_os;` makes `[`escape_os`]` a perfectly
# good link in a crate that does not define it -- `tar` does exactly this, and
# treating it as dead was this checker's first false positive.
#
# Every identifier in the statement is taken, module names included. That is
# over-generous by design: an extra name can only silence the checker, and
# silence is the safe direction (see the module docstring).
USE_STMT = re.compile(r"^\s*(?:pub\s+)?use\s+([^;]+);", re.M)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

# A declarative macro *defining an item named by its first argument* -- the
# idiom that generates `oils`'s `nested_parts` / `nested_parts_mut` from one
# `macro_rules! nested_parts_fn`. Both functions are real, both are linked to,
# and no scan of definition keywords can see either: their names exist only as
# arguments here.
#
# Only invocations of a macro the crate itself declares are read this way, which
# is what keeps the rule from swallowing `assert_eq!(status, 0)` and adding the
# local `status` to the crate's namespace. The cost of that restriction is an
# item generated by an imported macro (`paste!` and friends), which nothing here
# does; if one ever appears it shows up as a finding rather than as silence.
MACRO_CALL = re.compile(
    r"^\s*(?P<mname>[a-z_][A-Za-z0-9_]*)!\s*[(\[{]\s*"
    r"(?P<marg>[A-Za-z_][A-Za-z0-9_]*)\s*[,)\]}]",
    re.M,
)

# What a link target may look like. A path, and nothing else. Without this,
# `` `[` and `]` `` in prose -- "everything between `[` and `]`" -- parses as a
# link whose target is " and ", which strips to the identifier `and`.
PATH_SHAPE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*")

# Roots that mean "not in this crate". `crate`, `self` and `Self` are stripped
# before this is consulted; anything else lowercase with a `::` after it is
# another crate, and every uppercase name is checked normally.
EXTERNAL_ROOTS = {
    "std", "core", "alloc", "libc", "proc_macro", "test",
    "coreutils", "posix", "slateos",
}

# Names that resolve without being defined in the crate: the prelude, the
# primitives, and the handful of std items that get linked bare.
WELL_KNOWN = {
    # primitives
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize",
    "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "never",
    # prelude and near-prelude
    "Option", "Some", "None", "Result", "Ok", "Err", "Vec", "String", "Box",
    "Iterator", "IntoIterator", "Clone", "Copy", "Drop", "Default", "Debug",
    "Display", "PartialEq", "Eq", "PartialOrd", "Ord", "Hash", "From", "Into",
    "TryFrom", "TryInto", "AsRef", "AsMut", "Deref", "DerefMut", "Fn", "FnMut",
    "FnOnce", "Send", "Sync", "Sized", "ToString", "ToOwned", "Cow",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque",
    "Path", "PathBuf", "OsStr", "OsString", "CStr", "CString",
    "File", "Command", "Child", "ExitCode", "ExitStatus",
    "Ordering", "Duration", "Instant", "Range", "Rc", "Arc", "Mutex", "RwLock",
    "Cell", "RefCell", "Weak", "Pin", "Wrapping", "NonZero",
    "Write", "Read", "BufRead", "BufReader", "BufWriter", "Seek", "SeekFrom",
    "Error", "ErrorKind", "Metadata", "DirEntry", "ReadDir", "Permissions",
}

# `[`x`]` and `[text](x)`. Rustdoc also accepts `[x]` bare, but that form is
# indistinguishable from ordinary prose in square brackets -- which is the very
# thing the sweep found six of -- so it is left to rustdoc.
#
# The lookahead is load-bearing. In `[`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE)`
# the bracketed part is the link's *display text*, not its target -- the target
# is in the parentheses, and LINK_EXPLICIT already picks it up. Without the
# lookahead every such link is read twice, once correctly and once as a bare
# link to its own label, which is how thirteen working netstack links looked
# dead. `[label][ref]` is the same story with a reference instead.
LINK_BACKTICK = re.compile(r"\[`([^`\]]+)`\](?![(\[])")
LINK_EXPLICIT = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")

DOC_LINE = re.compile(r"^\s*(?:///|//!)(.*)$")

# A markdown *link reference definition*: `[label]: destination`. The label is
# then a working link everywhere in that doc comment, and rustdoc never resolves
# it as an intra-doc path -- `modechange`'s [`ere`] points at a URL this way.
#
# Two findings come from each one if this is not honoured: the use, and the
# definition line itself (which is not a link at all, but looks like one).
#
# Deliberately loose about what follows the colon. CommonMark would demand a
# real destination, and several doc comments here use `[`name`]: prose...` as a
# definition list, which is *not* a valid definition and so does not silence
# rustdoc. But this checker only ever flags a name that exists NOWHERE, and a
# name written in that definition-list style is one the author is describing --
# so treating it as resolved errs in the safe direction, same as everything else
# here.
REF_DEF = re.compile(r"^\s*\[`?([^`\]]+?)`?\]:\s*\S")


def crate_roots(repo: Path) -> list[Path]:
    """Every crate directory (one holding a Cargo.toml) under the scanned roots."""
    out = []
    for root in ROOTS:
        base = repo / root
        if not base.is_dir():
            continue
        for manifest in base.rglob("Cargo.toml"):
            if "target" in manifest.parts:
                continue
            out.append(manifest.parent)
    return sorted(out)


def rust_files(crate: Path) -> list[Path]:
    src = crate / "src"
    if not src.is_dir():
        return []
    return sorted(p for p in src.rglob("*.rs") if "target" not in p.parts)


def units(crate: Path) -> list[tuple[str, list[Path], list[Path]]]:
    """The crate's compilation units: the library, and each `src/bin` target.

    A package is not a namespace. `coreutils` has about a hundred binaries under
    `src/bin`, and each is its own crate that cannot see the others' items.
    Scanning the package as one blob makes every binary's private types visible
    to every other, which is how `ls.rs` came to be judged against the unrelated
    `enum Style` that `nl.rs` declares -- while `ls`'s actual `Style` is
    `quoting::Style`, whose `quote_with` exists and was reported dead.

    Returns `(name, own, shared)`: the files the unit is responsible for, and
    the library's files, which every unit gets as *definitions* because a binary
    really can reach the library's public items and routinely links to them.
    """
    src = crate / "src"
    if not src.is_dir():
        return []
    bindir = src / "bin"
    lib = sorted(
        p for p in src.rglob("*.rs")
        if "target" not in p.parts and bindir not in p.parents and p.parent != bindir
    )
    out: list[tuple[str, list[Path], list[Path]]] = []
    if lib:
        out.append((crate.name, lib, []))
    if bindir.is_dir():
        for entry in sorted(bindir.iterdir()):
            if entry.is_file() and entry.suffix == ".rs":
                out.append((entry.stem, [entry], lib))
            elif entry.is_dir():
                own = sorted(p for p in entry.rglob("*.rs") if "target" not in p.parts)
                if own:
                    out.append((entry.name, own, lib))
    return out


class Defs:
    """What reading a set of Rust files says about the names in them.

    `types` is what is defined by `struct`/`enum`/`trait`/`union`/`type`, and
    answers "can I read this type's members?" -- it is used only for a link's
    head. `scope` is every name defined or imported, and answers "does this name
    exist at all?" -- used only for a link's final segment. Keeping them apart is
    what makes the gate quiet; see DEF_RE for why the head needs the stricter
    test.

    `macros` and `macro_args` are held unresolved because resolving them needs
    the *whole* unit: a binary may invoke a `#[macro_export]`ed macro its library
    declares, so the declaration and the invocation can be in different files.
    """

    def __init__(self) -> None:
        self.types: set[str] = set()
        self.scope: set[str] = set()
        self.macros: set[str] = set()
        self.macro_args: list[tuple[str, str]] = []

    def union(self, other: Defs) -> Defs:
        out = Defs()
        out.types = self.types | other.types
        out.scope = self.scope | other.scope
        out.macros = self.macros | other.macros
        out.macro_args = self.macro_args + other.macro_args
        return out

    def resolve(self) -> Defs:
        """Fold in the item names the crate's own declarative macros generate."""
        for mname, arg in self.macro_args:
            if mname in self.macros:
                self.scope.add(arg)
        return self


def defs_in_text(text: str, d: Defs) -> None:
    """Add one file's definitions and imports to `d`."""
    for m in DEF_RE.finditer(text):
        ty = m.group("ty")
        if ty is not None:
            d.types.add(ty)
            d.scope.add(ty)
            continue
        mc = m.group("mc")
        if mc is not None:
            d.macros.add(mc)
            d.scope.add(mc)
            continue
        for g in ("it", "cn", "st", "va", "fl"):
            v = m.group(g)
            if v is not None:
                d.scope.add(v)
                break
    for m in USE_STMT.finditer(text):
        d.scope.update(IDENT.findall(m.group(1)))
    for m in MACRO_CALL.finditer(text):
        d.macro_args.append((m.group("mname"), m.group("marg")))


def definitions(files: list[Path]) -> Defs:
    """Everything `files` say about the names in them, left unresolved."""
    d = Defs()
    for f in files:
        try:
            text = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        defs_in_text(text, d)
    return d


def dependencies(crate: Path) -> set[str]:
    """Names in the crate's `[dependencies]`, which link like crate roots.

    `modechange`'s docs point at [`ere`], the regex crate it depends on. That is
    a working link and reading only `src/` cannot tell.
    """
    manifest = crate / "Cargo.toml"
    try:
        text = manifest.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return set()
    names: set[str] = set()
    in_deps = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("["):
            in_deps = "dependencies" in line
            continue
        if in_deps and "=" in line and not line.startswith("#"):
            name = line.split("=", 1)[0].strip().strip('"')
            if IDENT.fullmatch(name.replace("-", "_")):
                names.add(name.replace("-", "_"))
                names.add(name)
    return names


def link_targets(line: str):
    """The path each intra-doc link on this line points at."""
    for m in LINK_BACKTICK.finditer(line):
        yield m.group(1)
    for m in LINK_EXPLICIT.finditer(line):
        yield m.group(1)


def split_path(path: str) -> list[str] | None:
    """The path's segments, or None if this is not a path we can judge."""
    # Method-call and macro spellings, generics, and leading sigils. No strip:
    # whitespace anywhere means this was never a path (see PATH_SHAPE).
    p = path.split("<", 1)[0]
    p = p.removesuffix("()").removesuffix("!")
    p = p.lstrip("&")
    if not p or p.startswith("http") or "/" in p:
        return None
    if not PATH_SHAPE.fullmatch(p):
        return None
    segs = [x for x in p.split("::") if x]
    # Strip the roots that mean "in this crate" -- they say where to start
    # looking, not what to look for.
    while segs and segs[0] in ("crate", "self", "Self", "super"):
        segs.pop(0)
    return segs or None


def dead_link(path: str, types: set[str], scope: set[str]) -> bool:
    """Whether `path` names something that exists nowhere in the crate.

    `types` is what the crate defines with `struct`/`enum`/`trait`/`union`/
    `type`; `scope` is every name it defines or imports, plus its declared
    dependencies. A multi-segment path is judged only when its head is in
    `types`, because only then is the member list something reading this crate
    can see. `Stdio::null` and `Style::quote_with` both have heads defined in
    another crate, so their tails are unknowable here -- and guessing would
    produce exactly the false positive that gets a gate switched off.

    A lowercase head is never in `types` either, so `authlib::Authenticator` and
    `deflate::inflate_limited` -- module paths into dependencies -- fall out by
    the same rule rather than needing one of their own.
    """
    segs = split_path(path)
    if segs is None:
        return False
    head, last = segs[0], segs[-1]
    if head in EXTERNAL_ROOTS or head in WELL_KNOWN:
        return False
    if len(segs) > 1 and head not in types:
        return False
    return last not in scope and last not in WELL_KNOWN


def scan_file(f: Path, repo: Path, unit: str, types: set[str], scope: set[str]):
    """Every dead link in one file, judged in the unit that compiles it."""
    try:
        lines = f.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return
    # Labels this file defines by hand. Collected up front because a definition
    # may sit below its use (markdown does not care, and the convention in this
    # tree is to park them at the end of the block).
    labels: set[str] = set()
    for raw in lines:
        m = DOC_LINE.match(raw)
        if m:
            d = REF_DEF.match(m.group(1))
            if d:
                labels.add(d.group(1))
    for n, raw in enumerate(lines, 1):
        m = DOC_LINE.match(raw)
        if not m:
            continue
        for target in link_targets(m.group(1)):
            if target in labels:
                continue
            if dead_link(target, types, scope):
                # `as_posix`, not the native separator: the finding is meant to
                # be pasted into `git log -S ... -- <path>` or an editor, and on
                # this project's Windows hosts a `Path` renders with backslashes
                # that the shell then eats as escapes. Git reports paths with
                # forward slashes everywhere, so this matches its neighbours.
                yield (f.relative_to(repo).as_posix(), n, target, unit)


def crates_touching(repo: Path, paths: list[str]) -> list[Path]:
    """The crates that own `paths` -- the innermost Cargo.toml above each.

    Restricting a run to these is sound, not merely a shortcut. An intra-doc
    link is resolved inside one crate, and this checker only ever judges names
    it can see in that crate's own text; a rename in crate X therefore cannot
    turn a link in crate Y dead. (If Y `use`s the renamed item, Y stops
    compiling, which is a louder gate than this one.)
    """
    all_crates = crate_roots(repo)
    out: list[Path] = []
    for raw in paths:
        p = (repo / raw).resolve()
        best = None
        for c in all_crates:
            try:
                p.relative_to(c)
            except ValueError:
                continue
            if best is None or len(c.parts) > len(best.parts):
                best = c
        if best is not None and best not in out:
            out.append(best)
    return out


def scan(repo: Path, only: list[Path] | None = None) -> list[tuple[str, int, str, str]]:
    findings = []
    for crate in (only if only is not None else crate_roots(repo)):
        deps = dependencies(crate)
        # The library's definitions are the same for all ~100 of `coreutils`'
        # binaries, and re-deriving them per binary makes the scan quadratic in
        # a package whose library is the big part. Compute once, union per unit.
        all_units = units(crate)
        lib_files = all_units[0][2] if all_units else []
        for _, _, shared in all_units:
            if shared:
                lib_files = shared
                break
        lib_defs = definitions(lib_files)
        for unit, own, shared in all_units:
            d = definitions(own)
            if shared:
                d = d.union(lib_defs)
            d.resolve()
            types = d.types
            scope = d.scope | deps
            # Only `own` is scanned for links. `shared` (the library) is here to
            # define names, not to be re-judged once per binary -- and a library
            # file's own docs are resolved in the library's context, which is the
            # unit where it appears as `own`.
            for f in own:
                findings.extend(scan_file(f, repo, unit, types, scope))
    return sorted(findings, key=lambda x: (str(x[0]), x[1], x[2]))


SELFTESTS = (
    # (link path, types defined by keyword here, other names in scope, should_flag)
    # The class this gate exists for: a rename left the prose behind.
    ("destination_is_older", {"destination_is_up_to_date"}, set(), True),
    ("destination_is_older", {"destination_is_older"}, set(), False),
    ("Self::add_to_archive", {"add"}, set(), True),
    ("Self::add", {"add"}, set(), False),
    ("Df::replace_problematic_chars", {"Df", "scrub"}, set(), True),
    ("Df::scrub", {"Df", "scrub"}, set(), False),
    ("crate::diag!", {"diag"}, set(), False),
    ("crate::diag!", {"other"}, set(), True),
    ("crate::guard_std_fds!", {"guard_std_fds"}, set(), False),
    # In scope without being defined here: a `use` is as good as a definition.
    ("escape_os", set(), {"escape_os"}, False),
    ("ere", set(), {"ere"}, False),
    # Not ours to judge: an external root, or a head we do not own.
    ("std::fs::set_permissions", set(), set(), False),
    ("coreutils::human::humblock", set(), set(), False),
    ("io::ErrorKind::InvalidInput", set(), set(), False),
    ("Stdio::null", set(), {"Stdio"}, False),
    ("PathBuf::join", set(), set(), False),
    ("Vec", set(), set(), False),
    ("Option", set(), set(), False),
    ("https://example.com", set(), set(), False),
    # ...but a head we DO own is judged, which is how a renamed method is caught.
    ("Shell::read_all_bytes", {"Shell"}, set(), True),
    # A lowercase head is a module path into a dependency, never a type we can
    # read: `authlib::Authenticator` lives in authlib's source, not ours.
    ("authlib::Authenticator", set(), {"authlib"}, False),
    ("deflate::inflate_limited", set(), {"deflate"}, False),
    # A head that is only re-exported is unjudgeable even though its name occurs
    # in our text: `Ch` is `pub use ere::ch::Ch` and `U` is a variant of *ere's*
    # enum, so no amount of reading oils can find it. Same for `Style`, which is
    # `quoting::Style` re-exported as `coreutils::quote::Style` -- and
    # `quote_with` is a real method of it.
    ("Ch::U", set(), {"Ch"}, False),
    ("Style::quote_with", set(), {"Style"}, False),
    # A type name that occurs only as some *other* enum's variant is likewise
    # not a type we defined, so it cannot be a judgeable head.
    ("Ch::B", set(), {"Ch", "B"}, False),
    # Generics and call spellings must not defeat the match.
    ("Creator<'a>", {"Creator"}, set(), False),
    ("run_text()", {"run_text"}, set(), False),
    # Prose that merely looks bracketed. "everything between `[` and `]`"
    # parses as a link to " and " unless a target must be shaped like a path.
    (" and ", set(), set(), False),
    ("Self::add ", {"add"}, set(), False),
)


def selftest() -> int:
    bad = 0
    checks = 0

    def check(ok: bool, msg: str) -> None:
        nonlocal bad, checks
        checks += 1
        if not ok:
            print(f"selftest FAIL: {msg}", file=sys.stderr)
            bad += 1

    for path, types, extra, want in SELFTESTS:
        got = dead_link(path, types, types | extra)
        check(
            got == want,
            f"{path!r} types={sorted(types)} scope+={sorted(extra)} "
            f"-> flagged={got}, expected {want}",
        )
    # The link extractor must find both spellings and neither more nor less.
    got = sorted(link_targets("/// see [`a::b`] and [text](c::d) and `not_a_link`"))
    check(got == ["a::b", "c::d"], f"extractor got {got}")
    # An explicit link's label is display text. Yield the target once, not the
    # label as well -- `[`X`](m::X)` is one link, and it is to `m::X`.
    got = sorted(link_targets("/// [`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE)"))
    check(
        got == ["netipc::ring::ERR_BROKEN_PIPE"],
        f"explicit-link label taken as a target: {got}",
    )
    # ...and the same for the reference form `[label][ref]`.
    check(
        not list(link_targets("/// [`OP_SEND`][op] here")),
        "reference-link label taken as a target",
    )
    # Prose in brackets is rustdoc's to complain about, not ours.
    check(
        not list(link_targets("/// set[ug]id bits")),
        "bare-bracket prose was treated as a link",
    )

    # The definition scan. Tested directly because `dead_link` cannot see a
    # mistake here: a name the scan misses simply becomes a finding, and the
    # `const fn` bug below invented ten of them while every case above passed.
    src = """
        pub struct Outcome {
            pub reason: u8,
        }
        pub enum Nested {
            Sub,
            Other(u8),
        }
        pub type Alias = u8;
        impl Outcome {
            pub const fn user_message(self) -> &'static str { "" }
            pub const LIMIT: usize = 8;
            const fn reserved_ok(&self) -> bool { true }
            pub async unsafe fn slurp(&self) {}
        }
        static mut COUNTER: u64 = 0;
        macro_rules! diag { () => {} }
        mod inner {}
        use quoting::escape_os;
    """
    d = Defs()
    defs_in_text(src, d)
    d.resolve()
    want_types = {"Outcome", "Nested", "Alias"}
    check(
        d.types == want_types,
        f"types={sorted(d.types)}, expected {sorted(want_types)}",
    )
    for name in (
        "user_message", "reserved_ok", "slurp", "LIMIT", "COUNTER",
        "diag", "inner", "escape_os", "Sub", "reason", "Outcome",
    ):
        check(name in d.scope, f"{name!r} missing from scope")
    # ...and the keyword itself is never a definition.
    for name in ("fn", "mut", "pub", "const"):
        check(
            name not in d.scope and name not in d.types,
            f"keyword {name!r} captured as a name",
        )

    # An item whose name exists only as an argument to the crate's own
    # `macro_rules!`. `oils` generates `nested_parts`/`nested_parts_mut` this
    # way and links to both; no keyword scan can see either name.
    gen = """
        macro_rules! nested_parts_fn {
            ($name:ident, $slice:ident,) => { pub(crate) fn $name() {} };
        }
        nested_parts_fn!(nested_parts, as_slice,);
        nested_parts_fn!(
            nested_parts_mut,
            as_mut_slice,
        );
        fn caller() {
            assert_eq!(status, 0);
        }
    """
    d = Defs()
    defs_in_text(gen, d)
    d.resolve()
    for name in ("nested_parts", "nested_parts_mut"):
        check(name in d.scope, f"macro-generated {name!r} missing from scope")
    # ...but an invocation of a macro this crate did NOT declare contributes
    # nothing, or every `assert_eq!(x, ...)` would make its local `x` an item.
    check("status" not in d.scope, "an std macro's argument was taken as a name")

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="fail if any link is dead")
    ap.add_argument("--selftest", action="store_true", help="verify the checker itself")
    ap.add_argument("--list", action="store_true", help="print findings and exit 0")
    ap.add_argument(
        "paths",
        nargs="*",
        help="limit the scan to the crates owning these files (a whole-tree run "
        "reads 61 MB of Rust and takes about half a minute; a push usually "
        "touches one crate)",
    )
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    repo = Path(__file__).resolve().parent.parent
    only = crates_touching(repo, args.paths) if args.paths else None
    if only is not None and not only:
        print("ok -- no scanned crate was touched.")
        return 0
    findings = scan(repo, only)

    for f, n, target, crate in findings:
        print(f"{f}:{n}: [`{target}`] names nothing in crate `{crate}`")

    if args.list:
        print(f"\n{len(findings)} dead intra-doc link(s).")
        return 0
    if args.check:
        if findings:
            print(f"\n{len(findings)} dead intra-doc link(s).", file=sys.stderr)
            return 1
        print("ok -- no dead intra-doc links.")
        return 0
    ap.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
