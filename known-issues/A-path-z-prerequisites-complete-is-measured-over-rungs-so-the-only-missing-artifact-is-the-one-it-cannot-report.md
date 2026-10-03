### [A] `Path-Z prerequisites: complete` is measured over rungs, so the only missing artifact is the one it cannot report -- 2026-09-18
**Status:** OPEN (the blind spot closes when the cmake rung lands; the per-lane image divergence does not)

**In short:** the boot prints a line confirming that everything the
toolchain tests need is present. It is counted by asking each test whether
its files are there. `cmake` is on the list of things that should be in the
image, is **not** in mine, and has no test -- so nothing asks, nothing is
missing, and the line says complete. It is also the case that two lanes
building the image from the same commit get different images, because what
gets staged depends on build output git does not carry.

**The verdict.** Boot `a17b8e0fa`:

```
[spawn] Path-Z prerequisites: complete -- 0 rungs skipped
```

That is honest about what it measures. `pathz_fixtures_missing`
(`spawn.rs:144`) checks each rung's own fixture list, calls `pathz_skip` on
an absent source, and the count is surfaced at end of boot precisely so lost
coverage is visible rather than silent -- the design is already dd-942-aware,
which is what makes the gap interesting instead of careless.

**The population is rungs, and the thing at risk is artifacts.** Measured:

| spike artifact | staged by `create-ext4-rootfs.sh` from | named by a rung's fixture list | absence visible? |
|---|---|---|---|
| `bash-slateos.elf` | `build/spike/` | yes (1 ref) | yes -- skip counted |
| `pkgconf-slateos.elf` | `build/spike/` | yes (2 refs) | yes -- skip counted |
| `make-slateos.elf` | `build/spike/` | yes (2 refs) | yes -- skip counted |
| `python-slateos.elf` + `python312.zip` | `build/spike/` | yes (1 ref) | yes -- skip counted |
| **`cmake-slateos.elf` + `cmake-data`** | `build/spike/` | **no (0 refs)** | **no -- nothing to skip** |

So `0 rungs skipped` is true, and the artifact that is actually absent is
the only one it structurally cannot mention. A missing prerequisite becomes
visible only when something tries to use it -- dd-946's publisher/subscriber
shape, arriving this time as a *coverage* verdict rather than a feature.

**And cmake really is absent here.** Three independent confirmations rather
than one, because the whole point of this entry is not trusting a single
signal:

1. `build/spike/cmake-slateos.elf` does not exist in this worktree.
2. `create-ext4-rootfs.sh:1291` stages `/bin/cmake` only inside
   `if [ -e "$CMAKE_SLATE" ] && [ -d "$CMAKE_DATA/share" ]`.
3. Grepping `rootfs.ext4` itself: the fixtures are there
   (`slateos-deliberate-failure`, `slateos-input-payload`, `cmake-selftest`)
   and the binary's own strings are not (`Could not find CMAKE_ROOT`,
   `CMakeDetermineCCompiler`).

Lane B's five fixtures shipped and the binary they exercise did not.

**The second half, which does not close with the rung.** Seven staged paths
resolve under `$ROOT_DIR/build/`, which is gitignored:

```
BASH_SLATE  PKGCONF_SLATE  MAKE_SLATE  CMAKE_SLATE  CMAKE_DATA  PY_SLATE  PY_ZIP
```

So the image's contents depend on which spikes the *building lane happens to
have built*, and the artifact cannot travel through git. Two lanes running
the same script at the same commit get different images, and therefore
different boot coverage, with nothing in either boot saying so. Lane B's
request states "cmake is staged and linked and that is my half done" --
true where they are, false where I am, and neither statement is wrong.

That is the *where* twin of dd-937/938: an artifact true when written does
not say when it stopped being true, and one true where written does not say
where it stops being true.

#### Measured one level deeper 2026-09-18: the divergence is not only the artifact

Three caches with three different scopes, which is why this is structural
rather than an oversight:

| thing | scope | set by |
|---|---|---|
| source tarballs | **shared** between lanes | `SLATE_ZIG_CACHE=$HOME/.cache/slateos` |
| spike artifacts (`*-slateos.elf`) | **per-lane** | `SLATE_SPIKE=$SLATE_ROOT/build/spike` |
| `zig` itself | **per-lane if present, else shared** | declared `SLATE_ZIG=$SLATE_SPIKE/zig/zig`, but `slate_ensure_zig` reassigns it to the shared cache when no pinned per-worktree copy exists |
| `toolchain/sysroot/lib/libc.a` | **per-lane** | gitignored at `.gitignore:68` |

So `cmake-4.4.3.tar.gz` was downloaded once (2026-09-11, cached, still
there) while the binary built from it exists in exactly one tree. And the
last row is the one that matters most: **the thing the spike proves a claim
about is itself per-lane.** "cmake links clean against our `libc.a`" is a
statement about one lane's libc at one moment, not about the project.

*(Corrected within the hour: the `zig` row first said flatly "per-lane",
which I took from the variable's declaration at `worktree.sh:90` without
accounting for `slate_ensure_zig` reassigning it at lines 306/314/348. It
prefers a pinned per-worktree copy and falls back to the shared cache; in
this tree it resolved to the shared one, `build/spike/zig` does not exist,
and the function reported `/home/inhahe/.cache/slateos/zig-.../zig`. Reading
a declaration and not the call is the same error as reading a name and not
line 1 -- caught here only because I ran the function and read where the
file actually landed.)*

**Concretely, and this is why copying is not the shortcut it looks like:**

| | timestamp |
|---|---|
| `os-lane-b/build/spike/cmake-slateos.elf` | 22,525,984 bytes, 2026-09-15 19:31 |
| `os-lane-a/toolchain/sysroot/lib/libc.a` | 11,782,644 bytes, 2026-09-16 02:16 |

Lane B's binary predates my libc by about seven hours, so copying it would
trip `create-ext4-rootfs.sh`'s own guard -- *"cmake-slateos.elf is OLDER
than the sysroot libc.a -- it links a stale libc and proves nothing about
the current one"* -- and a rung built on it would report a green verdict
about a libc that is not the one in this tree. The guard is right and it is
the reason the build has to be local. Worth recording that the shortcut was
considered and refused on evidence, not skipped on principle.

**Proper fix, in two parts.** (a) Add the cmake rung -- which converts a
silently missing artifact into a counted skip, and is wanted on its own
merits (`requests/b-a-cmake-needs-a-ring-3-rung-like-the-other-three.md`).
(b) Make the verdict count *intended* artifacts, not just rung fixtures: the
staging script already knows all seven and prints a line per artifact, so
the honest verdict is one that reconciles what the script staged against
what the rungs found, rather than only the latter. Without (b) the next
artifact added without a rung reproduces this exactly.
