## B-A-BOOT-TEST-CAN-PASS-AGAINST-A-ROOTFS-IMAGE-OLDER-THAN-ITS-FIXTURES (lane B, 2026-08-16)

**Status: half fixed — the checker exists and is verified; the boot test does
not call it yet.** Filed to lane A as
`requests/b-a-boot-test-boots-a-rootfs-image-that-may-predate-the-fixtures-in-it.md`.

**Sibling, found the same day and independently:**
`B-A-A-FAILED-ROOTFS-REBUILD-LEAVES-THE-OLD-IMAGE-AND-THE-NEXT-BOOT-TEST-STILL-PASSES`
(lane A). The two are one false green reached by opposite routes — there the
rebuild *ran and aborted*; here it was never run at all. Lane A's fix makes the
abort loud at the moment it happens; this one makes the image answer for itself
afterwards. Neither subsumes the other: a loud abort is invisible to whoever
boots that image tomorrow, and a manifest cannot be written by a script that
exited before writing one. They compose — after an aborted rebuild the old image
keeps its old manifest, and `image-check` then names exactly which ELFs moved.

### What happens

`scripts/boot-test.sh` builds the kernel from the tree and then boots it
against `rootfs.ext4`, an image it does not build and does not inspect. The
ring-3 C fixtures (`services/ctest-*`) are executed from **inside that image**
(`load_test_elf()` reads `/mnt/tests/<name>.elf`), not from the tree. So if the
fixtures are rebuilt and the image is not repacked, the boot runs the previous
binaries and reports `=== Boot test PASSED ===`.

Observed today, on the change it would have falsified: 38 new `ctest-jobctl`
checks (150–187, the `WaitInfo` truncation and zero-fill coverage lane A asked
for by name), all nine ELFs rebuilt and re-stamped, a full 817 s boot test, and
a PASS in which none of the new checks executed. The tell was a single line —

```
[spawn] Running job control (ring 3, C, native ABI) integration test (2627416 bytes ELF)
```

— where 2 627 416 is the *committed* ELF and the tree's was 2 578 120. Without
that byte count in `spawn.rs`'s log line there would have been no signal at all,
and the merge to `main` would have carried a rung that had never run.

### Why every existing guard stayed quiet

This is the part that makes it worth an entry rather than a fix-and-forget.
Three gates already exist against fixture staleness, and all three were green:

| Guard | Compares | Verdict that day |
|---|---|---|
| `create-ext4-rootfs.sh` mtime gate | ELF vs `libc.a`; ELF vs `main.c`/`build.py` | pass — the ELF was the newest file involved |
| `ctest-fixtures.py check` | ELF *content* vs its inputs' content | pass — the ELF genuinely matched its source |
| `create-ext4-rootfs.sh` sysroot gate | `libc.a` vs `posix/src` | pass |

All three answer "was this ELF built from that source". None answers "is this
ELF the one in the image we are about to boot". A fourth question, unasked —
and note that a *stricter* version of any of the three would not have caught
it, because none of them can see the image at all.

### The fix, and the half of it that is done

`scripts/ctest-fixtures.py` gained `image-stamp` and `image-check`:

- `image-stamp` runs at the end of `create-ext4-rootfs.sh` (wired up, and fatal
  if it cannot run). It writes `rootfs.ext4.manifest` — gitignored, beside the
  image — recording the sha256 of every locally built ELF staged into it:
  `services/ctest-*/*.elf`, `services/fastpy-*/*.elf`, `build/spike/*.elf`
  (74 files today, so it covers the ported binaries too, which have no content
  stamp of their own).
- `image-check` compares that manifest against the tree and names each ELF that
  moved.

**Content hashes, not mtimes**, for a reason peculiar to this artifact: QEMU
writes to `rootfs.ext4` on every boot, so the image's mtime records when it was
last *run*, not when it was packed — it is reliably *newer* than the fixtures
it is stale with respect to. (The docstring's other reason applies as well: a
fresh clone flattens every mtime.)

What is missing is the call site. `scripts/boot-test.sh` is lane A's file, so
the `image-check` call before QEMU launches is in the request above. **Until it
lands the checker is inert** — it exists, it passes, and nothing invokes it, so
a stale image still buys a green boot in all three lanes.

### Reproducing

