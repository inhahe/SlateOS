#!/usr/bin/env python3
r"""Which settings does the program only ever read in order to print back?

A setting whose one reader is the startup banner is worse than a dead setting,
because the banner is evidence to the operator that the setting took effect.
Lane B's `logind` names the case exactly:

    `IdleActionSec` is in this list even though a field-level scan calls it
    READ, and that difference is the point. Its only reader is the startup
    banner, which prints `idle_timeout=600s` back at the operator -- so the one
    thing the setting does is CONFIRM ITSELF. A scanner asking "is this field
    ever read?" cannot see that, because printing is a read; the question that
    finds it is "does anything ACT on it?".

This is the same shape as every fabrication this tree has been clearing out
for a week -- **the observation that would falsify the claim is the same
observation that confirms it** -- except that here the observation is the
program's own output, which makes it the most persuasive form of it. An
operator who sets `sleep_interval=30`, restarts, and reads `sleep_interval=30`
in the banner has done the only check available to them, and it passed.

## Why "read only inside a print" is not on its own the rule

Because `--show-config` exists, and a config dump reads every field into a
print. That is the honest, useful shape of exactly this pattern, and a checker
that flags it flags the whole idea.

**The discriminator is per-STRUCT, not per-field.** A dump reads *all* of its
struct's fields into prints. An echoed setting is read only by the banner
*while its siblings are read by code that acts*. So:

    report a field that is read, and read only inside a print macro,
    IN A STRUCT WHERE AT LEAST ONE SIBLING IS READ SOMEWHERE ELSE.

That exonerates a `--show-config` dump wholesale without exonerating a
straggler hiding inside one. It is the reason this is worth running at all
rather than being the obvious idea everyone rejects.

## Why this reports and does not gate

The rule above is a good discriminator, not a proof. Three shapes defeat it
and all three are legitimate:

  * a field read by a print AND by code in another crate, which a
    single-crate scan cannot see;
  * a field whose acting reader goes through a macro or a trait method this
    does not resolve;
  * a struct that genuinely is a dump plus one field that is genuinely dead,
    where the right verdict is "dead", not "echoed".

A gate that is right most of the time and wrong occasionally teaches its
readers to pass it rather than to read it. So this prints and ranks, and the
judgement stays with a person.

## What counts as acting

Anything that is not an argument to a print-like macro. That is crude and it
is meant to be: `let t = cfg.idle_timeout;` counts as acting even if `t` is
then only printed, because following it would need dataflow this does not
have. The crudeness is in the direction of silence -- it under-reports rather
than over-accuses, which is the right direction for something a person reads.

Usage:  python scripts/find-echoed-settings.py [--roots=userspace,apps]
        python scripts/find-echoed-settings.py --all-structs
        python scripts/find-echoed-settings.py --self-test
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402
import selftestflag  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent

DEFAULT_ROOTS = ("apps", "gui", "userspace", "services", "net", "pkg")

# Structs holding operator intent. Narrowed on purpose: this is where a
# setting someone WROTE DOWN lives, and an ignored one there is a different
# thing from an unused field on an internal helper. `--all-structs` widens it,
# so the scope is inspectable rather than implied.
CONFIG_STRUCT = re.compile(r"(?:Config|Settings|Options|Opts|Prefs|Params)$")

STRUCT_DECL = re.compile(r"\bstruct\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{")
FIELD_DECL = re.compile(
    r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*[^,]+,",
    re.M,
)

# `format_args!` is included because `write!` expands through it and some
# callers use it directly; `assert!`/`panic!` are NOT -- a field read in an
# assertion is being checked, which is acting on it.
PRINT_MACROS = (
    "println", "print", "eprintln", "eprint",
    "writeln", "write", "format", "format_args",
)
PRINT_CALL = re.compile(
    r"(?<![A-Za-z0-9_])(?:" + "|".join(PRINT_MACROS) + r")\s*!\s*[(\[{]"
)

OPEN_TO_CLOSE = {"(": ")", "[": "]", "{": "}"}


def print_spans(code: str) -> list[tuple[int, int]]:
    """Half-open [start, end) spans covering each print macro's arguments.

    Nested calls are not merged: a span inside a span is still inside a print,
    which is the only question asked of it.
    """
    spans = []
    for m in PRINT_CALL.finditer(code):
        opener = code[m.end() - 1]
        closer = OPEN_TO_CLOSE[opener]
        depth = 0
        i = m.end() - 1
        while i < len(code):
            c = code[i]
            if c in OPEN_TO_CLOSE:
                depth += 1
            elif c in (")", "]", "}"):
                depth -= 1
                if depth == 0:
                    spans.append((m.end(), i))
                    break
            i += 1
    return spans


def in_any_span(pos: int, spans: list[tuple[int, int]]) -> bool:
    return any(a <= pos < b for a, b in spans)


def struct_fields(code: str) -> dict[str, list[str]]:
    """Field names per struct, from a brace-matched body."""
    out: dict[str, list[str]] = {}
    for m in STRUCT_DECL.finditer(code):
        depth = 0
        i = m.end() - 1
        end = None
        while i < len(code):
            if code[i] == "{":
                depth += 1
            elif code[i] == "}":
                depth -= 1
                if depth == 0:
                    end = i
                    break
            i += 1
        if end is None:
            continue
        body = code[m.end():end]
        # Only the struct's own fields: a nested struct or impl inside the
        # braces would otherwise contribute its fields to this one.
        flat = re.sub(r"\{[^{}]*\}", "", body)
        out.setdefault(m.group(1), []).extend(FIELD_DECL.findall(flat))
    return out


def field_reads(code: str, field: str) -> list[int]:
    """Offsets of `.field` reads. A `.field =` assignment is not a read."""
    rx = re.compile(rf"\.\s*{re.escape(field)}(?![A-Za-z0-9_])")
    hits = []
    for m in rx.finditer(code):
        tail = code[m.end():m.end() + 4].lstrip()
        # `==` and `!=` are reads; a lone `=` is a write.
        if tail.startswith("=") and not tail.startswith("=="):
            continue
        hits.append(m.start())
    return hits


FN_DECL = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*[(<]")


def enclosing_fn(code: str, pos: int) -> str | None:
    """Name of the function containing `pos`, by nearest preceding `fn`."""
    last = None
    for m in FN_DECL.finditer(code, 0, pos):
        last = m
    return last.group(1) if last else None


def is_called(code: str, fn: str) -> bool:
    """Is `fn` called anywhere in this (non-test) code, beyond its own `fn`?

    **Why this matters more than it looks.** A field read only inside a print
    has two very different explanations, and they want opposite fixes:

      * the print RUNS, and tells the operator their setting took effect --
        the program must stop claiming;
      * the print is in a formatter nothing calls -- the function must be
        wired up or deleted, which is `find-stranded-serialisers`' finding.

    `apps/mediaconvert`'s `ImageSettings::summary` is the second: `quality`,
    `strip_metadata` and `preserve_aspect` are read only by it, and its only
    callers are three assertions. Reporting that here would have been this
    checker taking credit for another one's hit, and two tools reporting one
    defect reads as two defects.
    """
    # The lookbehind deliberately does NOT exclude a preceding `.`: in Rust a
    # method's call site *is* `x.summary()`, and excluding it made every method
    # in the tree look uncalled -- which would have filed every echo as
    # stranded and produced a checker that reports nothing it was built for.
    for m in re.finditer(rf"(?<![A-Za-z0-9_]){re.escape(fn)}\s*\(", code):
        # Its own declaration is not a call.
        before = code[max(0, m.start() - 4):m.start()]
        if before.rstrip().endswith("fn"):
            continue
        return True
    return False


def analyse(
    code: str, *, all_structs: bool, stranded: list | None = None
) -> list[tuple[str, str]]:
    """(struct, field) pairs read only inside prints, with an acting sibling.

    A pair whose prints all sit in functions nothing calls is appended to
    `stranded` instead -- a different defect with a different fix, and one
    `find-stranded-serialisers.py` already reports.
    """
    spans = print_spans(code)
    found = []
    for struct, fields in struct_fields(code).items():
        if not all_structs and not CONFIG_STRUCT.search(struct):
            continue
        if len(fields) < 2:
            # "All siblings" needs siblings. A one-field struct cannot
            # distinguish a dump from a straggler, and guessing would make the
            # report's least reliable rows its most numerous.
            continue
        echoed, acting = [], False
        for f in fields:
            reads = field_reads(code, f)
            if not reads:
                continue  # dead, not echoed -- a different checker's finding
            if all(in_any_span(p, spans) for p in reads):
                fns = {enclosing_fn(code, p) for p in reads}
                if any(fn and is_called(code, fn) for fn in fns):
                    echoed.append(f)
                elif stranded is not None:
                    stranded.append((struct, f))
            else:
                acting = True
        if acting:
            found.extend((struct, f) for f in echoed)
    return found


def _self_test() -> int:
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

    # The `logind`/`IdleActionSec` shape: one field echoed, siblings acting.
    echoed_src = """
