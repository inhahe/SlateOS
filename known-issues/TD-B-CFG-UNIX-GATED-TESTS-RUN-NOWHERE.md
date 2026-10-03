## TD-B-CFG-UNIX-GATED-TESTS-RUN-NOWHERE (lane B, 2026-09-06)

**In short:** A test written `#[cfg(unix)]` is not compiled at all on Windows,
and Windows is the machine this project is developed and tested on. There are
**198 such tests in 40 files** under `userspace/**` and `posix/**` (census of
5239 `.rs` files, 2026-09-06). Every one of them has never run here and will
not run until someone tests on Linux. That is fine for the ones that are gated
because the *behaviour* is unix-only — you cannot check a umask on Windows.
It is not fine for the ones that are gated because the *test fixture* only
works on unix, because those are usually the byte-exactness tests: precisely
the assertions written to prove that a non-UTF-8 path survives a conversion.
The gate silently converts "we proved bytes are preserved" into "we proved
nothing, anywhere." The `sudo` conversion (see
`TD-B-SUDO-DIED-BEFORE-ITS-FIRST-STATEMENT-ON-A-NON-UTF-8-COMMAND-LINE`) hit
this live: seven tests written with `quoting::os_from_bytes` failed on Windows
with `left: [..., 239, 191, 189] right: [..., 255]` — the fixture itself had
been lossily converted to `U+FFFD` before the assertion ever ran. Had they been
gated `#[cfg(unix)]` instead of fixed, they would have "passed" forever by not
existing.

### Where it lives

The gated-test population, largest first:

| Tests | File |
|---|---|
| 58 | `userspace/coreutils/src/bin/cp.rs` |
| 22 | `userspace/coreutils/src/bin/mv.rs` |
| 17 | `userspace/coreutils/src/dirfd.rs` |
| 15 | `userspace/coreutils/src/fsattr.rs` |
| 15 | `userspace/coreutils/src/bin/tar.rs` |
| 9 | `userspace/coreutils/src/bin/rm.rs` |
| 7 | `userspace/coreutils/src/bin/touch.rs` |
| 5 | `userspace/coreutils/src/bin/ln.rs` |
| 4 | `userspace/sshd/src/lib.rs` |
| 3 each | `sudo/src/main.rs`, `coreutils/src/umask.rs`, `mkdir.rs`, `rmdir.rs` |
| 1–2 each | 27 further files |

The three in `sudo` and some of the coreutils ones are *correctly* gated — they
are the byte-exact controls that sit beside a portable test of the same
property, added deliberately in the same commit. The rest have not been
triaged.

### The root cause of the fixture half

`quoting::os_bytes` / `quoting::os_from_bytes`
(`userspace/quoting/src/lib.rs:1325–1364`) are byte-exact **only** under
`#[cfg(unix)]`, where an `OsStr` is its bytes. Off unix they route through
`String::from_utf8_lossy` / `to_string_lossy`, so `b"tool\xff"` arrives as
`tool\u{fffd}`. That is the right behaviour for the library — there is no
byte-exact `OsString` on Windows — but it makes the pair useless as a *test
fixture* for "not valid Unicode," and the natural reaction to the resulting
failure is to gate the test rather than to fix the fixture.

### The fix, which is cheap

Windows `OsString` is WTF-8: it can hold an **unpaired surrogate**, which no
`&str` can. So a portable "this is not valid text" fixture exists:

```rust
fn not_text(prefix: &str, suffix: &str) -> OsString {
    #[cfg(unix)]
    let out = { /* prefix + byte 0xff + suffix, via os_from_bytes */ };
    #[cfg(not(unix))]
    let out = {
        use std::os::windows::ffi::OsStringExt;
        let mut w: Vec<u16> = prefix.encode_utf16().collect();
        w.push(0xD800);                       // unpaired surrogate
        w.extend(suffix.encode_utf16());
        OsString::from_wide(&w)
    };
    assert!(out.to_str().is_none(), "the fixture must not be valid text");
    out
}
```

The trailing assertion is the important line: it makes a platform whose fixture
quietly became valid text fail loudly instead of asserting nothing. The
reference implementation is in `userspace/sudo/src/main.rs` (test module, near
the `-- argv is bytes --` block), together with the doc comment explaining why
`argv_bytes` may not be used off unix.

This covers everything that only needs *an argument that is not Unicode* —
parsing, propagation, comparison, non-collapse, error messages. It does **not**
cover "these exact bytes come out the other end," which genuinely cannot be
expressed on Windows; those stay `#[cfg(unix)]`, and should be written as a
*second* test beside a portable first one, never as the only one.

### What to do

Triage the 198. For each, decide which of three it is:

1. **Behaviour is unix-only** (umask, symlink, fifo, `st_mode`, `dirfd`) —
   correctly gated, leave it, but note that it is unverified on the dev host.
2. **Fixture was unix-only** — rewrite with the `not_text` shape above and
   remove the gate. This is the bug class; every conversion in this tree
   written before 2026-09-06 has its non-text coverage confined to unix and
   therefore, in practice, to nowhere.
3. **Correctly gated byte-exact control with a portable sibling** — leave it.

Start with `cp.rs` (58) and `mv.rs` (22), which are more than a third of the
total and are also the two programs whose conversions found the most bugs.

A checker for this belongs in `scripts/` eventually — something that flags a
`#[cfg(unix)]` test whose body mentions `os_from_bytes`/`os_bytes` and has no
non-gated sibling — but the triage does not need it and should not wait for it.

### Why it survived

Because a gated-out test is indistinguishable, in `cargo test` output, from a
test that does not exist: the count simply is what it is, and nothing reports
"40 files contributed zero tests on this host." The failure mode is silence,
and it is self-reinforcing — the fastest way to make a failing byte-exactness
test green on Windows is to gate it, which is exactly the wrong move and looks
like the right one.