```
python scripts/ctest-fixtures.py image-check          # ok
printf 'x' >> services/ctest-pgroup/ctest-pgroup.elf
python scripts/ctest-fixtures.py image-check          # ERROR ... exits 1
```

### The adjacent gap this exposed

`scripts/bash-spike/cross2.sh` untarred `build/spike/bash-5.2.tar.gz`, a
gitignored path that **nothing in the tree ever wrote**. Repacking the image
requires relinking `bash-slateos.elf` (the rootfs script refuses a stale one),
relinking requires bash's objects, and rebuilding those requires the tarball —
which was in no script and no document, exactly the defect the zig pin fixed
earlier the same day, one line further down. Now pinned by version and sha256
as `slate_ensure_bash_src` in `scripts/lib/worktree.sh`, verified against
Chet Ramey's signature via GNU's own keyring before the hash was written down.
`run.sh` extracts on demand from the same pinned tarball rather than assuming
`build/spike/bash-5.2/` exists.

### And the third one, found by looking rather than by being bitten

Having pinned zig and bash on the same day, the obvious question was which
other third-party source this tree compiles into a shipped binary. There was
exactly one left: `scripts/pkgconf-spike/run.sh` fetched upstream pkgconf with

```sh
[ -f "pkgconf-$VER.tar.xz" ] || curl -sSLO "https://distfiles.ariadne.space/..."
```

That is a worse failure than either of the other two were, and worth stating
precisely rather than filing under "no hash". `curl -O` **without `--fail`
writes the response body on an HTTP error**, and without a `.part` file it
writes it under the final name. So a 404 page, or a connection cut halfway,
leaves a file that satisfies `[ -f ]` from then on. The next run does not
retry — it is *cached*. It untars whatever arrived and, if tar happens to
succeed, compiles it into `pkgconf-slateos.elf` and stages it in the image.
Neither the zig nor the bash gap could do that: those failed loudly by being
absent. This one fails quietly by being present and wrong.

Now `slate_ensure_pkgconf_src` in `scripts/lib/worktree.sh`, pinned at 2.3.0 /
`3a9080ac51d03615e7c1910a0a2a8df08424892b5f13b0628a204d3fcce0ea8b`, with
`--fail` and a `.part` file so an interrupted download is loud instead of
sticky. The hash was cross-checked against two independent packagers rather
than the distfiles server — asking a server to vouch for its own bytes is not
a check — and both also agree on the 316160-byte size: OpenBSD ports' distinfo
(base64 `OpCArFHQNhXnwZEKCiqN8IQkiStfE7BiiiBNP8zg6os=`, which decodes to that
hex) and OpenEmbedded-core's recipe.

`run.sh` also now **re-extracts unconditionally** instead of `[ -d
"pkgconf-$VER" ] ||`. That guard would have left the pin with a hole its own
size: a tree unpacked by an earlier run from an unverified tarball satisfies
`[ -d ]`, so on any machine that had already run the script once, verifying the
tarball would have changed nothing at all. It costs a second and is not even a
rebuild, since `configure` and `make` below run unconditionally regardless.

Both paths were exercised, not assumed — a pin whose *rejection* path has never
run is one you are trusting on the strength of its happy case:

| case | result |
|---|---|
| both copies good | resolves the scratch copy, `tar xf` from the verified path, link exit 0, 0 missing symbols |
| scratch copy corrupted | ignored *loudly*, naming both hashes; falls through to the cache copy; builds |
| both copies corrupted | `refusing to extract`, script exits **non-zero**, nothing is built |

`pkgconf-slateos.elf` came out byte-identical (`9f0a7647f11a71c0…`) on all four
rebuilds, including across the two different source paths, so this port is
reproducible and a change in that hash is evidence rather than noise.

One methodology note for whoever tests this next, because it cost a wrong
conclusion here first: **`$`-expressions inside `wsl -d Ubuntu -- bash -c '…'`
are eaten before WSL sees them**, single quotes notwithstanding. `…; echo
"RC=$?"` reported `RC=` and a control `bash -c "exit 7"` reported `0` — which
reads exactly like "the script refuses but exits 0", a serious bug and a
phantom. Use `&& echo ZERO || echo NONZERO`, which contains no `$`, and it
gives the true answer. The same artifact makes `echo "$SLATE_ROOT"` after
sourcing `scripts/lib/worktree.sh` look like the file sets nothing.
