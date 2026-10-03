### B-MKDIR-ALL-BUILT-A-RELATIVE-PATH-FROM-AN-ABSOLUTE-ONE. Every `mkdir -p` in the kernel failed `InvalidArgument`; boot panicked mounting `/tmp` — 2026-08-13 — FIXED 2026-08-13

**Where:** `kernel/src/fs/vfs.rs`, `Vfs::mkdir_all` (~line 2217); the same bug
in `kernel/src/fs/pathbar.rs`, `parse_breadcrumbs`.

**Symptom:** the QEMU boot test died at
`panicked at kernel\src\container.rs:6996:40: add /tmp tmpfs: InvalidArgument`.
Not a `/tmp`-specific fault: *every* caller of `mkdir_all` was broken, and
`container.rs` was simply the first one on the boot path that treats the
failure as fatal.

**Root cause — the one sharp edge of `Path::components()`.** `components()`
drops empty components, which is exactly what makes it robust against `//`,
a trailing `/`, and `.`-free normalisation. The corollary is easy to miss:
the leading `/` of an absolute path *is* an empty leading component, so it is
dropped too. `components()` on `/a/b` yields `a`, `b` — nothing that says
"absolute".

`mkdir_all` rebuilt the prefix chain by pushing components onto a fresh
`PathBuf::new()`:

```rust
let mut built = PathBuf::new();          // WRONG
for comp in &components { built.push(comp); ... Self::stat(&built) ... }
```

so the first probe was `stat("a")`, a *relative* path, which
`validate_path` rejects with `InvalidArgument` ("must be absolute") before
any filesystem is consulted. The function could never get past its first
component.

**Fix:** seed the accumulator with the root separator, with a comment naming
the trap so the next reader does not re-introduce it:

```rust
// Seed with the root separator, not an empty buffer: `components()`
// drops the leading `/`, so pushing the first component onto an empty
// `PathBuf` would build a *relative* path and the `stat` below would
// fail `validate_path` ("must be absolute") before touching the disk.
let mut built = PathBuf::with_capacity(norm.len().saturating_add(1));
built.extend_bytes(b"/");
```

`pathbar::parse_breadcrumbs` had the identical shape and emitted relative
breadcrumb paths (`home`, `home/user`) for an absolute input; it is now seeded
with `PathBuf::from(if normalized.is_absolute() { "/" } else { "" })`.

**Audit.** All 19 `components()` rebuild loops in the kernel were checked
after this. The rest are either correctly seeded (`cap/file_tags.rs` ×2,
`vfs.rs::normalize_path`, `ipc/namespace.rs::normalize_jailed`,
`memfs.rs::parent_path_of`) or deliberately relative (`overlay.rs` ×3,
`container.rs:3106`, `oci.rs::archive_norm`, `oci.rs:1154`, `cpio.rs:349`,
`pathutil.rs:121`, `kshell.rs:79987`). **This is a recurring trap, not a
one-off** — treat "rebuilding a path from `components()`" as a code smell and
check the seed every time.
