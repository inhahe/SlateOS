#!/usr/bin/env python3
r"""Which `Option` fields does a program read and only ever empty?

WHY THIS EXISTS. `apps/kanban`'s `selected_card: Option<Id>` was `None` at
construction; the running program assigned it `None` in five places and
`Some` in none. Only the tests filled it. So every key that acts on the chosen
card -- Enter to open it, P for its priority, M and B to move it, Ctrl+D and
Ctrl+A -- was refused in the window for want of a chosen card, and passed in
the tests, which chose one themselves before pressing anything. Found on
2026-09-27 by reading, after a separate cleanup.

It is the sibling of `frozen-flag-survey.py`, and neither can see the other's
case. That survey asks whether a flag is ever *assigned*; here the field is
assigned all the time -- always to `None`. The shape is the absent half of a
feature: the clearing is written (Escape, a delete, a board switch), the
choosing is not, and nothing warns, because every line of it is used.

HOW IT DECIDES. For each app -- the struct behind `impl <path::>App for X`,
and the singleton structs it holds, exactly as `frozen-flag-survey.py` scopes
it (and by importing that survey, so the two cannot scope differently) -- a
field declared `name: Option<...>` is reported when, in live (non-test) code
across every file of the crate:

  * it is read -- `.name` appears somewhere; and
  * nothing can fill it: no `.name = <anything but None>`, no
    `.name.replace(` / `.insert(` / `.get_or_insert(`, no `&mut ...name`, no
    `name: Some(` or `name: <a call>` in a struct literal, and no wholesale
    replacement of the struct that holds it.

Each row says whether the crate's tests fill the field. "Tests fill it" is the
sharp form -- kanban's -- where the suite passes on a state the program never
reaches; a field nothing fills anywhere is plainer dead state.

KNOWN LIMITS. A field filled only through a function that takes the whole
struct by `&mut` and writes the field by a name this cannot follow -- a
`fn choose(state: &mut State)` in another module that does
`state.selected = Some(..)` *is* seen, because the assignment is written out;
one filled through a macro is not. A field whose name repeats across two
structs in a crate is one name, as in the survey: a fill of either
exonerates both. Report-only, like the rest of the set.

Usage:  python scripts/find-options-only-emptied.py [--self-test] [--roots=apps,gui]
"""

from __future__ import annotations

import importlib.util
import io
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))
import rustlex  # noqa: E402
import selftestflag  # noqa: E402


def _survey():
    """`frozen-flag-survey.py`, for its scoping, loaded once."""
    spec = importlib.util.spec_from_file_location("frozen_flag_survey", HERE / "frozen-flag-survey.py")
    if spec is None or spec.loader is None:
        raise SystemExit("find-options-only-emptied: cannot load frozen-flag-survey.py")
    module = importlib.util.module_from_spec(spec)
    # Registered before it runs: its dataclasses look themselves up there.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


SURVEY = _survey()

#: `selected: Option<...>,` in a struct body, generics nested one level deep.
OPTION_FIELD_RE = re.compile(
    r"\b(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*Option\s*<(?:[^<>]|<[^<>]*>)*>\s*,"
)


def fill_re(name: str) -> re.Pattern[str]:
    """Anything that can put a value in field `name`."""
    n = re.escape(name)
    return re.compile(
        # An assignment to anything but `None`. The lookahead takes the
        # spaces itself: `\s*(?!None)` lets `\s*` match nothing, and " None"
        # then passes the lookahead -- every clearing read as a fill.
        r"\.%s\s*=(?!=)(?!\s*None\b)" % n
        # A method that fills an Option in place.
        + r"|\.%s\s*\.\s*(?:replace|insert|get_or_insert|get_or_insert_with)\s*\(" % n
        # A mutable borrow, which can fill it through a callee.
        + r"|&\s*mut\s+[A-Za-z_][A-Za-z0-9_.]*\.%s\b" % n
        # A struct literal giving it anything but `None` -- `Some(..)`, a
        # call, another value's field -- and not the declaration, whose
        # "value" is the type `Option<..>`.
        + r"|\b%s\s*:(?!\s*None\b)(?!\s*Option\s*<)\s*[^\s,}]" % n
        # Field-init shorthand, `Meta { title, year }`: filled from a local.
        + r"|[{,]\s*%s\s*[,}]" % n
    )


