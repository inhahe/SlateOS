## 744. The differential reference's own dependencies are built from source into the cache, rather than the comparison being given up

**Date:** 2026-09-01
**Decided by:** Claude (autonomous)
**Lane:** B

**In short:** the two harnesses that certify our `cp` and `mv` do it by running
the real GNU ones side by side and comparing everything both leave behind. The
GNU ones are not the machine's installed copies — they are built from the
official 9.4 tarball, for reasons in §726. It turned out that a program built
that way is only as capable as the *libraries present on the build machine*, and
this machine was missing one: with no libattr, GNU's `cp` is compiled with its
whole extended-attribute routine replaced by "succeed, do nothing". So the first
five cases written about extended attributes came back saying we were wrong,
when what had happened is that the thing we were being compared against could
not do it. The decision is that when the reference is missing a dependency, the
harness **builds that dependency too**, into the same cache, rather than
recording the comparison as impossible.

### What actually happens

`scripts/diff-wsl.sh` tries the system's libattr first, so a machine with the
development package installed pays nothing. Failing that it fetches attr 2.5.2
from Savannah, configures it `--prefix` into
`~/.cache/slateos-diff-gnu/attr-prefix`, and passes `-I…/include` and
`-L…/lib -Wl,-rpath,…/lib` to coreutils' `configure` as `CPPFLAGS`/`LDFLAGS`.
Failure at any step is never fatal: the reference is then built as before, and
the harnesses say so in their header and skip the cases that need it.

Two details that are not incidental:

* **`-Wl,-rpath` and not `LD_LIBRARY_PATH`.** The reference binaries are run
  through symlinks in a throwaway `PATH` directory, by two harnesses, in an
  environment each case controls. A library path baked into the binary cannot be
  lost by any of that — and, more to the point, cannot leak into the *subject's*
  environment, where it would let our binaries silently acquire a library the
  real target does not have.
* **What was achieved is read back from the built tree's `config.h`,** not
  inferred from the flags that were passed. `gl_FUNC_XATTR` can decide it cannot
  use libattr and only `AC_MSG_WARN` about it, so a tree configured with the
  flags can still hold `/* #undef USE_XATTR */`. Intent is not evidence.

### The alternatives, and why each is worse

**Accept the degraded reference and mark the cases `xfail` forever.** This is
what was in the tree that morning: three `xfail_case`s in `cp-diff.sh` reading
"the built reference has USE_XATTR undefined and refuses the word", and a
comment stating the situation "will not turn green by anything done here". It is
honest, and it is cheap, and it is also how a certification suite quietly stops
certifying: an `xfail` is a case that has been promised never to fail. The three
were promoted the same day the dependency was built, and the harness reported
eight new passing cases — eight comparisons that had been declared impossible.

**Ask the operator to install `libattr-dev`.** One `apt install`, and then the
harness works on exactly one machine. Every other machine — a fresh checkout, a
different account's WSL, whatever runs this in a year — is back to the degraded
reference, and back to reporting our correct behaviour as a difference in us,
which is the worst direction for a false result to point. The cache build is
strictly more portable than the instruction to install, and it costs seconds.

**Compare against the *system's* `/usr/bin/cp`, which does have xattr support.**
This trades one wrong reference for another and is the thing §726 exists to
refuse: Ubuntu carries `debian/patches/cp-n.diff`, so certifying against the
installed binary certifies us into Debian's behaviour and away from the
specification, while looking green.

### What is genuinely given up

A network fetch and a compile, once per cache, for a library the harness does
not otherwise need — and a second upstream tarball whose checksum nobody here
verifies, on top of the coreutils one that already had that property. That is a
real cost and it is the reason this is written down rather than assumed: the
harness now builds two third-party projects to certify one of ours, and each one
added is a thing that can fail on a machine that is not this one. The mitigation
is that every step is non-fatal and the header says out loud what was and was
not compared, so the failure mode is a *skipped* section rather than a wrong
verdict.

### Where this goes next

The same host also has no libacl, so the reference's `copy_acl` is likewise
reduced — and our `cp` does carry POSIX ACLs, storing them exactly as Linux does
in `system.posix_acl_access`. Since that is an ordinary extended attribute, the
comparison added today would already see a difference; what is missing is a case
that seeds an ACL, and a reference that could match it. The precedent set here
says to build attr's sibling `acl` the same way rather than to write another
permanent `xfail`.
