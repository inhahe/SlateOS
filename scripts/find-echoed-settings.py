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

## This checker cannot see its own fix, and that is on purpose

The repair for an echoed setting is not to delete the field. It is for the
program to say, beside the value, that nothing applies it -- `gui/desktop`'s
seven settings pages now do. **The field is still read only into output
afterwards, so it still reports here, forever.**

That is the right behaviour and not a wart. The finding is "this value reaches
the operator and nothing acts on it", which remains true of a disclaimed
setting; what changed is that the program stopped implying otherwise. A
checker that went quiet when a disclaimer appeared would be measuring the
disclaimer rather than the defect, and the day someone wires the setting up for
real it would have nothing to say.

The cost is re-triage: the next person runs this and re-reads rows already
dealt with. **That is paid with a note, not with a looser rule.** The entry
`TD-C-SETTINGS-THAT-ONLY-CONFIRM-THEMSELVES` in `known-issues.md` lists which
have been answered and how, so the second reading is a lookup rather than an
investigation.

## The question to ask of every row: is it a CONTROL or a LABEL?

Lane B triaged 28 rows down to 5 real defects with one question, and it is a
question this checker structurally cannot answer:

  * A **control** is set by somebody expecting behaviour to change.
    `sshd`'s `ListenAddress` is a control -- `ListenAddress 127.0.0.1` is how an
    administrator says "do not accept ssh from the network". Nothing on the
    bind path carries an address, so it produced a daemon reachable from the
    network *and a log line saying it was listening on 127.0.0.1*.
  * A **label** is an identifier being displayed. `thermald`'s UUID comes from
    a vendor XML file and exists to be shown. Read-only-into-output is what a
    correct label looks like.

The row this checker prints is identical for both. A human separates them in
about two seconds, and it is the fastest triage available -- so ask it first,
before reading any code.

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

# Two sets, because they mean different things and lumping them cost this
# checker most of its precision on the first run.
#
# `println!` and friends put text somewhere a person reads. A field read only
# into one of those is the `IdleActionSec` case.
#
# `format!` builds a String, and a String is as often DATA as it is display:
# `userspace/curl`'s `Options.user_agent` goes into a request header, and
# `userspace/objdump`'s `NmOpts.radix` picks a number base. Neither is being
# shown back to the operator, and both looked identical to the banner case
# until these were split.
#
# `assert!`/`panic!` are in neither: a field read in an assertion is being
# checked, which is acting on it.
SHOWN_MACROS = ("println", "print", "eprintln", "eprint", "writeln", "write")
BUILT_MACROS = ("format", "format_args")
PRINT_MACROS = SHOWN_MACROS + BUILT_MACROS


def _macro_re(names):
    return re.compile(
        r"(?<![A-Za-z0-9_])(?:" + "|".join(names) + r")\s*!\s*[(\[{]"
    )


PRINT_CALL = _macro_re(PRINT_MACROS)
SHOWN_CALL = _macro_re(SHOWN_MACROS)

OPEN_TO_CLOSE = {"(": ")", "[": "]", "{": "}"}


def print_spans(code: str, rx: re.Pattern | None = None) -> list[tuple[int, int]]:
    """Half-open [start, end) spans covering each print macro's arguments.

    Nested calls are not merged: a span inside a span is still inside a print,
    which is the only question asked of it.
    """
    spans = []
    for m in (rx or PRINT_CALL).finditer(code):
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


_AUDIT = None


def _refuses_honestly(code: str) -> bool:
    """`audit-cli-fabrication.py`'s predicate, imported rather than copied.

    **Why an import through `importlib` and not a second definition.** The
    rule -- every `exit(0)` is behind `--help`, and there is at least one
    non-zero exit -- exists once, in the checker it was written for. Two
    copies of one predicate is the arrangement where a fix lands in one and
    not the other and nobody notices, which is the same argument this lane
    made to lane B about two `/proc` parsers in one repository. The file name
    has a hyphen in it, so a plain `import` cannot reach it; that is a reason
    to use `importlib`, not a reason to retype the function.

    **What it clears.** `userspace/lp` parses `printer` and never acts on it,
    because `lp` exits non-zero: "cannot queue a print job: nothing on this
    system prints". The parsed value is a *record of what was asked for*,
    which the refusal is then able to name. Deleting it would make the
    refusal less informative, not more honest.

    **The asymmetry is the point, and is written down so nobody removes it.**
    This rule can only ever fire under `userspace/`: a GUI app has no exit
    status to read, so it can never be an honest refusal in this sense. That
    does not make it a worse rule -- **it is a rule about programs that exit,
    and the absence of findings under `apps/` is a property of the tree rather
    than of the check.** Anyone who later "fixes" the asymmetry by loosening
    the predicate until it reports something under `apps/` will have replaced
    a rule that means something with one that fires.

    Applied per file, which for these programs is per crate. A file that
    refuses on every path is not confirming a setting back to anybody,
    because it does not get far enough to.
    """
    global _AUDIT
    if _AUDIT is None:
        import importlib.util

        path = Path(__file__).resolve().parent / "audit-cli-fabrication.py"
        spec = importlib.util.spec_from_file_location("audit_cli_fabrication", path)
        _AUDIT = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(_AUDIT)
    # **Stricter than the shared predicate, deliberately.** That one has a
    # fallback branch -- no `exit(0)` anywhere, at least one non-zero exit --
    # which is sound enough where it is used (only for crates that do no I/O
    # at all) and much too loose here. An ordinary tool that errors with
    # `exit(1)` and succeeds by returning normally from `main` matches it, and
    # applying that broadly cleared 18 files that are nothing of the kind.
    #
    # What is wanted is the `lp`/`unshare` shape specifically: a `--help` arm
    # that exits 0, and every other path non-zero. Requiring an `exit(0)` site
    # to exist selects exactly that, and leaves the fallback branch unused
    # here rather than reimplemented.
    return bool(_AUDIT._EXIT_OK.search(code)) and _AUDIT.refuses_honestly(code)


