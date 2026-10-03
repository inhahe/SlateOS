## B-LS-DIFF-HAS-BEEN-SKIPPING (lane B, 2026-08-30) — **FIXED 2026-08-30**

**In short:** `scripts/ls-diff.sh` is the harness that certifies our `ls`
against GNU's. It builds its own reference from the GNU tarball, and the
`make` command it used was wrong in a way that could never succeed. So on
every run it printed "coreutils 9.5 did not build; SKIPPED — a C compiler and
make are needed", **on a host that has both**, and exited 0. Our `ls` has been
uncertified for as long as that line existed. Found while generalising the
build block into `diff-wsl.sh` for an unrelated reason.

### The bug

`ls-diff.sh` ran, from §366 onward:

```sh
./configure --quiet --disable-nls \
  && make -s -j"$(nproc)" src/ls
```

Automake's generated `all` rule is

```make
all: $(BUILT_SOURCES)
	$(MAKE) $(AM_MAKEFLAGS) all-am
```

— that is, `BUILT_SOURCES` are a prerequisite of **the default target only**.
Naming `src/ls` on the command line skips them. In coreutils the built sources
are gnulib's replacement headers: `lib/fcntl.h`, `lib/stdckdint.h`,
`lib/wchar.h`, `lib/stdio.h` and some forty more, generated at build time from
`.in.h` templates by rules in `lib/Makefile.am`. Without them gnulib compiles
against the system headers it exists to shield itself from:

```text
lib/binary-io.h:54:10: error: 'O_BINARY' undeclared
lib/backupfile.c:28:10: fatal error: stdckdint.h: No such file or directory
lib/openat-proc.c:82:17: error: 'O_SEARCH' undeclared
lib/mcel.h:184:13: error: unknown type name 'wint_t'
```

Not one of those messages says "you named the wrong target". They read exactly
like a host with a broken libc, which is what the harness's own diagnostic
then concluded and reported.

### Why it went unnoticed

Three layers each hid a bit of it:

1. **The harness exits 0 on a skip**, which is the right policy — a machine
   with no compiler should not fail the suite — but makes a permanent skip
   indistinguishable from a transient one at the exit-status level.
2. **`all-diff.sh` prints each harness's last line**, and the last line here is
   the parenthetical hint `(a C compiler and make are needed; or set
   GNU=/path/to/a/9.5/ls)`, not a count. In a column of "N passed, 0 differed"
   rows it reads as a note rather than an absence. (`all-diff.sh` *did* score
   it red — no `0 differed` in it — so the aggregate exit status was right;
   what failed was the human scan.)
3. **The diagnostic blamed the host.** "A C compiler and make are needed" sends
   the reader to check for gcc, find it, and shrug.

### The fix

`design-decisions.md` §726 moved the build into `diff-wsl.sh` as
`DIFF_GNU_SOURCE`, and it builds the whole package (`make -s -jN`, ~90 s at
`-j8`) rather than one target. The full build is now the *correct* thing as
well as the shared thing, and the header comment above the block records why,
so the economy cannot be reintroduced by someone who sees only a slow build.

Two further hardenings came out of the same investigation:

- **A failed build is wiped.** A tree left half-built by the broken `make
  src/ls` does not recover when a correct `make` is run over it: the stale
  objects were compiled against the wrong headers and the link fails with
  `undefined reference to rpl_mbrtoc32`, a third message naming neither cause.
  Every failure path now `rm -rf`s the tree.
- **Completion is marked, not inferred.** The cache is considered built when a
  `.slateos-built` marker exists, written only after a whole `make` returned 0
  — because "the binary exists" is exactly what a half-built tree can also be
  true of.

### The general lesson

This is the second harness-level failure in a week whose symptom was *silence*
rather than a wrong answer — see `B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT`
for the first, where the bug reported success. A differential harness's whole
value is that it fails loudly; a skip path is the one branch where it does not,
and it therefore deserves the same suspicion as an `unwrap`.

Worth doing, and not yet done: make `all-diff.sh` report a *skipped* harness as
its own state rather than by whatever its last line happened to be, so a
permanent skip is visible as a skip. Tracked in `todo.txt`.
