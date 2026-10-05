## TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS — `ed`'s `s` matches a literal string, and there are no `/RE/` addresses (lane B, 2026-08-30) — **FIXED 2026-08-30**

**Fixed the same day it was written.** `ed` now compiles every pattern through
`ere::bre` (or `ere::Regex` under `-E`), has the `/RE/`, `?RE?`, `//` and `??`
addresses, `&` and `\1`…`\9` in a replacement, and all four of `g`, `v`, `G`,
`V`. The five `kbug_pipe` cases that named this entry are ordinary `run_pipe`
comparisons now, and `scripts/ed-diff.sh` grew a `regular expressions` and a
`global commands` section — about fifty cases — around them. The rest of the
original text is kept below because the *fix plan* is what was carried out and
the table is the record of what changed.

Four things were learned in the doing that the plan did not anticipate, and
they are the reason this paragraph exists rather than a bare "fixed":

1. **A `g` command list is an input *source*, not a list of steps.** GNU feeds
   it through the ordinary line-reading path, which is what makes `a` inside a
   list take its text from the list and stop when the list ends — no `.`
   required. `Editor::global_input` is that source; `read_line` consults it
   first. The same field is the nesting guard: a `g` that finds it `Some`
   answers `Cannot nest global commands`.
2. **An empty command in a list means `p`.** Measured: `g/a/` prints every
   matching line, and `g/a/\` followed by a blank line — two empty commands —
   prints each one *twice*. The empty command cannot simply be handed to
   `execute`, which would read it as the bare-newline command (`.+1p`).
3. **`-G`/`--traditional` was never about a regex dialect**, which was the
   premise of `design-decisions.md` §713 decision 1 and was wrong. It is a
   compatibility mode over `G`, `V`, `f`, `l`, `m`, `t` and `!!`; of the
   commands this `ed` has, the single thing it changes is that `l` ends a line
   without the `$` marker. §713 records the correction.
4. **`ed`'s empty-match rule for `s///g` is not `sed`'s**, and reaching for
   `ere`'s ready-made iterator — which implements `sed`'s — was wrong. `sed`
   advances one character past a match that consumed nothing; `ed` **refuses**,
   with `Infinite substitution loop`. Measured: `,s/a*/X/g` on `alpha` is an
   error in GNU and was `XlXpXhX` here. Getting it right needs `REG_NOTBOL` —
   `^` may match on the first search of a line and not on any later one, which
   is the whole difference between `s/^x*/X/g` printing `Xalpha` and wrongly
   reporting the loop. `ere` grew a real `StartOfLine::No` for it (and
   `Regex::search`, so the walk can be the caller's without re-decoding the
   subject on every step), rather than `ed` faking the flag with a sentinel
   byte.

What remains missing is eight *commands*, which never had anything to do with
regular expressions — that is `TD-B-ED-IS-MISSING-EIGHT-COMMANDS`.

---

**In short:** `ed` is the line editor. Its search-and-replace command, `s`,
is supposed to take a *regular expression* — a small pattern language in which
`.` means "any character", `*` means "the thing before, repeated", `^` means
"the start of the line", and so on. Ours takes the pattern **literally**: it
looks for a full stop, an asterisk, a caret. So `s/./X/` replaces the first
full stop where GNU replaces the first character, and a script written for a
real `ed` will quietly do the wrong thing rather than fail. The same gap
removes the `/pattern/` form of naming a line, and with it the `g` and `v`
commands that act on every line matching one.

Nothing is wrong with what `ed` *does* today — the literal path is correct for
literal patterns, and the differential harness pins all of it. What is missing
is the pattern language.

### What is affected

| Missing | What GNU does | What we do |
|---|---|---|
| `s/RE/repl/` | matches a basic regular expression | matches the bytes literally |
| `/RE/`, `?RE?` as an address | the next (previous) matching line | `Invalid address` |
| `//` and `??` | repeat the last pattern | `Invalid address` |
| `g/RE/cmd`, `v/RE/cmd` | run `cmd` on every (non-)matching line | `Unknown command` |
| `G/RE/`, `V/RE/` | the interactive forms of the above | `Unknown command` |
| `&` in a replacement | the whole match | a literal ampersand |
| `\1`…`\9` in a replacement | the *n*th parenthesised group | a literal backslash-digit |

Also absent, and worth doing in the same pass because they are what a real
script uses next: `m` (move), `t` (copy), `j` (join), `k` (mark) and the `'x`
address, `r` (read a file in), `e`/`E` (edit another file), `u` (undo), `x`/`y`
(the cut buffer), `#` (a comment), and `!command` (run a shell command, which
also decides what a file name starting with `!` means). `h`/`H` — the last
error's explanation — are absent too; `-v` covers the same ground for a script
but not for a person at a terminal.

### The fix

`ere::bre::compile(pattern, ci) -> Result<Regex, EreError>`, which is already
in the tree and already used by `sed` (`userspace/coreutils/src/bin/sed.rs`
line ~626 — the same POSIX *basic* dialect `ed` wants, so no new engine and no
second dialect to keep in step). `ere`'s alphabet is bytes (`BStr<'a> = &'a
[u8]`, `Str = Vec<u8>`), which is what `ed`'s buffer already holds, so nothing
has to be decoded on the way in.

Concretely, in `userspace/coreutils/src/bin/ed.rs`:

1. `Substitution.pattern` becomes a compiled `Regex` instead of a `Vec<u8>`,
   and `substitute_line` uses `find_at`/`capture_spans_at` rather than a
   window comparison. Keep the last compiled pattern on the `Editor` so `//`
   and a bare `s//repl/` can reuse it.
2. `parse_address` grows the `/`…`/` and `?`…`?` forms, searching forward and
   backward from `.` and wrapping, which is what GNU does.
3. `g`, `v`, `G`, `V` join the `execute` dispatch: mark every matching line
   first, *then* run the command list, because the command list can renumber
   the buffer underneath a one-pass loop.
4. `&` and `\1`…`\9` in the replacement, from `capture_spans`.
5. A compile failure is its own diagnostic. GNU's wording for a bad pattern is
   the regex library's own text (e.g. `Unmatched [ or [^`), not one of ed's
   twelve sentences; measure it rather than invent one.

### How to see it

```
$ printf '1s/./X/\n,p\nq\n' | ed f.txt     # f.txt is alpha/beta/gamma
17
?                                          # GNU prints Xlpha
```

`scripts/ed-diff.sh` carries 14 `kbug_pipe` cases naming this entry — the
substitutions above, the `/beta/p` address, `g/a/p`, and one for each missing
command. They are loud on every run and do not fail it. When this is fixed
they turn into `KFIXED`, which *does* fail the run, so the harness will insist
the entry be closed at the same time.

### Where

| | |
|---|---|
| The literal matcher | `userspace/coreutils/src/bin/ed.rs`, `substitute_line` |
| The parser that would grow the `/RE/` forms | same file, `parse_address`, `parse_substitute` |
| The engine to use | `userspace/ere/src/bre.rs`, `compile` |
| The precedent | `userspace/coreutils/src/bin/sed.rs`, which compiles the same dialect |
| The harness cases | `scripts/ed-diff.sh`, the `kbug_pipe` block |