struct DaemonConfig {
    pub idle_timeout: u64,
    pub handle_power_key: bool,
    pub kill_user_processes: bool,
}
fn main() { run(&load()); }
fn run(cfg: &DaemonConfig) {
    println!("logind: idle_timeout={}s", cfg.idle_timeout);
    if cfg.handle_power_key { register_power_handler(); }
    if cfg.kill_user_processes { reap(); }
}
"""
    expect("a setting read only by the banner is reported",
           analyse(echoed_src, all_structs=False),
           [("DaemonConfig", "idle_timeout")])

    # The `--show-config` dump: every field printed, none acting. Must be
    # silent -- this is the shape that makes the naive rule unusable.
    dump_src = """
struct DaemonConfig {
    pub idle_timeout: u64,
    pub handle_power_key: bool,
    pub kill_user_processes: bool,
}
fn main() { show(&load()); }
fn show(cfg: &DaemonConfig) {
    println!("idle_timeout={}", cfg.idle_timeout);
    println!("handle_power_key={}", cfg.handle_power_key);
    println!("kill_user_processes={}", cfg.kill_user_processes);
}
"""
    expect("a whole-struct config dump is not reported",
           analyse(dump_src, all_structs=False), [])

    # A straggler INSIDE a dump: the case the per-struct rule exists to keep.
    straggler = dump_src.replace(
        "fn main() { show(&load()); }",
        "fn main() { show(&load()); run(&load()); }\n"
        "fn run(cfg: &DaemonConfig) { if cfg.handle_power_key { reap(); } }",
    )
    expect("...but a straggler inside one still is",
           sorted(analyse(straggler, all_structs=False)),
           [("DaemonConfig", "idle_timeout"),
            ("DaemonConfig", "kill_user_processes")])

    # A field nothing reads at all is a different finding and not this one.
    dead = """
