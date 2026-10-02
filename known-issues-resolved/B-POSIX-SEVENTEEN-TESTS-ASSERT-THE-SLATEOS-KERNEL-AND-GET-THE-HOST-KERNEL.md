## `B-POSIX-SEVENTEEN-TESTS-ASSERT-THE-SLATEOS-KERNEL-AND-GET-THE-HOST-KERNEL` (lane B, 2026-08-26) — **CLOSED 2026-08-26, and the diagnosis below was wrong**

**Resolution.** Fixed, but not as this entry proposed — the diagnosis under
"Why it fails" is incorrect and is kept below only so the mistake is legible.
The seventeen were not seventeen target-behaviour assertions. They were
seventeen tests that call `pipe()` *as setup* and assert it returns 0, and
`pipe2` was issuing a raw, ungated `SYSCALL` instruction on host builds. One
defect, one line, seventeen symptoms. Gating it — as this entry recommended —
would have hidden a live host-safety bug behind seventeen `#[cfg]`s and
deleted the coverage rather than fixing anything.

The tell was in the entry's own evidence and went unread: the sample failure
is `assert_eq!(…, 0)` at `posix/src/file.rs:9106`, and line 9106 is the
`pipe()` call, not the `sync_file_range()` call the test is named for.

Full write-up: `B-POSIX-PIPE2-ISSUES-AN-UNGATED-RAW-SYSCALL-ON-HOST-BUILDS`
below. `cargo test -p posix` is now 20552 passed / 0 failed on both
`x86_64-unknown-linux-gnu` and `x86_64-pc-windows-gnu`, with all seventeen
passing on their own merits and four new tests covering `pipe2`'s success
path.

**Lesson worth keeping.** "These tests assert target behaviour and the host
answers differently" is a *comfortable* diagnosis — it explains a red suite
without implicating any shipping code, and it prescribes a fix (`#[cfg]`)
that is easy and makes the red go away. That is exactly why it deserves more
suspicion than it got here. Before gating a test as host-conditional, read
the line the assertion actually fails on and confirm it is the line the test
is *about*. Seventeen failures sharing one setup call is a single root cause
wearing a costume, not seventeen instances of a pattern.

---

### Original entry (diagnosis incorrect — retained for the record)

**In short:** `cargo test -p posix` on a Linux host reports 20531 passed and 17
failed. None of the seventeen is a bug in `posix`: they assert what the *SlateOS*
kernel answers for four syscalls, and on a Linux host the real kernel answers
instead, correctly and differently. The suite is nonetheless red, which is the
actual harm — a permanently-failing test run teaches everyone to skim past the
failure line, and the next real regression goes with it.

**The seventeen**, all in `posix/src/file.rs` and `posix/src/dirent.rs`:

```
dirent::tests::test_getdents_valid_args_reach_enosys
dirent::tests::test_workflow_legacy_program_calling_raw_getdents
file::tests::test_copy_file_range_phase89_ebadf_then_valid_progression
file::tests::test_copy_file_range_phase89_einval_then_valid_progression
file::tests::test_copy_file_range_phase89_zero_len_with_valid_fds_ok
file::tests::test_copy_file_range_zero_len
file::tests::test_posix_fadvise_pipe_returns_espipe
file::tests::test_sync_file_range_phase90_ebadf_then_valid_progression
file::tests::test_sync_file_range_phase90_einval_then_valid_progression
file::tests::test_sync_file_range_phase90_endbyte_overflow_einval
file::tests::test_sync_file_range_phase90_high_bit_flag_einval
file::tests::test_sync_file_range_phase90_known_flag_combo_passes_prologue
file::tests::test_sync_file_range_phase90_max_offset_zero_nbytes_ok_prologue
file::tests::test_sync_file_range_phase90_negative_nbytes_einval
file::tests::test_sync_file_range_phase90_negative_offset_einval
file::tests::test_sync_file_range_phase90_unknown_flag_einval
file::tests::test_sync_file_range_valid_fd_no_crash
```

Sample: `test_sync_file_range_valid_fd_no_crash` asserts `0` and gets `-1`
(`posix/src/file.rs:9106`). The names give the shape away — the `getdents` pair
literally expects `ENOSYS`, which is what SlateOS returns and what Linux does not.

**Not a regression, and not from the clippy work.** Measured on both sides of
`d51dc736d`: 20531 passed / 17 failed identically, so the count predates it.

**Proper fix.** These are target-behaviour assertions, so gate them on the
target rather than deleting or `#[ignore]`-ing them: a `#[cfg(target_os =
"slateos")]` (or a runtime probe that skips when the syscall is genuinely
implemented) keeps the assertion meaningful where it is true and stops it lying
where it is not. Deleting them would lose real cover on the SlateOS side;
`#[ignore]` would lose it on both. Same shape as
`BUG-OILS-REOPEN-TEST-IS-UNIX-ONLY` above, and worth doing in one pass with it.
