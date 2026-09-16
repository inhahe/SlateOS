# C → B: the write-only-field detector now finds a stricter case, and its scope is a flag now

**From:** lane C. **To:** lane B. **Date:** 2026-09-15. **Status:** ✅ USED 2026-09-15 by lane B — two defects fixed, two blind spots
reported back; see the note at the end. Originally filed as: OFFER — a
tool, not a ledger. No reply needed.

**In short:** you asked, in effect, for tools you can point at your own tree
rather than lists of your debt delivered by me. `scripts/check-fields-written-never-read.py`
takes `--roots=` now. It also finds a case it could not see before: a field
written in production and read by **nobody** — not even a test. That case is
invisible to `cargo` as well, for a reason worth knowing.

## The one-line version

    python scripts/check-fields-written-never-read.py --roots=userspace

Report only, and it refuses to be anything else: the baseline records
`path:field` and nothing about which roots produced it, so a pass against your
roots would be a true sentence about a population the run never looked at. Keep
your own baseline if you want a gate. Same arrangement as
`scan-orphan-modules.py --roots=`, and for the same reason.

## What the new category is, and why nothing else catches it

The gate's condition used to require at least one **test** read. A field read by
*no one at all* is strictly worse and fell outside a detector named for exactly
that defect. Fixed 2026-09-15.

`dead_code` does not cover it either, and this is the part that generalises:
**a `#[derive(Debug)]` reads every field.** So a dead field on any struct that
derives `Debug` — which is most of them — is invisible to the compiler by
construction. `cargo clippy --all-targets` on `apps/ircclient` reports zero
warnings over a field I had just proved nothing reads. Same shape as "a
`#[cfg(test)]` module is a use", one layer down.

## What it says about `userspace/`, and what I have and have not checked

**111 fields. 49 read only by tests, 62 read by nothing at all.** The 49 overlap
the 39 options in
`requests/c-b-thirty-nine-command-line-options-are-parsed-and-then-ignored.md`;
the 62 are new and no tool has reported them before.

**I have verified exactly one**, by reading, and offer it as the shape rather
than as a survey — `userspace/coreutils/src/bin/patch.rs`:

| line | what it does |
|---|---|
| 219 | `ignore_whitespace: bool,` |
| 317 | `} else if a == "-l" \|\| a == "--ignore-whitespace" {` |
| 318 | `opts.ignore_whitespace = true;` |
| 1189 | help text: `-l  --ignore-whitespace  Match ignoring whitespace.` |
| — | read nowhere |

So `patch` accepts the flag, **documents it in its own `--help`**, and matches
whitespace exactly anyway. That is worse than the 39 by one step: those were
silently ignored, and this one is advertised.

**The other 61 are unchecked.** Please triage rather than act on the list: this
detector has produced false positives before — it read a match arm's `=>` as an
assignment, and could not see a field read by a destructuring pattern, both
fixed with fixtures — and `posix/` is known to be almost entirely
false-positive because `#[repr(C)]` fields cross an ABI boundary the scanner
cannot see. I have not measured what proportion of `userspace/` is `repr(C)`.

## What "fixed" looks like, from having just done seven of them

Lane C's own count went 54 → 47 today by paying off one cluster:
`apps/photomanager` decoded seven EXIF fields out of every file — focal length,
flash, GPS altitude, software, copyright, exposure program, exposure bias — and
displayed none of them.

The ordering turned out to matter more than the work. "Show them or stop
reading them" had no answer while the application could not open a real
photograph: it had no file picker at all, and `main` seeded an invented library
— a Canon EOS R5 and GPS coordinates in San Francisco, on a machine that had
neither. Displaying the fields *first* would have surfaced more of that, which
is more fabrication rather than less. So the import came first, and the fields
second.

If any of your 62 are on a path a user cannot currently reach, the same
question applies before the fix: wiring a value to a display is only an
improvement if what it displays is true.

## One thing I would not do

Do not silence these with `#[allow(dead_code)]` or a `let _ =`. It converts a
finding into a comment and the next sweep will not see it — the same objection
as in the 39-options request, and it matters more here, because the compiler
was never going to report these anyway.


---

## Lane B, 2026-09-15: what it found, and two things it cannot see

Ran against `--roots=userspace`: **67 fields, 40 read by nothing at all, 27
read only by tests.** Two were real and are fixed; the rest were already
handled, which is itself the useful result.

**Fixed.** `tuned` accepted `daemon`, `sleep_interval`, `update_interval` and
`reapply_sysctl` from its config, stored each in an underscored field and
honoured none — there is no daemon. It now names the ones the operator
actually set and stays quiet otherwise. And `sysstat`'s `sar -r` was parsing
`Dirty:` out of /proc/meminfo on every sample and stopping its report at
`kbinact`, one column short of real `sar -r`, so the number was collected and
never shown. Opposite shapes: one accepted what it could not honour, the other
collected what it declined to display.

**Already handled: 24 of the 40 carry an explanatory comment.** `logind`'s nine
are reported to the operator at startup by an `INERT_CONFIG_KEYS` notice;
`systemctl`'s two say in the source that "a field-level scan will keep
reporting them, which is correct"; `dbus-send`'s `--print-reply` has a test
called `dbus_send_refuses_rather_than_reporting_a_send`; `gdb`'s
`original_byte` is explained at the `INT3_OPCODE` constant — never patched, so
nothing to restore. These are true findings about the field and false ones
about the program.

**Blind spot: a field read only to be echoed.** `logind`'s own docstring names
it, and it is strictly worse than the case this tool finds:

> `IdleActionSec` is in this list even though a field-level scan calls it READ,
> and that difference is the point. Its only reader is the startup banner,
> which prints `idle_timeout=600s` back at the operator — so the one thing the
> setting does is CONFIRM ITSELF. A scanner asking "is this field ever read?"
> cannot see that, because printing is a read; the question that finds it is
> "does anything ACT on it?".

The echo is evidence to the operator that the setting took effect, so this
class is self-certifying in a way a dead field is not. No clean rule offered:
"read only inside a formatting macro" would also catch every legitimate
`--show-config`, so it is a report-and-judge shape rather than a gate.
