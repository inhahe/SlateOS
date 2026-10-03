### TD-OILS-POSIX-MODE-BEHAVIOURS-ARE-NOT-IMPLEMENTED. The `posix` option is now tracked, but almost none of what bash *does* in posix mode is — 2026-08-03 — ⚠️ **OPEN**

**Where:** `userspace/oils/src/interp.rs`.

TD-OILS-XPG-ECHO-AND-POSIX-SWITCH made `set -o posix` a real, observable switch
(`set -o`, `set +o`, `$SHELLOPTS`, `[[ -o posix ]]`, `[ -o posix ]`,
`shopt -o posix`, `$POSIXLY_CORRECT`) and implemented the two behaviours needed
for it to be self-consistent: `echo`'s option scan under `xpg_echo`, and the
persistence of an assignment prefix on a special builtin. bash changes several
dozen other things in posix mode; none of the rest are modelled, so a script
that sets the flag and then relies on posix *semantics* will not get them.

**Done since:** the *expansion and variable-assignment* half of the fatality
rule, implemented as `Shell::note_shell_error` + the `FatalWhen` enum and
covered by `a-shell-diagnostic-can-end-the-shell-under-errexit-or-posix-mode.sh`
plus three lib tests. Probing it turned up that this is really **two**
independent rules with overlapping-but-different error sets:

* `set -o posix` — the POSIX rule. No errexit needed; carries the
  word-expansion abort status (1 in a script, 127 under `-c`).
* `set -e` — bash's `report_error()` ends with
  `if (exit_immediately_on_error) exit_shell (…)`, so for these diagnostics the
  *report itself* is fatal, ahead of the usual failed-command check. None of
  errexit's exemptions spare it (`|| true`, an `if` condition, `!`, a function
  body), and the status is always 1.

They do not cover the same errors: a bad subscript on the *read* side and a
readonly rejection of an assignment *prefix* are errexit-only; an arithmetic
error in `$(( … ))` is posix-only.
`posix-mode-is-the-posixly-correct-variable.sh` now does its
readonly-`SHELLOPTS` check inside posix mode again (in a subshell, so the script
survives to report it).

**Also done since — all four special-builtin failure classes.** POSIX
also terminates a non-interactive shell when a *special builtin* fails. bash
does not apply that to every non-zero status (`eval '(exit 2)'`, `shift 99`,
`trap x NOSUCHSIG` all fail harmlessly) but to four specific failure *classes*,
each with its own gate. All four are implemented, via a `BuiltinFailure` side
channel consumed in `run_builtin_body`'s teardown plus two checks in
`exec_simple_inner` and one in `exec_and_or`, covered by
`a-failure-in-a-special-builtin-can-end-a-posix-mode-shell.sh` and two lib tests:

* **Redirection failure** (status 1) — on any of the sixteen special builtins.
  Decided from the command word as written, so `command`/`builtin` both take it
  off (neither is itself special) and a brace group, function call or null
  command is never in it.
* **Assignment error** (status 1) — `export`/`readonly` refusing a readonly
  name, or an assignment *prefix* on a special builtin (`R=x eval :`, `R=x :`).
  Same word-based gate. The prefix form is the one exception to the flat status:
  bash ends it through the same variable-assignment abort a *bare* assignment
  takes, so it is 127 under `-c`.
* **`.`/`source` open failure** (status 1) — gated on bash's
  `executing_command_builtin` instead, which is a *duration*: `builtin . /nope`
  is fatal, while `command . /nope` and any `. /nope` reached from inside a
  running `command` are not (modelled as `Shell::command_builtin_depth`).

* **Usage/invalid-option** (status 2) — reported by `unset export readonly set
  trap times eval exec return exit . source` (plus `eval 'syntax ( error'`, a
  bare `return` outside a function and a `.` with no filename); never by
  `shift break continue : true command let local`, and never by a regular
  builtin (`declare -Q`, `read -Q`, `cd -Q`). Both prefixes strip it.

