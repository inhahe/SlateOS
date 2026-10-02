## §374 — The four WSL-built differential harnesses share one target directory, and building for Linux under WSL is now the standing answer for a `#[cfg(unix)]` binary

**Date:** 2026-08-23
**Decided by:** Claude (autonomous)

**In short:** Some utilities here only compile on Unix, so on this Windows
build machine they compile to a stub that prints "unix-only utility" and quits
— which means the automated tests never run a line of their real code. §365
solved that for `du` by building it for Linux inside WSL and comparing it
against the real GNU tool there. `find`, `ls` and now `cmp` do the same. This
entry does two things: it promotes that from "what `du` does" to the standing
answer for every such binary, and it merges the four separate build caches
those scripts had accumulated into one, because they were storing four copies
of the same compiled dependencies.

### The generalisation

§365 argued the case for `du` in `du`'s terms — `du` measures disk usage, and
disk usage only exists on a real filesystem. The argument turns out not to
depend on that at all. What actually drives it is the shape of the binary:

```rust
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("cmp: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(EXIT_TROUBLE)
}
```

Eight binaries in `userspace/coreutils/src/bin` are built this way — `chmod`,
`chown`, `cmp`, `du`, `find`, `id`, `ls`, `stat`. For all eight, `cargo test`
on the host reaches the argument parser and the string formatters and *nothing
else*: not the syscalls, not the I/O loop, not the error paths that wrap them.
And an ordinary `scripts/*-diff.sh` — which builds a native Windows subject and
reaches into WSL only for the GNU reference — would run the stub every time and
report a uniform disagreement that says nothing about the program.

Four of the eight now have a WSL-built harness (`du`, `find`, `ls`, `cmp`);
four do not (`chmod`, `chown`, `id`, `stat`). Several `known-issues.md` entries
for that second group end with "not verified end-to-end; needs QEMU". That is
no longer the right conclusion, and this entry is here so the next reader does
not draw it: they do not need QEMU, they need one script each. `cmp`'s is the
evidence — it had 33 passing unit tests and was clippy-clean on two targets
when the harness found three real defects, all of them in the half no host test
can reach.

*What this does not claim:* WSL's kernel is Linux and its filesystem is ext4,
so where our POSIX layer diverges from Linux the harness is silent. A QEMU
harness measuring the real target is still worth building, and when it is these
scripts are not thrown away — the case tables move into it and the WSL run
stays as the fast pre-check.

### The consolidation

The three existing scripts each named their own cache: `$HOME/du-diff-target`
(270 MB), `$HOME/find-diff-target` (116 MB), and `ls`'s. They build different
binaries out of *the same workspace, for the same target triple*, so the
compiled dependencies in them — `libc`, `memchr`, `bstr`, every local crate a
binary pulls in — are byte-identical across all three and stored three times.
`cmp` would have made four.

All four now use `$HOME/.cache/slateos-diff-target`. Cargo keys artifacts by
crate, features and target, so sharing is exactly what it is designed for: the
second harness to run after the first mostly links.

| | *What changes* |
|---|---|
| **A directory per tool** | Four caches, ~500 MB, most of it the same objects four times. Each harness's first run compiles the whole dependency graph again. |
| **One shared directory** *(chosen)* | One cache. A harness run after another one builds only its own binary. |

The argument for separate directories is isolation — one harness cannot
invalidate another's cache. It does not apply here: the invalidation that
matters is a *feature-set* difference, and these four builds share a workspace,
a profile and a triple, so there is nothing to differ over. (The genuine
conflict, which is why none of these use the repository's own `target/`, is
between the *Windows* build and a Linux one: same directory, different
fingerprint database, each forcing a full rebuild of the other. That remains
true and the shared directory is still outside the repository.)

Two further reasons for the location, both inherited from §365 and worth
restating: the repository is reached through `/mnt/d` inside WSL, where a Rust
build is an order of magnitude slower and where a second `target/` would be a
tens-of-gigabytes surprise for whoever next runs `du -sh` on the worktree; and
`D:` is the drive that actually runs out of space, while the WSL volume is not.
Each script's header says how to delete the cache.

**If this is revisited,** the thing most likely to force separate directories
again is a harness that needs a *different profile* — a release build, or one
with a feature flag the others do not set. At that point give that one script
its own directory and say so in its header, rather than un-sharing all four.
