## 722. `more`'s option set: where it follows util-linux, and the two places it deliberately does not

**Date:** 2026-08-30 · **Decided by:** Claude (autonomous)
**Lane:** B
**Where:** `userspace/coreutils/src/bin/more.rs` — `parse_args`,
`takes_next_word`, `parse_count`, `seek_start`, `run`.

**In short:** `more` is the pager — it shows a long file one screenful at a
time. Ours accepted no options whatsoever: `more -n 3 file` looked for files
named `-n` and `3`. Giving it util-linux's option set meant deciding what to do
in four awkward corners, and in two of those the honest answer turned out to be
*not* to copy util-linux, because util-linux's own answer throws away something
the user typed. Everything else here matches it byte for byte, verified by
running the same 25 cases through both programs and diffing the transcripts.

**How the answers were obtained.** Not from the manual. `scripts/`-local probe
script, run once under WSL with `MORE=more` (util-linux 2.39.3) and once under
MSYS with `MORE=<our exe>`, emitting status, stderr and stdout size for each
case; terminal-only behaviour (the prompt, `-s`, `-l`) captured separately with
`printf 'q' | script -qec "more -n 3 f" /dev/null | cat -v`. Four things I had
already written from the manual were wrong and were corrected by that
measurement — most sharply, a failed `+/pattern` search, which the manual does
not describe at all: it prints `Pattern not found` **on stdout in reverse
video**, then shows the file **from the top**, and exits **0**. I had it on
stderr with status 1.

### Decision 1: `+/pattern` is compiled in the paging path, not in the parser

*What changes:* `more '+/[' file | cat` prints the whole file and exits 0,
exactly as util-linux does, instead of failing at parse time with a usage error.

`Start::Pattern` holds the pattern's raw **bytes**; `seek_start` compiles them.

- **For:** it is what util-linux does, and the reason it does it is sound — a
  bad pattern is a *runtime* notice ("Invalid regular expression", reverse
  video, stdout, status 0, file shown from the top), the same shape as
  "Pattern not found". Making it a parse error would put it on stderr with a
  nonzero status and no file, which is a different program's behaviour.
- **Against:** it is later than the earliest point the error is detectable, and
  it means an invalid regular expression is discovered once per file rather
  than once per invocation. It also keeps a `Vec<u8>` in `Options` where a
  compiled `Regex` would be tidier.
- **Why the "for" wins:** the alternative is a measurable behaviour difference
  in a case that scripts hit (`more +/foo *.log` with one unmatched file), and
  the tidiness argument is worth nothing against that. The per-file recompile
  is real but bounded by the operand count.

### Decision 2: `more -n -3 f` gives `-3` to `-n`, where it was typed

*What changes:* the diagnostic names the thing that is actually wrong.
util-linux says `more: argument error: 'f'`; ours says
`more: argument error: '-3'`.

Both programs pre-scan for the historical `-NUM` form (`more -5 f` = a five-line
screen) before ordinary option parsing, because no getopt can express it.
util-linux's scan is position-blind: in `more -n -3 f` it lifts `-3` out as a
screen height, which leaves `-n` with no value of its own, so `-n` consumes `f`
— and the file name becomes the "bad argument". Ours (`takes_next_word`) knows
that `-n` claims the following word and steps over it before the `-NUM` rule
can fire.

- **For:** the message points at the word the user got wrong. Under util-linux
  the user is told their *file name* is a bad numeric argument, which is
  actively misleading, and the file is not paged either way.
- **Against:** it is a divergence from the reference implementation, and
  divergences are the thing this codebase spends most of its effort avoiding.
  A script that somehow depends on the exact text would see a different string.
- **Why the "for" wins:** both programs fail, with status 1, on stderr, for the
  same input; only the quoted word differs, and ours quotes the word the user
  actually typed wrongly. No script can depend on this — the case is a typo by
  construction. Reproducing a reference bug is only worth it when something
  downstream parses the output, and nothing parses `argument error:`.

### Decision 3: `--` means what it says

*What changes:* `more -- -5 f` treats `-5` as a **file name** (and reports
`more: -5: No such file or directory`). util-linux pages `f` with a five-line
screen and never looks for a file called `-5`.

Same root cause as Decision 2: util-linux's `-NUM` pre-scan runs over the whole
`argv` without honouring `--`, so the one mechanism POSIX gives a user for
naming a file that starts with a dash does not work for the one syntax that
most needs it.

- **For:** `--` is a guarantee — POSIX Utility Syntax Guideline 10 — and a
  program that quietly ignores it leaves the user with *no* way to open a file
  named `-5`. Ours sets `only_operands` in the pre-scan loop, so the historical
  syntax stops at `--` like everything else.
- **Against:** divergence again; and `more -- -5 f` under util-linux is not an
  error, so this is the one case where we fail on input the reference accepts.
- **Why the "for" wins:** the input is unambiguous and the user's intent is not
  in doubt — they typed the escape hatch. Honouring it costs one branch;
  ignoring it means a file that cannot be opened at all.

### Decision 4: a screen height too large for `usize` is an argument error, not a clamp

*What changes:* `more -n 999999999999999999999 f` fails with
`more: argument error: '999999999999999999999': Numerical result out of range`
and status 1, rather than silently paging with a maximal screen.

- **For:** it is measured util-linux behaviour, down to the suffix — that text
  is what `strtoul` leaves in `errno` (ERANGE) and util-linux appends
  `strerror` of it. Clamping would accept a number the user cannot have meant
  and produce a screen height they did not ask for.
- **Against:** the suffix is a **string literal** in our source, not a
  `strerror` call, which is normally exactly the smell `scripts/host-errmsg.py`
  exists to catch. It reads like a hard-coded host message.
- **Why the "for" wins, and why the literal is correct here:** there is no
  `io::Error` in this path to ask. The text is the errno that `strtoul`
  *would* have set on a C implementation — it is part of the interface we are
  reproducing, not text the host handed us. Deriving it by manufacturing an
  ERANGE `io::Error` purely to call `strerror` on it would be a more elaborate
  way of writing the same constant. The parse is `str::parse::<usize>` with an
  `IntErrorKind::PosOverflow` arm, so overflow is distinguished from "not a
  number" (which gets the bare `argument error: 'abc'`, also measured).
