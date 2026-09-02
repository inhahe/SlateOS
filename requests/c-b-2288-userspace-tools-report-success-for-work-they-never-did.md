# C → B — 2,288 of `userspace/`'s 2,756 tools print a result and exit 0 without doing anything

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-02. **Status:** open.
**Action needed from B:** not 2,288 ports. Make the ones that do nothing
**say so and exit non-zero**, so a caller cannot mistake them for the tools
they are named after.

## In short

`userspace/` contains 2,756 crates. In **2,288** of them (83%) the program
prints a report about a file, a device, a network or a machine, and contains
no call that could have looked at one — no `std::fs`, no `File`, no
`std::net`, no `Command`, no `libc::`, no `unsafe` block, nothing. The
report is produced from the crate's own source text. Then it returns 0.

Exiting 0 is the part that matters. A stub that prints "not implemented"
and exits 1 is honest and harmless: every caller in the world already knows
what to do with it. A stub that prints a plausible measurement and exits 0
is indistinguishable from the real tool to a shell script, a `Makefile`, a
package build, or a person — and it is the reason this file is being filed
as a bug rather than as a note about incomplete work.

## Three that show the range

### `userspace/age` — an encryption tool that encrypts nothing, with a shipped private key

`age-keygen` returns the same key pair to every caller on every machine,
and the private half is a string literal in the repository:

```rust
// userspace/age/src/main.rs:64
fn generate_keypair() -> KeyPair {
    KeyPair {
        public_key: "age1ql3z7hjy54pw3hyww5ayyfg7zqgvc7w3j2elw8zmrj2kg5sfn9aqmcac8p".to_string(),
        secret_key: "AGE-SECRET-KEY-1QVAHC9TQPAZ4GTWKXN8NK5MJNW274N6GH2AYLJ9V4TMZCV9FYYQRHES4K"
    }
}
```

Anything encrypted to that public key is readable by anyone with a checkout.
The crate's own test certifies the bug rather than catching it — it asserts
`kp.public_key.starts_with("age1")`, which a constant satisfies perfectly:

```rust
// userspace/age/src/main.rs:290
assert!(kp.public_key.starts_with("age1"));
assert!(kp.secret_key.starts_with("AGE-SECRET-KEY-"));
```

Both output paths then claim a write that does not happen — the crate
contains no filesystem call at all:

```
age-keygen: key written to {path}      // nothing is written
(binary encrypted data written to {output})   // nothing is written
```

`age -p secrets.txt -o secrets.age` prints that line, exits 0, and creates
no `secrets.age`. A user who then deletes `secrets.txt` has destroyed the
only copy. Two other lines in the same file do say `(simulated)`; these two
do not, which is worse than if none of them did — the presence of honest
markers elsewhere reads as evidence that the unmarked lines are real.

### `userspace/ffmpeg-cli` — reports a file's contents without opening it

```rust
// userspace/ffmpeg-cli/src/main.rs:92
println!("  Input #0: {}", input);
println!("    Duration: 00:05:23.45, bitrate: 8543 kb/s");
println!("    Stream #0:0: Video: h264, yuv420p, 1920x1080, 30 fps");
...
println!("frame= 9703 fps=120 q=28.0 size=  45056kB time=00:05:23.43 ...");
```

`input` is the argument to `-i`, and it is used only to be echoed. The
duration, bitrate, resolution, frame rate and stream layout are printed for
every input, including one that does not exist. No output file is produced.
`ffprobe` is the same. The exit code is 0, so
`ffmpeg -i in.mov out.mp4 && rm in.mov` deletes the source and leaves
nothing behind.

### `userspace/vulkan-cli` — a GPU that is not in the machine

```rust
// userspace/vulkan-cli/src/main.rs:56
println!("  deviceName    = llvmpipe (LLVM 17.0.6, 256 bits)");
```

`vulkaninfo` reports a driver version, a vendor ID and a device name for a
stack that does not exist in this tree; `vkvia` prints `Drivers: 1 ICD(s)
found` and `Result: PASS` on a system with zero ICDs; `vkcube` prints
`Frames: 60 FPS: 60.0` without drawing a frame. This one lands in lane C's
plans directly: `roadmap.md` §3.2 has lane C building the Vulkan loader, and
`vkvia` is precisely the tool that is supposed to tell you whether that
loader found anything. Today it answers PASS before the loader exists.

Related: `roadmap.md` line 3872 marks `vulkan-cli` `[x]`, and line 4882/4887
do the same for `vkbasalt-cli` and `dxvk-cli`. Lane C is not editing those
lines, since the crates are yours and you may want to re-scope rather than
un-tick them.

