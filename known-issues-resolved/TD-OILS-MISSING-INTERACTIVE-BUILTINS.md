### TD-OILS-MISSING-INTERACTIVE-BUILTINS. `history`, `fc`, `logout`, `suspend` and `bind` all exist — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `BUILTIN_NAMES`, `HELP_TABLE`, the
dispatch in `run_builtin`,
`Shell::{history,hist_base,hist_max,hist_seen,hist_file_seen,hist_session,hist_saved,hist_own_entry,hist_on}`
and the `hist_*` methods around `record_history` (`hist_record`,
`hist_record_read`, `hist_record_stored`, `hist_drop_own_entry`, `hist_file`,
`hist_file_op`, `hist_truncate_file`, `hist_sync`); `src/parser.rs`
(`IncrementalParser::{unit_lines,split_unit_lines,classify_line,line_text}`);
`src/lexer.rs` (`Tokenized::conts`).

**Resolution 2026-08-03.** All five are in. `history`, `fc` and `logout` landed
2026-07-29 (below); `suspend` and `bind` followed, `bind`'s inputrc reader last
(TD-OILS-NO-BIND-BUILTIN). `compgen -b` now agrees with bash's count. The two
`history` fidelity bugs found by a later audit — `-a` bailing instead of clamping
its count, and the line being un-recorded once *per builtin* rather than once per
line — are fixed and documented in place below.

**What.** The five builtins bash provides for an *interactive* session were all
absent, which is invisible until something enumerates the builtin set:

```
$ bash -c 'compgen -b | wc -l'   # 61
$ osh  -c 'compgen -b | wc -l'   # 59 as of 2026-07-29 (was 56)
```
bash also carries a `%` help topic (the job-spec syntax) that osh does not.

**Done (2026-07-29) — the history store, the history *file*, and the `history`
builtin.** `set -o history`, `HISTCMD`, `HISTSIZE`, `HISTFILE`, `HISTFILESIZE`
(including the truncation it performs on assignment), and
`history [-c] [-d offset] [-anrw [file]] [-s|-p arg…] [n]`, all pinned by
`tests/corpus/history.sh` and `tests/corpus/history-file.sh` (both byte-identical
to bash) plus eleven unit tests in `interp.rs`. The model below is *measured*,
not read off the readline source — where the two disagree, the measurement won,
repeatedly and decisively.

*Enablement.* A non-interactive shell starts with `set -o history` **off** and
`HISTFILE`/`HISTSIZE`/`HISTFILESIZE` unset. `HISTCMD` exists regardless and
reads `0`. `set -o history` materialises `HISTSIZE`/`HISTFILESIZE` at `500` (if
unset) and starts recording from the *next* line; `set +o history` stops
recording but keeps both the list and the variables.

*What gets recorded.* Only lines read by the **top-level** input reader — a
`source`d file's, an `eval` string's and a function body's lines are not; the
`.`/`eval`/function *call* is. One parse unit is one entry. The list is cloned
into subshells and command substitutions, since bash's `( history )` and
`$(history)` both list the parent's entries.

