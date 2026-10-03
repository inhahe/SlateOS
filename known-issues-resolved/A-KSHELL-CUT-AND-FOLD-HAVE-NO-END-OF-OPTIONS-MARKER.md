## `A-KSHELL-CUT-AND-FOLD-HAVE-NO-END-OF-OPTIONS-MARKER` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25

> **Fixed, and wider than filed.** `cut` and `fold` are the two this entry
> named, but the fix was preceded by a survey of *every* option parser in
> `kshell.rs`, because a bug filed under the shape of where it was noticed
> describes its own blind spot. The survey found the same missing `--` in
> `grep`, `comm`, `zip`, `find`, `locate`, `dedup`, `undelete`, `batch` and
> `mapfile` — and, in seven of those, a strictly worse defect that this entry
> did not suspect: an option the command did not recognise was *discarded*
> rather than refused. That half is written up separately as
> `A-KSHELL-SEVEN-COMMANDS-DISCARD-AN-OPTION-THEY-DO-NOT-RECOGNISE` below.
> Both halves landed together, tested by self-test rung 63.

**Where.** `kernel/src/kshell.rs` — `parse_cut_args`, `parse_fold_args`.

**What.** Neither treats `--` as the end of the options, so a file whose name
begins with `-` cannot be named at all. `cut -f1 -- -weird.txt` refuses the
`--`, and without the `--` the name is parsed as options.

**Why it is small but not nothing.** It is a refusal, not a wrong answer: the
command fails loudly and nothing is misread as data. That is what keeps it out
of the silent-guess class and off the front of the queue. But `-` is a legal
leading character for a filename here (our paths allow every byte except `/`
and NUL), so this is a reachable file that two commands cannot open, and the
usual workaround — `./-weird.txt` — depends on the caller knowing to write it.

**What the proper fix looks like.** The same three lines `tr` and `sed` already
have: a `flags_done` flag set by a bare `--`, tested at the top of the word
loop before the `starts_with('-')` test. Both parsers already have the loop
shape for it; `cut`'s gained the bundling structure in `a24d8f5fa`.

**Not a regression.** True since both commands were written. Deliberately left
out of `a24d8f5fa` so that bundling and end-of-options stayed separate changes.
