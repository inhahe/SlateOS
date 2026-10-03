## D-SAFEIO-ADOPTION-IS-NOT-ENFORCED-BY-ANY-TEST (lane C, 2026-08-16) — RESOLVED 2026-08-16

**In short:** We have a small library, `apps/safeio`, whose whole job is to save
a file without the risk of destroying the old one if the save fails part-way.
Five programs now use it. But nothing stops someone from quietly changing one
of them back to the unsafe `fs::write` — no test would fail, because the two
behave identically except in the rare failure the library exists to survive.
The protection is real but it is held in place by convention alone.

**Where:** `apps/safeio/src/lib.rs` (the library), and its callers —
`apps/editor`, `apps/markdowneditor`, `apps/paint`, `apps/installer/src/grub.rs`,
`apps/screenshot`.

**The gap, concretely.** Restoring `std::fs::write(path, &data)?` in
`screenshot::write_bmp` in place of `safeio::write_atomically` leaves the whole
screenshot suite green (verified by injection, 2026-08-16 — 68/68 pass with the
unsafe write in place). The same is true of the other adopters: none has a test
that distinguishes the atomic path from the naive one.

**Why it is hard to test at the adopter level.** The difference between the two
is only observable when the write fails *after* the target has been opened —
ENOSPC, EIO, power loss. None of those is portably inducible from a unit test.
The properties that *are* observable are either shared by both implementations
(no litter left behind; a rejected encode leaves the file alone) or are
incidental side effects of the rename strategy rather than the guarantee itself
(the target gets a new inode, so a hard link to it keeps the old contents;
saving over a read-only file succeeds on POSIX and fails on Windows). Pinning
an incidental side effect would encode an implementation detail as a
requirement, which is why it was not done.

**What the proper fix looks like** — options, in rough order of preference:

1. **A fault-injection seam in `safeio`.** A `#[cfg(test)]`-only hook, or a
   `write_atomically_with` taking a writer, so the *library's* tests can force a
   failure at each stage. This makes safeio's own guarantee testable at every
   step (it currently tests the happy paths and cleanup), but still does not
   prove an adopter calls it.
2. **A workspace lint.** `clippy.toml`'s `disallowed-methods` can ban
   `std::fs::write` outside `safeio` and test modules, which enforces adoption
   *directly* rather than by proxy, and fails at build time rather than needing
   a test. This is the one that actually closes the stated gap.
3. **Accept it and document it**, which is the current state — every adopter's
   call site carries a doc comment explaining why it is there, so a reader
   changing it has been warned even though the compiler has not.

**Assessment.** Option 2 is cheap and precise, but it needs a decision about
scope: `fs::write` is entirely legitimate in tests, in scratch/cache files, and
in code where truncation is the intent, so the ban needs an allowlist and that
allowlist is a maintenance surface. Logged rather than done because it touches
the workspace lint configuration, which is shared across all three lanes and is
not lane C's to change unilaterally. If it is wanted, it should be a request to
whoever owns the workspace `Cargo.toml` lint table.

### Resolution (2026-08-16)

Closed by a fourth option not listed above: **audit counters in `safeio`**,
which enforce adoption per-call-site from within lane C and need no shared lint
configuration.

`safeio` gained an off-by-default `audit` feature exposing `writes_performed()`
and `copies_performed()`. Each adopter depends on `safeio` twice — normally, and
again in `[dev-dependencies]` with `features = ["audit"]`. Cargo unifies the two
when building that crate's tests, so the counters exist for `cargo test` and are
absent from every shipped binary. A test then asserts the counter *moved* across
a save, which is exactly the thing `fs::write` cannot fake.

This is a proxy for the guarantee rather than the guarantee itself — it proves
the call reached `safeio`, not that `safeio` is crash-safe (that remains
`safeio`'s own 9 tests). But it closes the stated gap, which was that adoption
rested on convention alone.

**Every assertion was verified by injection**, one call site at a time:
restoring `fs::write`/`fs::copy` fails the new test *and nothing else*.

| Adopter | Call sites covered | Injection result |
|---|---|---|
| `screenshot` | `write_bmp` | 68 passed, 1 failed |
| `editor` | `Document::save` | 119 passed, 1 failed |
| `markdowneditor` | `save`, `save_as` (separately) | 205 passed, 1 failed (each) |
| `paint` | `save_bmp` — previously had **no test at all** | 158 passed, 1 failed |
| `installer` | `grub::install`, `grub::update` (separately) | 92 passed, 1 failed (each) |
| `backup` | `store_bytes`, `store_file` (both counters) | 63 passed, 1 failed (each) |
| `backup` | `manifest.json`, `meta.json`, restore, `schedules.json` | 64 passed, 1 failed (each) |

That "and nothing else" is the point: it reconfirms the original finding that
the rest of each suite stays green through the regression.

**One note for whoever picks this up next.** The adopter list above was
incomplete when this entry was first written — it said "five programs" and
omitted `apps/backup`, which is the adopter with the *most* at stake (see the
content-addressed-store reasoning in `safeio`'s docs).

### Follow-up 2026-08-16 — `backup`'s remaining four sites are now covered

This entry first shipped with `backup`'s other four writes unenforced —
`manifest.json` and `meta.json` (`create`), the restore write, and the
schedule-list write — on the grounds that each sits inside a large command
function rather than a unit-testable seam. That reason did not survive
contact: `cmd_create`, `cmd_restore` and `cmd_schedule` are all plain
functions over a directory, so the test drives each command **end-to-end**
and no factoring was needed at all. Driving the real command is also the
better test, because it exercises the same call the binary makes and cannot
drift away from it the way an extracted helper can.

**A counting bug was found and fixed while doing it.** The original
assertions were `writes_performed() > before` — a strict inequality, because
the counters are process-global and `cargo test` runs tests in parallel, so a
delta is an *upper* bound on the span's own writes, not an equality. That
asymmetry points the wrong way. A site that regressed to `fs::write`
contributes one fewer write, and a concurrent test can make up the
difference, so the check would pass and the regression would ship — the test
disarming itself precisely when it was needed. The fix is a test-module
`audit_lock()` mutex held across each measured span, which makes the delta
exactly that span's own and lets every assertion be an equality
(`== 2` for create's two metadata files, `== one per restored file`, `== 1`
for the schedule). The pre-existing blob-store test was moved under the same
lock and tightened from `>` to `== 1` for the same reason.

Worth generalising: **a routing check that can only be inflated by unrelated
activity is not a check.** Any assertion of the form "the counter went up"
over a shared counter has this shape, and it fails open.

The severity ordering, for whoever reads the four sites as interchangeable —
they are not. A truncated `manifest.json` leaves the blobs intact but
unreachable (bytes with no index); `meta.json` doubles as the completion
marker `list_backups` keys off, so a half-written one can make a *finished*
backup unlistable; `schedules.json` is rewritten whole, so a truncated write
loses every schedule rather than the one being added; and the restore write
is the worst, because it writes *over* a file the user still has — a
truncating write there destroys the original and fails to supply the
replacement, which is the one outcome a restore must never produce.

All nine-plus-four call sites across six crates are now enforced. Option 2
(the workspace `disallowed-methods` lint) remains the stronger, build-time
enforcement — it would catch a *new* call site, which no counter test can, since
a test can only assert about code someone thought to test. Still worth filing as
a cross-lane request if the workspace lint table's owner wants it; the counters
do not preclude it.
