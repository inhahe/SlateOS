## TD-B-sh-IS-NOT-A-SHELL — `if`, `while`, `case`, functions, `$(())`, globs, here-docs and backticks are all absent, non-ASCII text is corrupted, and `yes | head -2` hangs forever (lane B, 2026-08-30) — **FIXED 2026-08-30**

**In short:** `userspace/coreutils/src/bin/sh.rs` is the program that ships as
`sh` — the thing an init script, a `Makefile` recipe and a `system()` call all
end up running. It advertises seventeen features in its own module header. Of
those, measured against `dash` on 2026-08-30, roughly **four work**. A one-line
`if true; then echo yes; fi` does not run; it tries to execute a program called
`true;`. Any word containing a non-ASCII byte is silently corrupted before it
reaches the command. And `yes | head -2` never returns.

Found while converting the file for `TD-B-ARGV-IS-DECODED-AS-UTF-8-AND-A-BAD-BYTE-IS-A-PANIC`
(it is one of the seven remaining findings). The panic that checker sees is, as
that entry predicts, the smallest of the defects in the file.

### Measured

`sh -c CMD` against `dash -c CMD`, both built from this tree's
`x86_64-unknown-linux-gnu` build, WSL, `LC_ALL=C.UTF-8`:

| Command | dash | ours |
|---|---|---|
| `echo héllo` | `héllo` | `hÃ\|Â\|Ã\|Â©llo` — six bytes for two |
| `X=world; echo "hi $X" tail` | `hi world tail` | `hi world tailX tail` |
| `echo hi > f; cat f` | `hi` | `hi` **twice** — the builtin ignores the redirect |
| `for i in 1 2 3; do echo $i; done` | `1 2 3` | *nothing at all* |
| `if true; then echo yes; fi` | `yes` | `sh: true;: Permission denied` |
| `while false; do echo x; done` | — | `sh: false;: Permission denied` |
| `case a in a) echo m;; esac` | `m` | `sh: case: Permission denied` |
| `f() { echo in f; }; f` | `in f` | three `Permission denied` lines |
| `echo "a\|b"` | `a\|b` | `sh: b: Permission denied` |
| `echo $((2+3))` | `5` | *empty* |
| `echo ${UNSET:-def}` | `def` | *empty* |
| `echo ${#PATH}` | `2775` | *empty* |
| `set -- a b c; echo $#; echo $@` | `3` / `a b c` | dumps the whole environment |
| `echo *` | the directory | `*` |
| ``echo a`echo b`c`` | `abc` | ``a`echo b`c`` |
| `( echo sub )` | `sub` | `sh: (: Permission denied` |
| `{ echo grp; }` | `grp` | `sh: {: Permission denied` |
| `read x <<EOF` … | `hello` | three `Permission denied` lines |
| `exec echo hi` | `hi` | `sh: exec: Permission denied` |
| **`yes \| head -2`** | `y` `y` | **hangs forever** |

What does work: `echo hi`, `a && b \|\| c`, `$(...)` (only unnested at the end of
a word), `cd`, `;`, and simple external commands.

### Why, in each case

The file has **no lexer and no parser**. `execute_script` splits the text into
*lines*, `execute_if`/`execute_while`/`execute_for` look for their keywords with
`str::starts_with` on a whole trimmed line, and everything else is handed to
`execute_command`, which splits on `;` and then hands words to `Command::new`.
Six separate consequences follow:

1. **Compound commands must be spread over lines.** `if true; then echo yes; fi`
   is one line, so `execute_script` never sees a line equal to `then`; the `if `
   prefix strips to `true; then echo yes; fi`, `trim_end_matches("; then")`
   removes nothing, and the condition run is the whole rest of the line. `for`
   on one line finds `body_start == body_end` and runs an empty body — which is
   why it prints nothing rather than erroring.
2. **`case`, `until`, `{ }`, `( )`, `!`, functions and here-documents are not
   implemented at all.** `f() { … }` is only recognised when the `{` ends the
   line *and* the closing `}` is alone on its own line.
3. **Non-ASCII is destroyed twice.** `expand_variables` and `tokenize` both copy
   input with `result.push(bytes[i] as char)`. A byte ≥ 0x80 becomes the
   `char` of that code point, which re-encodes to *two* UTF-8 bytes; the word
   passes through both functions, so two bytes become six. This is silent data
   corruption on ordinary UTF-8 text, not merely on invalid input.
4. **The double-quote branch of `expand_variables` abandons its position.** On
   seeing `$` inside `"…"` it expands *the entire remainder of the string* —
   past the closing quote — appends it, `break`s, and then lets the outer loop
   resume one byte later, so the tail is emitted a second time. That is the
   `hi world tailX tail` row.
