## TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-KILL (lane B, 2026-09-16) — FIXED on lane-b, pending a boot test on main

**Status:** FIXED on `lane-b` 2026-10-09, pending a boot test on `main`;
then it moves to `known-issues-resolved/`. The operator answered B-Q22
(design-decisions §1072): one `kill`, and SlateOS's message route and the
Linux signal route both inside it, chosen per program. The one `kill` is now
procps-ng 4.0.4's, ported into coreutils and held to Ubuntu's by
`scripts/kill-diff.sh`; `userspace/kill` is deleted, and with it the forced
end a plain `kill PID` became whenever the build linked it last. The
message route -- *close* and *terminate* to a program that answers them --
joins that program when the lifecycle protocol exists (§1072's work, on
lane B's roadmap), which is the merge this entry asked for. `killall`
leaves the image until psmisc's is ported
(`requests/b-d-killall-is-not-kill-under-another-name-any-more.md`); the
baseline line in `scripts/check-bin-collisions.py` is gone.

Found 2026-09-16, same cause as
TD-B-TWO-PACKAGES-BUILD-A-BINARY-CALLED-LOGGER and found by the same sweep.

**In short:** as with `logger`, two packages build a binary called `kill` into
the same directory, so `/bin/kill` is whichever linked last. Unlike `logger`,
the two are not a rich version and a poor version of one program — they are
two different *designs*, and choosing between them is a real decision rather
than a cleanup.

### The two programs

- `userspace/kill` (package `kill`) — SlateOS-native. Its help begins
  `Slate OS kill v0.1.0 -- Send termination messages to processes`, and it
  works by sending **IPC messages**, with `-KILL/-9` documented as
  "Force kill (no IPC attempt)". It also has `--name`. This is what
  `design.txt` requires: *"No Unix signals for process control. Use IPC
  messages for shutdown, etc."*
- `userspace/coreutils/src/bin/kill.rs` (bin `kill` of package `coreutils`) —
  the POSIX surface: `kill [-s SIGNAL | -SIGNAL] PID...` and
  `kill -l [EXIT_STATUS...]`. This is what every shell script expects, and
  what a differential harness against GNU would compare.

### The measured comparison (2026-09-16)

Both built to separate files and run side by side against the reference, which
for `kill` is **procps-ng 4.0.4** (`/bin/kill`) rather than util-linux:

| invocation | coreutils applet | standalone `kill` | procps-ng |
|---|---|---|---|
| `-l` | `HUP INT QUIT ILL TRAP ABRT ...` | `Available signal names (Slate OS compatibility mapping):` | `HUP INT QUIT ILL TRAP ABRT ...` |
| `-l 9` | `KILL` | the same header; does not decode | `KILL` |
| `-s TERM 999999` | reaches the send path | `kill: unknown signal: s` | `/bin/kill: (999999): No such process` |
| no arguments | `kill: missing operand` | `kill: no process specified` | usage |

**This is the opposite of the `logger` case and the reason the two entries do
not share a conclusion.** There, the standalone had the richer surface and the
applet had two better diagnostics. Here the APPLET is the one that speaks the
reference's language: it lists signals in procps' format, decodes `-l 9` to
`KILL`, and understands `-s`. The standalone does not implement `-s` at all --
it reads the `s` as a signal name and refuses -- so a script running
`kill -s TERM $pid`, which is the POSIX spelling, fails outright against it.

What the standalone has that the applet does not is the part that matters on
this OS: it sends IPC messages, which `design.txt` requires, and it has
`--name`. So neither is deletable, and "which one is better" has no answer --
they are better at different halves.

### Why this one is not a simple deletion

The architectural rule and the compatibility remit point opposite ways, and
lane B's job is both of them. Deleting the coreutils applet loses `-s` and
`-l` and the POSIX spelling; deleting the standalone crate loses the IPC
mechanism the design spec mandates and the `--name` lookup.

The likely right answer is neither deletion but a **merge**: one `kill` whose
command-line surface is POSIX (`-s SIGNAL`, `-SIGNAL`, `-l`, numeric signal
names) and whose implementation is the IPC path, since a signal number on a
system with no signals is simply a name for "which termination message".
That is a larger change than either deletion and wants its own task.

Until then the gate keeps this from getting worse, and nothing else does: the
collision is silent at build time apart from one cargo warning, and silent at
runtime because both programs answer `kill -9 <pid>` plausibly.
