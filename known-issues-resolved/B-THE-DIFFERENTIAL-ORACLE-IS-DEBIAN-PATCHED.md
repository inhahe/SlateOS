## B-THE-DIFFERENTIAL-ORACLE-IS-DEBIAN-PATCHED (lane B, 2026-08-30) — FIXED, and an inventory

**In short:** The `scripts/*-diff.sh` harnesses are how this project knows its
utilities behave like GNU's: each runs our program and "GNU's" side by side and
compares. The GNU side was whatever the machine had installed, and that binary
is **not** GNU's — WSL ships Ubuntu's `coreutils 9.4-3ubuntu6.1`, which carries
Debian and Ubuntu patches that change behaviour. Two of those patches are known
to reach a harness here, and the changelog names four more that could. The
fix is `DIFF_GNU_SOURCE` (`design-decisions.md` §726): a harness is compared
against the GNU release tarball, built from source. **Every coreutils harness
is now converted** (2026-08-30), so the GNU side really is GNU's. The entry
stays as the record of what was wrong, of the patch inventory below, and of the
one gap that is left — the built reference is compiled without ACL, xattr and
capability support, which needs the operator to install three `-dev` packages.
It is also the place to record distribution patches as they are found.

### Why this is not a theoretical concern

A harness compares against a patched binary and reports green. That green means
"we match Ubuntu", which is what the harness *says* only if someone remembers
the distinction. Both known instances were found by accident, in the middle of
unrelated work, and in both the first hypothesis was that our program was
wrong:

- **`df`** (2026-08-29, `design-decisions.md` §700). Ubuntu adds `devtmpfs` and
  `squashfs` to gnulib's `ME_DUMMY_0`, so the reference omits `/dev` from every
  whole-table listing and ours prints it — 37 cases. Establishing which side
  was upstream needed `LD_PRELOAD` interposition of `stat` to prove the row was
  gone *before* the stat, plus `strings -a /usr/bin/df`.
- **`cp -n`** (2026-08-30, this entry, `design-decisions.md` §726). Debian
  9.4-3 reverted `-n` to its pre-9.3 meaning and added a deprecation warning:

  | | `cp -n a b`, `b` exists |
  |---|---|
  | upstream 9.4 (`cp.c:1070` → `I_ALWAYS_NO`; `copy.c:2437`) | `cp: not replacing 'b'` on stderr, exit **1** |
  | installed `9.4-3ubuntu6.1` | nothing on stderr but a warning, exit **0**, `b` untouched |

  The warning is `cp: warning: behavior of -n is non-portable and may change in
  future; use --update=none instead`, printed at option-parse time — even
  `cp -n a newfile`, which overwrites nothing, prints it. It appears in no
  upstream file at `v9.4`, `v9.5`, `v9.6` or `master`, nor anywhere in the 9.4
  tarball; `strings /usr/bin/cp` has it. The Debian changelog is explicit:
  `revert cp -n behavior to debian 12 & prior (Closes: #1058752)` and
  `add deprecation/compatibility warning for above`.

  The patched `-n` is `I_ALWAYS_SKIP`, i.e. exactly `--update=none`:
  `cp -n --debug a b` prints `skipped 'b'`, which is `I_ALWAYS_SKIP`'s
  diagnostic and not `I_ALWAYS_NO`'s.

### The inventory — patches named in the changelog, and the harness each reaches

Read from `/usr/share/doc/coreutils/changelog.Debian.gz` on the dev machine.
"Reached?" is whether a harness in `scripts/` exists that could see it, not
whether it has been checked.

