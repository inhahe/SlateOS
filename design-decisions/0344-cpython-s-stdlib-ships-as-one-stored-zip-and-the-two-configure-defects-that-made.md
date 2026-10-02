## §344 — CPython's stdlib ships as one STORED zip, and the two configure defects that made the first measurement meaningless

**2026-08-21** · **Decided by:** Claude (autonomous)

**In short:** we had a Python interpreter that linked against our own C library
and could not run a single line of Python. Two settings in CPython's build
system were quietly describing the *build machine* instead of SlateOS, so the
thing we had measured and called a success was a stripped-down interpreter with
no working standard library. Fixing them turned 31 usable modules into 83 and
cost six new C-library functions. The library itself is now one 20 MB file
rather than 569 small ones, which is the layout CPython was already looking for.

### What was wrong, and why it looked right

Both defects produced a build that **exited 0**. Neither announced itself.

**1. `MODULE_BUILDTYPE` defaults to `shared`.** CPython 3.12's `configure`
contains `MODULE_BUILDTYPE=${MODULE_BUILDTYPE:-shared}` and only overrides it
for wasm hosts. We were passing `--disable-shared`, which reads like it settles
the question, and does not: it governs *libpython*, while the stdlib's C
extensions are a separate axis. So `_struct`, `_json`, `math`, `_socket`,
`select` and forty-odd others were built as `.so` files in `lib-dynload` — on a
system with no dynamic loader, files that can never be opened.

The failure surfaced only when something tried to *use* the interpreter:
`import struct` gives `ModuleNotFoundError: No module named '_struct'`.
`sys.builtin_module_names` held **31** entries, the frozen bootstrap set and
nothing else. With `MODULE_BUILDTYPE=static` it holds **83**.

**2. `pkg-config` answers for the host.** `configure` finds zlib, OpenSSL,
libffi, sqlite3, liblzma, bzip2, ncurses, readline and libuuid through
`pkg-config`, which in a cross build cheerfully describes Ubuntu's library set.
CPython therefore compiled `zlib`, `binascii` and `_ctypes` against host
headers and then failed to link them — and the spike's README recorded this as
"expected and irrelevant". It was neither: `MAKE_EXIT` was **2**, the build was
failing, and the failure had been read as normal for weeks.
`PKG_CONFIG_LIBDIR=/nonexistent-slateos-cross` makes every probe answer "no",
which is *the truth for our sysroot*. `MAKE_EXIT` is now 0 with zero error
lines.

The general rule, which is not specific to CPython and is worth carrying to the
next port: **in a cross build, a probe that consults the host is not a failed
probe, it is a wrong answer.** A cross build should be configured so that every
"do we have X?" question is answered by the sysroot or answered "no".

### What it cost, and what it bought

`REFERENCED_EXTERNAL` — the count of libc symbols CPython genuinely needs
somebody else to provide — went from **363 to 478**. That is not a regression;
those 115 symbols were always going to be needed by an interpreter that can
`import struct`. The 363 was an honest measurement of the wrong binary.

Six of the 478 were missing and are now written (`posix/src/shadow.rs`,
`posix/src/socket.rs`): `getspnam`, `getspent`, `setspent`, `endspent`,
`gethostbyname_r`, `gethostbyaddr_r`. `MISSING_AT_LINK` is back to 0.

### The standard library as one file

**Decision: pack `Lib/` into a single `ZIP_STORED` `python312.zip`, containing
both `.py` and `.pyc`, compiled with `unchecked-hash` invalidation and with the
target path baked into the code objects.**

*Why a zip at all.* `<prefix>/lib/python312.zip` is the **first** entry of
CPython's default `sys.path` and `zipimport` is frozen into the binary. This is
not an optimisation bolted on afterwards; it is the layout CPython already
looks for before anything else. The alternative is 569 files and ~12 MB of
small reads our ext4 driver walks at every boot to deliver exactly the same
modules.