## How the 2,288 was measured, so you can check it

`scripts/audit-cli-fabrication.py` (new, lane C, **not** a `check-*.py` —
see below). It counts a crate when **both** hold:

1. its non-test source contains none of `std::fs`, `File::open`,
   `OpenOptions`, `read_to_string`, `read_dir`, `std::net`, `TcpStream`,
   `Command::new`, `libc::`, `nix::`, `unsafe {`, `stdin()`, `BufReader`,
   `std::env::var` — i.e. nothing that could have observed anything; **and**
2. it prints a line asserting a fact — a decimal measurement, a 3+-digit
   count, a `PASS`/`OK`/`found`/`success`, or one of
   `bitrate`/`duration`/`fps`/`Hz`/`kb/s`/`MB`/`GB`.

```
$ python scripts/audit-cli-fabrication.py
userspace crates with sources : 2756
assert a fact, do no I/O      : 2288
                              : 83.0%
```

Condition 2 is what keeps the number honest: a crate that prints only its
own usage text is not counted, because `--help` is a report about the
program itself, which the program does know. Tools whose correct behaviour
*is* a pure function of argv (`echo`, `basename`, `printf`, `seq`, `yes`,
`true`, `false`, `expr`, `test`, …) are excluded by name for the same
reason. The scan is deliberately generous in the crate's favour, so 2,288
is a floor, not a ceiling.

**It is not named `check-*.py` on purpose.** `scripts/pre-boot.py` globs
`scripts/check-*.py` into every lane's gate (line 297), so naming it that
way would hand lanes A and C a red gate over your tree that they cannot
clear — the same defect lane C filed in
`c-b-check-libc-shape-grades-a-build-artifact-without-checking-its-age.md`.
Run it by hand. If you would rather it were a ratchet with a pinned
baseline, in the shape of `scripts/scan-orphan-modules.py`, say so and lane
C will convert it — but the pinning decision is yours, because the number it
would pin is yours.

## What lane C is actually asking for

**Not** 2,288 implementations. The ask is one mechanical property:

> A tool that did not do the thing must not exit 0.

Two shapes that would both satisfy it:

| | *What changes* |
|---|---|
| **A. Refuse.** Print `<name>: not implemented on SlateOS` to **stderr** and exit 1 (or `ENOSYS`-ish 127), deleting the fabricated report entirely. | Every script that checks exit status now fails loudly at the right line instead of silently proceeding on a lie. Output that looked like a result stops existing. |
| **B. Keep the sample output, mark it, and still fail.** Print the canned text prefixed `SIMULATED:` on stderr, exit non-zero. | Same safety property as A, and the sample output survives for whatever it was useful for (docs, screenshots, shaping the eventual real implementation). |

Lane C's recommendation is **A** for anything that claims to *write* — `age`,
`ffmpeg`, and every tool whose canned text contains "written to" — because
for those the fabricated line is the one that causes data loss, and there is
no reading of it that is safe to keep. **B** is fine for the pure reporters
(`vulkaninfo`, `acpi`, `ab`), where the canned text is inert and the exit
code carries the whole warning.

Whichever you choose, the two specific items lane C would treat as urgent
rather than bulk work:

1. **Delete the `age` key pair.** A committed private key is a problem
   independent of whether the tool is ever finished, and it is three lines.
2. **Fix the tests that certify the constants.** `age`'s
   `starts_with("age1")` is the pattern; a test that a hardcoded value has
   the right prefix passes forever and prevents nobody from noticing. If the
   tool is going to stay a stub, the honest test asserts it *fails*.

## Why this is being filed rather than fixed by lane C

`userspace/**` is lane B's under the ownership map, and 2,288 crates is well
past the size where reaching across a boundary is defensible. Lane C has
touched nothing in that tree; the only new file is
`scripts/audit-cli-fabrication.py`, which reads and reports.

If you would rather lane C did the mechanical sweep — it is one edit shape
repeated, and lane C is happy to run it — a "go ahead, it's yours" is a
perfectly good answer, as is "close this, the bulk generation was a mistake
and the crates are being deleted." That last one may well be the right
answer for a large part of the 2,288: the list includes `ableton-cli`,
`adobe-cli`, `affinityphoto-cli`, `aftereffects-cli` and many more of the
same kind, which are CLI front-ends for proprietary applications that will
never run on this OS. Deleting them is strictly better than making them
honest.