| Patch | Effect | Harness | Reached? |
|---|---|---|---|
| `cp-n.diff` (Debian #1058752) | `cp -n` skips silently, exit 0, plus a parse-time warning | `cp-diff.sh` | **yes** — see above; harness converted 2026-08-30 |
| `treat-devtmpfs-and-squashfs-as-dummy-filesystems.patch` | `df`/`du` hide `/dev` and `/snap/*` rows | `df-diff.sh`, `du-diff.sh` | **yes** — §700; both harnesses converted 2026-08-30 |
| `80_fedora_sysinfo.patch` | `uname -i -p` print the real processor rather than `unknown` | none | no harness yet |
| `tail-fix-tailing-sysfs-files-where-PAGE_SIZE-BUFSIZ.patch` | `tail` on sysfs files with a 64 K page | `tail-diff.sh` | **no** — converted 2026-08-30, no count moved; no fixture is on sysfs |
| `CVE-2024-0684.patch` | `split --line-bytes` heap overflow fix (also upstream in 9.5) | `split-diff.sh` | **no** — converted 2026-08-30, no count moved |
| `suppress-permission-denied-errors-on-nfs.patch` | `ls -l` does not report EACCES reading attributes over NFS | `ls-diff.sh` | **no** — that harness builds 9.5 from the tarball |
| `99_float_endian_detection.patch` | floating-point endianness detection at configure time | `printf-diff.sh`, `extfloat-diff.sh` | **no** — `printf-diff.sh` converted 2026-08-30, no count moved; `extfloat-diff.sh` has no coreutils reference at all |

`ls-diff.sh`'s immunity is accidental — it built its own reference since §366
for an unrelated reason (9.4 and 9.5 lay columns out differently). That is
what made the general mechanism cheap enough to adopt.

### What has been done, and what remains

Done: `scripts/diff-wsl.sh` grows `DIFF_GNU_SOURCE=<version>`, which fetches
`coreutils-<version>.tar.xz` from `ftp.gnu.org`, configures it once under
`$HOME/.cache/slateos-diff-gnu`, `make`s only the binaries a harness names, and
verifies `--version` before comparing anything. It skips rather than falling
back if it cannot build, and treats a version mismatch as fatal. `ls-diff.sh`
is converted and its bespoke copy of that sequence is deleted.

Done since: **`cp-diff.sh` → 9.4** (2026-08-30), which unblocked `cp -n` and
then `cp -i`; the conversion moved no count, because every `-n` case was
written after the patch was known and none had been certified against it.
**`df-diff.sh` and `du-diff.sh` → 9.4** (2026-08-30), which retired §700's
`drop_hidden` subtraction, its `=` exemption marker and its
`!Ubuntu hides devtmpfs …` xfail outright — the pristine `df` prints the `/dev`
row, so there was nothing to subtract. `df-diff.sh` gained three
`--output` forms that the subtraction had made uncomparable (`--output=source`
among them, which had been the xfail), and now runs 177 cases with 174 passing
and 3 xfails, none of them about the distribution. `du-diff.sh` moved no count
at all: 188 pass, 5 xfail, exactly as before, which is the outcome the note
below predicts for most of the remaining conversions and is still worth having.

Done since, and this is the last of it: **the remaining 37 coreutils harnesses
→ 9.4** (2026-08-30), in one sweep. Every coreutils harness in `scripts/` now
compares against the built tarball, so the gap this entry describes is closed;
it is left here as the record of it rather than as live work.

The three that carried a named hypothesis from the table above were the point
of the sweep, and all three came back a no-op — which is a result, not a
non-result. Each had been an unchecked "probably":

| Harness | Hypothesis | Measured |
|---|---|---|
| `tail-diff.sh` | sysfs patch unreachable: no fixture is on sysfs | 218 pass, 2 xfail — unchanged |
| `split-diff.sh` | CVE-2024-0684 makes the *installed* binary the more correct one, so a difference here would be legitimate | 207 pass, 5 xfail — unchanged |
| `printf-diff.sh` | float-endianness is a configure-time fix, a no-op on x86-64 | 1197 pass, 5 xfail — unchanged |

The other 34 moved no count either. That is the expected outcome and was
always the argument for doing it: it converts an unexamined assumption into a
checked one, and every case written against these harnesses from now on is
certified against GNU rather than against Ubuntu.

One harness needed a new mechanism to get there. **`test` cannot attest its own
version**: it has no options at all, so `--version` is an ordinary
one-argument expression and a non-empty string is true — `test --version`
prints nothing and exits 0, on the built 9.4 and on the installed binary
alike. The version guard therefore read `test-diff: … is not coreutils 9.4
(it says: )` for a tree that was exactly right. `diff-wsl.sh` grew
`DIFF_GNU_VERIFY_WITH` for it: the tree is one `make` of one tarball, so any
sibling in it attests the version, and `test-diff.sh` borrows `cat`. The guard
keeps its full strength — still a real runtime check on a real binary, still
fatal. The alternative of skipping verification whenever `--version` produced
nothing recognisable was rejected: that is also exactly what a *wrong* binary
would produce.

The same knob fixed a defect the sweep exposed in the three **family**
harnesses. `write-error`, `digest` and `interleave` name themselves for the
property they test, and there is no `src/write-error` in the coreutils tree to
ask, so the witness defaults to the first `DIFF_BINS` entry rather than to
`DIFF_PROG`.

Remaining: nothing, for coreutils. Harnesses whose reference is *not* coreutils
are out of scope and always were: `awk`, `bc`, `cmp` (diffutils), `ed`, `find`
and `xargs` (findutils), `grep`, `more` (util-linux), `osh`/`sh` (bash), `sed`,
`tar`, `time`, and `extfloat` (whose reference is a C probe it compiles
itself). The one live item left under this heading is the ACL/xattr/capability
gap immediately below, which needs the operator.

### A second, smaller gap: the built reference is upstream's source, not its build

`configure` on this machine turns three features off, because their development
headers are absent and `sudo` here needs a password:

```text
configure: WARNING: GNU coreutils will be built without ACL support.
configure: WARNING: GNU coreutils will be built without xattr support.
configure: WARNING: GNU coreutils will be built without capability support.
```

Ubuntu's binary has all three. Three things could show it: the `+` after
`ls -l`'s mode string (ACL), `cp --preserve=xattr`, and `--color`'s `ca` slot
(file capabilities). **None is reachable today** — no fixture in `scripts/`
creates an ACL, an xattr or a capability, and no subject program implements any
of them — so this is a note for whoever writes the first such case, not a live
defect. The fix is `apt install libacl1-dev libattr1-dev libcap-dev`, which
needs the operator; after it, wipe `$HOME/.cache/slateos-diff-gnu` so the
reference is rebuilt with them.

### How to check a suspected patch, cheaply

The sequence that settled both, in increasing order of cost:

1. `dpkg -s coreutils | grep Version` — is the binary a distribution build?
2. `zcat /usr/share/doc/coreutils/changelog.Debian.gz | grep -i <utility>` —
   distributions document their patches, and Debian's changelog names the
   patch file. This is the step that should be *first* and was last both times.
3. `curl -sfL "https://git.savannah.gnu.org/cgit/coreutils.git/plain/src/<f>.c?h=v9.4"`
   — the `?h=` tag pin works; check it does by diffing two tags' line counts.
4. `strings -a /usr/bin/<utility> | grep <string>` — a message in the binary
   and in no source tree is a patch, conclusively.
5. Only then the expensive tools: `LD_PRELOAD` interposition, a Python
   reimplementation of the algorithm, a bisect over versions.