def top_level_args(code: str, span: tuple[int, int]) -> list[tuple[int, int]]:
    """Spans of the macro's own comma-separated arguments, nesting excluded."""
    a, b = span
    out, start, depth = [], a, 0
    for i in range(a, b):
        c = code[i]
        if c in OPEN_TO_CLOSE:
            depth += 1
        elif c in (")", "]", "}"):
            depth -= 1
        elif c == "," and depth == 0:
            out.append((start, i))
            start = i + 1
    out.append((start, b))
    return out


_PASSED_ON = re.compile(r"\([^)]")


def directly_interpolated(code: str, pos: int, spans: list[tuple[int, int]]) -> bool:
    """Does the value at `pos` reach the reader verbatim, bound to a `{}`?

    **The distinction, in two lines that look identical to a span check:**

        format!("idle_timeout={}s", cfg.idle_timeout)   // 600 reaches the
                                                        // operator -- ECHO
        format_temp(t, cfg.fahrenheit)                  // nothing prints
                                                        // "true" -- it picks
                                                        // a rendering

    Both are reads inside a print. Only the first is the setting confirming
    itself. The second is what a *correct* display flag looks like: its whole
    job is to change output, so "read only into output" is not evidence
    against it. Lane B triaged twelve of this checker's hits and four were
    this shape -- `acpi`'s `fahrenheit`, `arp`'s `numeric`, both of
    `objdump`'s `radix` -- the largest single class of false positive.

    The test is whether the field sits in a top-level macro argument that
    hands nothing to a function: no `(` followed by anything but `)`. So
    `cfg.idle_timeout` qualifies, `cfg.path.display()` qualifies (empty parens
    -- still shown verbatim), and `format_nm_value(sym.st_value, opts.radix)`
    does not.

    Per-use, and it composes with the per-struct rule rather than replacing
    it: a struct still has to have an acting sibling before any of this is
    consulted.
    """
    # **Every containing span, not the first.** Macros nest -- a GUI app
    # writes `println!("{}", format!("q={}", cfg.quality))` and a toolkit
    # label is `push_str(&format!(...))`. The outer macro's top-level argument
    # is the whole inner call, which hands something to a function, so judging
    # by the first span found would call a plain interpolation a rendering
    # selector. Any span in which the value is bound directly makes it an echo.
    for span in spans:
        if not (span[0] <= pos < span[1]):
            continue
        for a, b in top_level_args(code, span):
            if a <= pos < b:
                arg = code[a:b].strip().lstrip("&*").strip()
                if not _PASSED_ON.search(arg):
                    return True
    return False


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
    # `main` has no call site in the source and is called by definition.
    # Without this, everything a program prints directly from `main` -- which
    # for a small command-line tool is most of what it prints -- was filed as
    # a formatter nobody calls. It was caught by a control fixture whose whole
    # job was to be the opposite of the case above it, and which returned the
    # same answer: two cases agreeing for a reason neither was testing.
    if fn == "main":
        return True
    for m in re.finditer(rf"(?<![A-Za-z0-9_]){re.escape(fn)}\s*\(", code):
        # Its own declaration is not a call.
        before = code[max(0, m.start() - 4):m.start()]
        if before.rstrip().endswith("fn"):
            continue
        return True
    return False


