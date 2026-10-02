### B-KSHELL-APPEND-TRUNCATES-BINARY-FILES. `cmd >> file` silently discarded the entire existing contents of any file that was not valid UTF-8, and reported success — 2026-08-24 — ✅ FIXED 2026-08-24 by lane A (`kernel/src/kshell.rs`, `redirect_write`)

**Where:** `kernel/src/kshell.rs`. Four duplicated copies of the append path,
at (pre-fix) lines 5896, 5982, 6038 and 6128 — the plain `>`/`>>`
(`execute_redirect`), the `>>`-with-piped-input, the heredoc suffix, and the
input-redirect suffix.

**What it was.** The VFS has no append mode, so `>>` is a read-modify-write:
read the file, concatenate, write it back. All four copies did the read like
this, byte-for-byte identically:

```rust
let existing = Vfs::read_file(path).unwrap_or_default();     // <-- (2)
let mut combined = match core::str::from_utf8(&existing) {
    Ok(s) => String::from(s),
    Err(_) => String::new(),                                 // <-- (1)
};
combined.push_str(&output);
Vfs::write_file(path, combined.as_bytes())
```

At **(1)**, a failed UTF-8 decode yields **an empty string**. So for any file
whose bytes are not valid UTF-8 — an archive, an image, a compiled object,
anything binary — `echo x >> file` read the file, threw all of it away, wrote
back just the new output, exited 0, and printed nothing. Append became
truncate, silently, for exactly the files where the loss is unrecoverable.

At **(2)**, the same shape one level up: a *permission* or *I/O* error was
also laundered into "no previous contents", so a transient read failure
overwrote the file rather than refusing.

**Why it survived:** it is invisible to any test written in text. Every `>>`
test in the tree appends to a UTF-8 file, where the decode succeeds and the
code is correct. And four copies meant four chances to get the read half right
and none to notice that none of them had.

**The fix.** One `redirect_write(path, output, append)`; the four sites now
call it. It concatenates `Vec<u8>` with no decode at all, and it distinguishes
the errors instead of flattening them: `NotFound` means "start empty" (`>>` is
allowed to create), and **every other error is propagated** — the caller
reports `Redirect error: …` and sets exit 1, rather than overwriting.

**How it is kept fixed:** `kshell::self_test` section 6 writes
`\x80\xffhead`, appends `tail\n`, and asserts the whole of both is on disk;
asserts `>` still truncates; and asserts `>>` onto an absent path creates it.
Real VFS writes under `/tmp`, not a mock, because the bug lived in the
read-modify-write and not in the decision to append.

**Found by:** the byte-clean output-sink conversion above — changing
`capture_command`'s return type made the compiler point at all four copies at
once. Worth noting against the previous entry's warning that "the types will
not tell you": there, a `display()` inside `format!` type-checked its way past
review; here, a deliberate type change was exactly what surfaced the bug. The
lesson is not that types are useless, it is that they only help where the
conversion actually reaches.