def read_re(name: str) -> re.Pattern[str]:
    """A use of the field that is not an assignment to it.

    `self.name = None` mentions the field without reading it, and counting
    it as a read reported `apps/whiteboard`'s `Selection::marquee` -- a field
    nothing read at all, whose only mentions were two clearings. That is dead
    state, not an unreachable feature, and not this finder's.
    """
    return re.compile(r"\.%s\b(?!\s*=(?!=))" % re.escape(name))


def split(src: str) -> tuple[str, str]:
    """`(live, tests)` of one file, each with noise blanked.

    `rustlex.live_code` answers the live code twice (raw, and with noise
    blanked) -- not live and tests: the tests are what it blanked. It blanks
    in place, so a character that differs from the source is a character of
    test code. The first version of this took the second value for the tests,
    so every "tests fill it" was computed over live code and none was
    reported.
    """
    live, _masked = rustlex.live_code(src)
    tests = "".join(
        s if s != l else ("\n" if s == "\n" else " ") for s, l in zip(src, live)
    )
    return (
        rustlex.strip_noise(live, keep_literals=False),
        rustlex.strip_noise(tests, keep_literals=False),
    )


def crate_code(crate: Path) -> tuple[str, str]:
    """`(live, tests)`: every `.rs` file of the crate, noise blanked, split."""
    live_parts, test_parts = [], []
    for path in sorted((crate / "src").rglob("*.rs")):
        try:
            src = io.open(path, encoding="utf-8").read()
        except (OSError, UnicodeDecodeError):
            continue
        live, tests = split(src)
        live_parts.append(live)
        test_parts.append(tests)
    return "\n".join(live_parts), "\n".join(test_parts)


def examine(live: str, tests: str) -> list[tuple[str, bool]]:
    """`(field, tests fill it)` for every never-filled Option of the app."""
    if not live.strip():
        return []
    body, scoped, walked = SURVEY.owned_state(live)
    if not scoped:
        return []
    wholesale = SURVEY.replaced_wholesale(live, walked)
    found = []
    for name in sorted(set(OPTION_FIELD_RE.findall(body))):
        if name in wholesale:
            continue
        if not read_re(name).search(live):
            continue
        if fill_re(name).search(live):
            continue
        found.append((name, fill_re(name).search(tests) is not None))
    return found


#: Rows read and found not to be defects, with why -- so nobody reads them
#: twice. A row here that the scan no longer reports is printed as stale:
#: an answer about code that has changed reads as a decision about the code
#: as it is now, and it is not one.
KNOWN = {
    ("apps/defrag", "scan"): "no scanner, on purpose: the window says it cannot read a drive, "
    "and a test pins that the shipping app has none",
    ("apps/netscan", "traceroute_result"): "this program sends nothing; traceroute refuses "
    "and says so, and the result is the slot a working one fills",
    ("apps/sysmonitor", "battery_percent"): "nothing reports a battery yet; the row is drawn "
    "only when there is a reading",
    ("apps/vpnmanager", "last_connected_id"): "no tunnel can come up, so nothing is ever "
    "connected to reconnect to; Reconnect says so",
    ("apps/magnifier", "picked"): "nothing can capture the screen, so picking refuses and says "
    "so rather than name a colour it never read; the swatch is tested with one set by hand",
    ("apps/videoplayer", "album"): "mediaprobe reads no tags yet; the rows show only when present",
    ("apps/videoplayer", "artist"): "as album",
    ("apps/videoplayer", "encoder"): "as album",
    ("apps/videoplayer", "genre"): "as album",
    ("apps/videoplayer", "year"): "as album",
}


def crates(roots: list[str]) -> list[Path]:
    out = []
    for root in roots:
        base = ROOT / root
        if not base.is_dir():
            raise SystemExit(f"find-options-only-emptied: no {root}/ under {ROOT}")
        out.extend(sorted(p.parent for p in base.glob("*/Cargo.toml")))
    return out


# ---------------------------------------------------------------- self-test

_APP = """
struct Board {{ cards: Vec<u32> }}
struct State {{
    board: Board,
    chosen: Option<u32>,
    {extra}
}}
impl oswindow::app::App for State {{}}
impl State {{
    fn new() -> Self {{ Self {{ board: Board {{ cards: vec![] }}, chosen: None, {extra_init} }} }}
    fn escape(&mut self) {{ self.chosen = None; }}
    fn act(&self) -> bool {{ self.chosen.is_some() }}
    {extra_fn}
}}
"""