struct AppConfig {
    pub sleep_interval: u64,
    pub daemon: bool,
}
fn main() { run(&load()); }
fn run(cfg: &AppConfig) { if cfg.daemon { spawn(); } }
"""
    expect("a field read by nothing is not an echo", analyse(dead, all_structs=False), [])

    # `write!` to a buffer counts: it is still output, and `logind`'s banner
    # could as easily have been built that way.
    writes = """
struct AppConfig {
    pub retries: u32,
    pub daemon: bool,
}
fn main() { run(&load(), &mut String::new()); }
fn run(cfg: &AppConfig, out: &mut String) {
    writeln!(out, "retries={}", cfg.retries).unwrap();
    if cfg.daemon { spawn(); }
}
"""
    expect("writeln! to a buffer is a print", analyse(writes, all_structs=False),
           [("AppConfig", "retries")])

    # An assertion is ACTING on a value, not echoing it.
    asserted = """
struct AppConfig {
    pub retries: u32,
    pub daemon: bool,
}
fn main() { run(&load()); }
fn run(cfg: &AppConfig) {
    assert!(cfg.retries < 10, "retries={}", cfg.retries);
    if cfg.daemon { spawn(); }
}
"""
    expect("a field checked by an assert is acting, not echoed",
           analyse(asserted, all_structs=False), [])

    # A write is not a read: setting a field from parsed config, then never
    # using it, is the dead case again and not this one.
    assigned = """
struct AppConfig {
    pub retries: u32,
    pub daemon: bool,
}
fn main() { load(&mut AppConfig::default()); }
fn load(cfg: &mut AppConfig) {
    cfg.retries = 3;
    println!("retries={}", cfg.retries);
    if cfg.daemon { spawn(); }
}
"""
    expect("an assignment is not a read", analyse(assigned, all_structs=False),
           [("AppConfig", "retries")])

    # Scope: a non-config struct is skipped by default and found with --all.
    plain = echoed_src.replace("DaemonConfig", "Daemon")
    expect("a non-config struct is out of scope by default",
           analyse(plain, all_structs=False), [])
    expect("...and in scope with --all-structs",
           analyse(plain, all_structs=True), [("Daemon", "idle_timeout")])

    # --- stranded, not echoed ----------------------------------------------
    # `apps/mediaconvert`'s `ImageSettings::summary` in miniature: the fields
    # are read only into a `format!`, and the only callers of the formatter
    # are assertions -- which `live_code` has already removed. Nothing is being
    # told to the operator, because nothing runs. That is
    # `find-stranded-serialisers`' finding, and reporting it here would be two
    # tools reporting one defect, which reads as two defects.
    stranded_src = """
