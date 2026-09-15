r"""Which event dispatchers can return before reaching their own Tick arm?

An application that puts a dialog, a modal or an overlay on screen has to route
input to it while it is up, or a keystroke meant for a filename reaches the
window behind. The usual way to write that is an early return:

    if self.modal.is_some() {
        return self.handle_modal_event(event);
    }
    match event {
        ...
        Event::Tick { .. } => self.retire_a_batch(),
    }

and the early return covers **every** event, not just input. So the tick never
arrives, and whatever the tick drives stops for as long as the overlay is up.

WHAT THIS COST, in this tree, before anyone looked:

    apps/podcast       playback and the download queue stopped advancing
    apps/reminders     stopped noticing what had become overdue
    apps/calendar      never rolled over at midnight
    apps/photomanager  the slideshow stopped between one picture and the next
    apps/explorer      a file operation stopped retiring batches -- a copy
                       stalls for as long as a confirmation is on screen

None of them had a test. The defect lives in the line you write to route
input, and it is invisible from outside: nothing errors, and the next keypress
makes the clock appear to catch up.

**It is not tied to an idiom.** It arrives through an early-return block, a
helper returning `false`, and a helper returning `Some(Consumed)`. What the
cases share is *routing to the overlay and then returning, without deciding
which events that covers* -- which is why this looks for the shape of the
consequence rather than the shape of the code.

HOW IT DECIDES

For every `match event` whose arms mention `Event::Tick`, it looks at the text
between the enclosing `fn` and the match. A `return` there can bypass the tick.

WHAT IT CANNOT SEE, and this matters because most of its hits are fine:

  * **A guarded return is reported even when the guard is correct.** The two
    good patterns both trip it. `apps/diskimager` returns early only for
    `matches!(event, Event::Key(_) | Event::Mouse(_))`, and ticks the dialog
    separately above -- it is the only application here that *names* the
    distinction, and it is worth reading as the model. Anything converted to
    `guitk::dialog::FilePicker` returns on `Picked::Handled` and falls through
    on `Picked::Ignored`, which is correct and indistinguishable from here.
  * It cannot see an overlay that swallows ticks in a *callee*. The four found
    by hand were all in the dispatcher; `apps/explorer` was not found by hand
    at all, because it routes through `self.modal` rather than a
    `file_dialog`, so no grep for the dialog field would ever have reached it.
  * Guard arms on the outer match (`Event::Key(k) if self.picker.is_open()`)
    are correct by construction and are not reported, which is the right
    answer for the right reason: every other event still meets its own arm.

Report-only. Read the guard before changing anything.

Usage:  python scripts/find-swallowed-ticks.py [--roots=apps,gui]
"""

import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402
from rustlex import live_code, strip_noise  # noqa: E402

MATCH_EVENT = re.compile(r"\bmatch\s+\*?event\b")
FN = re.compile(r"\bfn\s+([a-z_0-9]+)\s*[(<]")


def arms_of(code, at):
    """The braced block of the match starting at `at`, as (start, end)."""
    start = code.index("{", at)
    depth = 0
    for i in range(start, len(code)):
        if code[i] == "{":
            depth += 1
        elif code[i] == "}":
            depth -= 1
            if depth == 0:
                return start, i
    return start, len(code)


def scan(roots):
    findings, checked = [], 0
    for root in roots:
        base = pathlib.Path(root)
        if not base.is_dir():
            continue
        for f in sorted(base.glob("*/src/**/*.rs")):
            code = strip_noise(live_code(f.read_text(encoding="utf-8", errors="replace"))[0])
            for m in MATCH_EVENT.finditer(code):
                start, end = arms_of(code, m.end())
                if "Event::Tick" not in code[start:end]:
                    continue
                checked += 1
                fns = list(FN.finditer(code, 0, m.start()))
                if not fns:
                    continue
                head = code[fns[-1].start() : m.start()]
                if re.search(r"\breturn\b", head):
                    findings.append((f.parent.parent.name, fns[-1].group(1)))
    return checked, sorted(set(findings))


def _self_test():
    bad = 0
    good = """
    fn handle_event(&mut self, event: &Event) -> bool {
        match event {
            Event::Tick { .. } => self.advance(),
            _ => false,
        }
    }
    """
    swallows = """
    fn handle_event(&mut self, event: &Event) -> bool {
        if self.modal.is_some() {
            return self.modal_event(event);
        }
        match event {
            Event::Tick { .. } => self.advance(),
            _ => false,
        }
    }
    """
    import tempfile

    for src, want in ((good, 0), (swallows, 1)):
        with tempfile.TemporaryDirectory() as tmp:
            crate = pathlib.Path(tmp) / "fixture" / "src"
            crate.mkdir(parents=True)
            # An explicit newline, because the default translates every
            # newline to CRLF on Windows -- silently, and invisibly to git.
            (crate / "main.rs").write_text(src, encoding="utf-8", newline="\n")
            _, hits = scan([str(pathlib.Path(tmp))])
            if len(hits) != want:
                bad += 1
                print(f"FAIL  wanted {want} hit(s), got {hits}")
    print(f"self-test: {bad} failure(s)")
    return 1 if bad else 0


def main(argv):
    if selftestflag.wants_selftest(argv):
        return _self_test()
    roots = ["apps"]
    for arg in argv[1:]:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            print(f"unknown argument: {arg}", file=sys.stderr)
            return 2
    checked, findings = scan(roots)
    print(
        f"{checked} dispatcher(s) with a Tick arm; "
        f"{len(findings)} can return before reaching it\n"
    )
    print("  MOST OF THESE ARE FINE. A correct guard trips this too --")
    print("  apps/diskimager returns early only for Key and Mouse, and is the")
    print("  model worth copying. Read the guard before changing anything.\n")
    for app, fn in findings:
        print(f"    {app:<16} {fn}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
