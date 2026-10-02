## B-BACKUP-`restore`-WROTE-OUTSIDE-ITS-DESTINATION-AND-EXITED-0-AFTER-FAILING (lane C, 2026-08-16) — FIXED

**In short:** `backup restore --dest /tmp/check` was supposed to put everything
under `/tmp/check`. Some entries were written to absolute paths instead — a
backup of `/etc` could restore straight over the live `/etc`. Separately, a
restore in which every single file failed still printed a summary and exited 0,
so `backup restore … && rm -rf original/` would delete the original after
restoring nothing. Both fixed; the restore now refuses out-of-destination paths
and returns an error when anything failed.

### The bug

Three defects in `cmd_restore`, `apps/backup/src/main.rs`.

**1. `Path::join` replaces its base when the argument is absolute.**

```rust
let dest_path = opts.restore_dest.join(&entry.path);
```

`Path::new("/tmp/check").join("/etc/passwd")` is `/etc/passwd`. This is the
classic archive-traversal ("zip slip") vector, and it is *not* hypothetical
here — the backup program puts absolute paths in its own manifests.
`relative_path` is `full.strip_prefix(base).unwrap_or(full)`: whenever the
prefix does not match, the full absolute path is stored. `..` components were
equally unchecked, and reach anywhere the absolute case does.

**2. Failure was reported as success.**

```rust
println!("\nRestore complete:");
println!("  Files restored: {restored}");
if errors > 0 { println!("  Errors: {errors}"); }
Ok(())
```

Exit status 0, and the word "complete", after restoring nothing. Any script
that guards a deletion on the restore succeeding was guarding on a constant.

**3. Symlink creation results were discarded.**

```rust
std::os::unix::fs::symlink(target, &dest_path).ok();
restored += 1;
```

Counted as restored whether or not the link was made, so a restore that
dropped every symlink reported complete success. An `is_symlink` entry with no
`link_target` was silently skipped and also counted.

### The fix

A new pure function, `restore_path_within(dest_root, rel) -> Option<PathBuf>`,
walks the components and **refuses** — returns `None` — for `ParentDir`,
`RootDir` or `Prefix`, and for a path with no `Normal` component at all (which
names the destination directory itself, not a file).

Refusing rather than sanitising is the deliberate choice, and it is specific to
*restore*. Silently rewriting `/etc/passwd` to `<dest>/etc/passwd` would put
the user's data somewhere they did not ask for with no way to notice; a
refusal, counted as an error, is visible and the restore of everything else
still proceeds.

Failures are counted per entry and the run returns `io::Error` at the end if
any occurred. Directory-creation failures stay counted rather than propagated —
one unwritable directory must not strand the other several thousand files —
but they now reach the exit status.

### Verification

Five tests for `restore_path_within`: the ordinary relative case (including `.`
noise), an absolute entry, a `..` climb, an entry naming no file at all, and a
Windows drive prefix (`#[cfg(windows)]`).

Clippy: this work also converted the file's path formatting from `{:?}` to
`.display()` and the restore counters to `saturating_add`, taking backup-app
from 198 non-test warnings to 183.

### Still open in this program

- **The blob GC can delete the blobs of an *in-progress* backup.** `meta.json`
  is written last, so a backup still being written is invisible to
  `list_backups`, and a concurrent `prune` sees its already-stored blobs as
  unreferenced. Needs the same kind of liveness primitive the indexer now uses
  (an OS-held lock on the destination), not a marker file. Not fixed.
