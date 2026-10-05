## osh: a special redirection filename must re-open fd N's file, not dup fd N

**Status:** fixed 2026-08-26 (found the same day while triaging
`the-status-of-a-fatal-expansion-abort-belongs-to-whoever-caught-it`). The
`source` half landed in `0f3dfac29`; the redirect half, and the two structural
blind spots it exposed, are described at the end of this entry.

`interp.rs::resolve_special_redirect` resolves `/dev/stdin`, `/dev/stdout`,
`/dev/stderr` and `/dev/fd/N` into the *dup* of descriptor N, and says so:

```rust
    /// See [`special_redirect_fd`] for the names. `read` picks the input side:
    /// `< /dev/fd/3` shares fd 3's cursor exactly like `<&3`, while
    /// `> /dev/fd/3` writes to it like `>&3`.
```

That claim is false, and it is false on *both* hosts — so it is not one of the
MSYS artifacts this migration has been turning up, just an unmeasured
assumption. bash does not special-case these names at all when the host
provides `/dev/fd` (`HAVE_DEV_FD`): it calls plain `open()` and lets the kernel
resolve the path, which yields a **new file description positioned at 0**, not a
second reference to the existing one. Measured, bash 5.2, glibc and MSYS
identical, against `t2.sh` = `echo A\necho B`:

```text
                                                         bash          osh
exec 3< t2.sh; read -u 3 l; cat < /dev/fd/3              echo A|echo B  echo B
exec 3< t2.sh; read -u 3 l; cat <&3                      echo B         echo B
{ read -r l; cat < /dev/stdin; } < t2.sh                 echo A|echo B  echo B
exec 3< t2.sh; read -u 3 l; cat < /dev/fd/3 >/dev/null
  ; read -u 3 m; echo "m=[$m]"                           m=[echo B]     m=[]
```

The last line is the half that a dup can never reproduce: after the re-open the
shell's *own* cursor on fd 3 is exactly where `read` left it, because the two
descriptions are independent. Under `<&3` it is at end of file (`m=[]`), which
is what osh produces for both.

The write side is wrong in the more damaging direction — `>` on a re-opened
path carries `O_TRUNC`:

```text
exec 3> w1;  echo AAAA >&3; echo B >  /dev/fd/3; …; cat w1   bash B/          osh AAAA/B/
exec 3>> w3; echo AAAA >&3; echo B >> /dev/fd/3; …; cat w3   bash AAAA/B/     osh AAAA/B/AAAA/B/
```

so osh keeps data that bash discards, and the append case additionally doubles
its output.

The same root cause is why `source` is wrong: `builtin_source` opens its
operand as a *host* path (`std::fs::read(self.host_path(path))`,
interp.rs:54582), so `/dev/stdin` reaches the process's real fd 0 rather than
the shell's modelled one. Measured with `printf 'echo REAL\n' |` feeding the
shell's ambient stdin:

```text
                                              bash                         osh
source /dev/stdin < t3.sh                     THREE                        REAL
source /dev/stdin <<< "echo A"                A                            REAL
source /dev/stderr 2< t3.sh                   THREE                        hangs (rc=124)
source /dev/stdout 1< t3.sh                   THREE + write error, st=1    hangs
exec 3< t3.sh; source /dev/fd/3 3<&-          No such file or directory,1  THREE
source /dev/stdin < dd     (dd a directory)   …: is a directory, st=1      REAL, st=0
source /dev/stdin <&-                         No such file or directory,1  REAL, st=0
{ read -r l; source /dev/stdin; } < t2.sh     A|B  (rewinds)               REAL
printf 'echo A\necho B\n' | { read -r l
  ; source /dev/stdin; }                      B    (pipe, no rewind)       REAL
printf 'echo FROMFILE\n' > o3
  ; source /dev/stdout >> o3                  sources it — o3 doubles      hangs
```

`/dev/fd/N` for N >= 3 is right on Linux today only by accident: Rust's first
`File::open` tends to land on host fd 3, so the host path resolves to the same
file. It is wrong on Windows, wrong on SlateOS, and wrong on Linux the moment
the descriptor is closed for the command (`3<&-`).

**The rule both sides need.** Opening a special filename means: find the file
behind the shell's *modelled* descriptor N and open it afresh with the
redirect's own mode. Consequences, all measured above:

| fd N's binding | opening the special path yields |
|---|---|
| a seekable file | an independent description at offset 0; `>` truncates, `>>` appends |
| a pipe | the same pipe — no rewind is possible, so reading continues |
| a here-document / here-string | **this row was wrong** — it is a *pipe* at or below 64 KiB and a temp file above, and only the temp file rewinds. Corrected and measured in "a here-document over 64 KiB is a temp file, which `/dev/stdin` re-opens from the start", below. |
| write-only (fd 1 over a file) | still readable — `source /dev/stdout >> f` sources `f` |
| unbound or closed (`<&-`) | `<path>: No such file or directory`, status 1 |
| a directory | `<path>: is a directory`, status 1 |

**Fix.** One resolver used by both `resolve_special_redirect` and
`builtin_source`, replacing the dup. The handles osh models are real host
descriptors, so the faithful re-open is the one the kernel would do —
`/proc/self/fd/<raw>` on Unix, `GetFinalPathNameByHandleW` + re-open on
Windows — with `InputSrc::Bytes` re-read from index 0 for the here-document
case and the closed/unbound/directory shapes mapped to the diagnostics above.
Recording the origin path on the handle instead would be simpler but diverges
for the `exec 3< f; rm f; cat < /dev/fd/3` idiom, which bash serves from the
still-open inode.

### How it was fixed

`/proc/self/fd/<raw>` is not merely *a* way to re-open the descriptor's file —
it is literally what `/dev/fd` is a symlink to, so substituting it and letting
the ordinary open proceed **inherits** every one of the six behaviours in the
table above rather than emulating them. `O_TRUNC`, `>>`, `set -C` having a real
file to protect, the descriptor's access mode not constraining the redirect,
the live inode behind an unlinked path, and the fresh offset all fall out for
free. So the shape of the fix is *substitute the path and fall through*:
`resolve_special_redirect` now returns a path to open, and the callers open it
exactly as they would have opened the word the user wrote.

Two consequences of that shape needed structure of their own:

- **The word the user wrote is what a diagnostic must name.** `>` on a
  protected file has to say `/dev/fd/3: cannot overwrite existing file`, never
  `/proc/self/fd/3: …`. `noclobber_check`, `open_input_source` and
  `open_output_target` therefore take the original word alongside the path to
  open.
- **`/proc/self/fd/N` names a file only while fd N is open.** Some of the
  descriptors answered here are made on the spot — a snapshot of a pipeline
  stage's pipe, a dup of the real stdout — so returning the path alone let the
  handle drop at the end of the resolver and the caller's `open` fail with
  `ENOENT`. Measured: `{ echo hi > /dev/stdout; } | sed` died exactly that way.
  `struct Reopen { path, _keep: Option<Arc<File>> }` ties the two lifetimes
  together.

### Two blind spots the fix exposed, both now closed

Neither was introduced by this change; both were latent and only became
visible once the special names started asking the shell "what *is* fd N here?"
in earnest.

**Redirect resolution could not see the command's own stdin.** A `RedirPlan` is
built from the redirect list alone, so when nothing in the list and no `exec`
had touched fd 0, the resolver had nothing left to consult and fell back to the
*process's* fd 0 — the harness's pipe, not the command's. `AmbientStdin`
(`Process` / `Fd(InputFd)` / `Opaque`) is now recorded on the plan when it is
built, so fd 0 resolves to what the command was actually handed. The `Opaque`
arm — a pipeline stage's reader, which has no name and no rewind — deliberately
falls through to the dup, because that is what bash's re-open of a pipe yields
anyway.

**The write side had the same hole, one number over, and it was worse.**
`write_special_src` fell back to `host_reopen_path(1)` / `(2)`: the process's
descriptors. Inside a command substitution fd 1 is a capture buffer, in a
pipeline stage it is the stage's pipe, and a scoped `2>` lives on
`stderr_stack` — naming `/proc/self/fd/1` walked straight past all three and
wrote to the terminal. `x=$(echo hi > /dev/stdout)` printed `hi` and left `x`
empty. The fix reuses `alias_write_fd(n, out)`, which already answers "what is
fd `n` *here*" for `N>&n` and already knows about `exec_stdout_shadowing`,
`Out::Capture`, `Out::Pipe` and `snapshot_std_fd`; fd 2 additionally consults
`stderr_target_at`. Writing a second, parallel answer would have been the
mistake.

### Verified

`tests/corpus/redirect-dev-fd.sh` was rewritten around the open-not-dup
contract — the old file asserted the dup reading in its header *and* in a probe
(`"and it is a dup, so noclobber has no file to protect"`), both false — and
now probes all six differences. A 28-probe scratch suite run side by side with
glibc bash matches on 26; the two exceptions involve no special filename at all
and are the separate pipeline-stage-stdin defect recorded below.
