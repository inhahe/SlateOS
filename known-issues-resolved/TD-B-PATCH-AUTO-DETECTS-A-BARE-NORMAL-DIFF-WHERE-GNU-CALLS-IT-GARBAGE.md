## TD-B-PATCH-AUTO-DETECTS-A-BARE-NORMAL-DIFF-WHERE-GNU-CALLS-IT-GARBAGE (lane B, 2026-09-16) — **FIXED** 2026-10-01

**Resolution (2026-10-01).** GNU's reading was adopted, because it is GNU's
rule and not a choice: `patch.c` passes `need_header = !(inname ||
posixly_correct)` to `intuit_diff_type`, which skips every line that is not a
header naming a file (`*** `, `+++ `, `Index:`, `diff --git `, `--- `) until
one has -- so with no file operand a bare normal diff is never seen, and the
input is garbage. `detect_dialect` now takes the same flag, and also requires
a `< `/`> ` line after a normal command, as GNU's test does. Two related
differences went with it, found by the `Index:` row: a normal diff's leading
text is quoted under "The text leading up to this was:", and the `-p` hint is
given by dialect (never for a normal diff) rather than by whether there was
text to quote. Rows in `patch-diff.sh`: `patch -i n.patch` and
`patch -i ni.patch`. (POSIXLY_CORRECT clears `need_header` as well; that row
is not in the harness because GNU then reads its prompt from /dev/tty.)

**In short:** a "normal" diff — the bare `2c2` kind, with no `---`/`+++` header
naming a file — carries no filename at all. Handed one with **no target on the
command line**, GNU refuses the whole thing as garbage; ours accepts it, then
discovers it has nowhere to apply it and asks which file to patch.

**Measured**, GNU patch 2.7.6, `diff base.txt base.new > n.patch`:

| invocation | GNU | ours |
|---|---|---|
| `patch -i n.patch` | `patch: **** Only garbage was found in the patch input.` exit **2** | `can't find file to patch at input line 1`, the `File to patch:` prompt, exit **1** |
| `patch -i n.patch base.txt` | `patching file base.txt`, exit 0 | same |
| `patch -n -i n.patch` | prompts, exit 1 | same |

So the divergence is exactly one cell: **no target, and no `-n` forcing the
dialect.** With a target the two agree, and with `-n` they agree — it is only
auto-detection that differs.

**Where:** `userspace/coreutils/src/bin/patch.rs`, `detect_dialect`. It returns
`Dialect::Normal` on the first line that parses as a normal command, without
regard to whether anything has told it which file to patch.

**Which behaviour is right** is not obvious, and that is why this is filed
rather than changed. GNU's is defensible: a patch with no filename and no
target cannot be applied, so refusing it outright says so in one line instead
of prompting for something the caller never supplied. Ours is also defensible,
and matches what it does for the same patch WITH a target. The exit codes
differ too — 2 (garbage) against 1 (hunk ignored) — and a script that
distinguishes them sees different things.

**Severity: low.** The useful case agrees. A bare normal diff with no target is
unusable either way; the two programs merely disagree about which unusable
answer to give, and both refuse to modify anything.

**Found by** a harness row added for `-n`, which was the first case in
`patch-diff.sh` combining a header-less patch with no target file. The existing
`n.patch` rows all pass a target, so nothing had taken this path in either
program.

**The proper fix,** if GNU's reading is adopted: make `detect_dialect` answer
`Unknown` for a normal-format patch when no target operand was given, so the
existing empty-versus-garbage branch produces GNU's sentence and exit 2. Add
the row above and keep `patch -n -i n.patch` beside it, since the two must not
move together.