None of the *first three* is suppressible — not by `|| true`, `!`, an
`if`/`while` condition, a function body, `eval`, or an ERR trap. A subshell
contains them like any other abort.

The fourth is different, and modelling it was the bulk of the work. bash keeps a
global `special_builtin_failed` flag, cleared at the top of every
`execute_simple_command`, set when a special builtin returns `EX_BADUSAGE`, and
checked at the `cm_simple` node **after** the ERR trap, guarded by
`ignore_return == 0 && invert == 0`. osh reproduces that with a second counter
`usage_suppress` (distinct from `errexit_suppress`, which folds in `!` and is
therefore wrong here) plus a node-local negation test, and the check placed in
`exec_and_or` right after the ERR-trap block, narrowed to a single-`Simple`
un-negated pipeline so it lands on exactly the node bash checks. Consequences,
all pinned by the corpus case:

* Suppressed by a non-final `&&`/`||` operand and by `if`/`while`/`until`
  conditions, and that propagates *dynamically* into function bodies
  (`f || true`, `if f`, `f(){ g || true; }`) but **not** into `eval`/`.`, which
  parse fresh nodes (`eval 'unset -q' || true` is fatal; `eval 'unset -q || true'`
  is not) — hence `usage_suppress` is zeroed across `run_source_flow_out`.
* `!` is **node-local**: `! unset -q` is suppressed, but `! { unset -q; }` and
  `! f` are fatal.
* An ERR trap that actually *runs* disarms it (`trap ':' ERR` suppresses;
  `trap '' ERR` and `trap - ERR` do not), because the handler's own simple
  command clears the flag before the check is reached.

**Also fixed while probing:** `unset -f -v x` now reports
`cannot simultaneously unset a function and a variable` and returns 1 (it is
*not* the usage class — status 1, and it survives posix mode), and `times`/`eval`
now scan for an invalid option instead of ignoring one.

**Also done since — the posix-mode spelling of `export -p`/`readonly -p`.**
Outside posix mode both borrow `declare -p`'s output form; inside it they print
in their *own* spelling — the builtin's keyword in place of `declare`, and of
the attribute letters only the array kind surviving, so `declare -rx RX="1"`
lists as `export RX="1"` under one and `readonly RX="1"` under the other. `-pf`
drops the reconstructed body and prints just `export -f NAME` /
`readonly -f NAME`. `declare -p` and the `typeset` spelling are untouched, and
the mode is read when the listing *runs*, not when the name was made.
Implemented as one line-rewriting helper (`Shell::posix_attr_line`) at the four
listing sites, covered by
`posix-mode-makes-export-and-readonly-list-in-their-own-spelling.sh`.

**Also fixed while writing that corpus case — `declare`'s listing flags now
select which names are listed.** `declare -rp` and friends ignored their
attribute letters entirely and printed the whole table; `declare +rp` filtered
where bash does not; and a bare `declare` printed nothing at all. bash's rule,
measured across a 110-case matrix: `-a`/`-A` *restrict* the source list to that
array kind (both together select nothing); of the **remaining** minus-signed
letters a name needs at least one (they union, they do not intersect); with no
letters left, everything in the source list passes. A plus-signed word takes an
attribute *off*, so it selects nothing — but its `p` still makes the command a
listing. And a listing that selects no attribute and names nothing prints
`set`-style `name=value` lines rather than `declare -FLAGS` ones, passing over
the merely-declared names. `declare -p`'s filtering now goes through the one
`Shell::listing_names` helper, and the no-letter case routes through
`builtin_declare` first so `declare -Q` is still an invalid option rather than a
listing.

