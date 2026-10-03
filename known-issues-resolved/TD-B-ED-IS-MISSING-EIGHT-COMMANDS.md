## TD-B-ED-IS-MISSING-EIGHT-COMMANDS — `ed` answers `?` to `m`, `t`, `j`, `k`, `r`, `e`, `u` and `#` (lane B, 2026-08-30) — **FIXED 2026-08-30**

> **FIXED 2026-08-30 (lane B).** All eight implemented and measured against GNU
> ed 1.20.1; the eight `kbug_pipe` cases below are now ordinary `run_pipe`s and
> `scripts/ed-diff.sh` grew ~150 more. Four things the fix plan below got wrong
> or did not know, every one of them found by measuring rather than by reading:
>
> 1. **Note 1 is backwards.** A line that `m` moves *loses* its `g`/`v`
>    selection — GNU's `move_lines` calls `unset_active_nodes` over the moved
>    range. Visible: `g/^\(one\|four\)$/4m0p` over `one two three four five`
>    runs its list **once**, because moving `four` deselects it. Carrying the
>    mark, as the note said to, would run it twice. `t` keeps the source's mark
>    and never selects the copy; `j`'s joined line is a new, unselected line.
>    The `k` marks *do* travel with `m` — those are GNU node pointers — which is
>    what `Taken` in `ed.rs` exists to carry.
> 2. **A global clears the undo record the moment it starts**, whether or not it
>    changes anything: `1d`, `g/beta/p`, `u` answers `Nothing to undo` rather
>    than bringing the line back (GNU's `exec_global` calls `clear_undo_stack`
>    unconditionally). So the whole-global snapshot is *set aside* and installed
>    only by the first modifying command inside — `Editor::global_before`.
> 3. **`u` restores the current line as well as the buffer**, and restores it to
>    where `.` was before the change — including before the whole `g`, not
>    before the first line the `g` selected. That is why the snapshot is taken
>    at the top of `global` rather than by an inner command's `begin_change`.
> 4. **The `Warning: buffer modified` rule is two rules, not one**, and neither
>    is "sticky until the next command". A change *or* an error retracts the
>    warning (`1d q 1d q` and `1d q zzz q` both warn twice) while a mere look
>    does not (`1d q 1p q` quits); and the *end-of-input* warning asks a
>    different question again — was the command just before EOF the refusal — so
>    `1d q` exits 1 with one warning and `1d q 1p` exits 2 with two. Two flags:
>    `Editor::warned` and `Editor::warned_last`.
>
> Note 3 of the plan was right as written: `Editor::load` was split so `e` can
> fail without ending the session. Two further gaps found on the way are logged
> separately as `TD-B-ED-IS-MISSING-SEVEN-MORE-COMMANDS`, and the claim below
> that GNU ed has no `z` is **false** — see that entry.

**In short:** `ed` is the line editor. Eight of its commands are simply not
written yet, so typing one gets a bare `?` — the same answer as a typo. Nothing
is *wrong* with what `ed` does; there is less of it than there should be. A
script written for a real `ed` that moves a line, copies one, joins two, sets a
mark, reads a second file in, switches files, undoes a change, or carries a
comment will stop at that line rather than do something wrong. This is the
successor to `TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS`, which was about the pattern
language and is closed; this one is about the command list and never involved
patterns at all.

### What is missing

| Command | What GNU does | What we do |
|---|---|---|
| `(.,.)m(.)` | **move** the addressed lines to after another line | `Unknown command` |
| `(.,.)t(.)` | **copy** ("transfer") them there instead | `Unknown command` |
| `(.,.)j` | **join** the addressed lines into one | `Unknown command` |
| `(.)kx` and the `'x` address | set a **mark** on a line and name it later | `Unknown command` |
| `(.)r FILE` | **read** a file in after the addressed line | `Unknown command` |
| `e FILE`, `E FILE` | **edit** another file (`E` without the modified warning) | `Unknown command` |
| `u` | **undo** the last buffer change | `Unknown command` |
| `#comment` | ignore the rest of the line | `Unknown command` |

Also absent, and smaller: `x`/`y` (the cut buffer), `h`/`H` (the last error's
explanation — `-v` covers the same ground for a script but not for a person at
a terminal), and `+line` on the command line.

**Not on this list, on purpose:** `!command` and a file name beginning with `!`.
Those hand text to a shell and are a *deliberate* refusal, not a gap — see
`design-decisions.md` §713 decision 2. They answer `Shell access not implemented
by this ed` and are `xfail_pipe` cases in the harness, not `kbug_pipe` ones.
`z` is not on it either: GNU ed answers `?` to `z` as well.

### The fix

All eight are ordinary arms in `Editor::execute`
(`userspace/coreutils/src/bin/ed.rs`). Three notes that are not obvious from
the command descriptions:

1. **`m`, `t` and `j` must carry the `g` marks with the lines**, exactly as
   `insert` and `delete` already do (`Editor::marks`). A global command list
   may contain any of them, and a mark array that does not follow its lines
   turns "visit each selected line once" into "visit some twice and some never".
2. **`u` needs a change record, which does not exist yet.** The cheapest honest
   shape is a single snapshot of `(buffer, current, modified)` taken before each
   command that modifies the buffer, since GNU's `u` is one level deep and `u`
   after `u` redoes. A full undo *stack* is not what GNU has and would be a
   difference, not an improvement.
3. **`e` re-enters `Editor::load`**, which today is only called at startup and
   returns `Some(status)` meaning "the session is over before it started". That
   return has to become "the load failed, keep the old buffer" for the `e`
   caller, so `load` needs splitting rather than reusing as-is.

### How to see it

```
$ printf '1m$\n,p\nq\n' | ed f.txt     # f.txt is alpha/beta/gamma
17
?                                      # GNU prints beta/gamma/alpha
```

`scripts/ed-diff.sh` carries eight `kbug_pipe` cases naming this entry, one per
command. They are loud on every run and do not fail it; when a command lands its
case turns `KFIXED`, which *does* fail the run, so the entry cannot be silently
outlived.

### Where

| | |
|---|---|
| The dispatch to extend | `userspace/coreutils/src/bin/ed.rs`, `Editor::execute` |
| The marks that `m`/`t`/`j` must maintain | same file, `Editor::marks`, `insert`, `delete` |
| The loader `e` needs to reuse | same file, `Editor::load` |
| The harness cases | `scripts/ed-diff.sh`, the `kbug_pipe` block |
