## 726. The differential harnesses build their own GNU reference, because the installed one is Debian-patched — §700's trigger has fired

**Date:** 2026-08-30
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** Every `scripts/*-diff.sh` harness proves our version of a utility
behaves like GNU's by running both and comparing the output. The "GNU" it ran
was whatever the machine had installed — and that binary is not GNU's. Ubuntu
ships `coreutils 9.4-3ubuntu6.1`, which carries Debian and Ubuntu patches that
change what some of these programs *do*. A second such patch has now been
found, which is the condition §700 named for changing the arrangement. So
`diff-wsl.sh` can now download the GNU release tarball, build it, and compare
against that instead. It is opt-in per harness; `ls` is converted with this
change and the rest follow one at a time.

### Context — the two known patches

§700 found the first, in `df`: Debian and Ubuntu add `devtmpfs` and `squashfs`
to gnulib's list of "dummy" file system types, so the installed `df` hides the
`/dev` row that upstream prints, on 37 cases. That section decided the *policy*
— our source follows upstream, and the harness subtracts the divergence from
both sides rather than importing the patch — and explicitly deferred the
*mechanism*:

> **Compare against a locally-built pristine coreutils 9.4 instead.** Genuinely
> correct, and rejected only on cost/benefit … If a *second* distribution patch
> is ever found, this becomes the right answer and should be revisited for all
> the harnesses at once.

The second turned up on 2026-08-30, in `cp -n`, while implementing the
overwrite-policy options. The measurement and the source disagreed:

| | `cp -n a b`, `b` exists |
|---|---|
| upstream 9.4 (`cp.c:1070` → `I_ALWAYS_NO`, `copy.c:2437`) | `cp: not replacing 'b'`, exit **1** |
| installed `9.4-3ubuntu6.1` | silent skip, exit **0**, plus a warning on stderr |

The warning is `cp: warning: behavior of -n is non-portable and may change in
future; use --update=none instead`, and it is emitted at option-parse time —
`cp -n a newfile`, where nothing is overwritten, prints it too.

Establishing which side was upstream took the same shape of elimination §700
needed. `curl`ing `cp.c` at tags `v9.4`, `v9.5`, `v9.6` and `master` found the
string in none of them (and confirmed the tag pin works: 1290, 1307 and 1336
lines respectively). The full 9.4 tarball has it in no file at all. `strings
/usr/bin/cp` has it. And then the changelog names it outright:

```text
coreutils (9.4-3) unstable; urgency=low
  * revert cp -n behavior to debian 12 & prior (Closes: #1058752)
  * add deprecation/compatibility warning for above
```

The same changelog names four more behavioural patches whose blast radius
includes a harness here — `uname -i -p` (no harness yet), `tail` on sysfs
files, `split`'s CVE-2024-0684 fix, `ls -l` on NFS — so two is a lower bound
on how often this will happen, not a total.

### Decision

1. **`diff-wsl.sh` grows `DIFF_GNU_SOURCE=<version>`.** A harness that sets it
   is compared against coreutils of that version, fetched from `ftp.gnu.org`,
   configured once and `make`d one binary at a time into a cache under `$HOME`.
2. **Opt-in, one harness at a time.** Not a flag day. Converting a harness may
   surface real differences — that is the point — and a change that flipped 50
   harnesses' oracle at once would mix an infrastructure change with an
   unknown number of behavioural findings in one commit.
3. **Skip rather than fall back.** No compiler, no network, no tarball ⇒ the
   harness exits 0 saying it was skipped. It never quietly reverts to the
   installed binary, which is the failure the whole knob exists to prevent and
   which would look green.
4. **A version mismatch is fatal, not a skip.** Unlike a missing compiler it
   cannot be an accident of the host: something handed us the wrong binary, and
   running on would report the version gap as differences in *our* program.
5. **`ls-diff.sh` becomes a caller rather than the owner.** It had been
   building 9.5 from source since §366, for the unrelated reason that 9.4 and
   9.5 lay columns out differently. That block *is* the mechanism §700 wanted,
   and it moves into `diff-wsl.sh` unchanged in substance.

### Rationale

The cost §700 weighed the alternative against had already been paid. Its two
objections were "it means building coreutils inside the WSL environment as a
harness prerequisite" and "one harness with a different reference is a trap for
whoever reads two of them". The first was already true — `ls-diff.sh` had been
doing it for weeks, and the fetch/configure/make/cache/verify sequence was
written, debugged and working. The second was *inverted*: with `ls` on a built
9.5 and everything else on the installed 9.4, the odd harness out existed
already, and the trap was live. Generalising removes it in both directions.

There is also the argument `diff-wsl.sh`'s own header makes about
`DIFF_NO_BINDIR`: "Two copies of one judgement is one copy that is wrong, and
the wrong one is the one nobody rereads." A download-and-build sequence living
inside one of fifty harnesses is exactly that shape. `df-diff.sh` met a
distribution patch and solved it locally; `cp-diff.sh` met one and had to
rediscover the whole category from scratch, because nothing outside
`df-diff.sh` and §700 recorded that the category existed.