**Also done since — posix mode now turns on the two `shopt` options it implies.**
bash's `posix_initialize` sets `inherit_errexit` *and* `shift_verbose` when the
mode goes on, and clears only `shift_verbose` when it goes off — so
`inherit_errexit` survives a `set +o posix`, and a `shift_verbose` the script had
set by hand does not. Probing showed it is a **transition**, not a value derived
from the mode: while posix stays on, a `shopt -u shift_verbose` sticks, and only
leaving and re-entering turns it back on. osh has no posix *field* (the mode is
`$POSIXLY_CORRECT` existing), so the hook is `Shell::sync_posix_shopts`, called
from the end of `refresh_shellopts` — the one function every route into and out
of the mode already funnels through, including an assignment prefix, an `unset`,
and a `local POSIXLY_CORRECT` going out of scope — with a `posix_shopts_applied`
shadow flag to make it edge-triggered (and carried across a subshell clone, so
the child does not re-fire the edge). `inherit_errexit` was already honoured;
`shift`'s out-of-range message was gated on posix mode *directly* and is now
gated on `shift_verbose`, which is what makes
`set -o posix; shopt -u shift_verbose; shift 5` silent as bash is. Covered by
`posix-mode-turns-two-shopts-on-as-it-is-entered.sh` and two lib tests.

