### [A] `A-CONSOLE-ECHO-IS-WHATEVER-THE-LAST-READER-LEFT` — typed keys can be drawn twice on the console, because the keyboard's echo is a global switch -- 2026-10-14
**Status:** OPEN (lane A). Cosmetic; the fix is the one
`A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER` already designs.

**In short:** on the text console, a key typed at init's shell is sometimes
drawn twice -- `03` typed shows as `0303` -- though the shell receives it
once. Two things draw it: the shell, which echoes each key it reads, and the
keyboard driver, whose own echo is a single switch that whichever program
last touched the console's terminal left on or off.

**Seen.** `scripts/screen.py probe` at the end of a boot (2026-10-14, lane
A's quick boot with `selftest.skip=1`) typed `echo screenprobe160355` and
Enter while the `ticker` service and the deferred benchmark task were
starting. The first 18 characters were drawn once; `03` and `55`, typed a
second later, were drawn as `0303` and `5555`; the command ran with the right
text. The serial log, which mirrors the console's writes but not the
keyboard driver's echo, holds each character once -- so the second copy is
the driver's.

**Why.** `keyboard::ECHO_ENABLED` is one global. `tty::read_common` and
`tty::set_termios` set it from the console terminal's `ECHO` bit on every
read and every mode change, so it is whatever the last Linux-ABI reader of
the console left -- on by default. init's shell (`services/init/src/main.rs`,
lane D) reads raw keys with `SYS_CONSOLE_TRY_READ_CHAR`, past the terminal,
and echoes each itself, without saying so to anyone: with the driver's echo
on, each key is drawn by both. What turned it off for the first characters
and on again a second later was not traced; any console read or mode change
in between does it.

**The fix.** Not a third echo rule: the console's keys go through the
terminal's line discipline as they arrive, which echoes by the terminal's
`ECHO` bit like a pty's, and init's shell reads through the terminal in its
own mode -- `ECHO` off while it edits a line, as every line editor sets it.
Both halves are already designed and filed: the kernel's in
`A-CONSOLE-CTRL-C-IS-ONLY-SEEN-BY-A-READER`, the readers' in
`requests/a-bd-read-the-console-through-the-terminal-so-ctrl-c-can-stop-a-program-that-is-not-reading.md`.
`ECHO_ENABLED` goes with them.

**Until then** the screen probe is unaffected: it looks for the answer on a
line of its own, which the doubled echo does not touch.
