## TD-KSHELL-COMMANDS-TAKE-A-FLAT-STRING-NOT-ARGV — tech debt, lane A

**In short:** kshell hands each of its ~750 commands a single string of
arguments rather than a list. A string cannot record that `a b` was written as
`'a b'` and is therefore *one* argument, so `grep 'a b' file` searches for `a`
in files `b` and `file`. Real shells pass a list (argv) and do not have this
problem.

**Where:** `kernel/src/kshell.rs` — 630 functions with the signature
`fn cmd_x(args: &str)`, dispatched from `dispatch` (6648) and
`dispatch_with_input` (6554).

**Status:** the quoting *structure* now survives all the way to the dispatch
boundary — see `A-KSHELL-BRACE-EXPANSION-DELETED-EVERY-QUOTE-IN-THE-SHELL`
above, which fixed the stage that was destroying it earlier. What remains is
that the boundary itself is lossy: `remove_quotes` flattens
`'a b'` to `a b`, and a command that then calls `split_whitespace` sees two
words. That is not a regression — it is what the shell has always done — but it
is now the *only* place the information is lost, rather than one of several.

**What the proper fix looks like:** pass `argv: &[String]` (produced once, by
the existing quote-aware `split_words`) instead of `args: &str`. The migration
does not have to be a single 750-function change: `command_parses_own_quotes`
is the incremental path. A command joins that list when it learns to parse its
own arguments quote-aware; until then it keeps receiving the dequoted string it
was written for, and nothing regresses. When every command is on the list,
`command_parses_own_quotes` and `remove_quotes` both delete themselves in
favour of a real argv.

**Migrated so far** (2026-08-25): `trap`, `awk`, `fold`, `base64`, `cut`, `tr`,
`sed`. Each moved when a concrete bug forced it, not as a sweep — `sed` joined
because `classify_sed_args` used `split_whitespace`, so
`sed 's/hello world/hi/'` split into `s/hello` and `world/hi/` and was refused
as an unterminated `s` command. That is the useful property of doing this one
command at a time: every conversion arrives with a test for the thing it broke
(here, kshell self-test rung 47), which a 750-function sweep would not have.

**Not purely mechanical**, which is why it is debt rather than an afternoon:
the commands disagree about what they want. `echo` wants the rest of the line
joined; `grep` wants words; `trap` wants word 1 as a command string and word 2
as a signal. Each conversion is a small semantic decision, and there are no
per-command tests to catch getting one wrong.

### Two smaller bugs found in the same area

- ~~**`parse_inline_assignment` splits the first word on raw whitespace**~~
  ✅ **FIXED 2026-08-25.** It read `FOO='a b' cmd` as `first_word = FOO='a`,
  `command = b' cmd`, so the shell set `FOO` to the fragment `'a` and then ran
  `b'` as a command — two wrong things, neither reported. It was equally broken
  before the quoting fix (it read `FOO=a` and ran `b`); that fix changed the
  wrong answer, not its wrongness.

  The fix is the quote-aware scan the rest of the shell's parsers already use,
  factored out as `first_unquoted_space`, sitting beside `unquoted_positions`
  and `split_unquoted` and following the same convention (an unterminated quote
  runs to the end of the string). Pinned by self-test rung 57.

  **A bare quoted assignment went through the same door**, which is the part
  that was easy to miss when this was written up: `ZZ='a b'` with no command
  after it still contains a space, so the *inline* parser claimed it, set `ZZ`
  to `'a`, and ran `b'`. `parse_bare_assignment` — which handles it correctly —
  was never reached, because it is tried second. So the defect was not limited
  to the construct it was found in; it swallowed the more common one too.

  **Left alone deliberately:** the value still goes through `strip_quotes`
  (outer pair only) rather than `remove_quotes` (all quoting), so
  `A=x" "y cmd` keeps its interior quotes where bash would remove them. That is
  a different, rarer defect in a different function, shared with
  `parse_bare_assignment`, and it does not run the wrong command.
- ~~**`grep` with no arguments prints its usage and exits 0.**~~ — ✅ **the
  claim was already stale when it was written, and the audit it asked for has
  now been done (2026-08-25).** `cmd_grep`'s two usage arms both call
  `set_exit(2)` — deliberately 2 rather than 1, with a comment saying why:
  within `grep`, exit 1 is the reserved meaningful answer "searched, found
  nothing", so a usage error must not be spelled the same way as a successful
  empty search.

  The second half of the bullet — "very likely not unique to `grep`, the usage
  arms of the other ~750 commands have not been audited" — was correct, and
  the audit found **87 more sites**. They are a different shape from the 710
  fixed in `A-KSHELL-A-MISTYPED-COMMAND-REPORTED-SUCCESS` below, which is why
  that sweep did not catch them: those ended in a bare `return;`, whereas
  these simply *fall out of a `match` arm*, so a search for the earlier
  pattern could not see them. See
  `A-KSHELL-A-USAGE-ARM-THAT-FALLS-OUT-OF-A-MATCH-REPORTED-SUCCESS`.
