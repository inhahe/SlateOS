## TD-B-REGCOMP-HAS-NO-TEST-THROUGH-THE-PUBLIC-API (lane B, 2026-08-21)

**In short:** every one of `regex.rs`'s ~200 match tests calls the *internal*
`compile_pattern`/`try_match` on a stack-allocated `RegexProgram`. Not one goes
through `regcomp`/`regexec`/`regfree`, which is the only path a C caller can
use. So the public API's own error mapping, its `regex_t` handling and its
`malloc`/`free` pairing are untested.

**Why it was like that, and why that reason is gone.** The test module said
plainly: "Because the POSIX API allocates memory via our custom `malloc`
(which calls `mmap` → `syscall`), it cannot run on the host test target." That
was true and is no longer — `malloc` gained a host backing store today (see
`B-HOST-MALLOC-NEVER-WORKED-…` above), so the public path is now reachable from
`cargo test`. The comment has been updated in place so nobody re-derives the
old conclusion.

**Where.** `posix/src/regex.rs`, the `tests` module's "Helpers" preamble and
the `new_program`-based tests below it.

**Proper fix.** Add regcomp-level tests alongside the existing ones — they are
an addition, not a replacement, since the internal helpers usefully isolate the
matcher from allocator behaviour. At minimum: compile-then-exec round trip,
`regfree` leaves `re_nsub == 0` and a null `program`, double `regfree` is safe,
`regcomp` of a bad pattern returns the right `REG_*` code and leaves nothing
allocated, and `regexec` on an unfreed-but-never-compiled `regex_t` does not
dereference a wild pointer.

**Related.** The same "it cannot run on the host" reasoning may be load-bearing
in other modules' test preambles for the same now-false reason. Worth a grep
for `mmap`/`syscall`/`host` in `#[cfg(test)]` comments across `posix/src`.
