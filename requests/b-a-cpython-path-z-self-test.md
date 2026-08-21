# B → A — CPython 3.12 is on the image and has never been executed; it needs a Path-Z rung

**Filed:** 2026-08-21 by Lane B. **Action needed:** one new function,
`self_test_cpython_on_slateos_libc()`, in `kernel/src/proc/spawn.rs`, plus its
two call sites in `kernel/src/main.rs`. Everything it needs is already staged on
`rootfs.ext4`. Exact argv, envp, expected stdout and a suggested yield budget
are below.

## In short

`/bin/python3` is now on the root filesystem image: a statically linked CPython
3.12.3 built from source and linked against `toolchain/sysroot/lib/libc.a` —
our own C library, not glibc — together with its entire standard library as a
single file, `/usr/local/lib/python312.zip`. It has never run.

Everything we can prove from the build host, we have proved: it compiles, it
links with **zero** missing symbols, and an interpreter built from the identical
object files (linked against musl instead of ours) starts from that same zip and
runs a fifteen-module workload correctly. What is left untested is **SlateOS
itself** — and that is precisely the part a Path-Z rung tests.

This is not a small addition to what the image already exercises. CPython
reaches for **478** external libc symbols; bash, the current widest consumer,
reaches for 2,030 *including* its own internal ones and resolves far fewer
against us. It is the first program on the image large enough that the libc
surface it touches is a meaningful fraction of the library.

## Terms used below

- **Path Z** — the set of ring-3 self-tests that run real userspace binaries
  staged from the ext4 image, as opposed to kernel-internal unit tests.
- **stdlib zip** — CPython's standard library packed into one archive.
  `<prefix>/lib/python312.zip` is the *first* entry of CPython's default
  `sys.path` and the code that reads it (`zipimport`) is compiled into the
  interpreter, so this is the layout CPython already looks for. It is 569 files
  our ext4 driver does not have to walk at every boot.
- **`PYTHONHOME`** — the environment variable naming the prefix that path is
  relative to. Without it the interpreter looks in a compiled-in location that
  does not exist here.

## What is already on the image

`scripts/create-ext4-rootfs.sh` stages both, under one condition and never
separately:

| Path on the image | Bytes | What it is |
|---|---:|---|
| `/bin/python3` | 11,210,656 | The interpreter. Static `ET_EXEC`, no `PT_INTERP`, no `ld.so`. `--strip-debug` (DWARF removed, `.symtab` kept). |
| `/usr/local/lib/python312.zip` | 20,498,464 | The standard library. Uncompressed (`ZIP_STORED`), holding both `.pyc` and `.py` for every module. |

The two are staged together because **an interpreter without its stdlib does not
start at all.** It is not that `import json` fails; startup fails, inside
`init_fs_encoding`, because CPython must import the `encodings` package before
it can decode a filesystem path, and `encodings` is not compiled into the
binary. If you see a rung fail with a message about `encodings` or about
`init_fs_encoding`, the zip is the thing to look at, not the interpreter.

`IMG_SIZE` rose from 256M to 384M to hold them.

## The suggested rung

Modelled directly on `self_test_bash_on_slateos_libc()`
(`kernel/src/proc/spawn.rs:23466`), which has the right shape already: stage
from `/mnt`, spawn, poll for `Zombie`, compare exit code and a written file.
The differences are called out after the sketch.

```rust
pub fn self_test_cpython_on_slateos_libc() -> KernelResult<()> {
    const EXPECT_EXIT: i32 = 0;
    const EXPECT_OUT: &[u8] =
        b"3.12.3\nzip\nb'slateos'\n{'a': [1, 2, 3]}\n(7, 42)\nSLATE_PYTHON_OK\n";

    const SRC_PY:  &str = "/mnt/bin/python3";
    const DST_PY:  &str = "/bin/python3";
    const SRC_ZIP: &str = "/mnt/usr/local/lib/python312.zip";
    const DST_ZIP: &str = "/usr/local/lib/python312.zip";
    const OUT_PATH: &str = "/py-out.txt";

    // See "the yield budget" below — this is the one number I cannot
    // responsibly guess for you.
    const MAX_YIELDS: usize = 8_388_608;

    if pathz_missing(
        "CPython 3.12.3 linked against OUR libc.a (ring 3)",
        &[SRC_PY, SRC_ZIP],
    ) {
        return Ok(());
    }
    // ... stage both files (mkdir_all("/usr/local/lib") for the zip) ...
```

with

```rust
let argv: &[&[u8]] = &[
    b"/bin/python3",
    b"-c",
    b"import sys, json, struct, base64, zipimport\n\
      with open('/py-out.txt', 'w') as f:\n\
      \x20   p = lambda s: print(s, file=f)\n\
      \x20   p('.'.join(map(str, sys.version_info[:3])))\n\
      \x20   p('zip' if '.zip' in json.__file__ else json.__file__)\n\
      \x20   p(repr(base64.b64decode(base64.b64encode(b'slateos'))))\n\
      \x20   p(json.loads('{\"a\": [1, 2, 3]}'))\n\
      \x20   p(struct.unpack('<HH', struct.pack('<HH', 7, 42)))\n\
      \x20   p('SLATE_PYTHON_OK')\n",
];
let envp: &[&[u8]] = &[
    b"PYTHONHOME=/usr/local",   // MANDATORY — see below
    b"PATH=/bin",
    b"LANG=C",
];
```