def _app(extra: str = "", extra_init: str = "", extra_fn: str = "") -> str:
    return _APP.format(extra=extra, extra_init=extra_init, extra_fn=extra_fn)


def self_test() -> int:
    cases = [
        ("read and only emptied is reported", _app(), "", [("chosen", False)]),
        (
            "...and says so when the tests fill it",
            _app(),
            "fn t() { let mut s = State::new(); s.chosen = Some(3); }",
            [("chosen", True)],
        ),
        ("an assignment of a value fills it", _app(extra_fn="fn pick(&mut self) { self.chosen = Some(1); }"), "", []),
        ("so does an assignment of an expression", _app(extra_fn="fn pick(&mut self, c: Option<u32>) { self.chosen = c; }"), "", []),
        ("so does replace", _app(extra_fn="fn pick(&mut self) { self.chosen.replace(1); }"), "", []),
        ("so does a mutable borrow", _app(extra_fn="fn pick(&mut self) { fill(&mut self.chosen); }"), "", []),
        (
            "a field that starts full is filled",
            _app(extra="other: Option<u32>,", extra_init="other: Some(4)", extra_fn="fn o(&self) -> bool { self.other.is_some() }"),
            "",
            [("chosen", False)],
        ),
        (
            "a field nobody reads is not this finder's",
            _app(extra="unread: Option<u32>,", extra_init="unread: None"),
            "",
            [("chosen", False)],
        ),
        ("a comparison is not an assignment", _app(extra_fn="fn same(&self) -> bool { self.chosen == None }"), "", [("chosen", False)]),
        (
            "a field only ever cleared is dead, not unreachable",
            _app(extra="cleared: Option<u32>,", extra_init="cleared: None", extra_fn="fn c(&mut self) { self.cleared = None; }"),
            "",
            [("chosen", False)],
        ),
        ("no App, no population", "struct X { chosen: Option<u32> } fn f(x: &mut X) { x.chosen = None; let _ = x.chosen; }", "", []),
    ]
    # Through the file split, as a crate is read: the test module's fill must
    # be seen as the tests', and must not exonerate the program.
    whole = _app() + (
        "\n#[cfg(test)]\nmod tests {\n"
        "    fn t() { let mut s = State::new(); s.chosen = Some(3); }\n}\n"
    )
    live, tests = split(whole)
    cases.append(("a test module's fill is the tests', not the program's", live, tests, [("chosen", True)]))
    failures = 0
    for label, live, tests, want in cases:
        got = examine(live, tests)
        ok = got == want
        failures += 0 if ok else 1
        print(("ok   " if ok else "FAIL ") + label + ("" if ok else f": got {got}, wanted {want}"))
    print(f"\nfind-options-only-emptied self-test: {failures} failure(s)")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    roots = ["apps", "gui"]
    for arg in argv:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"find-options-only-emptied: unrecognized option {arg!r}")
            print("usage: find-options-only-emptied.py [--self-test] [--roots=apps,gui]")
            return 2
    rows = []
    for crate in crates(roots):
        live, tests = crate_code(crate)
        for name, tests_fill in examine(live, tests):
            rows.append((crate.relative_to(ROOT).as_posix(), name, tests_fill))
    seen = {(crate, name) for crate, name, _ in rows}
    known = [r for r in rows if (r[0], r[1]) in KNOWN]
    open_rows = [r for r in rows if (r[0], r[1]) not in KNOWN]
    sharp = [r for r in open_rows if r[2]]
    print(
        f"{len(open_rows)} Option field(s) read and never filled by the program, "
        f"not yet answered; the tests fill {len(sharp)} of them\n"
    )
    for crate, name, tests_fill in open_rows:
        print(f"  {crate:<34} {name:<28} {'tests fill it' if tests_fill else ''}")
    if known:
        print(f"\n{len(known)} answered, not defects:")
        for crate, name, _ in known:
            print(f"  {crate:<34} {name:<28} {KNOWN[(crate, name)]}")
    # Only about the roots scanned: an answer for a crate outside them is not
    # stale, just not looked at this run.
    stale = [
        key for key in KNOWN
        if key not in seen and any(key[0].startswith(root + "/") for root in roots)
    ]
    for crate, name in stale:
        print(f"\nSTALE ANSWER: {crate} {name} is no longer reported -- remove it from KNOWN")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
