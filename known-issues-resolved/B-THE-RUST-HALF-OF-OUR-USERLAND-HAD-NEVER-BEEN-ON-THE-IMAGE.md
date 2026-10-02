## B-THE-RUST-HALF-OF-OUR-USERLAND-HAD-NEVER-BEEN-ON-THE-IMAGE (lane B, 2026-09-13) — **fixed**

> **Renamed and corrected 2026-09-13, hours after filing.** It was first filed as
> `B-THE-USERSPACE-THIS-PROJECT-WRITES-HAS-NEVER-BEEN-ON-THE-IMAGE`, and that
> title was wrong: 14 utilities this project writes *were* already on the image.
> See **Correction** below — the mistake is more instructive than the finding.

**In short:** SlateOS boots an image carrying five upstream ports and 14
small utilities compiled from our Python. It carried **none** of the 278
command-line programs written in Rust under `userspace/` — `cp`, `awk`, `sed`,
`tar`, `find` and 273 others were written, tested, marked done in `roadmap.md`,
and then copied nowhere. They were being tested on the developer's
*Windows* machine, against Windows' own filesystem, and never once compiled for
SlateOS, let alone run on it. The cause was not a missing toolchain — the
toolchain worked the first time it was asked — it was that no script ever
performed the copy.

### What was actually shipping

Three scripts can build a bootable image, and the population of
locally-written software each one puts on it is:

| script | what it stages from `userspace/` |
|---|---|
| `scripts/create-ext4-rootfs.sh` | nothing |
| `scripts/build-image.ps1` | nothing |
| `scripts/build-usb-image.py` | nothing |

The kernel embeds three programs directly (`init`, `hello`, `ticker`, all from
`services/`, via `include_bytes!` in `kernel/src/main.rs`), and the fastpy block
promotes 14 compiled-Python utilities into `/bin`. Those 17 were the entire set
of software written here that had ever reached a running SlateOS.

Everything `create-ext4-rootfs.sh` does stage is either a host Linux binary
used as a fallback, or one of the five upstream ports (bash, pkgconf, make,
CMake, CPython). Those five get long, careful staging blocks — the CMake one
runs to 40 lines of commentary about its module tree. Our own userland got no
line at all.

### How it surfaced