*The line-join rule (`cmdhist` on, `lithist` off — bash's defaults).* A unit
spanning several physical lines is stored as one entry with its top-level
newlines replaced by `"; "`, or by `" "` when the preceding line already ends in
something a `;` cannot follow (`;`, `;;`, `;&`, `;;&`, `&`, `&&`, `|`, `|&`,
`||`, `{`, `(`, or `do`/`then`/`else`/`elif`/`in`). Newlines that are *not*
top-level survive verbatim: inside a quoted string, inside `$( … )`, and in a
here-document body (whose entry keeps its trailing newline). osh implements this
exactly rather than heuristically: a top-level newline is precisely a
`Tok::Newline`, so the entry is the unit's raw source sliced at those tokens'
offsets and rejoined. A `\<newline>` **the lexer joined away** is likewise gone
from the entry — `Tokenized::conts` records the offset of every such backslash so
`IncrementalParser::line_text` can cut it back out; one the lexer *kept* (inside
`'…'`, `"…"`, `$( … )`, or a quoted-delimiter here-document) survives, which is
the same split bash draws.

*Numbering.* An entry's number is `hist_base + index`, so a deletion renumbers
everything after it and the numbers are always contiguous. `HISTCMD` is the
number of the entry just added. Two things move the base, and they are **not**
the same operation:
* **Append eviction** (list already at `HISTSIZE`): the front entry is dropped
  and `hist_base += 1`, so survivors *keep* their numbers.
* **Assigning `HISTSIZE`** runs readline's `stifle_history`, which
  **renumbers**: survivors come out numbered from `len − cap` whatever they were
  numbered before. Verified at base 1 (len 5, caps 5/4/3/2/1 → bases 1/1/2/3/4),
  base 3 (len 4, cap 2 → base 2) and a large base (len 2, cap 1 → base 1).
`HISTSIZE=0` stores nothing and does *not* move the base, so `HISTCMD` then sits
one below the number of lines read. Unset/empty/negative unstifles; a
**non-numeric** value is a no-op that leaves the previous cap in force (which is
why osh keeps `hist_max` and `hist_seen` rather than re-parsing the variable).
`history -c` empties the list and resets the base to 1.

*Builtin quirks that fall out of that.*
* Exactly one branch runs, in the order **`-c` → `-s` → `-p` → `-d` → `-anrw` →
  list**. `-c` is the odd one: it clears, resets the base to 1 and the session
  count to 0, and then **returns immediately unless one of `-anrw` came with
  it** — so `history -c -s x`, `-c -p x`, `-c -d 1` and `-c 5` all do nothing
  further, while `history -c -w f` writes an empty file.
* `history -s TEXT` **replaces** the entry its own line made: that line was
  recorded when it was read, so bash drops it and appends `TEXT` in its place.
  With the option **off** there is no such entry, so it just appends.
* `history -p` prints its arguments and, like `-s`, **un-records its own line**.
  Both only do so when they were actually given operands: a bare `history -p` or
  `history -s` keeps its entry.
* **The line comes out once, not once per builtin** (corrected 2026-08-03 — osh
  used to drop on every call, so `history -s a; history -s b` lost `a`). The
  guard is bash's `hist_last_line_added`, modelled as `Shell::hist_own_entry`:
  the reader raises it, and what lowers it is **`-s` putting its own entry on
  top** — *not* the drop. So the pair behaves asymmetrically, and measurably so:
  `history -s a; history -s b; history -s c` on one line keeps all three, while
  `history -p a; history -p b` eats its own line *and* the entry before it. Mixed
  the same rule reads both ways: a `-s` after a `-p` still finds the reader's
  entry, a `-p` after an `-s` does not. A `$( … )` body clears the flag (bash's
  `remember_on_history = 0`), so a `-s` inside one appends beside the `x=$(…)`
  entry — a plain `( … )` subshell does **not**, and there `-s` takes the line
  back out as usual. A non-listing `fc` consumes the flag outright, since osh
  physically drops the line where bash instead offsets `last_hist` by it.
* Options cluster getopt-style (`-cs`, `-aw`, `-cw f`) and `-d` takes its offset
  attached (`-d1`) or following (`-d 1`).
* `history -d` takes an offset, a negative offset (counting back from the
  newest), or a `START-END` range — the separator is the first `-` at index > 0,
  so `-2--1` is the range (−2, −1) while `-3` is a single offset. Every
  malformed or unreachable operand is `history: ARG: history position out of
  range`, rc 1 — never "numeric argument required". A range *start* below the
  oldest entry is clamped; a *reversed* range fails silently with rc 1.
* `history N` rejects a non-number with `history: N: numeric argument required`,
  rc 1; `history 0` lists nothing.
* Two **different** `-anrw` flags are `history: cannot use more than one of
  -anrw`, rc 1 (no usage line, and the flag is not reported); the *same* flag
  twice is fine.

*The history file.* With no operand the file is `$HISTFILE`, and if that is unset
or empty it is `$HOME/.history` — readline's fallback, **not** `~/.bash_history`
(which is only bash's own default *value* for `HISTFILE`, set by an interactive
startup). `-r` reads the whole file, `-n` only the part a previous read has not
accounted for, `-w` writes the whole list, `-a` appends the entries added since
the last save. A line of the file is an entry unless it is *wholly* empty — a
whitespace-only line is an entry. Only `-a` reports a failure (`history: FILE:
cannot create: <strerror>`, rc 1, after trying to create a missing file);
`-r`/`-n`/`-w` fail **silently** with rc 1.

*The two counters.* `-a` appends the last `min(hist_session, len)` entries, where
`hist_session` counts everything recorded since the last `-a`/`-w`/`-c`. The
count is **clamped to the list, not checked against it** (corrected 2026-08-03 —
osh used to bail when `session > len`): a `HISTSIZE` that trimmed the list can
leave the count larger than there is history to append, and bash then appends all
of what is left. Measured: with the list stifled to 2 and a session count of 4,
`history -a f` appends exactly those 2 — and a path it cannot create still fails
with `cannot create`, rc 1, where osh silently returned 0. Only a count of
**zero** touches the file not at all (so a `-a` straight after another one cannot
even fail to create it). `hist_saved` (bash's
`history_lines_in_file`) is where the next `-n` starts, and its update rule is
the one thing here that no amount of reading the source predicts correctly:
* `-r`/`-n` set it to **the file's line count** — not the list length, not the
  number of lines actually consumed;
* `-a` adds the session count to it;
* `-w` and `history -c` leave it **alone**.
Measured with a probe that infers the counter from how much of a known 9-line
file a following `-n` reads, across 13 setups.

*`HISTFILESIZE`.* Assigning it truncates the file `$HISTFILE` names to its last N
lines **then and there** — the effect lands on the filesystem, which is why osh
watches the value (`hist_file_seen`) instead of reading it on demand. Only a
non-negative number does anything; `-1`, `abc`, empty and `unset` truncate
nothing, and `0` empties the file. Assigning `HISTFILE` truncates nothing, and
`history -w` ignores the limit entirely.

*How the variables are watched.* bash applies `HISTSIZE`/`HISTFILESIZE` from a
variable-assignment hook. osh re-derives both in `hist_sync`, called at the end
of `exec_simple` — the one place every way of writing a variable (a plain
assignment, `unset`, `declare`, `read`, `printf -v`, `let`) funnels through —
rather than hooking all 60+ `vars.insert`/`vars.remove` sites. Per *simple
command*, not per parse unit, so `HISTSIZE=2; HISTSIZE=1` stifles twice like
bash does.

**Residual divergences (accepted, low value).**
1. A `\<newline>` inside a `${x/y \<nl> z}` replacement: bash deletes it from the
   entry, osh keeps it. The main lexer captures `${…}` raw and the deletion
   happens in a throwaway sub-lexer whose `conts` are dropped.
2. `echo a &` followed by a newline: bash records **two** entries, osh one. A
   parse-unit-granularity divergence that shows up beyond history too.
3. A `-n` whose starting point is already **at or past** the end of the file:
   readline counts the lines it skipped by inspecting one byte *past* its own
   buffer, so bash's resulting counter is `lines` or `lines − 1` depending on
   unrelated heap state — renaming `$HISTFILE` flips the answer for the same
   file. osh always uses the intended `lines`. Not reproducible and not worth
   reproducing; `tests/corpus/history-file.sh` steers clear of the shape, with a
   comment saying why.

**Done (2026-07-29) — Stage 3: history expansion and a real `history -p`.**
The `!`-style rewriting itself is written up under `TD-OILS-NO-HISTEXPAND`.
`history -p` is the builtin door onto the same engine, and it differs from the
reader in three ways bash makes observable, all pinned by
`tests/corpus/history-p.sh`: it expands regardless of `set -H` (so it is the
*only* way to reach the expansion with the switch off); it un-records its own
line *before* expanding, so `!!` names the command before it; and a failure is
`history: ARG: history expansion failed` naming the **whole argument** — not the
reader's `EVENT: event not found` naming the event — after which the remaining
arguments are still expanded and printed, with 1 returned at the end. Each
argument is written as it is produced rather than buffered, so an error lands
between the lines it falls between. A `:p` modifier prints like any other, since
`-p` never runs anything. `history -s` does **not** expand; it stores literally.

**Done (2026-07-29) — `set -v` / `set -o verbose`, which `fc`'s editing mode
needs.** bash's `echo_input_at_read` is what `fc` raises while it re-runs the
commands it edited, so it is implemented first, on its own terms. Unlike `set -x`
— which reports commands as they are *executed*, after expansion — this is a
property of **reading**: the text goes out to stderr raw, before the unit runs
and whatever the unit turns out to be. That distinction is observable, and
`tests/corpus/verbose-option.sh` pins it. The echo carries the comment and blank
lines before a command, a here-document's body *and* its delimiter, a
`\<newline>` still spelled with its backslash, and the newlines inside a quoted
string — none of which survive into the *history's* version of the same lines.
So the parser now keeps a second span:
`IncrementalParser::unit_raw`/`last_unit_raw()` is the unit's uncooked source,
beside the `unit_lines` the history cooks. The echo reaches every source the
shell parses (an `eval` string, a `source`d file, a trap action) but not a body
the parser already swallowed: a function's, and — because bash *clears* the flag
in the subshell, so `$-` inside reads `hB` — a command or process substitution's.
Two things fell out of the work: `run_exit_trap_out` now reads its action a unit
at a time through `run_source_flow_out` like every other trap path (so the units
before a syntax error in the action still run, as in bash), and `$-` had been
missing `m` as well as `v`.

**Done (2026-07-29) — Stage 2 (remainder): `fc`.** Three modes behind one name:
`-l` lists a range, `-s` (spelled `-e -` as well) re-runs one command after a
*global* `pat=rep` substitution, and bare `fc` dumps the range into a temporary
file, runs `$FCEDIT` then `$EDITOR` then `vi` on it, and re-reads whatever comes
back with `set -v` raised for that re-read only — which is why `set -v` was
built first. Listings are `%d\t %s`, a tab and a space, not `history`'s
`%5d  %s`. `tests/corpus/fc.sh` pins all of it against bash, stdout *and*
stderr.

The range arithmetic is the fiddly part, and deriving it from measurement alone
kept producing rules that the next probe broke (`fc -l -0` printing only its own
entry; `fc -l 900 901` printing the whole history *forward*). It was settled by
reading bash's own `builtins/fc.def`, and the implementation now mirrors
`fc_gethnum` structurally: `last_hist` is the newest entry *minus the `fc` line
itself* while `real_last` is not, so `-0` yields `real_last` when listing but is
"history specification out of range" when not; an out-of-range positive clamps
to `0` for the first endpoint and `last_hist` for the second rather than
failing; the out-of-range test is `n >= last_hist`, so the newest usable entry's
own number is itself rejected; a non-numeric endpoint is a **prefix of the start
of the line**, searched newest-first (so `fc -s pick` does not find
`echo pick-this-one` — `fc -s 'echo pick'` does), and a miss is
`fc: no command found`. A single endpoint lists just that entry, except that a
`beg` landing on `real_last` gets its own branch; no endpoints at all means the
last 16. `first > last` reverses without `-r`. The two editing modes un-record
their own line before resolving any of this — bash's `fc_replhist` — which is
why `fc -l` names itself and `fc -s` names the command before it.

The read-eval loop grew a proper type for this rather than another boolean:
`run_source_flow_out`'s `record: bool` is now `HistRead::{Off, Reader,
RecordOnly}`, because `fc` needs "record, but do not `!`-expand, and record even
with `set +o history`" — a combination no caller had needed. `fc_replhist`'s pop
is unconditional, matching the measurement that with history off `fc -s` still
eats the last real entry and puts the re-run command in its place.

Two deliberate divergences, both unobservable in a script that does not go
looking: the editor command line single-quotes the temporary path (bash
concatenates it raw and so breaks on a path with spaces — ours cannot produce
one, and quoting is strictly safer), and no `SourceFrame` is pushed for the
replayed temporary file. bash names that file in diagnostics from the replayed
code, but the name is randomly generated and therefore unpinnable, and a frame
would perturb `BASH_SOURCE`/`FUNCNAME`/`caller` for no benefit.

**Done (2026-07-29) — Stage 4 (first half): `logout`, and the login-shell state
it reads.** `logout` is `exit` with a gate in front of it, but the gate needed
something osh did not have: a real notion of a **login shell**. A shell is one
because of how it was *started* — `-l`/`--login`, or an `argv[0]` beginning with
`-`, which is how `login(1)` marks it — so `Shell::login_shell` is set once by
the binary and there is no way back. `shopt login_shell` now answers truthfully
(through `shopt_default`, since [`shopt_is_read_only`] guarantees nothing can
shadow it), `shopt -p login_shell` renders the matching `shopt -s` line, and it
appears in `$BASHOPTS` — while `shopt -s`/`-u login_shell` stay the accepted
no-ops bash makes them. `-l` joins `-i` as an invocation-only cluster letter
(`-lc` works, and bash accepts `+l` and does nothing with it); `--login` is the
long spelling. `logout` outside a login shell is `logout: not login shell: use
`exit'` with status 1, and the gate runs **before** the operand is read, so
`logout abc` and `logout 3 4` are that same single line.

