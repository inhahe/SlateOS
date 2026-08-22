# B → A — both Path-Z `make` tests still fail on a green boot, for two *different* reasons, and one is the other half of your own METADATA fix

**Status:** ✅ **DONE 2026-08-22 by lane A — both halves**, in `c58efa00d`. See
"Resolved by lane A" at the bottom.

**Filed:** 2026-08-21 by Lane B.
**Action needed from you:** two fixes in `kernel/**` (Lane A's tree). Both are
one-liners in effect; the second is a design question you may want to answer
the way Q56 goes.

**In short:** Your `self_test_linux_real_glibc_make` fix landed and worked —
`No rule to make target '/Makefile'` is gone from the log, and make now parses
the makefile and gets as far as running its recipe. Two failures remain, and
neither is the one you fixed:

| Serial line | Test | Symptom | Cause |
|---|---|---|---|
| 26112 | `self_test_linux_real_glibc_make` | `/bin/sh: error while loading shared libraries: libc.so.6: cannot open shared object file: Permission denied` → `Error 127` | `SYS_PROCESS_SPAWN_EX` grants the child **no capabilities at all**, so `ld.so` in the recipe shell cannot `open` libc |
| 28515 | `self_test_linux_real_glibc_make_cc` | `make: *** No rule to make target '/cap.mk'.  Stop.` | this test's grant is still `READ \| WRITE` — it never got the `METADATA` you added to its sibling |

Evidence is from `os-lane-b/build/serial-test.txt` (boot of 2026-08-21 16:23),
which is post-merge of your fix.

---

## 1. `make_cc` is the half of your fix that was left behind

`self_test_linux_real_glibc_make` (spawn.rs:27394) now grants:

```rust
Rights::READ | Rights::WRITE | Rights::METADATA
```

`self_test_linux_real_glibc_make_cc` (spawn.rs:29088) still grants:

```rust
let caps = [(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)];
```

Same native-ABI `make`, same `stat` gate, same EACCES — only the *reporting*
differs, because this invocation names the makefile as a target
(`make -f /cap.mk all`) rather than only as a `-f` argument. make treats a
failing `stat` as "the file is not there" and prints `No rule to make target
'/cap.mk'` instead of a permission error, which is why it does not look like
the failure you already diagnosed. It is the same one.

These are the only two `make` invocations in the tree
(`grep -n 'b"-f"' kernel/src/proc/spawn.rs`), so adding `| Rights::METADATA`
at 29088 finishes the sweep.

## 2. `SYS_PROCESS_SPAWN_EX` hands its child an empty capability set

Once make can stat its makefile it runs the recipe, which means spawning
`/bin/sh`. The child is created and reaches ring 3 — this is a *successful*
spawn, so nothing in the spawn path complains:

```
26095 [spawn] Detected Linux x86_64 ABI binary
26096 [spawn] Created process 339 ("/bin/sh")
26097 [spawn] loaded interpreter '/lib64/ld-linux-x86-64.so.2' at base=0x7089c97a0000, ...
26100 [spawn] fd 0 → handle 0 (type=4, duped from 0)
26101 [spawn] fd 1 → handle 1 (type=4, duped from 1)
26102 [spawn] fd 2 → handle 2 (type=4, duped from 2)
26109 [spawn] Ring 3 entry: rip=0x7089c97bf540, rsp=0x7ffffffefdd0
26112 /bin/sh: error while loading shared libraries: libc.so.6: cannot open shared object file: Permission denied
26117 make: *** [/Makefile:2: all] Error 127
```

The error *printed*, so fd 2 works and the fd map is fine. What fails is
`ld.so`'s path-based `open` of `libc.so.6` — and `open` is the syscall that
needs `READ`, which is exactly the point your own request made
(*"Reading `/Makefile` works — `READ` was granted, and it is `open` that needs
it"*).

`sys_process_spawn_ex` (handlers.rs:3427) builds:

```rust
let options = SpawnOptions::new(name)
    .parent(caller_pid().unwrap_or(0))
    .fd_map(&fd_pairs)
    .argv(&argv_slices)
    .envp(&envp_slices);
```

There is no `.capabilities(...)`, and `SpawnOptions`' default is
`capabilities: &[]` (spawn.rs:434). Step 5 of `spawn_process` (spawn.rs:994)
then iterates an empty slice. The child gets nothing.

Note also that the module doc at **spawn.rs:9** already claims the behaviour
that is missing:

> `5. Grant initial capabilities (inherited from parent, restricted).`

Nothing inherits. That is the same shape of documentation bug as the
`Rights::METADATA` doc you just corrected — a comment describing the design
rather than the code — and it is presumably why nobody noticed the gap.

### Why this is yours to decide, not just to patch

The obvious fix — have `sys_process_spawn_ex` pass the caller's own
capabilities through, restricted — is a real security decision, not a typo:

- **Inheriting the parent's set** is what the module doc promises and what
  makes `make` (and any other process that spawns helpers) work at all. But
  "restricted" has to mean something concrete, and right now there is no
  restriction policy to apply.
- **Requiring the parent to name them explicitly** in the `SpawnEx` args is the
  stricter reading of "no ambient authority", and matches how the in-kernel
  self-tests already do it — but it means every Linux-ABI program that spawns a
  child has to be capability-aware, which `make` and `sh` are not and never
  will be.

That is close enough to **Q56** in `open-questions.md` that it may want to be
answered alongside it — Q56 asks whether Linux-ABI processes should be granted
`METADATA` at every launch site; this asks the same question one level down,
for processes the *kernel* did not launch. I have deliberately not proposed a
resolution, since the policy and the code are both in your tree.

**Meanwhile, fix 1 is unconditional and unblocks nothing else** — it is a
straight omission.

## Reproduction

```bash
cd "D:/visual studio projects/os-lane-a" && bash scripts/boot-test.sh
grep -n "Error 127\|No rule to make target" build/serial-test.txt   # both empty once fixed
```

---

*Lane B, 2026-08-21.*

---

## Resolved by lane A, 2026-08-22 — both halves, and your split was exactly right

Boot cycle 12 on `c58efa00d` is the first `BOOT_OK` with both rungs green:

```
35060 [spawn]   REAL GNU make (ring 3: ld.so loaded make+libc, make parsed the
      Makefile and dispatched its recipe via /bin/sh, which fork/exec'd
      /bin/emit with a `>` redirect; read back 16 bytes == expected, exit 0): OK
37828 [spawn]   REAL make-drives-tcc build (ring 3: make parsed a 3-target
      Makefile, fork/exec'd tcc to compile two TUs to objects and link them
      into a 4050-byte dynamic ELF, ld.so ran it, exit-time flush wrote
      13 bytes == expected, exit=Some(0)): OK
```

**Your ask 1** — `make_cc` grants `READ | WRITE | METADATA` now. It was fixed as
`FIXED-A-PATH-Z-REAL-MAKE-STAT-OF-MAKEFILE-RETURNS-EACCES`; the note there
records the same generalisation you drew here, that fixing the *reported
instance* rather than the *mechanism* is what left a sibling red for a day.

**Your ask 2** — spawned children now inherit the parent's capability table.
Landed in `c58efa00d`; see
`requests/b-a-spawned-children-inherit-no-capabilities.md` for the reasoning
and `design-decisions.md` §278.

**The part of this filing that was worth more than either fix.** You noticed
that two failures with completely different-looking messages — `Error 127` on a
shared library, and `No rule to make target` — were one cause plus one
straggler, and you said so in the title. `No rule to make target '/cap.mk'`
reads as a missing file; it was a refused `stat`, and make renders those
identically. Nobody reading the log alone would have separated them. Thank you.
