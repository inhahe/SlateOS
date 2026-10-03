## B-tee-REPORTS-SUCCESS-AFTER-LOSING-THE-DATA (lane B, 2026-08-22) — FIXED 2026-08-22

**In short:** `tee` copies a stream to a file so you have a durable copy of it —
`build 2>&1 | tee build.log`. The shipped `tee` threw away every error it hit
while doing that and then exited 0, so a log that could not be created, could
not be written, or was left half-written in a buffer that failed to empty all
looked to the calling script exactly like a log that was written perfectly. A
script that checks `tee`'s exit status — which is the only thing it *can* check
— was told the copy was safe when the copy did not exist.

**The offending code**, `userspace/coreutils/src/bin/tee.rs` as of `f631b7f23`:

```rust
        match file {
            Ok(f) => files.push(f),
            Err(e) => {
                eprintln!("tee: {}: {e}", quotef_os(path));   // warns, then carries on
            }
        }
...
            Ok(n) => {
                let _ = out.write_all(&buf[..n]);             // discarded
                for f in &mut files {
                    let _ = f.write_all(&buf[..n]);           // discarded
                }
            }
...
    }
}                                                             // no flush, no exit status
```

Three separate ways to lose the data, one shared consequence — exit 0:

| What happens | What the user saw | What GNU does |
|---|---|---|
| a file cannot be opened (bad path, no permission, read-only fs) | message on stderr, **exit 0**, other copies written | message, **exit 1** |
| a write fails (disk full, I/O error, quota) | **silence**, exit 0 | message, **exit 1** |
| buffered output fails when the file is closed | **silence**, exit 0 | message, **exit 1** |

The third is the worst of the three because it is the *normal* failure mode for
a full disk: the writes all succeed into an 8 KiB buffer and the error surfaces
only on the final flush. Dropping the `File` at end of `main` flushes it and
discards the result, so `tee` could hold the last block of a log in memory,
fail to write it, and report success. Nothing on stderr either — the previous
code did not merely mis-report the status, it never mentioned the failure at
all.

**What it does now.** A `status` variable starts at 0 and is set to 1 by any of
the three, and `main` ends in `process::exit(status)`; the handles are held as
`Vec<(String, File)>` so a write error can name the file that failed (a `File`
cannot report its own path); a destination that fails a write is *dropped* via
`retain_mut` rather than retried, so one full disk produces one message rather
than one per 8 KiB of input; and every flush — stdout's and each file's — is
checked. `parse_args` also gained `--` and the `--append` spelling, so a file
genuinely named `-a` can now be written to.

**The one thing deliberately left as success:** `BrokenPipe` on stdout.
`cmd | tee log | head -1` shuts the pipeline down by closing the reader, and
that is how a pipeline is *supposed* to end, not a failure of this program. It
exits with whatever status the file copies earned.

`-i`/`--ignore-interrupts` is absent on purpose rather than missing. The
standalone `userspace/tee` implements it (`main.rs:121`), but `design.txt`
rules out Unix signals for process control, so there is no SIGINT here to
ignore; a flag that accepts an argument and does nothing is worse than no flag.

**How this was found.** The same way as `chmod -r` and `dd seek=`: the survey in
`scripts/dup-bins-survey.py` ranks the duplicated binary names by size, and a
much *smaller* shipped `coreutils` bin against a much bigger standalone twin
(139 lines against 397) is a prompt to read the small one. The standalone
`userspace/tee` gets all three cases right — it checks both flushes
(`main.rs:323,334`) and has an explicit `BrokenPipe` predicate (`main.rs:204`) —
and cannot run, because nothing produces an executable for it. **Three for
three: every tool read this way so far had a silent-wrong-behaviour bug.**

**No harness covers this.** There is no `tee-diff.sh`, and there could not
usefully be one built on `diff`ing stdout: every defect here is about a *file*
and an *exit status*, neither of which a stdout comparison sees. The 11 unit
tests in the file cover argument parsing only; the write/flush paths are
verified by reading, not by test, which is the gap to close if a filesystem
harness is ever built (the same gap `dd` has).
