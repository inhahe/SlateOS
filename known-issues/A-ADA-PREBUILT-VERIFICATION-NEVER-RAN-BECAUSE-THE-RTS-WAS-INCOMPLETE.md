## A-ADA-PREBUILT-VERIFICATION-NEVER-RAN-BECAUSE-THE-RTS-WAS-INCOMPLETE

**[A] 2026-09-05 — ✅ FIXED.**

**In short:** the kernel links a *committed* Ada object rather than compiling
Ada on every build. Two things are supposed to guard it: a stamp proving the
object was built from the current sources, and a byte-for-byte comparison
against what the compiler actually produces. The second one has never run —
not on this machine, not on any machine, not once since it was written. A
directory it needed could not survive a `git clone`, and the resulting failure
was swallowed by a warning.

### What was wrong

`kernel/build.rs:169` calls `verify_against_toolchain` whenever a cross GNAT is
found. That function compiles `kernel/ada/src/virtqueue_descriptors.adb` with
`--RTS=kernel/ada/rts` and asserts the result equals
`kernel/ada/prebuilt/virtqueue_descriptors.o`.

GNAT validates the `--RTS=` path before it reads a line of Ada, and rejects one
with no `adalib/` next to `adainclude/`:

```
gnat1: RTS path not valid: missing adalib directory
```

Our RTS is a stub — `adainclude/system.ads` and nothing else, because we link no
Ada runtime — so `adalib/` was legitimately empty. **Git cannot track an empty
directory.** It existed on whichever machine first generated the object, was
never committed, and has been absent from every checkout since.

The compile therefore failed on every build, and the failure landed in the `_ =>`
arm, which printed a `cargo:warning` and continued. The only symptom was one
line inside a build log — in the run that exposed this, a 10 712-second one:

```
warning: kernel@0.1.0: Ada toolchain found but failed to compile
virtqueue_descriptors; using the committed object. The stamp check still passed.
```

`regen-prebuilt.py` was broken by the same cause, with worse consequences: its
`compile_units` exits on a failed compile, so **there was no supported way to
move the stamp forward at all.** The one documented remedy for a stamp mismatch
would itself have failed.

### The warning was not merely quiet, it was wrong

"The stamp check still passed" is offered as reassurance, and it contradicts the
comment 90 lines above it in the same file, which is correct:

> the stamp records what the object should be built from, not what it contains.

The stamp hashes `ADA_FLAGS`, `system.ads`, `x86_64-elf.atp` and the `.ads`/`.adb`
sources. It cannot see the object. Detecting a tampered or mis-generated object
is the *entire* purpose of the byte comparison, so a message telling the reader
that the surviving check covers the skipped one states the opposite of the truth.
This is the project's recurring shape — "the check did not run" wearing the
costume of "the check passed" — and here the costume was hand-stitched.

### The object itself is fine

Verified 2026-09-05 before changing anything: creating `rts/adalib` makes the
compile succeed, and the produced object is **byte-identical** to the committed
`virtqueue_descriptors.o`, with and without a `.gitkeep` inside the directory.
The committed artifact is authentic and the gate would have passed every time.
What was lost was the guarantee, not the object — which is exactly why nothing
ever pointed at it.

### How it is fixed

- **`kernel/ada/rts/adalib/.gitkeep`** — holds the directory in the tree. Its
  contents explain what deleting it would silently cost, since an empty file
  named `.gitkeep` invites exactly the deletion that caused this.
- **`build.rs` now separates the two failures**, which were never the same kind
  of thing. A missing `adalib/` is a defect in the *repository*: identical on
  every machine, fixed by a checkout, and now a hard `assert!` that names the
  directory and the remedy. A toolchain that is present but fails to compile is
  a property of one developer's install and stays a warning — but the warning
  now says the verification was **skipped** and that the stamp cannot substitute
  for it.
- **`regen-prebuilt.py` gets the same guard**, diagnosing the missing directory
  itself rather than surfacing gnat1's wording, which suggests nothing about a
  checkout being the remedy.

Verified in both directions: with `adalib/` present, `regen-prebuilt.py --check`
prints `prebuilt is current` — the first time the byte comparison has ever been
made. With it renamed away, both the script and the build fail with the new
message and exit 1.

### Standing lesson

An empty directory is not representable in git, so any check whose *validity*
depends on one is a check that disables itself on the next clone and reports
nothing. The general form: when a guard degrades to a warning, the warning is
the only thing standing between a silent gap and a person noticing — so it must
state what is no longer being checked, never what still is. A degradation
message that lists the surviving checks reads as reassurance and buries the gap
it was written to expose.
