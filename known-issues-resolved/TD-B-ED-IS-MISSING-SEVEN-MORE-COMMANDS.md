## TD-B-ED-IS-MISSING-SEVEN-MORE-COMMANDS — `ed` answers `?` to `h`, `H`, `P`, `W`, `x`, `y` and `z` (lane B, 2026-08-30) — **FIXED 2026-08-30**

> **FIXED 2026-08-30 (lane B).** All seven landed together with the cut-buffer
> fills in `d`, `c`, `j` and `s`. `scripts/ed-diff.sh` is 507 cases, 0 differed,
> 8 differ on purpose, **0 known bugs** — the `kbug_pipe` case is now an
> ordinary `run_pipe` — and the file's command language is complete but for a
> `+line` operand on the command line.
>
> **One thing the measurements above missed, which only the harness found.**
> `z`'s default address is `.+1` *outside* a global and `.` *inside* one: GNU's
> `check_second_addr( current_addr + !isglobal, … )`. It is the only default
> address in ed that depends on where the command came from, and it is the
> right quirk — the global has just put `.` on the selected line, so a
> `g/RE/z` that started at `.+1` would page from the line after every match.
> The case that caught it is `g/line0/z2` on thirty numbered lines; every
> single-command probe agreed.
>
> **Two smaller corrections while in there.** `q`, `Q` and `u` were checking
> their print suffix *before* refusing an address, so `1q9` said `Invalid
> command suffix` where GNU says `Unexpected address`. That was invisible
> before — both answers are a bare `?` — and `h` is exactly what makes it
> visible, so the command that reports errors is also the command that found
> two. And `W` shares the `w` arm rather than duplicating it, with `write`
> taking an `append` flag; the byte count it prints is what *that* call wrote,
> which for an append is not the file's new size.

**In short:** `ed` is the line editor. Having just filled in the eight commands
`TD-B-ED-IS-MISSING-EIGHT-COMMANDS` was about, a sweep of *every* letter GNU ed
accepts turned up seven more that we still answer `?` to — the same answer as a
typo. Two of them (`h`, `H`) are how a person sitting at `ed` finds out *why*
something was refused; without them the only way to see a reason is to have
started `ed -v`, which you cannot do retroactively. The rest are the scrolling
command, the cut/paste pair, the prompt toggle, and append-to-a-file.

### What is missing

GNU ed 1.20.1's full command set, measured a letter at a time, is:

```
a c d e E f g G h H i j k l m n p P q Q r s t u v V w W x y z = # !
```

Ours has all of those except these seven:

| Command | What GNU does | What we do |
|---|---|---|
| `h` | print the **last error's** sentence; print nothing if there has been none | `Unknown command` |
| `H` | **toggle** printing that sentence after every `?`; printing the pending one if it turns them on | `Unknown command` |
| `P` | **toggle** the command prompt on and off | `Unknown command` |
| `(1,$)W FILE` | **append** the addressed lines to a file, rather than truncating it as `w` does | `Unknown command` |
| `(.)x` | **put** the cut buffer after the addressed line | `Unknown command` |
| `(.,.)y` | **yank** the addressed lines into the cut buffer | `Unknown command` |
| `(.+1)z(N)` | print a **window** of N lines and remember N as the new window size | `Unknown command` |

**Not on this list, on purpose:** `!command` and a file name beginning with `!`
are a *deliberate* refusal — see `design-decisions.md` §713 decision 2.

### What was measured

All of it against GNU ed 1.20.1 under WSL, since that is the only copy on this
machine. Recorded here because the measuring is most of the work:

- **`h`** prints the last error's sentence and nothing at all when there has
  been no error. `1h` is `Unexpected address`. `hp` takes an ordinary print
  suffix, as do `Hp` and `Pp`.
- **`H`** toggles. When the toggle turns *on* it also prints the pending
  sentence, so `1d`, `q`, `H` prints `Warning: buffer modified` twice: once as
  the `H` reports the `q`'s refusal and once for the `H`'s own. `-v` is exactly
  "`H` already on".
- **`P`** toggles the prompt; the default prompt string is `*`, and `-p*`
  starts it on.
- **`z`** is `(.+1)z(N)(style)`. The default window is **22**, a count
  *persists* as the new window (so `1z3` then `z` prints lines 4–6), the window
  clamps at `$`, `.` becomes the last line printed, and a trailing `p`/`n`/`l`
  styles the whole window. `1z0`, `1z-1` and `1zx` are all `Invalid command
  suffix`; `41z` on a 5-line buffer is `Invalid address`.
- **`y`** copies `(.,.)` into the cut buffer. It does *not* move `.` and does
  *not* mark the buffer modified.
- **`x`** puts the cut buffer after `(.)`; `.` becomes the last line put; it
  marks the buffer modified. With an empty cut buffer it is `Nothing to put`
  (a sentence we do not have yet). `9x` on a 5-line buffer is `Invalid
  address`.
- **The cut buffer is filled by `d`, `c`, `j`, `s` and `y`** — and *not* by
  `m`, `t`, `r`, `a`, `i` or `u`. It survives a `u`, and it carries no marks.
- **`W`** appends `(1,$)` to a file and prints the byte count. It clears
  `modified` even when the name is not the default one. `-s` drops the count;
  `-r` refuses a name with a separator; `1Wp` is `Unexpected command suffix`.

### The `z` correction

`TD-B-ED-IS-MISSING-EIGHT-COMMANDS` says, in its "not on this list" paragraph,
that "`z` is not on it either: GNU ed answers `?` to `z` as well". **That is
false.** GNU ed has `z` and it is the paging command described above. The claim
came from testing `z` on a buffer whose current line was already `$`, where
`(.+1)` is out of range and the answer is a perfectly ordinary `Invalid
address`. A `?` from a command that *exists* and a `?` from one that does not
are the same `?` — which is the whole reason `h` and `H` are worth having, and
is a neat argument for measuring a command at more than one starting state.

### The fix

Seven more arms in `Editor::execute` (`userspace/coreutils/src/bin/ed.rs`), plus
state they need that does not exist yet:

| New state | For |
|---|---|
| `cut_buffer: Vec<Vec<u8>>` | `x`, `y`, and fills in the `d`/`c`/`j`/`s` arms |
| `last_error: Option<EdError>` | `h`, `H` |
| `verbose: bool` as an `Editor` field rather than an `Options` one | `H` toggles it, so it cannot stay immutable on `opts` |
| `prompt_on: bool` | `P` |
| `window: usize`, defaulting to 22 | `z` |
| an `EdError::NothingToPut` variant | `x` on an empty cut buffer |

### How to see it

```
$ printf '1d\nq\nH\n' | ed f.txt
17
?
?                                # GNU prints `Warning: buffer modified` twice
```

`scripts/ed-diff.sh` carries a `kbug_pipe` case naming this entry. It is loud on
every run and does not fail it; when `H` lands the case turns `KFIXED`, which
*does* fail the run, so the entry cannot be silently outlived. (It did, and it
is now `run_pipe '1d\nq\nH\n' f.txt` in the `=== explaining the last error ===`
section, alongside 26 other `h`/`H` cases.)

### Where

| | |
|---|---|
| The dispatch to extend | `userspace/coreutils/src/bin/ed.rs`, `Editor::execute` |
| The error sentences | same file, `EdError`, `EdError::sentence` |
| Where `-v` is read today | same file, `Options::verbose`, `Editor::fail` |
| The harness case | `scripts/ed-diff.sh`, the `kbug_pipe` block |