Not from a failing test — nothing tested it. It came out of trying to do a
much smaller job: `logrotate` had just been written, and the roadmap's own
pkgconf entry (*"a port called 'proven' since 2026-08-14 had never once been in
an image"*) argued it should be put on the image before being called done. The
search for the mechanism that stages a userspace utility found that there is no
such mechanism, and never had been.

It was not entirely unrecorded. A lane-A boot-test analysis from 2026-08-21
contains the clause *"…from `userspace/coreutils/`, which is not staged on
`rootfs.ext4` at all yet"* — written as a supporting detail in an argument that
lane B's code could not be responsible for three failing tests. That is a true
sentence doing the opposite of the work it should have done: it was used to
establish that our binaries were *not implicated*, and nobody asked why they
were not there. **A fact used only to excuse a subject from suspicion is a fact
nobody is tracking.**

### What it was not

It was not a toolchain gap, which is the explanation that would have justified
the delay, and it is worth being precise that it was never true:

- `userspace/.cargo/config.toml` already sets `target = "../toolchain/x86_64-slateos.json"` with `build-std`.
- `toolchain/build-sysroot.ps1` already produces the `libc.a` and `libstubs.a` they link against, and both were present.
- Asked to build for the real target for the first time, `logrotate` produced a **795 KB statically linked SlateOS ELF in 58 seconds**, and the whole `coreutils` crate produced **86 SlateOS ELF executables in 50 seconds**. Zero errors, zero changes to any crate.

So the distance between "278 utilities that have never touched the OS" and
"71 of them on the image" was one `cargo build` and one `cp` loop. It had been
that distance the whole time.

### The fix

A new staging block in `scripts/create-ext4-rootfs.sh` copies every ELF in
`target/x86_64-slateos/release/` into `/bin`. It:

- identifies binaries by **ELF magic**, not by filename — cargo owns that
  directory and fills it with `.d` depfiles and `incremental/`;
- **skips and announces** a name an earlier block already staged, rather than
  clobbering it, because lane A's boot test asserts on `/bin/make`, `/bin/sh`
  and `/bin/tcc` by name;
- **warns** when a staged binary is older than the sysroot `libc.a`, matching
  the existing CMake staleness check — a binary linked against a stale libc
  proves nothing about the current one;
- reports the count, and when the count is zero says so loudly and names the
  command that fixes it.

Ten cases were added to `scripts/test-rootfs-staging.sh`, which extracts the
block *verbatim* from the shipped script and runs it against fake artifacts.
Both guards were mutation-tested: deleting the ELF-magic check and disabling
the collision guard each take the harness from 14/14 to 12/14 and exit 1.

### Correction — and how the mistake was made

The first version of this entry claimed that **nothing** this project writes had
ever been on the image. That was wrong, and it was wrong in a way worth keeping.

14 utilities were already there, promoted into `/bin` by the fastpy block:
`cat`, `chmod`, `chown`, `grep`, `head`, `ls`, `mkdir`, `mv`, `rm`, `rmdir`,
`sort`, `tail`, `uniq`, `wc`. They are compiled from our own Python, which is
the path `CLAUDE.md` actually prefers for userspace tools, so they are very much
software this project writes. `/bin/ls` on the image was ours all along — just
not the Rust one.

**How the error was made: I grepped for a verb.** To inventory the script I ran

    grep -oE '\[rootfs\] staged [^"]{0,70}' scripts/create-ext4-rootfs.sh

and read the result as the complete list of what reaches the image. The fastpy
promotion does not use that word. It prints

    [rootfs] promoted fastpy binary: /bin/ls  (fastpy-ls)

so the one block that disproved the thesis was invisible to the search that
built it. Every line I *did* find was accurate; the set was not complete, and a
grep over one phrasing cannot tell those two apart.

**What caught it:** running the script. The correction did not come from
re-reading the source, which I did several times, but from my own collision
guard printing `NOTE: /bin/ls is already staged by an earlier block` — a guard
written for a different purpose (protecting lane A's `/bin/make`) and which
turned out to be the only thing in the change capable of contradicting its own
author. It is worth noticing that an inventory taken by reading was wrong for
half a day and an inventory taken by executing was right immediately.

### Verified end to end

The image was built with the block in place (`ALLOW_STALE_FIXTURES=1`, needed
for a pre-existing stale cmake fixture unrelated to this change):

    [rootfs] staged 71 SlateOS-native utilities from userspace/ into /bin (60 MiB)
    [rootfs]          (15 skipped, already present -- see the NOTEs above)
    [rootfs] DONE.

384 MiB image written. The 15 skips are the 14 fastpy commands above plus `sh`,
which dash owns. 60 MiB is under the 96 MiB budget, so the tripwire correctly
stayed silent. `/bin` went from 36 entries to 107.

**One caveat on what those 71 are linked against.** `toolchain/sysroot/lib/libc.a`
is dated 2026-09-12 08:11 and four `posix/src/*.rs` files are newer than it, so
these binaries link a libc that is behind its own sources. The tree already
detects this — it is why the default image build refuses and why
`ALLOW_STALE_FIXTURES=1` was needed. The staleness check inside this block
compares each binary against `libc.a` and correctly stays quiet, because the
binaries are newer than the archive; it is the archive that is behind. The two
checks compose, but neither states the transitive claim on its own.

### The second defect: building a crate changed what the OS contains

The first version of the staging block scanned
`target/x86_64-slateos/release/` and staged every ELF it found. That is a
coupling defect, and it did not stay theoretical for an hour.

Wanting to know whether the *rest* of the tree cross-compiles, I built the
remaining 190 userspace crates. It is a good result on its own — **all 190
compiled, zero errors, 3m 08s**, so every one of the 276 binaries builds for
SlateOS. But the next image build then did this:

    [rootfs] staged 260 SlateOS-native utilities from userspace/ into /bin (204 MiB)
    [rootfs] WARNING: that is more than a quarter of the 384M image (96 MiB).
    mke2fs: Could not allocate block in ext2 filesystem while populating file system
    *** rootfs.ext4 was NOT written ***

So `cargo build` in a crate directory, run to answer a question, changed what
the operating system image contains and then broke it. The tripwire did fire and
named the exact failure that followed — but a warning is the wrong instrument
for this. **A developer debugging one crate must not be able to alter the image
at all.**

**The fix: `scripts/rootfs-bin-manifest.txt`.** What ships is now an explicit,
tracked list of 70 names, and a binary that is built but not listed does not
ship. Verified with all 276 binaries present in `target/`: the image build
staged 70 (59 MiB) and ignored the other 206. A missing manifest is an `exit 1`
rather than a NOTE, because the file is tracked in git — its absence means a
broken checkout, not a tree that has not built yet.

The manifest deliberately omits the 13 names the promoted fastpy commands own
(`cat`, `grep`, `ls`, `wc`, …) and `sh`, which dash owns. That is
`design-decisions.md` **§108 part 1**: fastpy stays *"additive only… No Rust
coreutil is touched, shadowed or retired"*, and *"a silent swap is a
user-visible policy change and is not Claude's to make."* Which implementation a
stock install should prefer is `deferred-questions.md` **DQ1**, whose trigger
— a fastpy utility with a parity suite and a performance bar — has not been
met, so it stays deferred and this change does not touch it. The manifest and
the collision guard are two independent things that would both have to be wrong
before a swap could happen quietly.

**Why the whole userland is not on the image.** All 276 build, and they come to
204 MiB against a fixed 384M image that already carries ~127 MiB of fastpy test
ELFs. They do not fit. Raising `IMG_SIZE` is available and nothing outside this
script reads it, but "which utilities earn their bytes" is a real question and
staging everything that happens to compile is not an answer to it.

### Why absence is a NOTE and not an error, for now

The neighbouring fastpy block exits 1 on an empty scan. This one does not, and
the difference is deliberate: a missing fastpy fixture makes a ring-3 self-test
**self-skip and still report PASS**, whereas nothing yet asserts on these
binaries at all. Making their absence fatal would break the image build in every
tree that has not yet built the slateos target — lane A's included — to protect
a test that does not exist. The block's comment says to turn it into an `exit 1`
in the same change that adds the first boot test asserting one of these runs.

### What is still open

**Staged is not run.** This entry fixes "never on the image"; it does not
establish that a single one of these binaries *executes* under SlateOS. That
needs a boot test, which is lane A's tree — filed as
`requests/b-a-our-own-utilities-are-on-the-image-now-can-a-boot-test-run-one.md`.
Until that rung is green, the honest claim is "71 SlateOS-native utilities are
present on the image", and no more than that. This is the same distinction the
pkgconf entry had to learn: cross-compiling, linking, being staged and being
*run* are four separate claims, and passing three of them is not passing the
fourth.

**And the image will not hold all 278.** A static Rust binary here averages
886 KiB, so the 86 that `coreutils` produces are 74 MiB — about a fifth of the
fixed 384M image, which after CPython had roughly 150 MiB free. Building the
remaining 193 `userspace/*` binary crates would add on the order of 167 MiB and
would not fit. That is not a reason to stage none of them, but it does mean
"put all 278 on the image" is not the finish line: the block now reports its own
MiB and warns past a quarter of `IMG_SIZE`, because nothing else in
`create-ext4-rootfs.sh` accounts for free space at all and the script's own
header records what running out looks like — `mke2fs -d` gives up partway and
the abort trap leaves a broken image behind. Deciding *which* utilities earn
their bytes is a real question, and it is not answered here.
