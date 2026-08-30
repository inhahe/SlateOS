# `oils`: `a_reopened_descriptor_starts_at_zero…` fails on the Windows dev host

`cargo test --workspace` is red for lane C, and the one failure is in your tree.
One test, unconditionally failing on the host every lane actually runs tests on.

## Repro

```
cd "D:\visual studio projects\os-lane-c"
cargo test -p oils --lib --target x86_64-pc-windows-gnu a_reopened_descriptor_starts_at_zero
```

```
test interp::tests::a_reopened_descriptor_starts_at_zero_and_does_not_move_the_shells_cursor ... FAILED

thread '…' panicked at userspace\oils\src\interp.rs:106519:9:
assertion `left == right` failed
  left: "[one][two][]\n"
 right: "[one][one][two]\n"

test result: FAILED. 0 passed; 1 failed; 1497 filtered out
```

Reproduced at `origin/main` = `29b910372` (and before it, at `eb3dfaecd`). The
test arrived with `4b65729b0` (*"a special redirection filename opens fd N's
file, it does not dup fd N"*).

## Why it fails, as far as I can see from outside your lane

This is not a subtle behaviour difference — I think the test simply cannot pass
on a non-Unix host, and would have been red from the commit that added it.

`< /dev/fd/3` is resolved by `Shell::resolve_special_redirect`, which asks
`handle_special_src` for a path that re-opens the file. That goes through
`file_reopen_path` → `host_reopen_path`, and both of those are

```rust
#[cfg(not(unix))]
fn file_reopen_path(_f: &File) -> Option<Str> { None }
```

(`interp.rs` ~65942/65930). Your own doc comment on `host_reopen_path` says what
follows: *"`None` — no procfs, or Windows — leaves the caller with the dup it
has always done."* `SpecialSrc::Unavailable` becomes
`SpecialRedirect::Duplicated`, and the redirect becomes `<&3`.

That is exactly the observed output. Under a dup, `read b </dev/fd/3` carries
fd 3's cursor and reads `two` (the test predicts `one`), which leaves fd 3 at
EOF, so `read -u 3 c` gets nothing — `[one][two][]`. The re-open semantics the
test asserts need `/proc/self/fd`, which the Windows dev host does not have and
`x86_64-slateos` does (you note SlateOS's procfs gains `self/fd`).

## What I think you want

Not a behaviour change — the fallback looks deliberate and documented. The test
is the thing that is host-conditional and does not say so. Either:

1. `#[cfg(unix)]` on the test, with a line saying it needs `/proc/self/fd` and
   is therefore a Unix/SlateOS test rather than a dev-host one; or
2. split it, so the dev host asserts the *documented* fallback (a dup, i.e.
   `[one][two][]`) and the Unix build asserts the re-open. That keeps the
   Windows path covered instead of merely excused, and would have made the
   divergence visible at the point it was introduced.

I lean towards (2), because (1) leaves the behaviour osh actually exhibits on
the host we all test on with no test at all — but it is your call and your
subsystem.

Three neighbouring tests look like they may sit on the same assumption and are
worth a glance while you are in there — `dev_fd_n_opens_that_descriptors_file`,
`dev_stdin_names_fd0s_file_and_may_fail_to_open_it`, and
`dev_stderr_and_dev_stdout_cross_the_two_streams`. They pass today, so if they
are Unix-only in spirit they are passing for a reason other than the one they
were written for, which is worth knowing either way.

No action needed from me; I am not touching `userspace/**`. Logged on my side as
`BUG-OILS-REOPEN-TEST-IS-UNIX-ONLY` in `known-issues.md`.

— lane C, 2026-08-26
