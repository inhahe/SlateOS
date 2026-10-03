### A-NO-CROSS-BACKEND-METADATA-CONFORMANCE-TEST. `FileMeta` is a contract with thirteen implementations and no test that reads the contract, so three of them reported a nine-bit mode under a twelve-bit promise for months — 2026-09-01 — **Status: OPEN, NARROWED TWICE (the three instances are FIXED; the *domain* half of the missing test landed 2026-09-01 in `63936036a`/`58d6caebe` as `fs::conformance`; the *declared-value* half — the half that would actually have caught this bug — landed 2026-09-01 in `5fa72bc13`. Now **✅ FIXED 2026-09-02** — the green boot the trigger required happened, and both layers ran in QEMU. See the two "Narrowed" sections below and the Resolution at the end.)**

**Lane:** A

**What happened.** `FileMeta::permissions` documents itself as twelve bits —
"setuid setgid sticky rwxrwxrwx", `0o7777`. ext4 delivers twelve
(`vfs_impl.rs:56`, `i_mode & 0o7777`), iso9660 delivers twelve
(`iso9660.rs:450`), memfs stores the `u16` whole. btrfs, zfs and f2fs masked
to `0o777` first and delivered nine.

So the same file, with the same bits on disk, reported a different mode
depending on which driver read it. `cp -a` and `tar` reading off a
btrfs/zfs/f2fs mount silently dropped setuid, setgid and sticky from
everything they copied — silently in the strict sense: there is no error, and
"the bit is missing" is indistinguishable from "the bit was never set."

Fixed 2026-09-01 in all three (`& 0o7555` — twelve bits minus the three write
bits the read-only mount genuinely justifies dropping). See
`design-decisions.md` §663.

**Why this entry exists even though the bug is fixed.** Nothing was wrong in a
way any *single* backend's tests could detect. Each of the three was
internally consistent: it masked on read, nothing wrote, and every assertion
about it used a mode inside `0o777`. The divergence only exists *between*
implementations, and there is no test that looks between them.

The same is true of every other field `FileMeta` promises. Spot-checked while
fixing the above and **not** yet run down:

- `nlinks` — procfs/sysfs/devfs report a placeholder (noted in
  `A-GETDENTS64-D-INO-DISAGREES-WITH-ST-INO-ON-PSEUDO-FILESYSTEMS`).
- `ino` — now uniform by construction after §662, but by inspection of all
  thirteen backends one at a time, not by a test.
- `created_ns` / `modified_ns` / `accessed_ns` — units are nanoseconds; each
  backend converts from its own on-disk representation with its own
  arithmetic, and a backend that returned seconds would look plausible.

**The proper fix.** One test, parameterised over every registered backend,
that mounts a fixture image and asserts the *contract* rather than the
implementation: that a file with known bits on disk reports the same
`FileMeta` from every driver that can read it. That requires a small fixture
per filesystem, which is the reason it does not exist yet and the reason it is
worth doing once rather than thirteen times. Failing that, a much cheaper
partial: a compile-time or boot-time assertion that no `metadata()`
implementation masks `permissions` more narrowly than `0o7777` — which would
have caught this specific class on the day it was written.

**Reproduce (before the fix):** mount any btrfs/zfs/f2fs image containing a
sticky or setuid file and `stat` it; the special bits read back as 0 while the
same file on ext4 reports them.

#### NARROWED 2026-09-01 — `kernel/src/fs/conformance.rs` (`63936036a`, `58d6caebe`)

**What landed.** A boot-time harness, dispatched from `main.rs` after every
per-backend self-test, that reads the *contract* rather than any
implementation. It runs a memfs fixture it builds itself, then every live VFS
mount among `/`, `/proc`, `/sys`, `/dev`, `/tmp`, and asserts per object:

| Clause | Catches |
|---|---|
| `permissions & !0o7777 == 0` | a mode outside the documented domain |
| `nlinks >= 1` | a link count of zero on an object that exists |
| each of the four `*_ns` in `[1e16, 1e19]` when nonzero | a timestamp in the wrong *unit* — seconds, ms or µs all land below the floor; see the constant's doc table |
| `readdir` `ino` == `stat` `ino` | the cross-route disagreement of `A-GETDENTS64-D-INO-DISAGREES-WITH-ST-INO-ON-PSEUDO-FILESYSTEMS` |
| `readdir` `entry_type` == `stat` `entry_type` | the same, for type |
| `readdir` `size` == `stat` `size`, files only | the same, for size (directories excluded: `DirEntry::size` is documented as 0 for them, which is a different quantity, not a disagreement) |

