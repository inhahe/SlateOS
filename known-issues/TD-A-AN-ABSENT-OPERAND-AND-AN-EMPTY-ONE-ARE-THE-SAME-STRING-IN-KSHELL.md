## TD-A-AN-ABSENT-OPERAND-AND-AN-EMPTY-ONE-ARE-THE-SAME-STRING-IN-KSHELL (lane A, 2026-09-04)

**In short:** in the kernel shell, a command that was given no filename and a
command that was given the filename `''` (two quote marks — an empty name) look
identical to the code, because both end up as the empty string. The empty string
then gets turned into "the current directory" on its way to the filesystem. So
`fold ''` does not say *"there is no file called that"*; it goes and reads the
current directory instead, and complains about **`/`** — a path the user never
typed. Nothing is corrupted and nothing reports false success, but the error
message names the wrong thing, which is the kind of misdirection that costs an
hour when it eventually matters.

### Where it lives

`resolve_path` (`kernel/src/kshell.rs:699`) joins its argument onto the current
working directory and normalises the result. For the empty string that join is a
no-op, the component loop sees no components, and `parts.is_empty()` returns
`/`:

```rust
if parts.is_empty() {
    return PathBuf::from("/");
}
```

That is *correct and wanted* for the great majority of its 257 callers, which
pass a command's whole argument line: `ls`, `du`, `df` and friends are written
as `let path = resolve_path(args);` precisely so that a bare `ls` means the cwd.
The empty string is doing double duty — "no argument was given" for those, and
"an argument was given and it is empty" for the handful of commands that collect
a list of operands.

### How it became reachable

TD-KSHELL (b′) — `split_words` now keeps an explicitly quoted empty word, so
`fold ''` produces a one-element file list whose element is `""`. Before that
the word was discarded by the splitter and the command saw no operand at all.
The commands that take operands through `split_words` are `sed`, `awk`, `tr`,
`cut`, `fold`, `base64`, `column`, `touch` and `printf`.

That commit audited all of them and fixed every case where the empty word
produced a *wrong answer*: `tr a ''` now refuses as GNU refuses, `column -s ''`
separates on nothing as util-linux does, `sed ''` and `awk ''` are valid empty
programs. `touch ''` reaches the same `file_path.is_empty()` guard as a missing
operand and is refused. What is left is only the three that pass the empty name
through to the filesystem — `cut`, `fold`, `base64` — where the outcome is a
refusal with exit 1, but with `/` in the message instead of `''`.

### Why it was not fixed there

The obvious one-line fix — have `resolve_path("")` return an empty `PathBuf`
rather than `/` — breaks every bare-argument command in the same file, because
those 257 call sites *want* the empty string to mean the cwd. Making that change
safely means auditing all of them and giving "no argument" its own
representation (`Option<&str>`, or a separate entry point), which is a
restructuring of the shell's argument plumbing and not a line in a commit about
word splitting.

### What the proper fix is

Give the two meanings two representations, at the point the operand is read
rather than at the point the path is resolved:

1. Commands that collect an operand **list** (`files: Vec<String>`) reject an
   empty element when they collect it, with GNU's wording —
   `fold: '': No such file or directory`, exit 1. That is three parsers
   (`parse_cut_args`, `parse_fold_args`, `parse_base64_args`) and is the whole
   user-visible fix.
2. Separately, and larger: audit the `resolve_path(args)` sites so that "the
   command was given no argument" is expressed as something other than an empty
   string. Until that is done, `resolve_path("") == "/"` must be treated as a
   documented property rather than an accident, which is what this entry makes
   it.

Step 1 is worth doing on its own and does not depend on step 2.

### How to reproduce

In the kernel shell: `fold ''`. Expect `fold: '': No such file or directory`;
observe a message naming `/`. Same for `cut -d, -f1 ''` and `base64 ''`.

**[A] 2026-09-04 — the list of nine above had one member the fix never reached:
`printf`. Found by a boot panic, ~47 rungs after the last one that had ever
executed.**

The sentence higher up this entry — *"The commands that take operands through
`split_words` are `sed`, `awk`, `tr`, `cut`, `fold`, `base64`, `column`,
`touch` and `printf`"* — is nine commands.
`command_parses_own_quotes` (`kernel/src/kshell.rs:1508`) listed **eight**. The
missing one was `printf`, and being off that list is not a cosmetic difference:
it is what decides whether `dispatch` hands the command its raw argument text
or a copy with the quotes already removed.

So for `printf` the empty word was destroyed one stage *upstream* of the
splitter that TD-KSHELL (b′) had just taught to preserve it. `printf '%s|%s|'
'' zzb` printed `zzb||` — `zzb` sitting in the slot the empty string should
have filled, plus a spurious extra format pass to consume it — where GNU prints
`|zzb|`. Fixing `split_words` could not help, because by the time it ran there
was no `''` left in the string to split.

**This is the third time in two days that a write-up has named the wrong
function**, and the three are the same mistake at different scales:

| Where | Named | Actually |
|---|---|---|
| TD-KSHELL (c), Correction 7 | `execute_single` | `dispatch` |
| `tab_complete`'s comment | "the same stage the dispatcher applies" | the dispatcher also expands first |
| this rung's comment | `split_words` discarded the `''` | `dispatch` did, before `split_words` ran |

The common shape: a stage was blamed because it is the stage where the loss
becomes *visible*, not the stage where it happens. A shell pipes one string
through many hands, and the last hand holding it when the damage shows is
rarely the one that did it.

**The fix, in two parts** — the second is required by the first:

1. Add `printf` to `command_parses_own_quotes`.
2. Replace `cmd_printf`'s private format-vs-operands split with `split_words`.
   Part 1 alone would have been a regression. That function found the format's
   closing quote with `args.find('"')` (or `find('\'')`) — no backslash
   awareness, no notion of the other quote character: precisely the shape of
   the eleven scanners `shellquote` was written to retire. It had survived
   *because* `printf` was off the list, which made those branches unreachable.
   Putting `printf` on the list would have woken them up, and `printf "a b" x`
   would have run with the format `a`, with `b` demoted to an operand.

That second part is the interesting one. **An opt-in migration list is also a
list of code that is not being exercised.** Every command still off
`command_parses_own_quotes` may be carrying its own dead quote parser, written
before the consolidation and never removed, which will come back to life on the
day that command is migrated. Whoever migrates the next one should check for
that parser first rather than after.

**Why it took a boot to find:** the rung had been written but never run — not
once, from the commit that wrote it to the commit that fixed it.

`e97f88c38` (TD-KSHELL b′) added both the fix to `split_words` *and* this rung
asserting it. The rung was correct about what should happen and wrong about
whether it did, and nothing said so, because no boot got that far:

| boot | commit | reached |
|---|---|---|
| 2026-09-04 08:17 | `26545e857` | PASS — but predates `e97f88c38`; the rung did not exist yet |
| 2026-09-04 13:44 | `be4600d6a` | `#UD` in `fastpy-countin.`, before the kshell battery |
| 2026-09-04 16:11 | `7631f6906` | rung 53 (`sed` usage text) |
| 2026-09-04 18:41 | `685c618ee` | **rung 100 — first execution of everything past 53** |

So the earlier green boots are not evidence about this rung; they are evidence
about a tree that did not contain it. A rung that has never run is
indistinguishable, in the source, from one that passes — and the gap here was
long enough that the write-up above went on citing `split_words` as fixed for
all nine commands while one of the nine had never been tried.

The practical consequence for anyone reading a self-test: **a rung's existence
is not evidence, and neither is a green boot from before it was written.** The
only thing that counts is a boot whose tree contained the rung, which
`bench/boot-history.jsonl` can answer by commit.

**[A] 2026-09-06 — step 1 is done in `968f55327`, and it is not in the three
parsers this entry told it to go in. The entry named the wrong location.**

Step 1 above says the fix is *"three parsers (`parse_cut_args`,
`parse_fold_args`, `parse_base64_args`)"*. That would have been a bug, for the
reason §910 exists: GNU attempts **every** operand and reports the **worst**
status. `fold a '' b` prints `a`, reports `''`, prints `b`, and exits 1 — three
outputs from a list containing one bad element. A parser that rejected the list
would abort before `a` was ever printed, so the fix as specified would have
turned one wrong error message into one missing file's worth of output.

All three commands already have the correct GNU-shaped run loop. The guard went
next to the existing `path == "-"` case in each:

| Command | Location | Shape |
|---|---|---|
| `cut` | `cut_run`, before `resolve_path` | print, `worst = worst.max(1)`, `continue` |
| `fold` | `fold_run`, same position | print, `worst = worst.max(1)`, `continue` |
| `base64` | `base64_run`, a `Some("") =>` arm placed after `None \| Some("-")` and before `Some(path)` | print, `set_exit(1)`, `return` |

`base64` is the exception that proves the rule rather than a departure from it:
it takes **at most one** FILE, so there is no list to keep processing and its
guard is correctly terminal.

**The generalisable mistake.** This entry located the fix by asking *where is
the empty string created?* and answering "the parser". The right question is
*where is the empty string given its meaning?* — which is the run loop, because
that is the only place that knows an operand list has other members waiting.
When a write-up proposes a location, treat it as a hypothesis about the code and
re-derive it; the entry was written by someone who had just finished reading
`split_words`, and it shows.

Rung 122 covers all of it (`fold ''`, `fold FILE '' FILE`, the exit status not
leaking into the next command, `cut -d, -f1 ''`, `base64 ''`, and a bare
`base64` still meaning stdin). Per the addendum immediately above, **that rung
has not executed yet** — no boot has been run against a tree containing it. It
is written, gated by `check-selftest-rung-numbers`, and unproven, which is
exactly the state that addendum warns against reading as evidence.

**Step 2 is untouched and still open**: `resolve_path("") == "/"` remains a
documented property, and the ~257 `resolve_path(args)` call sites still express
"no argument was given" as the empty string. This entry stays open for it.
