### A-KSHELL-TAB-COMPLETION-LOOKS-UP-THE-UNEXPANDED-WORD, so `$HOME/<TAB>` searches for a directory literally named `$HOME` — 2026-09-04 (lane A) — ✅ FIXED 2026-09-05 by lane A (`eb62e6501`, `completion_view` now calls `expand_vars_speculative_str` before `remove_quotes`)

**In short:** in the kernel shell, press Tab after typing a path that contains
a variable — `cat $HOME/no<TAB>`, `ls $PWD/<TAB>` — and nothing is offered.
Completion looks for a directory whose name is the six characters `$HOME`,
which does not exist, so it silently returns no candidates. Typing the same
line and pressing Enter works fine, because *running* a command expands the
variable first. So completion and execution disagree about what the word is,
and completion is the one that is wrong.

**Where:** `kernel/src/kshell.rs`, `tab_complete`. The word is taken with

```rust
let partial_owned = remove_quotes(text_before.get(word_start..).unwrap_or(""));
```

— `remove_quotes` and nothing else. `execute` (kshell.rs:6101) runs
`expand_vars(line)` and then `expand_braces(&expanded)` *before* dispatch, so
the command receives a name this stage never computed.

**The code says otherwise, which is the part worth fixing first.** The comment
directly above that line reads:

> `remove_quotes` is the same stage the dispatcher applies to a finished
> command, so completion now searches for exactly the name the command would
> receive.

That is false in the direction that matters: the dispatcher applies
`remove_quotes` to a line that has *already* been through `expand_vars` and
`expand_braces`. A future reader who trusts the comment will conclude the
lookup is already correct and look elsewhere.

**How it was found:** not by reading, but as the single residual disagreement
when the `quote_suffix` rules for TD-KSHELL (d) were put to real bash. The
`Ctx::Double` case for a file named `$HOME.txt` round-tripped to `/root.txt`,
because the `$` sat in the prefix the user had typed rather than in the suffix
completion inserts. Chasing why the prefix was unescaped is what surfaced that
completion never expands at all.

**What the proper fix looks like.** Expand the word before looking it up, and
complete against the *expansion* while inserting into the *source*. Those are
two different strings and the mapping between them is the whole difficulty:
having matched `/root/notes.txt` against the typed `$HOME/no`, what gets
inserted must be `tes.txt` — a suffix of the expansion appended to unexpanded
source text. That works whenever the variable lies wholly within the already
typed prefix, which is the only case worth supporting; a candidate that would
require *editing* the user's `$HOME` is one completion should decline.

**Related but distinct.** `A-KSHELL-TAB-COMPLETION-DOES-NOT-KNOW-WHAT-A-WORD-IS`
(fixed 2026-09-03) was about where the word *begins*; TD-KSHELL (d) is about
what completion *inserts*. This one is about what it *looks up*. Three
independent defects in one function, each invisible to the other two.

**Reproduce:** in the kernel shell, `ls $HOME/<TAB>` — no candidates. `HOME` is
set to `/` at startup (kshell.rs:3936), so the right answer is every entry in
the root directory, which is exactly what the otherwise-identical `ls /<TAB>`
does offer. Any variable reproduces it; a version that does not lean on the
default is a bare `FOO=/` (which persists — kshell.rs:6523) followed by
`ls $FOO/<TAB>`.
