### TD-OILS-HELP-HAS-NO-LONG-DESCRIPTIONS. `help cd` prints one line where bash prints thirty — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `HELP_TABLE` (three columns: name,
synopsis, short description) and the `help` builtin that formats it.

**What.** bash's long help is the synopsis line, the short description, a blank
line indented to four spaces, then a multi-paragraph body ending in an
`Exit Status:` stanza. osh stops after the short description:

```text
$ help declare
bash: declare: declare [-aAfFgiIlnrtux] …
      ····Set variable values and attributes.
      ····
      ····Declare variables and give them attributes.  If no NAMEs are given,
      ····display the attributes and values of all variables.
      …
      ····Exit Status:
      ····Returns success unless an invalid option is supplied or a variable
      ····assignment error occurs.
osh : declare: declare [-aAfFgiIlnrtux] …
      ····Set variable values and attributes.
```

`help -m NAME` reformats the same body under `NAME`/`SYNOPSIS`/`DESCRIPTION`
headings, so it is short by the same text.

(The patternless listing also differs in layout, but that is a separate and
deliberate divergence — see TD-OILS-HELP-LAYOUT.)

**Fixed.** A companion table `HELP_BODIES` holds the body of all **75** topics
(61 builtins + 14 reserved-word topics), and `builtin_help` prints synopsis /
short description / four-space separator / body. `help -m` is now a real man
page rather than an alias for the long form, and the three flags take bash's
precedence: `-d` beats `-m` and `-s`, `-m` beats `-s`.

Two things the shape of the fix turned on:

* The bodies are stored **with** bash's four-space indentation and trailing
  newline instead of being indented at print time. bash's paragraph separators
  are four spaces and nothing else, but `help variables` ends with a
  *genuinely empty* line, and no re-indentation rule produces both. (The cause
  is somewhere in `show_longdoc`/`mkbuiltins`' handling of that topic's
  `$DOCNAME variable_help`; it was not worth chasing, since storing what bash
  prints answers it either way.)
* The text is hard-wrapped in `builtins/*.def`, not at print time, so it does
  not depend on `COLUMNS` — verified with `COLUMNS=40` vs `COLUMNS=200`. That
  is what makes capturing it from the reference shell exact.

All 75 topics are byte-identical to bash in the long form. `help -m` is
byte-identical too **except** two identity lines: `SEE ALSO` says `osh(1)` and
`IMPLEMENTATION` says osh's version rather than bash's version + FSF copyright.
That follows the standing rule that osh reports its own identity wherever it
reports one (`--version`, `$BASH_VERSION`, the patternless `help` banner), and
is why the corpus case walks the long form rather than `-m`.

The unit test `help_table_and_bodies_agree` keeps the two tables from drifting
(same 75 names, every body indented and newline-terminated), and the corpus case
`help-says-of-each-builtin-what-bash-says-of-it.sh` walks the long form over
every topic. Generator used for the capture: `/d/tmp/hh/genhelp.py`. Reference:
`D:\refsrc\bash-5.2\builtins\help.def` (`show_longdoc`, `show_manpage`).

The patternless-listing layout noted above is *not* touched by this: it is a
separate, deliberate divergence already recorded as TD-OILS-HELP-LAYOUT
(INTENTIONAL / documented, 2026-07-20) — though the one part of it that was
information loss rather than layout, bash's star note and the per-name
disabled markers, has since been recovered there.
