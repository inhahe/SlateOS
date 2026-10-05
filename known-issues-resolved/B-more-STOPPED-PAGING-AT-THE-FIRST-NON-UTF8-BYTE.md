## B-more-STOPPED-PAGING-AT-THE-FIRST-NON-UTF8-BYTE (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** `more` is the program you use to look at a file. This one would
show you *part* of a file and stop, with nothing on screen saying there was
more, and exit as if it had succeeded. Three separate causes, each of which
silently truncated the output: one stray non-text byte anywhere in the file
ended the display there; sending the output anywhere other than a terminal
(`more big.txt | grep x`, `more big.txt > copy`) cut it off after one
screenful; and a file whose name was not valid text crashed the program
outright. It also mislabelled what it did show -- a file's name was announced
in a banner that was one character too narrow, was printed even for files that
could not be opened, and was left off in the one case util-linux prints it.
All of it is fixed and `more` is now byte-for-byte identical to util-linux
`more` 2.39.3 across 44 measured cases.

### The four truncations, measured

Each row is our binary and util-linux's, run side by side on the same file,
stdin `/dev/null`, stdout a pipe.

| case | before | after |
|---|---|---|
| `more bad.txt` -- a `\377` byte on line 2 of 3 | printed line 1 only, exit **0** | all three lines, byte-identical to util-linux |
| `more sixty.txt \| cat` with `LINES=10` | printed **9** lines of 60, exit **0** | all 60 |
| `more caf<0xE9>.txt` | `panicked ... called Result::unwrap() on an Err value: "caf\xE9.txt"`, exit **134** | pages it, exit 0 |
| file with no final newline | added one | reproduces the file exactly |

The first is the `argv-utf8` gate's entry for this bin and the reason it was
picked up. The loop read with `BufRead::lines`, which yields `String` and so
*fails* on a line that is not valid UTF-8 -- and the failure arm was
`Err(_) => break`, which is end-of-file's arm. A byte the program could not
decode and the end of the file were the same event. Reconstructing each line
into a `String` and printing it with `writeln!` is what appended the newline
the file did not have.

The second is the same class of bug reached from the other side. `more` paged
unconditionally, so into a pipe it printed a screenful, wrote `--More--`, and
read a keystroke from stdin -- which was `/dev/null`, so the read returned
0 bytes, which `read_key` maps to `Key::Quit`. The pipeline got one screen of
a file the user asked for all of, with status 0. A pager must page only when
stdout is a terminal.

The third was `env::args()` (the `String` iterator, which panics on an
undecodable argument) rather than `env::args_os()`.

### The labelling, also measured

`more` announces each file inside a banner of colons. Three things were wrong
with ours, all found by running the two binaries side by side rather than by
reading the code:

- The banner was **thirteen** colons. util-linux prints **fourteen**.
- It was printed **before** the `open`, so `more good.txt nosuch.txt`
  announced `nosuch.txt` and then showed nothing under it. util-linux opens
  first and never names a file it cannot show.
- It was printed on **operand count alone** (`files.len() > 1`). The real rule,
  established over the full `{1,2 files} x {stdin tty, pipe} x {stdout tty,
  pipe}` matrix, is `operands > 1 || !isatty(0)` -- keyed on **stdin**, which
  is what tells `more` that nobody is going to answer a prompt. That is why
  `more f > out` labels its output and `more f` at a terminal does not.

A directory operand was also wrong. `File::open` on a directory *succeeds*; it
is the read that fails with `EISDIR`, so we printed a banner promising a file
and then a diagnostic. util-linux stats first and writes `\n*** dir: directory
***\n\n` to **stdout** -- in-band, where the reader is looking -- and carries
on with status 0. We now do the same.

Finally, the diagnostic itself. Ours was
`more: nosuch.txt: No such file or directory (os error 2)`; util-linux's is
`more: cannot open nosuch.txt: No such file or directory`. Both halves are
fixed: the `cannot open` wording, and `coreutils::errmsg::strerror` in place of
the host's `io::Error` text, which removes this bin from the
`host-errmsg-baseline.txt` ratchet as well.

### Where the keystrokes come from now

Paging happens only when stdout is a terminal. When it is but stdin has been
redirected, commands cannot come from stdin -- that descriptor is somebody's
data and reading it for keystrokes would consume it -- so the controlling
terminal is reopened, as util-linux does. If there is no `/dev/tty`, the
fallback is *not to page*, which shows the whole file rather than a truncated
one. Verified: `LINES=5 more sixty.txt < /dev/null` on a pty stops at
`--More--` after four lines, and does not stop at all through a pipe.

The `--More--` prompt moved from descriptor 2 to stdout at the same time.
Descriptor 2 was chosen when `more` paged unconditionally; now that paging
implies stdout *is* the terminal, stdout is where the pager's screen is, and
on descriptor 2 the prompt vanished under `more f 2>/dev/null`, leaving a
pager that looked hung.

### The one deliberate divergence

util-linux has no `-` convention: `more -` reports
`cannot open -: No such file or directory`. Ours reads stdin, because every
other utility in this tree spells stdin `-` and a pager that alone refused it
would be the surprise. Stdin is never given a banner either way, which is also
what util-linux does with its own stdin copy.

### Verification, and why none of it was caught before

`more` had **no differential harness**, unlike `cat`, `cut`, `comm` and the
rest. Four independent truncations survived in a two-hundred-line program for
exactly that reason: not one of them is visible by eye in the source, and every
one of them is a single line of a hex dump. So the fix is not only the code --
`scripts/more-diff.sh` now exists, and `scripts/all-diff.sh` picks it up by
glob.

It compares stdout (as `od -An -tx1`), stderr as *text*, and the exit status,
over: the plain copy (plain, empty, NUL and `0xff`, CRLF, blank lines, no final
newline, invalid UTF-8, 60 lines); names (a nested path, a non-UTF-8 name); the
banner (two files, three files, the same file twice, an empty file between two
others); operands that do not open (missing, good+missing, missing+good, two
missing, unreadable, unreadable+good); and directories (alone, first, last, two
of them). Every one of those runs **twice**, at `LINES=1000` and at `LINES=10`
-- the second is the value under which our `more` used to emit one screenful
and stop, so running it is what makes "stdout is not a terminal, therefore copy
it all" a claim rather than an assumption. Three further cases put stdin on a
pty via `script`, which is the only way to reach the other half of the banner
rule, and two write to `/dev/full`. **53 passed, 0 differed, 1 differs on
purpose** (the `-` operand above).

`OURS=/usr/bin/more scripts/more-diff.sh` runs util-linux against itself and
reports the one xfail as an XPASS and nothing else, which is the check that the
harness discriminates at all.

What is deliberately *not* compared is the interactive screen: with stdout a
terminal, util-linux drives it through terminfo
(`ESC[7m--More--(Next file: x)ESC[27m
ESC[K`) and ours writes a plain
`--More--`. Those cannot match byte for byte without porting terminfo into a
pager. The decision to pause, and the keystroke handling, are covered by unit
tests in `more.rs` instead.