**Also done since — posix mode narrows what a function may be called.** bash's
grammar takes *any* word before the `()`, and outside posix mode almost any of
them may become a function (`a-b`, `a.b`, `1f`, `@`, `%f`, and even `unset`);
only a name the parser could see was not written as a bare word is refused, and
that refusal is mild (status 1, script continues). POSIX narrows it to a plain
identifier that is not one of the sixteen special builtins, and bash makes both
fatal: the identifier test runs first — so `:` and `.` are reported as bad
*names* and only `source` reaches `` `NAME': is a special builtin `` — and the
breach ends a non-interactive shell with status 2 through
`jump_to_top_level (ERREXIT)`, which nothing spares (not `!`, an `if` condition,
`|| true`, a function body or an ERR trap), though a subshell contains it and the
EXIT trap still runs. A non-special builtin's name is still free game
(`cd() { … }` shadows the builtin in posix mode too). The 40-line match arm moved
to `Shell::exec_function_def`, with the rule in
`Shell::posix_function_name_error`; covered by
`posix-mode-narrows-what-a-function-may-be-called.sh` and a lib test.

**Also done since — posix mode finds a special builtin before a function.** The
other half of the rule above: POSIX says the sixteen special builtins are found
*before* shell functions, so in posix mode a function named `unset` stops
shadowing the builtin — and starts again the moment the mode goes off. It can
only ever bite a function defined *before* the mode was entered, because the mode
itself refuses to make one with such a name. Unlike the fatality rules it is not
gated on interactivity, since it only picks between two definitions rather than
ending the shell. Three things it does *not* do, all checked: it leaves the
non-special builtins alone (`cd() { … }` still wins), it does not survive
`enable -n` taking the builtin away, and it is only *execution* that looks the
other way round — `type`, `type -t`, `command -v`/`-V`, `declare -f`/`-F` and
`unset -f` all still find the function. In `Shell::posix_special_builtin_first`,
consulted from the function-lookup branch of the command dispatcher (the table
lookup comes first, so the posix test — which reads a variable — is only paid
when there really is a function to shadow). Covered by
`posix-mode-finds-a-special-builtin-before-a-function.sh` and a lib test.

**Also done since — posix mode calls a special builtin special when describing
it.** bash normally words every builtin the same way, `NAME is a shell builtin`,
but in posix mode the sixteen become `NAME is a special shell builtin`. That is
the only place the distinction is ever *spoken*, and only in that mode, because
that is the only mode where being special means anything (the two rules above).
It reaches the three descriptions that are in words — `type`, `type -a` and
`command -V` — and none of the ones that answer with a machine-readable word:
`type -t`/`-at` still say `builtin` and `command -v` still prints the bare name.
A function still shadows the *description* either way, even where it no longer
shadows the execution. One helper, `Shell::builtin_kind_word`, at the three
places that spelled the phrase out; covered by
`posix-mode-calls-a-special-builtin-special-when-describing-it.sh` and a lib
test.

**Also done since — posix mode spells trap listings the POSIX way.** Three
changes that the manual lists as one item but that are *not* gated alike, as a
seven-round probe showed:

* The `SIG` prefix goes from **any** listing, a bare `trap` as much as a
  `trap -p` — POSIX spells a signal without it. The pseudo-signals (`EXIT`,
  `DEBUG`, `ERR`, `RETURN`) never had one to drop.
* A signal with *no* trap is shown as `trap -- - NAME` only under an **explicit
  `-p`**; a bare `trap` in posix mode still lists just what is set. So posix
  `trap -p` with no operands prints a line for every signal the shell knows, in
  the signal table's order.
* The lone-signal **reset form goes away entirely**: POSIX gives `trap` an action
  or nothing, so `trap EXIT` in posix mode is not a way to take the EXIT trap
  away but a usage error — and `trap` being a special builtin, that usage error
  *ends* a non-interactive shell. (The bare `return 2` at that site bypassed
  `Shell::note_builtin_usage_error`, so it was the one place the fatality gate
  was not consulted; it now goes through it like every other.)

Implemented as `sigspec_display`/`all_sigspecs` plus a `posix` flag threaded
through `trap_display_line`, with `trap_print` splitting the mode into two
independent gates: `posix` (controls the names) and `posix && print` (controls
whether untrapped signals earn a line). Covered by
`posix-mode-spells-trap-listings-the-posix-way.sh` and a lib test — the
no-operand `trap -p` listing can only be checked in the lib test, because the
signal *set* is the host's (see TD-OILS-SIGNAL-TABLE-IS-LINUXS-NOT-THE-HOSTS).

**Also done since — posix mode lists aliases without the `alias ` word.** POSIX
spells an alias listing as a bare `name=value` where bash normally writes a whole
`alias name=value` command that would re-enter it. The gate is not the mode
alone: bash's `print_alias` prints the word when the listing was asked for
*reusably* (`-p`) **or** when the mode is off, so only a posix-mode listing with
no `-p` goes bare — and it goes bare for a named query (`alias ll`) exactly as
much as for the whole-table dump, which is the part a mode-only reading would
miss. The `-- ` that guards a name beginning with `-` goes with the word, since
it exists only to stop the name being read back as an option. Not gated on
interactivity (checked under `bash -i`). One `reusable` flag on `alias_line`,
computed once in `builtin_alias`; covered by
`posix-mode-lists-aliases-without-the-alias-word.sh` and a lib test.

**Also done since — posix mode stops splitting a redirection word, but still
globs a dup one.** The manual gives this as one rule ("words in a redirection get
neither globbing nor word splitting"), and it is really two, applied to two
different halves of the redirection grammar. bash's `redirection_expand` sets
`W_NOSPLIT2` on the word and then hands it to `expand_words_no_vars`, which
*globs*; only the filename forms take the earlier `posixly_correct` branch that
skips the glob. So:

* a **filename** word (`<`, `>`, `>>`, `<>`, `&>`, `&>>`, `M>`) loses both, and
* a **dup** word (`<&`, `M>&`, `M<&`) loses splitting only, and is still globbed.

The decisive probe, in a directory holding a file named `1`: `2>& [1]` in posix
mode duplicates stdout, so `[1]` *was* globbed, while `< [1]` reports "No such
file". Corroborated the other way by `&> g*` creating a file literally named `g*`
where `>& g*` is an ambiguous redirect (two matches). Both are gated on
**non-interactivity** as well as the mode — bash tests
`posixly_correct && interactive_shell == 0` — which is now the shared
`Shell::posix_noninteractive` that the two other posix fatality gates were
already spelling out by hand.

Null-word removal survives in both, and is what made this more than a flag flip:
an *unquoted* expansion that produced nothing must still leave the word with zero
fields (so `> $unset` stays "ambiguous"), while a *quoted* empty one is a single
empty field that gets as far as the `open`. `expand_word(w, false)` could not be
reused because it returns one *empty* field for the unquoted case. So
`expand_word_annotated` gained the `split` flag itself — the honest home for
`W_NOSPLIT2` — and `expand_word_inner` split into `expand_word_fields(word,
split, glob)` and `expand_word_joined(word)`, with a new
`Shell::expand_redirect_word` choosing the two flags from the mode and a new
`RedirWord` operand on `expand_redirect_target`. Covered by
`posix-mode-stops-splitting-a-redirection-word-but-still-globs-a-dup.sh` and a
lib test (the interactivity exemption is lib-only — the corpus differ runs
scripts).

**Also done since — `.`/`source` search `$PATH`, and posix mode takes the `$PWD`
fallback away.** The manual's item is the second half, but probing it turned up
the first: osh's `.` did not consult `$PATH` *at all*. It read the operand as a
name in the current directory, so a script only on `$PATH` was "No such file or
directory" and one in both places always resolved to the wrong one — bash
searches `$PATH` first and falls back to the cwd only after. `Shell::builtin_source`
now resolves the operand before opening it, through a new
`Shell::find_source_in_path`. That is deliberately *not* `find_in_path`: bash's
`find_path_file` asks for `FS_READABLE` where a command lookup asks for
`FS_EXEC_ONLY`, so a `chmod -x` file is sourceable, there is no host-extension
probe (`. foo` never finds `foo.exe`), and a directory on the way is skipped
rather than ending the search. A hit becomes `$BASH_SOURCE`/`caller`'s answer;
the operand as written stays the answer when the fallback is what found it.

posix mode drops the fallback (bash: `source_searches_cwd = !posixly_correct`)
and reports `.: NAME: file not found` instead — which, `.` being a special
builtin, ends a non-interactive shell through the machinery already there. The
refusal itself is *not* gated on non-interactivity, only the fatality is.

Probing that also fixed **an unset or empty `$PATH`**, which osh read as "no
search". bash reads a missing value as `""`, which is one empty element, so it
searches the current directory and nothing else: `unset PATH; ls` is
`command not found`, but a program sitting in the current directory is still
found by a bare name with no `./` on it. `Shell::search_dirs` now says that, and
`compgen_path_commands` was folded onto it rather than keeping a second copy of
the `$PATH` walk that had already drifted.

Covered by `dot-searches-path-before-the-current-directory.sh` and a lib test
for the parts a single-entry `$PATH` cannot show.

**Also done since — posix mode leaves the functions out of a bare `set`.** POSIX
says `set` with no operands writes the shell's *variables*, so bash drops the
function definitions it otherwise appends. Probing showed the names do not appear
at all — not as `f ()` blocks, not as `f=` lines — which is what keeps the
listing re-inputtable the way the standard describes. Nothing else about the
listing moves with the mode, and `declare -f`/`-F` are bash's own spelling and
keep printing definitions in both. One gate on the function loop in
`Shell::builtin_set`; covered by
`posix-mode-leaves-functions-out-of-a-bare-set.sh` and a lib test.

**Also done since — posix mode changes the `time` reserved word twice over.**
Two unrelated rules, both sharper than the manual's wording and both probed
against the reference bash. *It takes `time`'s own options away:* bash stops
treating `time` as the reserved word as soon as the word after it looks like an
option, and goes looking for an external `time` instead — so `time -p echo hi`,
`time -- echo hi`, `time -x echo hi` and a bare `time -` are all `time: command
not found` with status 127. Since `-p`/`--` are only ever read in that one
position, taking the position away is what takes the options away; and the test
is on the word *as written*, so `time "-p" x`, `time \-p x` and `time $D x` keep
the reserved word and time a command named `-p`. *And a `time` with no command
reports the shell:* POSIX says a bare `time` writes the shell's own cumulative
user and system times, so bash drops the `real` line entirely — there is no span
to report — and prints the other two to two decimals, where outside the mode a
bare `time` times the null command and prints all three to three decimals after
a leading blank line. bash's test for "no command" is the word list *and* the
redirections, so `time x=1` and `time >f` report the ordinary way while `time ;`
and `! time` do not.

The first is a *parser* change, so `posix` joined `extglob` in `ParseOpts` (the
struct renamed from `LexOpts` for the occasion) and `Parser::parse_pipeline`'s
`time` arm breaks out when `Parser::bare_word_at(pos + 1)` starts with `-`.
Being parse-time, it needs the mode entered by a command of its own: a `( set -o
posix; time -p x )` is parsed whole before any of it runs and reads `-p` as
`time`'s option in either shell. The second is decided where the report is
printed — `Shell::is_null_command` gates a `shell_times` argument to
`Shell::time_format`. Covered by
`posix-mode-changes-the-time-reserved-word-twice-over.sh` and two lib tests.
(Every figure in the report is a real measurement — `$TIMEFORMAT` landed with
TD-OILS-TIMEFORMAT-IS-UNIMPLEMENTED and the CPU accounting with TD-OILS10 — so
the case counts lines and matches rather than showing them.)

**Also done since — posix mode respells `kill`'s signal names, both ways.** The
manual gives this as two items and they turned out to be independent, so both
were probed separately against bash 5.2.37:

* **The listing.** `kill -l` with no operands prints every *bare* name on one
  line, single-space separated, one closing newline and no trailing space —
  POSIX spells a signal without its `SIG`, and gives the listing no numbers, so
  the columnar `N) SIGNAME` layout goes entirely and takes the numbering with
  it. `kill -L`, whose whole purpose is to force the columns, takes this form
  too: the mode wins over the option. `trap -l` is **not** in the rule and keeps
  its columns in both modes, which a reading of the manual's one-line item would
  miss.
* **The lookup.** A `SIG`-prefixed name stops being a name, in every spelling
  `kill` reads one: `-SIGTERM`, `-s SIGTERM`, `-sSIGTERM`, `-n SIGTERM` and
  `kill -l SIGTERM` all become `invalid signal specification` (status 1), while
  `-TERM`, `-s TERM`, `-n 15` and `-18` keep working. The refusal is
  case-insensitive (`sIgTeRm` too) — it is the prefix that is gone, not one
  spelling of it. Three things it does *not* touch: the number→name direction
  (which never printed a prefix — `kill -l 9` is `KILL` and `kill -l 0` is
  `EXIT` in both modes), the pseudo signals (none has a `SIG` spelling to lose,
  so `EXIT`/`DEBUG` answer in both modes and `SIGEXIT` is refused in both), and
  `trap`, which accepts `trap ':' SIGUSR1` in posix mode.

Implemented as `signal_list_posix` next to `signal_list_columns`, plus bash's
`DSIG_SIGPREFIX` made explicit as `decode_signal_flags(spec, sig_prefix)` —
`decode_signal` is now the `sig_prefix = true` wrapper, so `trap` and everything
else are unchanged and only `kill` passes `false`. `builtin_kill` reads the mode
once before its option run and `kill_list` once at entry.

Covered by two lib tests, `posix_mode_lists_signals_on_one_line_without_the_sig_prefix`
and `posix_mode_refuses_a_sig_prefixed_name_to_kill` — lib rather than corpus
because the signal *set* is the target's, not the host's (see
TD-OILS-SIGNAL-TABLE-IS-LINUXS-NOT-THE-HOSTS). That entry blocks *differential*
cases only; the layout and the lookup rule are osh-internal and check fine
against osh's own table, which is why this was implementable after all.

**Still open:** the rest of bash's posix-mode list. It has now been *surveyed*
rather than guessed at — the GNU manual's "Bash POSIX Mode" page gives 75 items,
and a 42-case probe of them against osh leaves this one real gap:

* `cd` in logical mode validates the resulting path and falls back to physical.
  (Probed and **not reproducible** on the reference bash: with `lnk -> a/b` and
  `a/c` present, `cd lnk; cd ../c` fails identically in both modes rather than
  falling back to the physical `a/b/../c`. Either this Cygwin build already
  behaves the posix way, or the fallback needs a trigger the probe missed —
  there is nothing to make match until a case that *does* differ is found.)
