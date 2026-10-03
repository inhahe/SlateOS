## FIXED 2026-09-14 — and the paragraph directly above is WRONG

**It was a silent loss.** The table at the top of this entry followed the value
from the kernel backwards and stopped at `posix/src/spawn.rs` without reading
what that file does before it calls. It packs first:

```rust
if pos + needed > buf.len() {
    break; // Truncate silently if buffer is full.
}
```

`buf` is `EXEC_PACKED_MAX`, **128 KiB** — half the kernel's 256 KiB. So an
argument list over 128 KiB never reached the kernel check at all. It was packed
short, and `count_cstring_array` counts the whole list regardless of any
buffer, so the child was handed **a truncated buffer and the full `argc`**. A
program started with fewer arguments than its parent passed, and nothing
anywhere reported it.

Proved before fixing, with four strings and room for two:

    count_cstring_array -> 4
    pack_cstring_array  -> 22 bytes (two strings), no error

**The dilemma above dissolves, because its premise was false.** Option 1 was
rejected on the grounds that enforcing `ARG_MAX` "would start refusing spawns
between 128 KiB and 256 KiB that succeed today". Those spawns do not succeed
today — they truncate. Enforcing 128 KiB refuses nothing that works; it reports
something that was already broken. So the choice that looked like a
user-visible policy call was not one, and did not need the operator.

**Fixed:** `pack_cstring_array` returns `Option<usize>` and refuses rather than
truncating; `posix_spawn` and `execve` answer `E2BIG`, which is what POSIX
spells for exec with too long an argument list. That fixes the errno this entry
was originally about as a side effect.

**One thing worth noticing about the tests.** A test named
`test_pack_and_count_consistency` already existed and exercised only a case
where the buffer was ample — it asserted the invariant in the one situation
where it cannot fail. And `slateos_spawn_caps_rejects_an_oversized_list` passes
with the truncation reinstated, so it was not covering this either. Two tests
whose names describe the defect, neither of which could see it.

**Still open, and genuinely a question:** `sysconf(_SC_ARG_MAX)` advertises
128 KiB and the kernel enforces 256 KiB. They now agree in effect, because libc
refuses at 128 KiB first — but the kernel's limit is still twice what anyone is
told, which is worth reconciling on the kernel side rather than leaving as two
numbers that happen not to collide.