And the reason to prefer upstream at all is §700's, unchanged: the reference
binary is a *measurement instrument*, valuable exactly insofar as it settles
arguments about what GNU does. An instrument with an undocumented offset does
not settle anything — it silently moves the answer. Ubuntu's `cp -n` is a
reasonable choice *for Ubuntu* (it preserves what Debian users had before 9.3);
it is not what `cp.c` says, and `cp.c` is what this transcription is a
transcription of.

### Alternatives considered

- **Keep the installed binary and xfail each patched case, as `df` does.**
  This is what §700 chose and it worked, but it scales by discovery: each
  patch costs an investigation like the two above before anyone knows there is
  something to xfail, and the failure mode of *not* discovering one is a case
  that certifies the patch as correct behaviour. `cp -n` would have been
  written to match Ubuntu, in a file whose comments cite `copy.c`.
- **Vendor the coreutils source into the repository.** Rejected: 5.9 MB of C
  per version in a tree that does not build it, and a second thing to keep in
  step with the tarball. The download is cached and needed once per machine.
- **Build only the binaries a harness names (`make src/cp`).** Tried first, on
  the assumption that a whole build was the expensive option; it does not
  work, and the way it fails is the finding of the day. Automake makes
  `BUILT_SOURCES` a prerequisite of the **default target only**, and in
  coreutils the built sources are gnulib's replacement headers — `lib/fcntl.h`,
  `lib/stdckdint.h`, `lib/wchar.h` and forty more. Naming a target directly
  compiles gnulib against the system headers it exists to shield itself from,
  and it dies on `O_BINARY undeclared`, `stdckdint.h: No such file or
  directory`, `O_SEARCH undeclared`, `unknown type name 'wint_t'` — none of
  which says "you named the wrong target"; they read like a broken host.

  This is exactly what `ls-diff.sh` had been doing since §366, so on this
  machine it had been **skipping every run**, reporting "a C compiler and make
  are needed" on a host with both, and our `ls` was uncertified for as long as
  that line existed. `known-issues.md`, `B-LS-DIFF-HAS-BEEN-SKIPPING`.

  So the whole package is built, once per version per machine, and the
  economy is documented as a trap above the code rather than merely absent
  from it. It costs about 90 seconds at `-j8`, after which every harness on
  that version pays nothing. A related hardening came from the same
  investigation: a half-built tree does **not** recover when the correct
  `make` is run over it — the stale objects were compiled against the wrong
  headers and the link fails with `undefined reference to rpl_mbrtoc32` — so
  a failed build is wiped, and completion is recorded by a marker file
  written after `make` returns 0 rather than inferred from a binary existing.
- **Convert every harness at once, as §700's wording suggests.** Rejected in
  favour of "revisit for all the harnesses" meaning the *decision*, not the
  edit. See point 2 above.

### What it does not settle

`cp -n`'s own behaviour is now unambiguous — upstream's `not replacing`, exit
1 — but only once `cp-diff.sh` is converted, which is a separate change.
Nothing here changes any subject program.

**And the built reference is upstream's *source*, not upstream's *build*.**
`configure` on this machine reports three features off, because their
development headers are not installed and `sudo` here needs a password:

```text
configure: WARNING: GNU coreutils will be built without ACL support.
configure: WARNING: GNU coreutils will be built without xattr support.
configure: WARNING: GNU coreutils will be built without capability support.
```

Ubuntu's binary has all three. So a case that turns on an access control list,
an extended attribute or a file capability is comparing against a GNU
configured differently from the one a user would have — the `+` after `ls -l`'s
mode string, `cp --preserve=xattr`, and `--color`'s `ca` slot are the three
places it could show. None of them is reachable today: no harness fixture
creates an ACL, an xattr or a capability, and no subject program here
implements any of the three. It becomes reachable the moment one does, and the
fix is `libacl1-dev libattr1-dev libcap-dev`, which needs the operator.
Recorded in `known-issues.md` under the oracle entry so that it is found by
whoever writes the first such case rather than by whoever debugs it.

### Where it lives

- `scripts/diff-wsl.sh` — the `DIFF_GNU_SOURCE` / `DIFF_GNU_DIR` /
  `DIFF_GNU_CACHE` knobs, the fetch-and-build block in section 3,
  `diff_gnu_verify`, and the `gnu_dir` arms in the reference resolution and in
  the multi-binary `bindir` loop. The header section "Why a built reference"
  carries the short version of this entry.
- `scripts/ls-diff.sh` — now `DIFF_GNU_SOURCE=9.5` and nothing else; it no
  longer sets `DIFF_NO_REF` or `DIFF_NO_BINDIR`, and its `GNU` and
  `LS_DIFF_CACHE` knobs are replaced by the shared `DIFF_GNU_DIR` and
  `DIFF_GNU_CACHE`.
- `known-issues.md`, `B-THE-DIFFERENTIAL-ORACLE-IS-DEBIAN-PATCHED` — the
  inventory of patches and which harness each one can reach.

### How to reverse

Delete `DIFF_GNU_SOURCE` from a harness and it is back on the installed
binary immediately; the knob has no other effect. Reversing it for `ls` would
mean restoring its own build block from before this commit, since `ls` cannot
run against 9.4 at all.
