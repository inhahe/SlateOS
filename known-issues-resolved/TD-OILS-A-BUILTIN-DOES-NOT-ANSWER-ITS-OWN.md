### TD-OILS-A-BUILTIN-DOES-NOT-ANSWER-ITS-OWN---HELP. `cd --help` says `--: invalid option` where bash prints the long doc — 2026-08-12 — ✅ FIXED 2026-08-12

**Where:** `userspace/oils/src/interp.rs` — every builtin's own option parser.
osh already *has* the text: the `help` builtin's table carries bash's short doc
and long doc for each builtin (TD-OILS-HELP-HAS-NO-LONG-DESCRIPTIONS, fixed
2026-08-06). What is missing is the second way into it.

**What.** In bash `--help` is not a per-builtin option at all — it is caught by
the *shared* option scanner. `internal_getopt` (builtins/bashgetopt.c:83) tests
`ISHELP (lcurrent->word->word)`, i.e. `STREQ (s, "--help")` (builtins/common.h:27),
at the head of a fresh word and returns the pseudo-option `GETOPT_HELP` (-99,
bashgetopt.h:29). Callers turn that into `builtin_help ()` and status 2, by one
of three routes:

* `CASE_HELPOPT` — a `case GETOPT_HELP:` arm in the builtin's own option switch
  (builtins/common.h:38); 24 of the `.def` files use it;
* `CHECK_HELPOPT (list)` at the top of a builtin that has no options of its own
  (builtins/common.h:33), e.g. `shift_builtin` (shift.def:63);
* `no_options (list)` (builtins/common.c), which is the same test again for the
  builtins that take none.

`builtin_help` (builtins/help.def:193) then prints `printf ("%s: %s\n",
this_command_name, short_doc)` followed by `show_longdoc (ind)` — to **stdout** —
and the caller returns `EX_USAGE`, so the status is **2** even though the text
was asked for. It is looked up by `this_command_name`, so `builtin cd --help`
reaches the same entry.

The scanner only sees it at the *head of a word*, so `-x --help` reaches it but
a `--` before it ends the options and `cd -- --help` treats it as a directory. A
handful of builtins never run the scanner at all and so have no `--help`:
`:`/`true`/`false`, `test`/`[`, `echo`, `let`, `eval`, `break`/`continue`,
`return`, `exit`, `times`, and the reserved words. That list needs measuring
builtin by builtin rather than trusting the `.def` grep, since `no_options` and
`CHECK_HELPOPT` do not mention `GETOPT_HELP` by name.

**Reproduce.**

```
$ bash -c 'cd --help' | head -2
cd: cd [-L|[-P [-e]] [-@]] [dir]
    Change the shell working directory.
$ bash -c 'cd --help >/dev/null'; echo $?
2
$ osh -c 'cd --help'
osh: cd: --: invalid option
$ osh -c 'shift --help'
osh: shift: --help: numeric argument required
```

**Proper fix.** One shared check, not sixty copies — mirroring bash's own
structure. osh's builtins parse their options in their own loops, so the shape
that matches is a helper next to the `help` table that answers "is this operand
`--help`, at the head of a word, before any `--`?" and, if so, prints the same
short-plus-long text `help <name>` prints and returns 2. Wire it into the common
front of the builtin dispatcher for the builtins bash gives it to, and leave the
list above alone. The status must be 2 and the text must go to stdout.

**Fixed.** `builtin_help_opt` / `builtin_wants_help` (free functions next to
`HELP_TABLE`) and `Shell::builtin_answer_help`, asked once at the head of
`Shell::run_builtin_body` as the first, guarded arm of the dispatch match — so
the check is written once rather than in each of the sixty builtins, which is
bash's own structure.

The list above was measured builtin by builtin rather than grepped, and the
answer is finer than "does it run the scanner": *how far into its own arguments*
a builtin still sees the request is what the three routes differ in, so
`builtin_help_opt` records exactly that.

