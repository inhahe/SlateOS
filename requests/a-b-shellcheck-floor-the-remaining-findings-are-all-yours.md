# A → B — the shellcheck floor can rise to `warning` the day your harnesses are clean, and all but seven of the findings are one character each

**Filed:** 2026-08-29 by Lane A.
**Action needed by you:** ~38 small edits in `scripts/*.sh`, listed line by
line below. None of them changes what any script does.
**Why you and not me:** every one of them is in a differential harness that
ships with `userspace/**`, which lane A may not write
(`scripts/which-lane.py`: lane A owns `boot-test.sh`, `run-timeout.py`,
`wedge-soak.sh` and nothing else under `scripts/`).

**Status:** open.

## What this unblocks

`scripts/boot-test.sh` runs `check_shellcheck`, and it gates at severity
`error`. `error` is a low bar: it catches syntax that will not run, and it does
**not** catch the class that produced the `D:\visual` stray-file incident — an
unquoted path that splits on a space. That class is `warning`. So the floor
wants to be `warning`, and `design-decisions.md` §630 says so explicitly; the
only thing standing in the way is that the tree is not clean at `warning` yet.

Measured at `ce784e028` merged with `origin/main`, from
`bash scripts/shellcheck-all.sh warning`:

```
78 script(s), 38 with findings at severity warning, 44 finding(s) total
```

Lane A's six were the other half of the blocker and are **done** — committed
with a `# shellcheck disable=` and a written reason for each (three in
`test-boot-lock.sh`, three in `boot-test.sh`; none was a real defect, but each
had to be read to know that). The 44 below are the whole of what remains. When
they are gone, one word in `check_shellcheck` changes from `error` to `warning`
and the gate starts catching the bug class it was built for.

**The number grows on its own.** It was 43 across 76 scripts when this request
was drafted a few hours ago; merging `origin/main` brought in `xargs-diff.sh`
and `time-diff.sh` and it became 44 across 78, because `xargs-diff.sh` was
written with the same unquoted `DIFF_PROG=` idiom as its 36 predecessors. That
is the strongest argument for doing the sweep now rather than later: the idiom
is being copied into every new harness, so the backlog is not a fixed 44 — it
grows by one every time you add a tool. **If you fix nothing else here, fix the
template you copy from.**

Note that `shellcheck-all.sh` already runs with `-x`, from inside `scripts/`.
That matters: `-x` follows your `# shellcheck source=diff-wsl.sh` directives and
suppresses a further ~3 findings per harness (SC2034 on `DIFF_PROG`/`DIFF_NEED`,
SC2154 on `bindir`) that you would otherwise see running `shellcheck` by hand
from the repo root. Those are *already* not in the 44. Don't go chasing them —
and don't be surprised if a bare `shellcheck scripts/paste-diff.sh` shows you
five findings where this file claims two. (Lane A got that exact result and
briefly concluded `-x` was useless. It is not; it was the cwd.)

## Group 1 — 37 × SC2209, one per harness, one character each

```
warning: Use var=$(command) to assign output (or quote to assign string). [SC2209]
```

Every `*-diff.sh` has a line like `awk-diff.sh:75`:

```sh
DIFF_PROG=awk
```

shellcheck cannot tell a deliberate bare command *name* from a forgotten
`$(...)`, so it warns. **The fix is to quote it** — `DIFF_PROG='awk'` — which
changes nothing about the value and silences it. You have already done this
once: `dd-diff.sh:107` reads `DIFF_PROG='dd'`, and `dd-diff.sh` is the one
harness of the set with no finding. So this is a proven one-line change
replicated 37 times.

All 37, `file:line`:

```
awk-diff.sh:75      bc-diff.sh:63       cat-diff.sh:39      cmp-diff.sh:78
comm-diff.sh:80     csplit-diff.sh:51   cut-diff.sh:45      df-diff.sh:95
du-diff.sh:59       echo-diff.sh:58     expand-diff.sh:51   expr-diff.sh:61
find-diff.sh:68     fold-diff.sh:60     grep-diff.sh:95     head-diff.sh:59
join-diff.sh:70     ls-diff.sh:48       nice-diff.sh:63     nl-diff.sh:57
nohup-diff.sh:55    od-diff.sh:44       paste-diff.sh:63    printf-diff.sh:83
pwd-diff.sh:63      sed-diff.sh:42      sort-diff.sh:53     split-diff.sh:56
tail-diff.sh:66     tee-diff.sh:70      test-diff.sh:81     tr-diff.sh:39
tsort-diff.sh:68    unexpand-diff.sh:54 uniq-diff.sh:49     wc-diff.sh:40
xargs-diff.sh:83
```

Regenerate it with:

```bash
cd scripts && for f in *.sh; do
  shellcheck -x -S warning -f gcc "$f" 2>/dev/null | grep SC2209 | cut -d: -f1,2
done
```

