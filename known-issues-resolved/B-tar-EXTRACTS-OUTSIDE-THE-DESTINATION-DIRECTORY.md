## B-tar-EXTRACTS-OUTSIDE-THE-DESTINATION-DIRECTORY (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** `tar -xf downloaded.tar` unpacked an archive by writing each file
to whatever path the archive said, and the archive is written by whoever made
it. A file inside it called `../../etc/passwd` was written to `../../etc/passwd`
— outside the directory you unpacked into, and outside the one you named with
`-C`. So unpacking any archive you did not build yourself let its author
overwrite an arbitrary file with the permissions of whoever ran `tar`. This is
the long-known "tar slip"/"zip slip" vulnerability class, and it was present in
**both** implementations in this tree.

**The offending code**, `userspace/coreutils/src/bin/tar.rs` as of `88b52a28a`:

```rust
        let name = extract_string(header_buf.get(..100).unwrap_or(&[]));
        ...
            b'0' | b'\0' => {
                if let Some(parent) = Path::new(&name).parent() {
                    let _ = fs::create_dir_all(parent);      // creates ../../etc too
                }
                ...
                if let Err(e) = fs::write(&name, &file_data) { ... }
```

`name` goes from the 100 header bytes to the filesystem with nothing in
between. Two distinct escapes:

| Member name in the archive | Where it was written | Does `-C safe/` help? |
|---|---|---|
| `../../etc/passwd` | two directories above the destination | no — `..` is applied after the chdir |
| `/etc/shadow` | `/etc/shadow`, absolutely | **no** — an absolute path ignores the base entirely |

The absolute case is the nastier of the two, because `-C` is the thing a
careful user reaches for and it provides no protection whatsoever:
`Path::join` with an absolute right-hand side *discards* the left-hand side.

**Not just this copy.** `userspace/tar/src/main.rs:1064` does
`base_dir.join(&output_path_str)` with the same unchecked name, so the
standalone twin has the identical hole. This is the first tool in this audit
where the bigger standalone implementation was **not** the more correct one —
it has `--strip-components` and `--exclude`, neither of which is a security
boundary. That copy cannot currently run (nothing produces an executable for
it; see `B-DOZENS-OF-COMMANDS-EXIST-IN-SOURCE-AND-CAN-NEVER-BE-RUN`), so the
fix went into the shipped file; whichever survives **B-Q7** must carry
`sanitize_member_name` with it.

**What it does now.** `sanitize_member_name` sits between the header and every
filesystem call, and treats the two escapes differently on purpose:

- **A leading `/` is stripped**, with GNU's one-time `Removing leading '/' from
  member names` notice. Archives of system trees are routinely made with
  absolute paths and are safe to unpack elsewhere, so refusing them would break
  a common case for nothing.
- **A `..` component is refused and the member skipped.** It cannot be stripped
  safely: `a/../b` is equivalent to `b` only if `a` is a real directory rather
  than a symlink, and the archive is exactly the thing we will not trust about
  that. A rare loud refusal is the right trade against an arbitrary file write.
- `.` components and doubled slashes are dropped, since they name the same path
  and exist chiefly to disguise the first two.
- The `..` test also splits on `\`, which is not a separator in this OS but is
  on the hosts this file's unit tests run on. Names are rebuilt with `/` alone,
  so a slateos filename that legitimately contains a backslash survives; only a
  literal `..\` component is refused.

Verified end-to-end against the built binary, 2026-08-22: a crafted archive
holding `../../ESCAPED.txt`, `/tmp/…/ABSOLUTE.txt` and one legitimate member
extracted **only** the legitimate one plus the de-absolutised one under the
destination, wrote nothing outside it, and exited 2. `tar -tf` still lists the
raw names — the point of listing an archive is to show you what is actually in
it.

**Four more defects fixed in the same pass**, all of the "reports success after
failing" family that `chmod`, `dd` and `tee` also had:

1. **Every `write_all` in create mode was `let _ =`**, and there was no flush.
   `tar -cf backup.tar big-tree/` on a full disk wrote a truncated archive and
   exited 0. Now any write error is reported once, aborts the archive (every
   later member would land at the wrong offset anyway) and exits 2.
2. **`Vec::with_capacity(size)` from the header's own size field.** A 512-byte
   archive whose header claims 2^40 bytes made `tar` try to reserve a terabyte
   before reading a single block — a denial of service costing the attacker one
   header. Extraction now streams block by block; the crafted 1 TiB header
   returns in milliseconds with `unexpected end of archive` and exit 2.
3. **`f.read(&mut buf).unwrap_or(0)`** in create mode treated a *short* read as
   end-of-file and NUL-padded the rest of the block, punching holes through any
   file the OS chose to deliver in pieces. It also wrote however many blocks it
   happened to read while the header declared `meta.len()`, so a file that
   shrank mid-archive desynchronised every subsequent member — the archive
   would list fine and extract garbage. Now the body is always exactly the
   declared length, short reads are looped over, and a genuinely short file is
   padded *and reported*.
4. **Silent skips.** An unreadable directory (`if let Ok(entries)`) dropped its
   whole subtree from the archive without a word; symlink, hardlink and device
   members were skipped on extraction without a word. Both now report and set
   the exit status. Directory entries are also sorted, so archiving the same
   tree twice produces the same bytes.

Exit status is GNU's: 2 for a fatal error, 0 only when every member was
actually handled. `main` no longer exits inline from the middle of a mode.

**How this was found.** The same reading pass as `chmod -r`, `dd seek=` and
`tee`: `scripts/dup-bins-survey.py` flagged `tar` at 752 lines against
`userspace/tar`'s 1676, and the gap is a prompt to read the smaller one.
**Four for four** — every shipped `coreutils` bin read against its bigger
standalone twin so far has had a silent-wrong-behaviour bug, and this one was
exploitable.

**Not covered by any harness.** There is no `tar-diff.sh` and a stdout `diff`
could not have caught any of this — every defect above is about a *file*, an
*exit status*, or an *allocation*. The 40 unit tests now cover the sanitizer
and the block arithmetic; the create/extract I/O paths are verified by the
end-to-end runs recorded above rather than by test, which is the same gap `dd`
and `tee` have and the argument for a filesystem-level harness.

**Still not implemented, deliberately:** compression (`-z`/`-j`), pax/GNU long
names (paths over 100 chars, the `L` typeflag), sparse files, and restoring
mode/mtime on extraction. Adding them belongs with **B-Q7**, which decides
whether this file or `userspace/tar` survives.
