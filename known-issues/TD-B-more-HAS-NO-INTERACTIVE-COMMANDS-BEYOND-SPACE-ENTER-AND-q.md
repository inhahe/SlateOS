## TD-B-more-HAS-NO-INTERACTIVE-COMMANDS-BEYOND-SPACE-ENTER-AND-q (lane B, 2026-08-30)

**In short:** `more` is the pager — the program that shows a long file one
screen at a time and waits for you to press something. It now accepts every
*command-line option* util-linux's `more` accepts, but once it is on screen the
only keys it understands are **space** (next screenful), **Enter** (one more
line) and **q** (quit). util-linux understands about a dozen more, including
searching. Nothing is broken; a keystroke it does not know is simply treated as
"next screenful". The gap is that a user who types `/error` to search will get
a page turn instead of a search, with no message saying why.

**Where.** `userspace/coreutils/src/bin/more.rs` — `read_key`, and the `Key`
enum it returns. `page` consumes exactly the three variants.

**What util-linux has that we do not**, measured against util-linux 2.39.3:

| key | what it does there |
|---|---|
| `/pattern` | search forward for a basic regular expression; `Pattern not found` in reverse video if it misses |
| `n` | repeat the last `/` search |
| `b`, `Ctrl-B` | back up one screenful (needs the file to be seekable) |
| `=` | print the current line number |
| `:f` | print the file name and line number |
| `h`, `?` | the help screen listing all of these |
| `!cmd` | run `cmd` through the shell |
| `v` | open the file at the current line in `$VISUAL`/`$EDITOR` |
| `.` | repeat the previous command |
| `'` | return to where the last search started |
| `:n`, `:p` | next / previous file when several were named |
| a digit prefix | argument to the next command — `10<space>` shows ten more lines, `3b` backs up three screens |
| `Ctrl-L` | redraw |

**Why it is not simply "finish it".** Three of those need machinery this
program does not have and cannot fake:

- **`b`, `'` and `:p` need to go backwards**, and `page` reads forwards through
  a `BufRead` that may be a pipe. util-linux handles this by writing stdin to a
  temporary file when it is not seekable (`more.c`, `cache_stdin`). That is a
  real design choice with a cost — an unbounded temp file for `cat big | more`
  — and it should be decided deliberately rather than acquired by accident.
- **`!cmd` and `v` shell out**, which means `more` grows a dependency on a
  shell and on `$VISUAL`/`$EDITOR`, and inherits the whole quoting question.
- **`/pattern` interactively** needs a line editor on the prompt row
  (backspace, kill, ESC to cancel), which is a second input mode, not a key.

`=`, `:f`, `h`, `.`, `:n`, digit prefixes and `Ctrl-L` need none of that and are
the sensible first tranche.

**Proper fix.** Widen `Key` from three variants to a parsed
`(count: Option<usize>, command: Command)` read through a small state machine,
so a digit prefix is accumulated rather than discarded, and `page` grows a
`lines_remaining` budget it decrements instead of the current
`rows = lines_per_page - 1` special case. Do the seekable/temp-file decision
first — it determines whether `Command::Back` can exist at all, and it is the
one part of this that belongs in `open-questions.md` rather than being settled
in the code.

**Not a regression.** Before 2026-08-30 this program had *no* options at all
(`more -n 3 f` treated `-n` as a file name) and no prompt; the interactive set
was the same three keys. This entry records the gap that remains after that
work, not one it introduced.

**`-e` and `-u` are accepted and do nothing, on purpose.** util-linux's own
manual documents both as no-ops retained for compatibility: `-e` (exit on EOF)
is unconditional there since 2.39, and `-u` (suppress underlining) has no
effect because the program no longer emits underlining. Ours accepts both so a
script written for util-linux runs, and ignores both because there is nothing
to do. This is *not* an unimplemented option.
