## §205 — The Ada toolchain is the **`x86_64-elf` cross-GNAT** on a **ZFP runtime**; the object is committed and stamped, so no other lane needs a 1 GB install

**Date:** 2026-08-16
**Decided by:** Claude (autonomous) — these are the two sub-decisions §201
explicitly left to Lane A, plus a factual correction to §201 that emerged in
carrying them out.
**Lane:** A

**In short:** §201 approved installing Ada and its prover but deliberately left
two follow-on choices open: *which* Ada compiler to install, and *which*
cut-down runtime library to build against. Both are now settled by having
actually done it. The compiler is the one that produces code for bare metal
rather than for Windows; the runtime is the smallest one there is, which for
our purposes turns out to be a single 200-line file we now keep in the tree.
A third thing came out of the work: §201 says GPL "is not a problem here"
because we only *run* the toolchain — but we ended up copying one file out of
it into our own source tree, so that reasoning does not actually cover us. A
different licence provision does. This entry records what that is.

---

### 1. Which GNAT: `gnat_x86_64_elf`, not `gnat_native`

**What changes:** the compiler emits ELF objects that link into the kernel,
instead of Windows PE-COFF objects that cannot.

Alire publishes both under confusingly similar names, and they are not
interchangeable:

| Crate | Host | Emits | Usable for the kernel? |
|---|---|---|---|
| `gnat_native` | Windows | PE-COFF (mingw) | **No** |
| `gnat_x86_64_elf` | Windows | ELF, bare-metal `x86_64-elf` | **Yes** |

Both *run* on Windows; the difference is what they produce. `gnat_native` is a
mingw compiler whose output is a Windows object file, and `rust-lld` linking an
ELF kernel cannot consume it.

**This overturns a conclusion recorded in an earlier session.** That session
probed `gnat_native`, found PE-COFF, and concluded that Ada in the kernel
required building the whole toolchain under WSL. That conclusion was sound
reasoning applied to the wrong crate — the cross-compiler was on the same
Alire index the whole time. Installed path:

```
~/AppData/Local/alire/cache/toolchains/gnat_x86_64_elf_15.3.1_880685c3/bin/x86_64-elf-gcc.exe
```

`gnatprove` (FSF 16.1.0, `~/.alire/bin/`) is host-side and target-agnostic, so
it needs no cross build — but see the trap in §3 below, because "target-agnostic"
is precisely the problem.

*Consequence:* the WSL Alire install made on the PE-COFF premise
(`~/opt/alire`, `~/.local/bin/alr`) is now dead weight and can be removed.

### 2. The restricted runtime: **ZFP**, vendored as one file

**What changes:** instead of configuring and building a runtime library, we
carry a single `system.ads` in the tree and pass `--RTS=kernel/ada/rts`.

§201 framed this as "configuration work with real content." Measured, it was
not, and the reason is worth recording because it makes the choice obvious
rather than a judgement call:

| | ZFP (Zero FootPrint) | Light |
|---|---|---|
| Exceptions | none | propagation, needs unwinder |
| Tasking | none | none |
| Files needed here | **`system.ads` only** | `system.ads` + `adalib` (built) |
| Undefined symbols left | **one** | several |

For a package that does arithmetic on array indices and dereferences nothing,
ZFP costs a single file and leaves exactly one undefined symbol,
`__gnat_last_chance_handler`, which `kernel/src/ada.rs` exports and routes to
the kernel panic path. Light would buy exception propagation that this
component has no use for — SPARK *proves* the exceptions cannot be raised, so a
runtime able to propagate them is machinery for an impossible case.

The vendored `system.ads` needed exactly one edit from the stock copy:
`Duration_32_Bits` set for the target. Everything else is verbatim.

Choosing ZFP is reversible: moving to light later means building `adalib` and
changing one `--RTS=` path, with no source change in the Ada.

### 3. **Correction to §201's licence reasoning** — the Runtime Library Exception is what applies

§201 point 3 states: *"GPL is not a problem here. The toolchain is a tool we
run, not something we link; it does not reach our output."*

**That is true of the compiler and false of what we actually shipped.**
Vendoring `system.ads` copies a GPL-3 file *out of* the toolchain and *into*
SlateOS, and the compiled kernel contains code derived from it. The
"we only run it" argument does not reach this case at all.

What does reach it is the **GCC Runtime Library Exception**, which GCC's
runtime files (`system.ads` among them) carry precisely so that compiling
against them does not impose GPL on the result. It is the same provision that
lets proprietary programs be compiled with GCC and link `libgcc`. So the
outcome §201 asserts is correct — but for a different reason, and the
distinction matters the next time someone reasons from §201's stated rationale
to a case the Runtime Library Exception does *not* cover (e.g. vendoring a file
from a GPL project that has no such exception).

**§201 is left unedited** — it is Operator-attributed and the shared documents
are append-only. This entry is the correction of record; a reader arriving at
§201 point 3 should be brought here.

### 4. The consequence that shaped the build: prebuilt object + stamp

The cross-GNAT is a ~1 GB install, and the boot test builds the **whole
workspace** — so requiring the toolchain would block Lanes B and C over a
component neither touches. Requiring it is not acceptable; skipping
verification is not acceptable either. The resolution:

- `kernel/ada/prebuilt/virtqueue_descriptors.o` is **committed**, and
  `kernel/build.rs` links it rather than compiling Ada.
- `kernel/ada/prebuilt/stamp.txt` holds a SHA-256 over the compiler flags and
  every source input. **`build.rs` recomputes it on every build, everywhere**,
  and refuses to build on mismatch. This needs no toolchain, and it is the
  half that catches the dangerous case: a source edited without regenerating.
- Where a toolchain *is* present, `build.rs` additionally recompiles and
  **byte-compares** — catching a tampered or hand-edited object, which the
  stamp alone would not. GNAT's output was verified deterministic here (same
  digest across runs and working directories), which is what makes the
  comparison meaningful rather than flaky.
- `kernel/ada/regen-prebuilt.py` is the **only** supported way to move the
  stamp forward, and it **re-runs `gnatprove` and refuses to write if any check
  is unproved**. Regenerating the object is exactly the moment the proof stops
  applying, so a fresh-but-unproved object is worse than a stale one: the stamp
  would then assert a freshness the proof does not back.

The stamp is computed by both Python and Rust. That duplication is deliberate
and safe **because it is self-policing, not merely documented**: one side
writes the digest and the other checks it, so any divergence fails *every*
build on the next run, on every machine, with the mismatch printed. The
alternative — a build-time dependency to share one implementation — would add a
crate to the kernel build to remove a risk that already announces itself.

**Where it lives.** `kernel/ada/` (sources, `virtqueue.gpr`, `x86_64-elf.atp`,
`rts/adainclude/system.ads`, `regen-prebuilt.py`, `prebuilt/`);
`kernel/build.rs` (`ada_stamp`, `verify_against_toolchain`, `find_ada_gcc`);
`kernel/src/ada.rs` (FFI wrappers and the panic bridge). Current state:
**105/105 verification conditions discharged, zero unproved, zero warnings.**