*Why STORED rather than deflated.* `zlib` has no target build, so `zipimport`
cannot inflate a compressed member. This is measured, not reasoned: the
deflated variant of this archive fails at startup with `No module named 'zlib'`
raised from inside `<frozen zipimport>`. The cost is real — deflate takes the
same content from 10.3 MB to 2.6 MB — and the trade is revisitable the day zlib
is ported. Until then `create-ext4-rootfs.sh` asserts zero deflated members at
stage time, because a build host whose `zipfile` defaults changed would
reintroduce this invisibly.

*Why `unchecked-hash`.* A normal `.pyc` records the source's mtime and size and
the loader re-validates against them. Inside a zip that check is answered from
the zip's own directory entry, which is a different clock from the one that
compiled the file — one skewed timestamp and every module silently falls back
to re-parsing source on every single import. `unchecked-hash` removes the
question instead of trying to answer it, which is the right call for a stdlib
shipped as one immutable file. If it is ever edited in place, that is a rebuild
of the artifact, not a cache-invalidation event.

*Why both `.py` and `.pyc`, at the cost of roughly half the 20 MB.*
`zipimport` tries `.pyc` first, so imports never parse source; the `.py` rides
along purely so tracebacks and `inspect.getsource` show real lines. On a system
where a Python traceback may be the only debugging tool that works, that is
worth its megabytes — and it is verified rather than asserted: `stdlib.sh`
provokes a `json` error and checks the formatted traceback contains a source
line, from a file that does not exist on disk.

*Alternative considered and rejected: strip the pure-Python halves of modules
whose C extension is absent* (`ssl`, `sqlite3`, `bz2`, `lzma`). Keeping them
means `import ssl` fails with `No module named '_ssl'`, which names the actual
missing piece. Deleting them would report `No module named 'ssl'` and send
whoever hits it looking for the wrong thing. A misleading error is more
expensive than a megabyte.

### `sp_pwdp = "!"`

`pwd.rs` had long reported `pw_passwd = "x"` for root — the Unix convention for
"the hash lives in the shadow database" — with no shadow database for it to
point at. `shadow.rs` supplies one, and the entry it returns has `sp_pwdp` set
to `"!"`.

Three candidates, and the reasoning is a security argument rather than a
preference:

| Value | Consequence |
|---|---|
| `""` (empty) | An empty hash **authenticates anybody**. Not a candidate. |
| absent (`getspnam` returns NULL) | Contradicts `getpwnam`, which does report root. Any caller that consults both learns the database disagrees with itself. |
| `"!"` | Cannot be produced by `crypt`, so no supplied password can ever match it. The database exists, agrees with `pwd`, and can never grant access. |

### The interpreter ships stripped of DWARF but not of symbols

`--strip-debug`, not `strip`: 35.7 MB becomes 11.2 MB. Nothing on SlateOS can
read DWARF (there is no debugger, and the ELF loader maps `PT_LOAD` only, so
`.debug_*` is 24.5 MB the ext4 driver stores and never reads). The extra 1.1 MB
that `--strip-debug` keeps is `.symtab`, which is the only thing that could
turn a fault address from the kernel into a function name — and a *name* is
what survives having no source tree on the target, whereas a *line* is not.
The unstripped binary stays in the build tree beside the objects its debug info
refers to, which is the only place it means anything.

Verified rather than assumed: every `PT_LOAD` segment (offset, vaddr, filesz,
memsz, flags) and the entry point are byte-identical before and after, and
`.eh_frame`/`.eh_frame_hdr` survive.

### Status

The interpreter has still never executed on SlateOS. Everything above was
measured on the host — by the linker, or by the musl-linked control interpreter
`make` builds from the identical objects. The remaining unknowns are ours, not
CPython's: whether our ext4 driver, `mmap`, tty and `getrandom` behave the way
`zipimport` and `init_fs_encoding` assume. That needs a Path-Z self-test in
`kernel/src/proc/spawn.rs`, which is lane A's tree — filed as
`requests/b-a-cpython-path-z-self-test.md`.
