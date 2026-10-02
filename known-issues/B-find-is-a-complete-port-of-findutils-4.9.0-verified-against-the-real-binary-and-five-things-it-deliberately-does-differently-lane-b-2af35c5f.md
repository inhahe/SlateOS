## `find` is a complete port of findutils 4.9.0, verified against the real binary — and five things it deliberately does differently (lane B, 2026-08-23)

### In short

`find` used to be a small stub. It is now a 6200-line reimplementation of
the whole findutils 4.9.0 interface — every primary, every action, every
operator, the expression tree and the walk — and there is a harness,
`scripts/find-diff.sh`, that runs 448 command lines through *our* `find` and
through *GNU's* `find` inside WSL and compares stdout, stderr and the exit
status byte for byte. It currently reports **443 passed, 0 differed, 5
differ on purpose**. This entry records what those five are, why each is
deliberate rather than unfinished, and what debt is left behind.

### The harness, and why the control run matters

    MSYS2_ARG_CONV_EXCL='*' wsl -e env FIND_DIFF_INNER=1 LC_ALL=C.UTF-8 TZ=UTC \
        bash ./scripts/find-diff.sh

It runs inside WSL because half of `find`'s vocabulary asks questions only a
real inode can answer — `-inum`, `-links`, `-samefile`, `-perm`, `-user`,
`-type l`, `-xtype`, `-fstype`, `%i`, `%n`, `%b` — and the fixture tree is
built in WSL's own `/tmp` rather than under `/mnt/d`, because on 9p a
directory's link count is synthesised, hard links do not share an inode, and
`st_blocks` is invented. A harness on 9p would be comparing two
implementations against fiction.

`OURS=/usr/bin/find sh scripts/find-diff.sh` is the control: GNU against
itself. It reports 443 passed, 0 differed, 4 unexpectedly agreed — the four
being exactly the cases we mark deliberate, which of course agree when both
sides are GNU. That run is what makes the 443 passes mean something: without
it, a case that agrees because it exercises nothing would look identical to
one that agrees because both implementations got it right.

### The five deliberate differences

| Case | Why |
|---|---|
| `find --version` | Answers "which program is this". Copying GNU's banner is a false attribution. |
| `find --help` | Its bug-report block sends reports to us, not to `bug-findutils@gnu.org`. |
| `find t -name f -printf '%Z\n'` | SELinux context. We render it empty; GNU asks a kernel that has no policy loaded. |
| `find t -mtime nan` | GNU reaches `assert (! isnan (...))` in `get_comp_type` and dumps core; we diagnose the argument. Unusable as a straight comparison anyway — the shell prints the crashed child's *pid*, so the two sides could not agree even against themselves. |
| `find t -regex '.*'` | Byte-based regex versus glibc's multibyte matcher; see below. |

The first two are the interesting ones, because the harness surfaced them
only *incidentally*: they showed up as cases that **agreed**, which is to
say our binary was claiming to be GNU findutils by GNU's authors and
directing its bug reports at GNU's tracker. A differential harness normally
finds bugs by disagreement; these two were bugs *of* agreement, and they are
the reason the harness counts "unexpectedly agreed" as a distinct outcome
rather than folding it into "passed".

### The regex difference is a consequence of a decision already taken

The fixture tree contains a file called `t/n\377ame`, whose name is not
valid UTF-8. GNU compiles `.*` with glibc's multibyte matcher, which cannot
decode `\377` in a UTF-8 locale and therefore declines to match the name at
all — `find -regex '.*'` silently skips the file. Ours is byte-based and
matches it.

Ours is the deliberate answer, per `design-decisions.md` §322: a path on
this system is a byte string with no encoding attached, so a pattern meaning
"any sequence of characters" has to mean "any sequence of bytes" or it stops
being able to name some of the files that exist. Every *other* `-regex` case
in the harness agrees exactly, because every other one is over a name that
decodes — the difference is confined to the one input where the two models
of "character" cannot both be right.

### What the harness found in the shared `ere` crate, not in `find`

Driving the last few cases to zero turned up a defect that was never
`find`'s: the ERE parser had been written from the grammar rather than from
a measurement, and rejected a dozen patterns glibc accepts — an empty
pattern, a stacked quantifier (`a**`, `a{1}{2}`), an unopened `)`, `{,3}`,
an empty alternation branch, `a{0}`. Eight binaries share that crate, so
every one of them refused patterns GNU runs. Fixed in `8aca183de`, with
`parse_brace` rewritten as a transcription of glibc's `parse_dup_op` rather
than its own reading, because the interval errors divide on a line no
grammar suggests — whether a `}` ever turned up, not whether the content was
sane.

That is the argument for pointing a differential harness at a *port* even
when the port looks finished: the bug was two layers below the thing under
test, and no amount of reading `find.rs` would have found it.