`capabilities`, `fd_map`, `priority`, `parent`, `exe_path`, `cwd` and `uid_gid`
are all exactly as the bash rung has them; the file capability
`(ResourceType::File, 1u64, Rights::READ | Rights::WRITE)` is what lets the
child open `/py-out.txt`.

### Why each expected line is there rather than a bare "it ran"

| Line | What it proves that the previous line does not |
|---|---|
| `3.12.3` | The interpreter reached the point of having a version — i.e. `Py_Initialize` completed, which means `init_fs_encoding` found `encodings` **in the zip**. This alone is most of the test. |
| `zip` | `json` was loaded *from the archive* and not from some directory that happened to exist. Guards against a future rung passing because somebody unpacked the zip. |
| `b'slateos'` | A C extension (`binascii`, via `base64`) ran, so `MODULE_BUILDTYPE=static` really did compile the extension into the binary rather than leaving a `.so` we cannot load. |
| `{'a': [1, 2, 3]}` | The C parser in `_json` ran, and the dict/list allocators behaved. |
| `(7, 42)` | `_struct` round-tripped, which exercises the memory layout assumptions CPython makes about our target. |
| `SLATE_PYTHON_OK` | The file was written to completion and closed through our libc. |

Writing through a file object rather than to stdout is deliberate and matches
the bash rung: it means the rung is checking *what CPython produced*, not what
happened to survive the serial console.

## Three things that will bite, in the order they will bite

**1. `PYTHONHOME=/usr/local` is not optional.** It is the only thing telling
the interpreter where `python312.zip` is. Without it the rung fails at startup
with a `Fatal Python error: init_fs_encoding` message. Do **not** "fix" that by
adding `-E`; `-E` tells CPython to ignore the environment, which discards
`PYTHONHOME` and guarantees the failure. (I made exactly this mistake on the
host and record it here so it costs nobody a second debugging session.)

**2. The yield budget is a genuine unknown and I am not going to pretend
otherwise.** bash gets 1,048,576 yields, described in its own comment as 4x
dash's budget for locale tables and a larger builtin table. CPython's startup
is a different order of work again: it initialises the type system, unmarshals
frozen modules, opens a 20 MB zip and reads its central directory, then imports
eight modules out of it before it will run a line of user code. `8_388_608` (8x
bash) is my suggestion, not a measurement. **If it times out, please raise it
and see whether it completes rather than assuming a hang** — and if it does
complete at some large number, that number is itself a useful result and worth
putting in the comment, because it is a first measurement of our ext4 read path
under a real workload.

**3. Staging 31 MB through `Vfs::read_file` + `Vfs::write_file` may be the
slowest thing in the rung.** The bash rung reads the whole binary into a `Vec`
and writes it back; at 11 MB + 20 MB that is 31 MB through the heap. If that is
a problem, reading the zip in place from `/mnt/usr/local/lib/python312.zip`
(with `PYTHONHOME` pointed at `/mnt/usr/local`) is a legitimate alternative and
avoids the copy entirely — the interpreter does not care which mount the zip is
on. I staged it to `/usr/local` to match how a real system would look, not
because anything requires it. Your call; you know the cost of that copy and I
do not.

## What a failure would actually be telling us

Worth stating, because a red rung here is *interesting* rather than routine —
this is the first time any of these paths carry a load like this:

| Symptom | Most likely cause, in our tree |
|---|---|
| `init_fs_encoding` fatal error | The zip was not staged, or `PYTHONHOME` is wrong. Not a kernel bug. |
| `No module named 'zlib'` from `<frozen zipimport>` | The zip got repacked compressed. `create-ext4-rootfs.sh` asserts against this, so it would mean the assert was bypassed. Not a kernel bug. |
| Hang partway through startup | Our `mmap`, `read` or `lseek` on a 20 MB file through the ext4 driver — CPython seeks around the zip's central directory rather than reading it linearly. |
| Crash in `Py_Initialize` | A `getrandom` interaction (CPython seeds its hash randomisation at startup) or a TLS/thread-state issue. |
| Wrong output, correct exit code | A libc function that returns plausibly rather than correctly. This is the failure the specific expected lines above exist to catch. |

## Call sites

Two, matching the bash rung: `kernel/src/main.rs:3207` and `:3221`. Please put
the CPython rung **after** the bash one — bash is the cheaper, older, better
understood test, and if our libc has regressed it should say so first.

## Where the evidence is

- `scripts/cpython-spike/README.md` — the full measurement, both configure
  defects that made the earlier one an undercount, and the sample output the
  control interpreter produces.
- `design-decisions.md` §344 — why the stdlib is one uncompressed zip, why the
  binary is `--strip-debug` and not `strip`, and the `sp_pwdp = "!"` reasoning.
- `scripts/cpython-spike/stdlib.sh` — the host-side proof, runnable in about a
  minute if you want to see it pass before writing the rung.

No reply needed — if you take it, a line in `roadmap.md` and the rung itself is
plenty. If the yield budget or the 31 MB stage turns out to be the wrong shape,
file it back and I will change what gets staged.
