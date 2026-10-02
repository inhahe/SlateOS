## `C-DISKCLEANUP-A-REAL-DELETER-NEEDS-A-CONFINEMENT-INVARIANT-NOT-A-CONVENTION` (lane C, 2026-08-26) — ✅ **FIXED** 2026-08-26

**In short:** `apps/diskcleanup` was made real — it now measures what it finds
and actually erases it — and doing that surfaced a near-miss worth recording.
The tests for the old, harmless stub scanned `"/"`, because scanning `"/"` was
harmless when nothing was ever deleted. On the Windows machine these tests run
on, `"/"` is `C:\` and `"/tmp"` is `C:\tmp`, a directory holding about 4.5 GB of
the operator's files. Turning on the deleter without changing those tests would
have pointed a working `remove_dir_all` at them. Nothing was lost — the paths
the old tests injected did not happen to exist — but "did not happen to exist"
is not a safety property. The fix is not "be careful in tests": it is that the
program now physically cannot delete outside the directories it was asked to
scan, plus a gate that fails the build if a test asks it to scan a real root.

### What changed

Four things, and the order matters — each one is a precondition for trusting
the next.

**1. Paths became `PathBuf` end-to-end.** `CleanupItem::path` was a `String`.
For any filename the filesystem allows but Unicode cannot express, a round trip
through UTF-8 comes back either unopenable or — far worse — naming a *different*
file, and the operation waiting at the other end is `remove_dir_all`. This had
to land before the deleter did, not after. `display()` is now called in exactly
two places, both of them the moment text is handed to the renderer, and nothing
reads the result back.

**2. The scan became real, and bounded.** `enumerate_entries` lists what is
*inside* a directory and `measure_recursive` walks each entry for its size.
Three properties are load-bearing:

* **`symlink_metadata`, never `metadata`, in both the measuring and the
  deleting path.** A symlink in `/tmp` pointing at `$HOME` reports itself as a
  directory to `metadata`. That single call is the difference between a disk
  cleaner and a program that erases someone's documents — and, short of that,
  between measuring a temp directory and walking the whole disk.
* **Bounded walks** (`MAX_MEASURE_DEPTH`, `MAX_MEASURE_ENTRIES`). `/tmp` is
  world-writable by definition, so its shape is not ours to assume. Hitting a
  bound returns a *short* answer, which is the safe direction to be wrong in:
  under-promise the space freed, then free at least that much.
* **Age floors fail closed.** `age_in_days` returns `0` — "modified just now" —
  when the clock cannot answer, so an unknown age *fails* an age filter rather
  than passing it. A log still being written is not a log to delete.

**3. The confinement list.** This is the actual fix. `CleanupScanner` records
every directory it enumerated in `roots`; `CleanupPlan` carries that list; and
`CleanupExecutor::execute` refuses any item that is not inside one of them —
*before* any syscall, not as a check inside the deleter. So:

```rust
fn permits(&self, path: &Path) -> bool {
    self.roots.iter().any(|root| path != root && path.starts_with(root))
}
```

The set of things this program may delete is exactly the set of directories it
was asked to look at. That holds no matter what a caller injects into the item
list, no matter what a filename contains, and no matter how the UI is
refactored later. Two details are deliberate:

* `path != root` — cleaning `/tmp` must leave `/tmp` standing. (The scanner
  also never lists the root itself; two independent guards, because this one is
  cheap and the failure is not.)
* `Path::starts_with` compares **whole components**, so `/tmp` does not contain
  `/tmpfoo`. That is the bug every string-prefix version of a containment check
  has, and `test_executor_confinement_compares_whole_path_components` pins it.

`set_items` deliberately does *not* grant permission. "Here is a list of files"
and "you may erase these" are different statements, and the second one should
have to be made out loud — which is `allow_root`, and which is what makes every
injected-item test inert by default.

**4. Errors accumulate; they do not abort.** A permission denial in `/tmp` is
the *expected* case on a busy machine. A cleanup that stopped at the first one
would clean almost nothing while appearing to have tried. Only successful
removals are counted, so every byte of "space freed" is backed by an actual
unlink — which is the same principle as the entry above, one commit later.

### The gate

`scripts/check-diskcleanup-test-roots.py` fails if `#[cfg(test)]` code under
`apps/diskcleanup` names `DEFAULT_SCAN_ROOTS` (which *is* `["/"]`) or passes a
root-anchored literal — `"/"`, `"/tmp/…"`, `"C:\…"`, a UNC path — to anything
that walks or unlinks. Tests build a `ScratchDir` under `std::env::temp_dir()`
and scan that.

It exists because the code fix alone leaves a *convention*, and a convention is
a comment with better posture. `cargo test` on a green tree that has just
emptied `C:\tmp` exits 0; nothing in the toolchain would ever have mentioned it.
Run against the file one commit before the rewrite the gate names **eight**
sites and against the file after it, none — so it is measured to catch the
exact thing that happened, which is the only evidence that a gate is a gate.

Note what it does *not* flag: a root-anchored literal on its own.
`CleanupItem::new("/tmp/foo", …)` is inert under the confinement rule, and a
gate that argues with provably harmless code teaches its readers to route
around it.

There is no `BASELINE` ratchet, unlike `check-window-wiring.py`. That idiom is
for a pre-existing population too large to fix in one commit, where a gate red
on day one is commented out by day two. Here the tests were rewritten first, so
the count is already zero — and what it protects is not tech debt, it is the
operator's files. Zero is the only defensible ceiling.

### Collateral

`rustscan.strip_comments` grew a `keep_literals` flag. Every existing caller
matches braces and must keep the default, where a `"{"` inside a string is the
trap the blanking exists to close. This gate is the first whose *subject* is a
literal, so the default would have blanked away the only evidence there is. The
new gate reads structure from the fully-blanked text and findings from the
literal-preserving one, at identical offsets, because both passes replace
characters in place and keep every newline.

### Cost

Test suite 72 → 84 passing; runtime **152 s → 2.66 s**, because 152 s was the
time it took to measure the operator's `C:\tmp`. The slow test was not slow. It
was pointed at the wrong disk, and saying so out loud is the reason it was
noticed.

### The lesson

**When a program gains the power to destroy something, "the tests are careful"
stops being an acceptable safety argument.** The old tests were not wrong when
they were written — scanning `"/"` against a stub that deleted nothing was
genuinely harmless. What changed underneath them was the *consequence* of a
call they were already making, and nothing about them looked different
afterwards. So the fix has to be structural: an invariant the executor enforces
on every path before any syscall, and a gate that fails the build rather than a
comment that asks nicely. A rule no machine checks is a rule that holds until
the first person who has not read it.