def analyse(
    code: str,
    *,
    all_structs: bool,
    stranded: list | None = None,
    scope: list | None = None,
) -> list[tuple[str, str]]:
    """(struct, field) pairs read only inside prints, with an acting sibling.

    A pair whose prints all sit in functions nothing calls is appended to
    `stranded` instead -- a different defect with a different fix, and one
    `find-stranded-serialisers.py` already reports.
    """
    if _refuses_honestly(code):
        # A program that refuses on every path is not confirming a setting
        # back to anyone. What it holds is a record of what was asked for,
        # which its refusal can then name.
        return []
    spans = print_spans(code)
    shown_spans = print_spans(code, SHOWN_CALL)
    found = []
    for struct, fields in struct_fields(code).items():
        if not all_structs and not CONFIG_STRUCT.search(struct):
            continue
        if len(fields) < 2:
            # Counted before the `continue` below so the tally is "structs
            # this could have judged", which is the number a zero needs
            # beside it.
            # "All siblings" needs siblings. A one-field struct cannot
            # distinguish a dump from a straggler, and guessing would make the
            # report's least reliable rows its most numerous.
            continue
        if scope is not None:
            scope.append(struct)
        echoed, acting = [], False
        for f in fields:
            reads = field_reads(code, f)
            if not reads:
                continue  # dead, not echoed -- a different checker's finding
            if all(in_any_span(p, spans) for p in reads):
                fns = {enclosing_fn(code, p) for p in reads}
                if any(fn and is_called(code, fn) for fn in fns):
                    # Shown to a person, or only formatted into a String that
                    # may well be data. The second is ranked below, not
                    # dropped: `mediaconvert`'s settings panel builds its
                    # labels with `format!` and is a true positive.
                    # A field every one of whose reads is handed to some
                    # other function is selecting a rendering, not being
                    # shown. Cleared, not ranked: unlike the format!/println!
                    # split this is not a confidence question, it is a
                    # different thing entirely.
                    if not any(directly_interpolated(code, p, spans) for p in reads):
                        continue
                    shown = any(in_any_span(p, shown_spans) for p in reads)
                    echoed.append((f, shown))
                elif stranded is not None:
                    stranded.append((struct, f))
            else:
                acting = True
        if acting:
            found.extend((struct, f, shown) for f, shown in echoed)
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
           [("DaemonConfig", "idle_timeout", True)])

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
           [("DaemonConfig", "idle_timeout", True),
            ("DaemonConfig", "kill_user_processes", True)])

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
           [("AppConfig", "retries", True)])

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
           [("AppConfig", "retries", True)])

    # Scope: a non-config struct is skipped by default and found with --all.
    plain = echoed_src.replace("DaemonConfig", "Daemon")
    expect("a non-config struct is out of scope by default",
           analyse(plain, all_structs=False), [])
    expect("...and in scope with --all-structs",
           analyse(plain, all_structs=True), [("Daemon", "idle_timeout", True)])

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
    expect("...and the same formatter with a caller is an echo (formatted)",
           analyse(called, all_structs=False, stranded=bucket2),
           [("ImageSettings", "quality", False)])
    expect("...with nothing left in the stranded bucket", bucket2, [])

    # --- an honest refusal ---------------------------------------------------
    # `userspace/lp` parses `printer`, never acts on it, and exits non-zero:
    # "cannot queue a print job: nothing on this system prints". The parsed
    # value is a record of what was asked for, which the refusal names.
    refusal = """
struct Config {
    pub printer: String,
    pub copies: u32,
}
fn main() {
    let cfg = parse();
    if args[0] == "--help" {
        print_help();
        process::exit(0);
    }
    if cfg.copies > 1 { note_copies(); }
    eprintln!("lp: cannot queue a print job for {}: nothing prints", cfg.printer);
    process::exit(1);
}
"""
    expect("a program that refuses on every path reports nothing",
           analyse(refusal, all_structs=False), [])
    # The control: the SAME source reaching exit 0 is an ordinary echo. Without
    # this, the clause above could be clearing on something other than the
    # exit code and nothing here would tell.
    reaches_zero = refusal.replace("    process::exit(1);", "    process::exit(0);")
    expect("...and the same source that can exit 0 is not cleared",
           analyse(reaches_zero, all_structs=False), [("Config", "printer", True)])

    # --- bound to a {} vs handed to a function -------------------------------
    # `userspace/acpi`'s `fahrenheit`, `arp`'s `numeric`, `objdump`'s two
    # `radix` fields: a display flag's whole job is to change output, so "read
    # only into output" is what a CORRECT one looks like. Nothing ever prints
    # the word "true". Four of lane B's twelve triaged hits were this shape --
    # the largest single class of false positive this checker had.
    selects_rendering = """
struct Config {
    pub fahrenheit: bool,
    pub verbose: bool,
}
fn main() { report(&cfg()); }
fn report(cfg: &Config) {
    println!("{}", format_temp(read_temp(), cfg.fahrenheit));
    if cfg.verbose { trace(); }
}
"""
    expect("a flag handed to a formatter is not an echo",
           analyse(selects_rendering, all_structs=False), [])

    # The same struct, the same macro, the value bound to the placeholder.
    echoes = selects_rendering.replace(
        'println!("{}", format_temp(read_temp(), cfg.fahrenheit));',
        'println!("fahrenheit={}", cfg.fahrenheit);',
    )
    expect("...and the same field bound to a {} is",
           analyse(echoes, all_structs=False), [("Config", "fahrenheit", True)])

    # Macros nest. Judging by the first containing span would read the outer
    # macro's argument -- the whole inner call -- and call this a rendering
    # selector. A GUI app writes exactly this shape.
    nested = selects_rendering.replace(
        'println!("{}", format_temp(read_temp(), cfg.fahrenheit));',
        'println!("{}", format!("fahrenheit={}", cfg.fahrenheit));',
    )
    expect("a value interpolated inside a nested format! is still an echo",
           analyse(nested, all_structs=False), [("Config", "fahrenheit", True)])

    # An empty-paren method chain is still the value reaching the reader.
    displayed = """
struct Options {
    pub path: PathBuf,
    pub verbose: bool,
}
fn main() { show(&opts()); }
fn show(o: &Options) {
    println!("path={}", o.path.display());
    if o.verbose { trace(); }
}
"""
    expect("a display()/to_string() chain is still verbatim",
           analyse(displayed, all_structs=False), [("Options", "path", True)])

    # --- shown vs merely formatted ------------------------------------------
    # `userspace/curl`'s `user_agent` goes into a request header built with
    # `format!`. Nothing shows it to the operator, so it must rank below a
    # field that reaches a `println!` -- otherwise the loudest rows in the
    # report are the ones least likely to be real.
    into_data = """
struct Options {
    pub user_agent: String,
    pub verbose: bool,
}
fn main() { send(&opts()); }
fn send(o: &Options) {
    let header = format!("User-Agent: {}", o.user_agent);
    socket.write_all(header.as_bytes());
    if o.verbose { trace(); }
}
"""
    expect("a field formatted into data ranks as not-shown",
           analyse(into_data, all_structs=False),
           [("Options", "user_agent", False)])

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
    scope: list[str] = []
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
            hits = analyse(code, all_structs=args.all_structs, scope=scope)
            if hits:
                per_crate.setdefault(str(path.relative_to(ROOT)), []).extend(hits)

    total = sum(len(v) for v in per_crate.values())
    shown_n = sum(1 for v in per_crate.values() for h in v if h[2])
    print(f"files scanned                  : {files}")
    # **A zero needs this line beside it.** Without it, "0 settings" reads the
    # same whether the tree is clean or whether `CONFIG_STRUCT` matched no
    # struct at all -- and the second is a fact about the filter, not about the
    # code. `net*/` scans 32 files and judges very few structs; knowing which
    # is the difference between a clean result and an inert one.
    print(f"config-like structs judged     : {len(scope)}")
    print(f"settings read only into output : {total}")
    print(f"  ...reach a println!/write!   : {shown_n}")
    print(f"  ...only built with format!   : {total - shown_n}")
    # Said here rather than left for the reader to infer, because the obvious
    # inference is wrong in half the tree. `format!` is ambiguous ONLY for a
    # command-line program, where a String may be a request header
    # (`curl`'s `user_agent`) or a number base (`objdump`'s `radix`) rather
    # than something a person reads. A GUI app has no stdout to print to: every
    # label it shows is `format!`-built and handed to the toolkit, so [format]
    # under `apps/` and `gui/` means *shown*, not *maybe data*.
    print("  ([format] is ambiguous only under userspace/ -- a GUI app builds "
          "every label it\n   shows with format!, so there it means shown.)")
    print(f"in files                       : {len(per_crate)}")
    if not total:
        # A zero here is a real answer only because the self-test above proves
        # the pattern still matches the shapes it was built for. Say so, so a
        # silently-broken pattern is not read as a clean tree.
        if scope:
            print(f"\nnone -- across {len(scope)} struct(s) that were judged, "
                  "and --self-test passes, so the pattern still matches the "
                  "shapes it was built for")
        else:
            print("\nnone -- AND NOTHING WAS JUDGED. No struct here is named "
                  "Config/Settings/Options/Opts/Prefs/Params with two or more "
                  "fields, so this is a fact about the filter rather than "
                  "about the code. Re-run with --all-structs to widen it.")
        return 0
    print()
    for path in sorted(per_crate, key=lambda p: -len(per_crate[p])):
        hits = per_crate[path]
        print(f"{path}  ({len(hits)})")
        for struct, field, shown in sorted(hits):
            mark = "shown " if shown else "format"
            print(f"    [{mark}] {struct}.{field}")
    print("\nEach is read, and read only into output. Check whether the "
          "program ACTS on it; if it does not, the print is telling the "
          "operator their setting took effect.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