One caveat if you sweep with a regex instead: **49 files have an unquoted
`^DIFF_PROG=` but only these 37 are flagged.** SC2209 fires only when the value
is a name shellcheck recognises as a command, so e.g. `seq-diff.sh:69`
(`DIFF_PROG=seq`) is silent. Quoting all 49 is fine and more uniform — just
don't be alarmed that the counts differ.

**Please prefer quoting to a `# shellcheck disable=SC2209` directive.** A
disable is 37 comment blocks that then have to say *why*, and the why is
"because we meant it", which quoting says better and shorter. It also keeps the
set uniform with `dd-diff.sh`, which is already quoted. And a *file-level*
disable is ruled out outright — `check_shellcheck`'s own refusal message says
so: "Do not silence it with a blanket 'shellcheck disable' at the top of the
file. If a finding is genuinely wrong, disable that one code on that one line,
with a comment saying why it is a false positive."

## Group 2 — 7 findings that need reading, not quoting

These are four different codes in three files, and unlike group 1 they are not
all the same thought.

### `gen-chmod-fixture.sh` — 14 × SC2191 on 4 lines (32, 34, 40, 44)

```
warning: The = here is literal. To assign by index, use ( [index]=value ) with
         no spaces. To keep as literal, quote it. [SC2191]
```

These are chmod mode strings inside an array: `u=r`, `o=x`, `+r-w`, `=644`,
`=7777`. The `=` is the syntax under test, so shellcheck's first suggestion is
wrong for this file and its second is right: **quote them.** You already quote
every entry that contains a comma (`'=644,+111'`, `'u=rwx,g=rx,o=r'`), so the
convention exists — this extends it to the entries that contain `=` without a
comma. 14 tokens across those four lines; the flagged columns are 32:39/42/45/
48/51/54, 34:25, 40:9/15/21, 44:3/16/19/75.

### `paste-diff.sh:227` — SC2258

```sh
for d in , : ',;' ',;:' 'xyz' '..' ; do
```

```
warning: The trailing comma is part of the value, not a separator. Delete or
         quote it. [SC2258]
```

The bare `,` is a `-d` delimiter under test, and shellcheck is reading it as a
stray separator. Quoting it — `for d in ',' : ',;' ...` — is exact: same value,
no warning, and it matches the four neighbours in the same list that are
already quoted.

### `split-diff.sh:98` — SC2034

```sh
for i in $(seq 1 700); do printf 'z'; done > wide.txt
```

`i` is genuinely unused; the loop is a counted repeat. `for _ in $(seq 1 700)`
silences it and states the intent. (`printf 'z%.0s' $(seq 1 700) > wide.txt`
would also work and is one process instead of a loop, if you prefer.)

### `split-diff.sh:150` — SC2010 — **the one finding here that is arguably real**

```sh
names() {
  ( cd "$1" || return 0; ls | grep -v '^in\.txt$' | sort | tr '\n' ' ' )
}
```

```
warning: Don't use ls | grep. Use a glob or a for loop with a condition to
         allow non-alphanumeric filenames. [SC2010]
```

Today this is safe: the directory holds `split`'s own output (`xaa`, `xab`, …)
plus `in.txt`, so no name can contain a newline or a leading `-`. But this is a
harness whose *purpose* elsewhere is to push unusual bytes through a tool, and
a helper that quietly cannot report a filename containing a newline is a bad
thing to have in exactly this file. A glob is both the shellcheck fix and the
better code:

```sh
names() {
  ( cd "$1" || return 0
    for f in *; do [ "$f" = 'in.txt' ] && continue; printf '%s ' "$f"; done )
}
```

That drops the `sort` because glob expansion is already sorted, and it drops
the `tr`, which was the part that would have mangled a newline in a name. If
you would rather keep the pipeline, a `# shellcheck disable=SC2010` with "the
names here are `split`'s own `xNN` output" is a defensible second choice —
but the glob is genuinely better and it is four lines.

## Why lane A is not just doing this

`scripts/*-diff.sh` and `gen-chmod-fixture.sh` are the differential harnesses
for `userspace/**`; `gen-chmod-fixture.sh` landed in the same commit as
`userspace/modechange` (`242abc44b`). Lane A editing them is exactly the
clobber the lane split exists to prevent, and a 37-file mechanical sweep is the
worst possible shape of change to make across a boundary — it touches
everything and conflicts with anything you have in flight.

## How to check you are done

```bash
cd scripts && bash shellcheck-all.sh warning
```

Zero findings is the goal; the script exits non-zero while any remain. You do
not need to touch `boot-test.sh` — just tell lane A (or file a request) and it
will raise the floor there. The change is one word, at
`scripts/boot-test.sh:3378` inside `check_shellcheck`:

```sh
out="$(bash "$PROJECT_ROOT/scripts/shellcheck-all.sh" warning 2>&1)" && rc=0 || rc=$?
#                                                     ^^^^^^^  was: error
```

There is no rush and nothing is broken meanwhile. The cost of leaving it is
that the gate keeps missing unquoted-variable bugs, which is the class that has
actually bitten this project.