* `HelpOpt::None` — `:`, `true`, `false`, `echo`, `test`, `[`. No scanner, so
  `--help` is an operand: `echo --help` prints it, `test --help` is a
  one-argument test and true.
* `HelpOpt::First` — `CHECK_HELPOPT`/`no_options`: the first word only. `kill
  --help` is the help, `kill -l --help` is a signal named `--help`; `dirs -l
  --help` is a bad operand; `let 1 --help` is arithmetic. Also `break`,
  `continue`, `exit`, `logout`, `return`, `shift`, `caller`, `fg`, `bg`,
  `pushd`, `popd`, `builtin`, `eval`, `source`/`.`, `times`, `getopts`.
* `HelpOpt::Scan(optstring)` — `CASE_HELPOPT` inside the builtin's own
  `internal_getopt` loop. `builtin_wants_help` walks the words the way
  `internal_getopt` does, with bash 5.2's optstring for that builtin taken
  verbatim from its `.def`, so it stops where bash stops: `cd -L --help` is the
  help, `cd -- --help` and `cd x --help` are not, `mapfile -d --help` spends the
  word as the delimiter (`d:`), `ulimit -f --help` and `set -o --help` do *not*
  (`;` will not take an option-shaped word), and `type -z --help` never gets
  there because the bad letter ends the scan first. Only the exact spelling
  counts — `--helpx`, `---help`, `-help` are option groups and are refused as
  such.

Two optstrings are built at run time in bash and are spelled out in the table
instead: `set`'s (`flags.c:381` — `'+'`, every shell-flag letter, `o;`) and
`ulimit`'s (`ulimit.def:345` — `aSH` then every limit letter followed by `;`).
`pwd`'s is `LP` upstream; osh follows the build measured against in also taking
`-W`.

Three builtins are answered away from the shared check, because in bash they are
answered away from the shared *route* too:

* **`bind`** runs `internal_getopt` but its switch has no `CASE_HELPOPT` arm
  (`builtins/bind.def`), so `GETOPT_HELP` falls through to the `default:` and is
  answered by `builtin_usage ()` — the one-line synopsis on **stderr**, rc 2,
  and no long doc and no `invalid option` line (the pseudo-option never reached
  `sh_invalidopt`). It is `HelpOpt::None` in the table, with the test written
  inside `Shell::builtin_bind`'s own word loop so the `line editing not enabled`
  warning still comes out in front of it.
* **`command`** and **`builtin`** never enter `run_builtin_body` at all: osh
  dispatches them at exec level (`Shell::exec_command_builtin`,
  `Shell::exec_builtin_builtin`), so the check is written into each one's own
  option loop — a scan for `command` (`command -v --help` is the help,
  `command -- --help` is a command named `--help`), the first word only for
  `builtin`. Both share the text through `Shell::write_builtin_help`, which is
  the half of `builtin_answer_help` without `note_builtin_usage_error`: neither
  is a POSIX special builtin, so the usage-error class has nothing to do there.
  `command cd --help` and `builtin cd --help` still reach `cd`'s own answer,
  since the word is passed on rather than consumed.

The status is `EX_USAGE`, reported through `note_builtin_usage_error`, so the
posix special-builtin rule sees it: `bash --posix -c 'eval --help; echo after'`
prints the help and never reaches the `echo`, and so does osh.

Corpus:
`a-builtin-answers---help-from-the-shared-option-scanner-not-its-own.sh`; tests
`a_builtin_answers_its_own_help_option`,
`the_help_option_is_only_seen_while_options_are_still_being_scanned`,
`some_builtins_take_the_help_request_only_as_their_first_word`,
`a_few_builtins_treat_the_help_option_as_an_ordinary_word`,
`the_two_wrapper_builtins_answer_the_help_request_themselves`,
`bind_answers_the_help_request_with_its_usage_and_nothing_else`, plus
`every_builtin_has_a_help_topic` — the third-list check that would catch a
builtin with no `HELP_TABLE` entry to answer from.
