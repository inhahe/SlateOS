## B-TWO-SESSION-REGISTRIES (lane B, 2026-09-10) — open, and ENTIRELY OFF-IMAGE

**Severity corrected 2026-09-16, downward, and the direction is the point.**
The "What it costs" section below states user-visible harms in the present
tense. No user can reach any of them, because **none of the four programs
involved is on the image**:

| | in `scripts/rootfs-bin-manifest.txt`? |
|---|---|
| `who` | no |
| `su` | no |
| `loginctl` | no |
| `logind` | no |

Checked rather than inferred from the manifest alone, because the manifest is
not the only path into `/bin`: the rootfs script also stages a handful of named
tools (`dash`, `make`, `tcc`, `bash`, `pkgconf`) and "promoted" `fastpy-*`
fixtures. None of those is one of these four. `who` and `su` are not coreutils
personalities either -- there is no `who.rs` or `su.rs` under
`userspace/coreutils/src/bin/`. `loginctl` appears in the tree only in three
tooling baselines (`argv-utf8`, `multicall-aliases`, `workspace-lints`), never
in staging, and `init/` does not start `logind`.

So the split is **latent**: real in the source, unreachable from a booted
system. Every consequence listed below is a consequence of shipping these,
which nothing currently does.

**This is a size constraint, not an oversight, and the distinction matters to
whoever reads it next.** `TD-B-...-NOT-ON-THE-IMAGE`'s analysis already
explains it: all 276 userspace binaries build and come to 204 MiB against a
fixed 384M image already carrying ~127 MiB of fastpy test ELFs. They do not
fit. Raising `IMG_SIZE` is available and nothing outside the rootfs script
reads it, but "which utilities earn their bytes" is a real question and staging
everything that compiles is not an answer to it. Measured while checking this:
**3 of 214 `userspace/` crates reach `/bin`** -- `coreutils` provides 69 of the
75 manifest names, two have their own crate, and four are aliases.

**Why write this down rather than quietly lower the priority.** A severity
stated too high costs the same thing as one stated too low -- it moves work in
front of other work on evidence that does not hold. This entry reads like a
user-facing breakage and is a latent design defect, and the next person
triaging by severity would have started here.

**It also re-frames a day's work of mine honestly.** On 2026-09-15 I routed
nine `loginctl` commands through the service bus to the daemon, replacing a
local empty `Daemon` that answered every query wrongly. That work is correct
and tested and **is not reachable by any user**, for the same reason. It
improves the code; it does not yet improve the system. The prerequisite for
either that work or this entry to matter is wiring `logind` into the image and
starting it from `init/`, which nothing has asked for yet.

**What has NOT changed:** the two registries are still two, and the `su` record
that outlives a process that died without cleanup is still unreconciled. If
`logind` is ever shipped, this becomes live on the same day and should be fixed
before it is rather than after.
