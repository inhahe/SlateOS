### TD-REPO-IS-NOT-RUSTFMT-CLEAN-SO-RUNNING-CARGO-FMT-IS-A-TRAP. `cargo fmt -p posix` rewrites 244 files you did not touch — 2026-08-12 — ✅ FIXED (all of Lane B clean 2026-08-15/2026-08-17; `kernel` now clean too — `cargo fmt -p kernel -- --check` exits 0 as of 2026-09-08)

> **UPDATE 2026-08-15 — the operator answered Q42 with option A, and Lane B's
> half is done.** `design-decisions.md` **§310**: one-shot repo-wide reformat,
> with a `.git-blame-ignore-revs` file committed alongside.
>
> - **`posix` is now rustfmt-clean.** `cargo fmt -p posix`, 178 files, in the
>   formatting-only commit `06ad616e0`. Verified afterwards with
>   `cargo fmt -p posix -- --check` (passes), `cargo +nightly check-slateos -p
>   posix` (compiles) and the full suite — **20 289 passed, 0 failed**.
> - **`kernel` is untouched and still carries all 16 911 hunks.** It is Lane A's
>   tree; a single cross-lane reformat commit is exactly the silent clobber the
>   lane split exists to prevent, and at 17 000 hunks it would be the worst
>   possible instance. Requested in
>   `requests/b-a-rustfmt-repo-wide-reformat.md`.
> - **`.git-blame-ignore-revs` exists at the repo root** with the `posix` hash;
>   Lane A appends the kernel hash. Enable it locally with
>   `git config blame.ignoreRevsFile .git-blame-ignore-revs`. Note it does *not*
>   cover GitHub's blame view or `git log -S` — §310 records that as accepted.
>
> **The working rule below still applies to `kernel` and only to `kernel`.** In
> `posix` you may now use `cargo fmt -p posix` normally; that was the point.
> This entry closes when Lane A's commit lands.

> **UPDATE 2026-08-17 — the rest of Lane B's crates are clean too.** The table
> below was measured for four crates and `posix` was the only drifted one *of
> those four*; the userspace crates were never counted. Measured today with
> `cargo fmt -p <crate> -- --check`:
>
> | Crate | Hunks | Status |
> |---|---|---|
> | `oils` | 2 016 | ✅ clean since this date (17 files, +11 099/−3 695) |
> | `coreutils` | 32 | ✅ clean — every hunk was in `head.rs`/`tail.rs`, i.e. this session's own code |
> | `ere` | 20 | ✅ clean since this date |
> | `shell`, `term` | 0 | ✅ already clean |
>
> Each reformat is a formatting-only commit with the crate's suite re-run after
> it, and each hash is appended to `.git-blame-ignore-revs` per §310. The
> `coreutils` row is the one worth noting: a crate can be clean everywhere
> except the file you just added, and then the *next* author's `cargo fmt`
> reformats your work in the middle of theirs. Run `cargo fmt -p <crate> --
> --check` before committing new code in a clean crate — it costs a second and
> it is the whole mechanism by which a crate stays clean.
>
> **And a one-shot reformat does not stay done.** `kernel` was reformatted by
> Lane A in `c33bfa34f` (733 files) and measures **116 hunks** again today.
> That is not a criticism of the reformat — it is the point above, at scale:
> the flush is worth doing once, but only the pre-commit check keeps it. Lane A
> owns `kernel`; noting the number here rather than acting on it.

**Where:** repo-wide, unevenly. Measured 2026-08-12 with `cargo fmt -p <crate> --
--check`:

| Crate | Hunks needing reformat | Status |
|---|---|---|
| `kernel` | 16 911 | **still drifted — Lane A** |
| `posix` | 389 (244 of 2 299 files, ~11%) | ✅ clean since `06ad616e0` |
| `net` | 0 | ✅ |
| `fs` | 0 | ✅ |

CLAUDE.md states the convention as "`rustfmt` defaults. No manual formatting
overrides." Two crates comply; two do not.

**Why it is a trap, not a cosmetic issue.** It cost real work today. After
editing four `posix` files I ran `cargo fmt -p posix` — the ordinary,
correct-looking thing to do — and it reformatted **173 files**, producing a
1 403-insertion / 1 429-deletion diff that buried a ~150-line change. There is no
way to separate the two afterwards, so the whole edit had to be reverted and
re-applied by script with `cargo fmt` deliberately *not* run. `cargo fmt` is
package-scoped with no file filter, so the blast radius is the crate, and in
`kernel` it would be far worse.

A second, smaller hazard: it makes *pre-existing* oddities look like your own
damage. `cargo fmt` surfaced strange `CapGuard` formatting in
`linux_seccomp.rs` right after I had run a regex over that file; I assumed my
regex had mangled it. `git show <sha>~1:posix/src/linux_seccomp.rs` proved it
predated me. Every fmt run in a drifted crate costs an investigation like that.

**Working rule adopted now** (`Claude (autonomous)`, cheap and safe): format
*only the files you edited*, by invoking rustfmt directly —
`rustfmt --edition 2024 posix/src/semaphore.rs` — never `cargo fmt -p <crate>`
in `kernel` or `posix`. This removes the hazard without touching history.

**The proper fix is a one-shot repo-wide reformat**, which is *not* obviously
correct and was therefore the operator's call — it rewrites `git blame` for
~17 000 hunks of kernel code, and blame is the main tool for answering "why is
this line here?" in a codebase with no human reviewer. Raised as **Q42** in
`open-questions.md`; **answered A on 2026-08-15** → `design-decisions.md` §310.
See the update at the top of this entry: `posix` is done, `kernel` is Lane A's
and outstanding, and until that lands the working rule above holds *for
`kernel`*.

**Also note:** `cargo fmt --all` does not run in this workspace at all — it dies
with `The filename or extension is too long. (os error 206)`, a Windows command
line-length limit hit by the number of workspace members. So a repo-wide reformat
would have to iterate crates (or files) anyway.