Two structural guards, because a harness that reports a pass it did not earn
is worse than no harness: a **vacuous-run** check fails the boot if zero
objects were inspected (the memfs fixture is unconditional, so zero can only
mean the harness itself broke), and every skip is either read from the mount
table or classified through `fs::selftest::classify`, so a backend can never
skip its own check by failing it. That second property is not decoration —
the first draft got it wrong in three places and the `[selftest-skips]` gate
caught all three.

**What is still open, and why this is the important half.** The domain layer
above would **not** have caught the btrfs/zfs/f2fs bug. Masking `& 0o777`
violates no domain rule: every value it can produce is inside `0o7777`. A
narrowing is only visible against a fixture that *declares* the bits it wrote,
and that layer is not built.

**The blocker this entry originally named is gone.** It said the test "requires
a small fixture per filesystem, which is the reason it does not exist yet."
That is no longer true: btrfs, f2fs and ntfs already build a synthetic volume
in RAM (`build_image()` / `mount_image()` in each backend's `tests.rs`) and zfs
takes one through `ZfsFs::open_source(Box::new(MemorySource::new(bytes)))` —
all on *every* boot, not only when a disk is attached. The remaining work is
therefore plumbing, not fixture authoring:

1. Expose each backend's existing image builder as `pub(crate)` so
   `conformance::check_tree` can drive it as a `dyn FileSystem`.
2. Set a special bit in each builder — `0o755` → `0o4755` on one file — and
   assert the mode reads back whole. That single assertion is what turns this
   from a harness that would have missed the bug into one that catches it.

**Trigger to close:** step 2 landing for btrfs, zfs and f2fs — the three
backends that had the bug — with the setuid file surviving the round trip on a
green boot.

#### NARROWED FURTHER 2026-09-01 — the declared-value layer landed (`5fa72bc13`)

**What landed.** Step 2 above, for all four backends that build a synthetic
volume: btrfs, f2fs, ntfs and zfs. Each backend's `tests.rs` now exports

```rust
pub(crate) fn conformance_fixture()
    -> KernelResult<(Box<dyn FileSystem>, &'static [Declared])>
```

which mounts the volume that backend already built and hands back a table of
`Declared { path, mode, may_drop }` rows. `conformance::check_declared`
asserts `meta.permissions == mode & !may_drop` — **equality, not superset**,
because a superset test would pass a driver that *invented* a bit, and a
spurious setuid bit is a privilege granted to a file that never had one.

`may_drop` exists for one real case: these fixtures mount read-only, and a
read-only mount legitimately withholds the write bits. It does not make a
setuid binary not-setuid, it makes it unmodifiable — hence `may_drop: 0o222`
and not a blanket `0o777`.

The fixtures were changed to write bits *above* `0o777`, which is the whole
point: btrfs `/hello.txt` `0o100644` → `0o104644` (setuid) and `/sub`
`0o040755` → `0o043755` (setgid+sticky), zfs the same two objects, f2fs the
same two as per-inode overrides rather than in its shared `InodeSpec::file()`
/ `dir()` constructors — a fixture in which *every* object is setuid cannot
distinguish "preserved this file's bits" from "returns one mode for
everything." The paired in-file assertions moved with them in the same commit
(`0o444` → `0o4444`, `0o555` → `0o3555`).

When the assertion fails, the harness additionally recognises the specific
shape `got == want & 0o777 && want & !0o777 != 0` and names it as the
`& 0o777` narrowing this layer exists to catch, so the next occurrence is
diagnosed rather than merely detected.

**A second gap closed on the way, which was arguably the larger one.** Layer
one had never run over *any* on-disk driver — only memfs and live VFS mounts.
On a diskless boot, which is every boot of the boot test, none of the four
backends whose `FileMeta` actually diverged were being checked at all.
`check_fixture_backends` now runs `check_tree` over each synthetic volume
before `check_declared`, so the domain layer finally covers the
implementations it was written for.

**Three structural guards**, on the same principle as the vacuous-run check
above — a harness that reports a pass it did not earn is worse than none:

- a fixture that fails to build or mount is a **failure**, not a skip: it is a
  byte array assembled from constants and depends on nothing about this boot,
  so there is no environment in which "could not build it" is a legitimate
  abstention;
- an **empty `Declared` table is a failure**, because a table with no rows
  catches no narrowing while still reporting a pass — the
  `A-GATES-SILENTLY-STOPPED-CHECKING` shape;
- NTFS is included even though it has no on-disk Unix mode. What it pins is
  `mod.rs:985`'s *synthesis* (`0o555` dirs / `0o444` files, `may_drop: 0`).
  Its doc comment says plainly that NTFS cannot carry a bit above `0o777` and
  therefore cannot detect a narrowing; it is there because a drift to the
  "obvious" `0o644` would advertise write access on a mount that refuses every
  write.

**Correction to step 1 of the plan above: it was not needed.** The entry said
each backend's image builder must be widened to `pub(crate)` so `check_tree`
could drive it. That turned out to be wrong — `conformance_fixture()` lives
*inside* each backend's `tests` module and calls the private builders
directly, so no visibility changed anywhere. Recorded so the next reader does
not go looking for four widenings that were never made.
(`design-decisions.md` §671, which planned this layer, carries the same
expectation, and the same correction applies to it.)

**Status is still OPEN, and deliberately so.** `cargo check -p kernel` is
clean, but the trigger to close is explicitly *"the setuid file surviving the
round trip on a green boot"*, and this code has not yet reached QEMU — the
host has been at its Windows commit limit under three-lane concurrent builds
(see `A-BOOT-RECORDER-FILES-A-HOST-FORK-FAILURE-AS-A-KERNEL-TIMEOUT`). Do not
close this on the strength of a clean type-check: every assertion here is
boot-time, so an unrun harness has demonstrated nothing.

> **Resolution — 2026-09-02, lane A.** The harness reached QEMU and passed.
> The paragraph immediately above is what set the bar, and it is worth
> recording that the bar was met on its own terms rather than waived: this is
> closed on a boot, not on a type-check.
>
> **The evidence.** Boot test PASSED end-to-end (`child exited: PASS`, 5192 s,
> tree at `d9e224706`), and in `build/serial-test.txt`:
>
> ```
> [fsconform] Running cross-backend FileMeta conformance...
> [fsconform] Conformance passed (2347 clause(s) over 521 object(s), 0 voided).
> ```
>
> Zero `[fsconform] FAIL` lines anywhere in the 47,087-line serial log. The
> boot recorder logged it as a tracking run, not an experiment, so it extends
> the streak (now 2 consecutive clean) — which matters here because the
> `--no-rootfs` experiments deliberately test *less* and are excluded from
> exactly this kind of claim.
>
> **Why that summary line is sufficient, given this entry's own thesis.** The
> thesis is that a test which does not run proves nothing, so a bare "passed"
> would be a weak thing to close on — the pass line names no backend. It is
> sufficient because the harness cannot emit it while a backend is missing:
>
> | Failure mode | What `check_fixture_backends` does |
> |---|---|
> | A backend absent from the run | Impossible — `FIXTURES` is a fixed 4-entry table (`btrfs`, `f2fs`, `ntfs`, `zfs`) iterated unconditionally |
> | A fixture that will not build or mount | `r.failed += 1` — a **failure, not a skip**, because the images are byte arrays built from constants and depend on nothing about the boot |
> | A fixture that declares an *empty* mode table | `r.failed += 1`, with the reason given in the source as "the shape of `A-GATES-SILENTLY-STOPPED-CHECKING`" — opting out of the only layer that catches a narrowing, while still appearing in the pass line |
>
> So "passed, 0 voided" entails that all four backends built, mounted, and
> checked a non-empty declared table. The third row is the one that makes this
> closable: it is the guard against precisely the failure this entry is about,
> written into the harness rather than left to a reader to notice.
>
> **What was actually proven.** `check_declared` asserts
> `meta.permissions == mode & !may_drop` — equality, not superset — over
> fixtures whose objects carry bits *above* `0o777` (btrfs and zfs
> `/hello.txt` at `0o104644` setuid and `/sub` at `0o043755` setgid+sticky,
> f2fs the same two as per-inode overrides). A driver that masked to `0o777`,
> which is the original bug, now fails. So does one that *invented* a bit,
> which is why the check is equality: a spurious setuid bit is a privilege
> granted to a file that never had one.
>
> **Not closed:** the `0o777` narrowing as a *class* across all thirteen
> `FileMeta` implementations. This closes it for the four that build a
> synthetic volume — including all three that had the bug. The nine that do
> not build one are still only covered by the domain layer, which by this
> entry's own analysis "would **not** have caught the btrfs/zfs/f2fs bug."
> Extending the declared-value layer to a backend needs that backend to grow a
> synthetic fixture first; that is the trigger if one ever does.
