## §348 — A libc function that gnulib also defines must own its archive member, and the guard now asserts that instead of guessing which callers matter

**Date:** 2026-08-21
**Decided by:** Claude (autonomous)

**In short:** Our C library ships as one big `libc.a` file, which is really a
bag of smaller object files. When a program links against it, the linker takes
or leaves each object *whole* — it cannot take half. GNU programs carry their
own copies of a few dozen library functions, so if one of our objects contains
both a function the program brought itself *and* a function the program needs
from us, the program is stuck: taking the object gives it two copies of the
first function and the link fails. §340 fixed the four cases we knew about and
added a script to stop them coming back. The GNU coreutils port then hit a
fifth case the script could not see. This records why the script was blind to
it, and the stronger rule that replaced its guess.

**Related:** §339 (the GNU make link failure that started this), §340 (the
one-function-per-member split and the original guard), `scripts/coreutils-spike/README.md`
(the measurement).

### What happened

`scripts/coreutils-spike/` built GNU coreutils 9.5 unmodified and relinked all
107 of its binaries against `toolchain/sysroot/lib/libc.a`. Five of them — `ls`,
`dir`, `vdir`, `du`, `dircolors` — failed on a duplicate definition of
`wmempcpy`, while `check-libc-shape.py` pronounced the same archive clean.

The mechanism was §339's exactly. `posix::wchar` is a 156 KB single module, so
rustc emitted it as one object exporting 78 symbols, `wmempcpy` among them.
coreutils calls `mbrtowc`, which is in that object; the linker took the object;
`wmempcpy` came with it; gnulib had already defined `wmempcpy` because musl
lacks it. No link order can decline half an object.

Two things had to be understood before this could be fixed properly.

**`-C codegen-units=4096` is a ceiling, not a splitter.** §340's text reads as
though the flag were doing the work. It is not: rustc partitions at *module*
granularity and will not divide a single module, so `wchar.rs` is one codegen
unit at any ceiling. What actually created the seventeen separate members in
§340 was the seventeen `mod gnu_<name> { … }` blocks. The flag only stops rustc
*merging* modules that are already separate. Raising it further would have done
nothing here, and someone reaching for it as the fix would have concluded the
problem was unfixable.

**Adding `wmempcpy` to the guard's `REPLACEABLE` list would not have helped.**
That is the tempting one-line fix and it is worth being explicit about why it
fails, because the same reasoning applies to any future name. CHECK 2 fires when
one member holds a `REPLACEABLE` name *and* an `UNAVOIDABLE` one. The wchar
member's other 77 symbols are `mbrtowc`, `wcslen`, `iswalpha` and friends —
none of them in `UNAVOIDABLE`. The condition still would not have held.

### The decision

`UNAVOIDABLE` was defined as "names referenced by a hello-world or the startup
path". That is a proxy for the property actually wanted, which is "names the
program under port references". It is a weak proxy: coreutils references
hundreds of libc names, and any one of them is enough to extract a member.

The obvious repair is to widen `UNAVOIDABLE`. It was rejected because it has no
honest stopping point. Every name added covers one more program's habits and
still leaves the next program's; the limit of the process is *every name in
libc*, at which point the rule has degenerated into "a replaceable name may not
share its member with anything" — which is CHECK 3, arrived at by a long
detour, and in the meantime the list is a running guess that reads as though it
were a finding.

So state that directly. **CHECK 3: a `REPLACEABLE` name must own its member
outright, or share it only with other `REPLACEABLE` names.** The justification
is caller-side rather than statistical: if a program brings its own copy of a
replaceable function, our member holding that function must be one the program
can afford to decline *entirely*. That holds exactly when the member contains
nothing else the program might want — i.e. nothing but other replaceable names,
which by construction the program is equally content to supply itself.

CHECK 2 is kept even though CHECK 3 subsumes it (an `UNAVOIDABLE` name is not
`REPLACEABLE`, so every member CHECK 2 flags CHECK 3 flags too). Its diagnosis
is the sharper one where it applies — "every program needs this" is a stronger
claim than "some program might" — and a member it reports is skipped by CHECK 3
so nothing is said twice.

### What it cost, and what it caught

On its first run against the real archive CHECK 3 reported two members, neither
of which any spike had yet tripped:

| member | replaceable names | riding with |
|---|---|---|
| `posix::stdlib` | `mkstemp`, `mkostemp`, `mkstemps`, `mkostemps`, `mkdtemp`, `getsubopt` | `atoi`, `bsearch`, `abs`, `strtod`, … (44 more) |
| `posix::time` | `timegm`, `strptime` | `clock_gettime`, `ctime`, `localtime`, … (33 more) |

gnulib vendors all eight. Any program that calls `atoi` and brings its own
`mkstemp` would have failed exactly as coreutils did — `tar` and `findutils`
both qualify. Each of the eight is now its own `mod gnu_*` member, and the whole
archive is clean under all three checks.

That is the argument for the stricter rule in one table: the weaker check had
been green for two months over two live defects, and the stricter one found
both in its first second of running.

### How to decide whether a new name is `REPLACEABLE`

The comment in the script now records the generative rule, because "consult
gnulib's module list" is not usable advice — gnulib has modules named after
functions it will never define. gnulib replaces a function two ways:

- The target **has** the function but it is buggy → gnulib compiles its copy as
  `rpl_foo` and adds `#define foo rpl_foo`. Different symbol; cannot collide.
  (`fflush` is replaced this way, which is why `fflush` stays in `UNAVOIDABLE`
  and not `REPLACEABLE`.)
- The target **lacks** the function → gnulib compiles its copy as plain `foo`.
  That one collides with ours.

`./configure` decides which case applies by probing the libc it was configured
against — for every spike here, zig's bundled musl. So the hazard set is, near
enough, **the names our libc defines that musl does not**. `wmempcpy` qualifies
(glibc-only). `execl` does not, which is why the exec family is absent from
`REPLACEABLE` despite gnulib having modules for it — and why the nineteen
missing symbols the spike found were a *separate* problem from the one
duplicate, needing implementation rather than splitting.

### The residual weakness, stated plainly

`REPLACEABLE` is still a hand-maintained list, so CHECK 3 cannot catch a hazard
whose name is not on it. That is a real limit and it is the reason the spikes
exist: the list is a regression guard for things we have learned, and the spike
is the instrument that does the learning. The rule of thumb the two imply
together — run the spike when adding a *class* of function, run the script on
every build — is what `roadmap-detailed.md`'s porting policy already asks for,
and this entry is the case that shows why both halves are needed.

The list could in principle be computed rather than curated, by diffing our
exported names against musl's. That was not done because it would make the
sysroot build depend on having a musl `libc.a` to hand, which is true in WSL and
false on the Windows side where `build-sysroot.ps1` runs — and a check that
cannot run where the artifact is produced is not a check (the same reasoning
that keeps this script off `nm`). If the spikes are ever made part of a routine
build, revisit it.