struct ImageSettings {
    pub quality: u8,
    pub max_width: Option<u32>,
}
impl ImageSettings {
    pub fn summary(&self) -> String {
        let size = match self.max_width { Some(w) => w, None => 0 };
        format!("Quality {}%, {}", self.quality, size)
    }
}
"""
    bucket: list = []
    expect("a formatter nobody calls is not an echo",
           analyse(stranded_src, all_structs=False, stranded=bucket), [])
    expect("...it is reported as stranded instead",
           bucket, [("ImageSettings", "quality")])

    # The same source with a live caller IS an echo: now it reaches someone.
    called = stranded_src + "\nfn main() { println!(\"{}\", cfg.summary()); }\n"
    bucket2: list = []
    expect("...and the same formatter with a caller is an echo",
           analyse(called, all_structs=False, stranded=bucket2),
           [("ImageSettings", "quality")])
    expect("...with nothing left in the stranded bucket", bucket2, [])

    # A one-field struct cannot tell a dump from a straggler.
    lone = """
struct TinyConfig { pub only: u32 }
fn show(c: &TinyConfig) { println!("{}", c.only); }
"""
    expect("a one-field struct is not judged", analyse(lone, all_structs=False), [])

    print(f"find-echoed-settings: self-test "
          f"{'passed' if not failures else 'FAILED'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    if selftestflag.wants_selftest(sys.argv[1:]):
        return _self_test()

    ap = argparse.ArgumentParser()
    ap.add_argument("--roots", default=",".join(DEFAULT_ROOTS))
    ap.add_argument("--all-structs", action="store_true",
                    help="every struct, not only Config/Settings/Options/...")
    ap.add_argument("--self-test", "--selftest", "--self_test",
                    dest="self_test", action="store_true",
                    help="run this script's own fixtures")
    args = ap.parse_args()
    if args.self_test:
        return _self_test()

    asked = [r for r in args.roots.split(",") if r]
    roots = [ROOT / r for r in asked]
    missing = [r for r in roots if not r.is_dir()]
    if missing:
        # A root named on the command line and not present is a typo, and
        # scanning nothing while exiting 0 is the failure this tree keeps
        # finding. A default root that is not present is just a tree that does
        # not have it yet -- reported, so the scope stays visible.
        if args.roots != ",".join(DEFAULT_ROOTS):
            print(f"no such root(s): {', '.join(str(m) for m in missing)}",
                  file=sys.stderr)
            return 2
        print(f"absent, not scanned: "
              f"{', '.join(m.name for m in missing)}")
        roots = [r for r in roots if r.is_dir()]
    if not roots:
        print("no root to scan -- refusing to call that a pass", file=sys.stderr)
        return 2

    per_crate: dict[str, list[tuple[str, str]]] = {}
    files = 0
    for root in roots:
        for path in sorted(root.rglob("*.rs")):
            if "target" in path.parts or "tests" in path.parts:
                continue
            try:
                src = path.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue
            files += 1
            code, _ = rustlex.live_code(src)
            code = rustlex.strip_noise(code)
            hits = analyse(code, all_structs=args.all_structs)
            if hits:
                per_crate.setdefault(str(path.relative_to(ROOT)), []).extend(hits)

    total = sum(len(v) for v in per_crate.values())
    print(f"files scanned                  : {files}")
    print(f"settings read only to be shown : {total}")
    print(f"in files                       : {len(per_crate)}")
    if not total:
        # A zero here is a real answer only because the self-test above proves
        # the pattern still matches the shapes it was built for. Say so, so a
        # silently-broken pattern is not read as a clean tree.
        print("\nnone -- and --self-test passes, so the pattern still matches "
              "the shapes it was built for")
        return 0
    print()
    for path in sorted(per_crate, key=lambda p: -len(per_crate[p])):
        hits = per_crate[path]
        print(f"{path}  ({len(hits)})")
        for struct, field in sorted(hits):
            print(f"    {struct}.{field}")
    print("\nEach is read, and read only into output. Check whether the "
          "program ACTS on it; if it does not, the print is telling the "
          "operator their setting took effect.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
