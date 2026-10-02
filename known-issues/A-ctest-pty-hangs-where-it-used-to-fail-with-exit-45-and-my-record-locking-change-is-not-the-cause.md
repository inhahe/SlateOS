### [A] `ctest-pty` HANGS where it used to fail with exit 45, and my record-locking change is not the cause -- 2026-09-22
**Status:** ROOT-CAUSED 2026-09-24 — the record-locking change was indeed not the cause. The hang is the same two 2,000,000-iteration spins as exit 45, run on a debug kernel, where they outlast the 2400 s boot budget; both spins exist only because no `SIGINT` was ever raised. Fix: `A-PTY-CTRL-C-IS-ONLY-SEEN-BY-A-READER` at the end of this file.

**In short:** a userspace test that used to fail now hangs instead, which turns
a red boot into an incomplete one. That is worse, because a red-but-complete run
still verifies everything else, while this one stops the kernel before roughly
half its self-tests have run.

**What happened.** Boot 8 reached QEMU, ran `ctest-pty`, wrote VINTR (Ctrl-C)
to the pty master, and then produced 12 KB in 20 minutes before timing out:
*"BOOT_OK not found within 2400s"*. The previous green boot had this rung FAIL
with exit 45 and still reach `BOOT_OK` with 2.76 MB of serial (A-Q20).

**The cost is not the failure, it is the position.** The rungs downstream of it
never ran: `sealing`, `reclock`, `immutable` and `acl`'s Test 12 -- four of the
five file-identity rungs this lane spent the day building. `flock`'s rung and
both range self-tests are upstream and did report OK.

**Why my changes are probably not the cause.** I had a specific reason to
suspect them: I had just wired `fcntl(F_SETLK)` to real locking and added
`reclock::release_ofd` to `fs::handle::close`, which every close now runs. So I
checked what the fixture actually calls:

| call | count | reaches my changes? |
|---|---|---|
| `fcntl(fd, F_SETFL, O_NONBLOCK)` | 3 | **no** -- `F_SETFL` takes a different `fcntl` arm; the only `F_*` constant in the file is `F_SETFL`, and there is no `F_SETLK`/`F_GETLK` anywhere in it |
| `close` | 3 | yes, via `release_ofd` -- but that takes `reclock`'s `TABLE` briefly on an **empty** table, and nothing on the pty path holds that lock, so no cycle |
| `ioctl` | 1 | no -- the new arms match only `BLKDISCARD`/`BLKSECDISCARD`/`BLKZEROOUT` |
| `waitpid`, `signal`, `tcsetattr` | 15 | untouched by this lane today |

**So the likeliest explanation is the rung's known timing sensitivity.** Exit 45
was diagnosed as `waitpid(WNOHANG)` SPIN exhaustion, and a test that fails on a
spin budget is exactly the kind that hangs instead under different timing. What
changed the timing is not established -- `release_ofd` does add one lock
acquire/release to every final close, which is a real if small perturbation.

**Not proven, and I should be explicit about that.** Inspection cannot rule out
a timing interaction, and the discriminating experiment -- boot with
`release_ofd` reverted -- has not run. It costs a full boot, and boots are
currently blocked by WSL being down on this host.