5. **Pipelines are split on a bare `cmd.contains('\|')`, ignoring quotes**, so
   `echo "a|b"` becomes two commands; and `execute_pipeline` calls
   `child.wait()` on stage *i* before spawning stage *i+1*. A first stage that
   writes more than one pipe buffer therefore blocks on a reader that will never
   be started. `yes | head -2` is the two-line reproduction.
6. **Builtins ignore the redirections that were parsed for them.**
   `parse_redirections` runs before the builtin match, and only
   `execute_external` consults the result — so `echo hi > f` writes the file
   (via the parse) *and* stdout. Every builtin also writes through `println!`,
   which panics rather than reporting on `EPIPE`.

Two more, not visible above: `$@`/`$*` are absent, so `$@` falls through the
`$VAR` path, reads no variable, and `set` with no `--` handling prints the
environment; and `expand_variables` looks up `$X` in one flat `HashMap` that
also holds the imported environment, so a positional parameter and an
environment variable share a namespace.

### The fix

Rewrite the file around a real lexer → parser → expander → executor, working in
`&[u8]` throughout. Bytes end-to-end is what removes the two `argv-utf8`
findings (`sh.rs:55` argv, `sh.rs:183` environment) and is also, on its own, the
whole of fix 3: a shell that never converts to `str` cannot mangle a byte.

The scope is fixed by `design-decisions.md` §72, which says this file is to
"stay a small POSIX baseline" while `userspace/oils` (`osh`, 150k lines) is the
bash-superset shell. Small, therefore — but a shell that cannot run
`if true; then echo yes; fi` is not a baseline of anything.

### Where

| | |
|---|---|
| The file | `userspace/coreutils/src/bin/sh.rs` (1236 lines) |
| The corruption | same file, `expand_variables`, `tokenize` — `push(bytes[i] as char)` |
| The quote bug | same file, `expand_variables`, the `b'"'` branch |
| The deadlock | same file, `execute_pipeline` — `child.wait()` inside the spawn loop |
| The line-scanner | same file, `execute_script`, `execute_if`, `execute_while`, `execute_for` |
| The umbrella | `TD-B-ARGV-IS-DECODED-AS-UTF-8-AND-A-BAD-BYTE-IS-A-PANIC`, `scripts/argv-utf8-baseline.txt` |
| The reference | `dash` under WSL; harness to be `scripts/sh-diff.sh` |

### Fixed 2026-08-30 — `5ae8dfc41` (the rewrite) and `44027020a` (the certification)

The file is now ~6,000 lines built as the four classical stages: `Lexer` →
`Parser` → `Shell::expand_word` → `Shell::run_list`, in `&[u8]` from `args_os`
to the `exec`. Every row of the measured table above passes, and so does the
whole of what the header advertised.

**The evidence is `scripts/sh-diff.sh`**, written for this and now the standing
gate. It runs ours and dash over ~225 cases and compares **four** things byte
for byte — stdout, exit status, stderr, and the files the case left on disk —
and reports `217 passed, 0 differed, 8 differ on purpose`. `OURS=/bin/dash`
runs it against itself, which is what keeps the two normalisations honest: it
must report zero differences, and it does.

Only two fields are normalised away, both on dash's side only, because neither
is comparable and neither is ours to imitate: the `sh: <line>: ` prefix dash
puts on a diagnostic, and dash's private abbreviated `strerror` table, which
says `No such file` where every other program on the system — and POSIX — says
`No such file or directory`. The normaliser runs over stdout and the disk files
as well as stderr, since `2>&1`, `2>err` and `exec 2>log` move a diagnostic
between all three.

**The eight deliberate differences** are `trap`, `getopts`, aliases, `times`,
`readonly` and `local` (absent by scope — `design-decisions.md` §72 puts them
in `osh`), plus two wordings where dash is the outlier and we follow bash and
ksh: `echo $(( ))` is `0` here where dash calls it a syntax error, and a failed
`cd` names the errno (`cd: /nonexistent-dir: No such file or directory`) rather
than dash's `cd: can't cd to …`.

**Three limitations remain**, all documented in the module header and none
reachable by a harness case:

| | |
|---|---|
| `&` backgrounds only a single *external* command | no `fork`; a builtin, function or compound after `&` runs in the foreground and reports 0 (`Shell::run_background`) |
| descriptors above 2 reach an external child on unix only | `Command` names three, the rest are installed between fork and exec by `extra_fds`, which has no Windows counterpart. In-process builtins see them everywhere |
| `n>&-` hands a child the null device | not a genuinely closed descriptor, so a write is discarded rather than returning `EBADF` (`stdio_for`) |

Both `argv-utf8` findings are gone with it — `sh.rs` is out of
`scripts/argv-utf8-baseline.txt`, and `python scripts/argv-utf8.py --check`
now prints **5 findings across 5 files** (`diff fetch logger patch ps`), down
from 7 across 6.
