## TD-B-DIFF-PATCH-HOSTNAME-PARSE-ARGV-BY-EXACT-MATCH (lane B, 2026-09-25) — FIXED 2026-09-25

**In short:** `diff`, `patch` and `hostname` compare each argument against a
list of exact spellings instead of going through `coreutils::getopt`, so they
miss what every glibc-getopt program does. A long option abbreviated to a
unique prefix is refused -- GNU `diff --unif o n` prints a unified diff, ours
says `unrecognized option '--unif'` -- and `POSIXLY_CORRECT` changes nothing,
where GNU `diff o n -u` says `extra operand '-u'` and net-tools `hostname x -V`
prints its usage. `patch` has no `--` either.

**The fix** is upstream's command line on `Program::parse`: diffutils'
`shortopts` and `longopts` for `diff`, GNU patch's for `patch`, net-tools' for
`hostname`, each in upstream's declaration order, which the ambiguity message
makes observable. That brings abbreviations, bundling, `--` and
`POSIXLY_CORRECT` at once, and routes the argv bytes through a parser that
never decodes them -- the conversion this file already asks for under "The fix
is getopt, not a hand conversion". A `POSIXLY_CORRECT` rule added to the
existing loops would be the fourth thing each of them re-implements by hand,
which is why the change above did not add one.

**How it was closed (2026-09-25).** All three are on `Program::parse` with
upstream's tables in declaration order, and each refuses by name the options
upstream has and it does not, instead of calling them invalid.

- **`diff`**, diffutils 3.10's table and `main`: two different output styles
  are `conflicting output style options` (the ladder kept the last); repeated
  context lengths keep the largest, and `-u`/`-c` ask for three; the obsolete
  `-NUM` digits accumulate across words (`-1 -2` is twelve) and reconcile with
  `-C`/`-U` by upstream's rule; `--color` takes `never`, `always` or `auto`
  exactly and colours a terminal under `auto`; `-d`, `-h`, `-H`,
  `--horizon-lines`, `--inhibit-hunk-merge` and `--binary` are accepted, since
  this build already has their effect. The ladder's own `--no-color`, which
  diffutils never had, is gone. The missing-operand error names the last word
  after getopt's permutation, so `diff x -u` is `after 'x'`. An `-I` pattern
  that does not compile says glibc's sentence for it rather than one fixed
  phrase, and no longer goes through `from_utf8_lossy`. `diff-diff.sh`: 200
  passed, 0 differed.
- **`patch`**: see the entry above. 113 passed, 0 differed.
- **`hostname`**, net-tools 3.23's table: `-?` is help, `--long` is `-f`,
  `--yp` and `--nis` are `-y`, abbreviations resolve. `hostname-diff.sh` went
  from 21 passed / 39 differed to 26 / 34: the five abbreviation rows. What is
  left red is the environment (addresses, `-a`/`-A`, which need resolution this
  system cannot yet answer) and net-tools' way of refusing -- usage on stdout
  and exit 255 -- which this `hostname` does not copy.