Making `logout` share `exit`'s operand reading turned up a real bug in `exit`
and `return`: bash reads all three through `get_exitstat`, whose second-operand
check is `no_args()` — which reports "too many arguments" and then does
`top_level_cleanup()` + `jump_to_top_level(DISCARD)`. That is **not an exit**.
osh was exiting with the first operand's status instead (`exit 3 4` left 3).
It now raises `Flow::Abort`, the variant already modelling exactly this unwind
for `break 1 2`: the rest of the current top-level parse unit is discarded, the
next unit runs, the status is 1, and because of the cleanup it escapes an
enclosing `eval`/`source`. So `exit 3 4; echo x` on one line prints neither and
a following line still runs, and `exit 9 9` inside a `for` takes the whole loop
with it. A *non-numeric* operand is checked first and returns immediately, so it
never reaches that test — `exit abc def` is only the numeric complaint, and it
still exits, with 2. The three-way answer is `ExitArg::{Status, BadNumber,
TooMany}` from the new `Shell::exit_status_arg`, shared by all three builtins.

`tests/corpus/logout.sh` pins the lot. The login-shell half is reached by
re-invoking `"$BASH" -l -c …` — `$BASH` is each shell's own path, so both sides
run *themselves* — with the child's output marker-filtered so a reference bash
that chatters from `/etc/profile` cannot break the case. The in-process half
(`Shell::set_login_shell`, which no script can reach) is covered by
`a_login_shell_reports_itself_and_lets_logout_work` in `interp.rs`.

**Still to do.**
* *Stage 4 (remainder):* `bind` (needs a readline binding table) and `suspend`
  (needs job control to have a parent shell to stop), alongside the interactive
  line editor.

**Impact.** Nothing a script does today is affected — these are all interactive
tools. `compgen -b` is now 59 against bash's 61, the two being exactly the
Stage 4 remainder. The cost is coverage rather than correctness: `compgen -b`, `compgen -c`
and `compgen -A helptopic` can only appear in
`tests/corpus/compgen-actions.sh` behind a prefix the two shells happen to
agree on, so the corpus pins less of them than it otherwise would.
