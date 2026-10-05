## 1011. The ABI gate: Rust supplies the numbers, musl is the oracle, and the list of what to check is derived

**Date:** 2026-09-09
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** yesterday's bug was a structure whose fields our C library put in
the wrong order. Fixing that one structure by hand was not a response to it,
because nothing would catch the next one. This is a check that compares *every*
such structure against the real C library's own header files, automatically, at
push time -- and that notices when a new structure appears without being
checked.

### Three programs and no third copy

| | who says it |
|---|---|
| the numbers | Rust: `size_of` and `offset_of!`, emitted by `posix::abi_layout`, never typed by a person |
| the truth | musl: `zig cc --target=x86_64-linux-musl` compiles the emitted `_Static_assert`s against its own headers |
| what to check | derived: every `#[repr(C)] pub struct` that is also a pointer parameter of an exported `extern "C" fn` |

That third row is the one that took the thinking. A gate whose coverage is a
hand-kept list is a second list to rot, and `todo.txt`'s own sketch of this
warned about exactly that. So `scripts/check-libc-abi.py` parses the sources
for both halves -- the set of `#[repr(C)]` struct names, and the set of types
appearing as pointer parameters of exported functions -- and takes the
intersection. **The intersection is what makes it useful**: the parameter set
alone also catches `WcharT`, `TimeT`, `GidT` and friends, which are typedefs
with no layout to get wrong. Neither half is written down anywhere, so neither
can go stale.

There are 98 pointer-parameter types and 15 are checked today, so the gate
carries a `BASELINE_UNCOVERED` ratchet: it may shrink, never grow. A type that
starts crossing the boundary tomorrow is refused until it has an entry.

### Why the oracle is musl and not a table of numbers

Both alternatives were available. A table of expected offsets would be a third
copy, and a wrong one is exactly what caused §1010 -- the test that certified
the bug *was* such a table. Compiling against the real headers has no such
failure mode: the numbers cannot be stale, because they are not stored.

musl specifically, rather than glibc, because it is the C library every port in
this tree already links against. That makes it the right oracle rather than
merely an available one -- if our `struct stat` disagreed with glibc but agreed
with musl, the ports would still work and the gate should not fire.

### It found something on its first run, and it was mine

`struct utsname`'s last field. glibc declares `__domainname` and `#define`s the
short name; musl declares `domainname` directly under `_GNU_SOURCE`. I had
written the glibc spelling into the table. The gate refused it in sixteen
seconds.

A small thing, and worth recording because it is the shape of what this catches:
a plausible belief about a header, held confidently, wrong for the library we
actually use.

### The failure that would have made the gate a liar

`zig cc -fsyntax-only` does not work. It injects its own `-c`, warns that the
`-c` is unused, and then fails with `error: FileNotFound` looking for an output
it was told not to produce.

**That failure only appears when the clang stage succeeds.** So the sequence
was: a run with a real error reported the real error and looked fine; the run
after I fixed the error reported `FileNotFound` -- a checker that says "problem"
exactly when there is none. If that had shipped, the gate would have been a
permanent red that everybody learned to bypass, which is worse than no gate at
all. It compiles to a throwaway object file instead.

Caught only because the gate was run *after* a clean fix as well as before one.
Running a checker against a known-good input is as necessary as running it
against a known-bad one, and it is the half that gets skipped.

### The self-test asserts the failure path, not just the pass

Per the tree's gate rule, `--self-test` covers both directions of both checks:

* a true assertion is not reported as failing;
* **the exact §1010 defect is reported** -- it feeds
  `offsetof(struct sigaction, sa_flags) == 8`, the old wrong value, and fails
  if the gate stays quiet;
* a compile error is not reported as a pass;
* with the live baseline the ratchet reports nothing, and with one name removed
  from the baseline it reports exactly that name.

The last pair is what proves the ratchet would notice a type someone adds
tomorrow. A ratchet whose failure has never run is a ratchet nobody knows the
sign of.

### Cost, and where it is paid

The gate runs `cargo test -p posix --lib` to get the numbers, so it is minutes
on a cold target directory rather than seconds. It is therefore scoped to
pushes that touch `posix/src/` or the checker itself -- the only pushes that
can change either the layouts or the verdict. Without zig it says so by name
and still runs the coverage ratchet, rather than passing silently.

### The gate found two more the same hour, and both were glibc/musl divergences

Coverage went from 15 types to 28 by adding the by-value ones — the entries
this entry called the scary ones, where a C program puts the object on its own
stack. Two of the new thirteen failed immediately, and both failed in the same
way: our type matched **glibc** and the ports link **musl**.

| | glibc | musl | ours, before |
|---|---|---|---|
| `regoff_t` | 4 | **8** | 4 |
| `regmatch_t` | 8 | **16** | 8 |
| `struct sched_param` | 4 | **48** | 4 |

Measured by compiling one program twice, once with `gcc` under WSL and once
with `zig cc --target=x86_64-linux-musl`.

**`regmatch_t` is the serious one.** A C program writes `regmatch_t m[10]` and
calls `regexec(&re, s, 10, m, 0)`. Its array is 160 bytes; we wrote 80 into the
front of it, with every element after the first landing at the wrong offset and
every position truncated to 32 bits. Anything using POSIX regex with
sub-expressions — `sed`, `awk`, `grep -E` through the C API — got wrong offsets
for every group but the first. Fixed by widening `rm_so`/`rm_eo` to `isize`,
which is what `regoff_t` is here.

**`sched_param` is mild and worth stating anyway.** `sched_priority` is at
offset 0 in both, so reading one worked, which is why nothing noticed. What did
not work is a `sched_getparam` that is supposed to fill the caller's object and
filled a twelfth of it. Fixed by carrying musl's five reserved fields.

**Three more tests had certified the wrong answer**, and one of them is the
best example of the shape yet:

```rust
// glibc/musl: regoff_t is `int` (i32), so regmatch_t is 8 bytes.
```

It is `int` in glibc and `long` in musl. **A test that names two libraries and
describes one is worse than a test that names neither**, because it reads as
though the question had been asked. That is the same defect as §1010's "glibc
x86_64" comment on the kernel's struct, twice in two days, in code written
months apart — which is what makes it a class rather than a slip, and why the
answer had to be a gate rather than three more careful comments.

**This is also the case that justifies choosing musl as the oracle.** Had the
gate compared against glibc it would have passed all three of these and left
the `regmatch_t` corruption in place. "The C library every port here already
links against" was the right reason, and it turned out to be load-bearing
within the hour.

### Batch three: 28 → 42 types, and the worst one yet

The types real ports touch — sockets, `passwd`/`group`/`shadow`, `statvfs`,
`rusage`, `epoll_event`. Three more failures.

**`struct addrinfo` had `ai_addr` and `ai_canonname` transposed**, and this one
is not a glibc/musl divergence: both put `ai_addr` first, and ours put
`ai_canonname` there. It was simply wrong.

The consequence is the sharpest of any found today. `getaddrinfo` fills a list
and the caller does the canonical thing:

```c
for (p = res; p; p = p->ai_next)
    if (connect(fd, p->ai_addr, p->ai_addrlen) == 0) break;
```

`p->ai_addr` read the **canonical hostname string pointer** and handed it to
`connect()` as a `struct sockaddr`, with `ai_addrlen` bytes of a NUL-terminated
name reinterpreted as an address family and port. Every C program that resolves
a name and connects to it — which is every network client — was affected.

**`struct rusage` was 144 bytes against musl's 272**, and **`struct statvfs`
was 88 against 112**. Both for the same reason: a trailing reserved array we
had left off. Every *named* field was already at the right offset in both,
which is exactly why neither was ever noticed — `getrusage` and `statvfs` fill
the caller's object correctly as far as they go and then stop a third of the
way in, leaving the tail as the caller's allocator left it.

### A fourth kind of bad test: the tautology

§1010 found a test asserting a wrong fact about an external library. This batch
found a different failure:

```rust
// Statvfs has 11 u64 fields = 11 * 8 = 88 bytes.
assert_eq!(mem::size_of::<Statvfs>(), 11 * 8);
```

That is not a claim about C at all. It restates the declaration it is checking,
so it passes for any declaration and fails only if `size_of` is broken. **A
test whose expected value is derived from the thing under test can only ever
pass**, and it occupies the space where a real check would go — which is worse
than the space being empty, because the file now looks covered.

The running tally of what tests were doing in place of checking:

| | shape |
|---|---|
| §1010, `Sigaction` | a wrong fact about an external library, stated confidently |
| §1011, `regmatch_t` | a fact about *one* of the two libraries it named |
| §1011, `Statvfs` | a restatement of our own declaration |

All three read as though the question had been asked. None of them asked it.
The gate asks it, on every push, against the library we actually link.

### Batch four: 42 → 62 types, and the third state the ratchet was missing

`BASELINE_UNCOVERED` says "nobody has looked". Batch four needed a state it
could not express: **somebody looked, it is broken, and here is where it is
written down.** Taking a failing type back *out* of `abi_layout.rs` makes it
indistinguishable from one that was never added, and throws the measurement
away.

So the gate gained `KNOWN_MISMATCH`, a **two-way** ratchet. A failure naming a
listed type is reported and does not refuse the push. A listed type that
produces **no** failure is a hard error — it means somebody fixed it and left
the exemption behind, and an exemption for a defect that no longer exists is
how an exemption list starts covering real ones. Every entry must name a
`known-issues.md` key.

Five types are in it, and **all five are ours being smaller than musl's**:
`aiocb` (136 against 168, and the field order differs too), `sysinfo` (112
against 368 — every named field at the right offset, musl's trailing
`__reserved[256]` absent), `utmpx` (384 against 400, because musl's `ut_tv` is a
full 16-byte `struct timeval` and ours is two `i32`s), and the two System V IPC
status structures, which flatten `ipc_perm` and come up short.

**The first version of this paragraph had three of those backwards**, including
a claim that `sysinfo` wrote 256 bytes *past* the caller's object. The gate
prints `'368 == 112'`, and the assertion is `sizeof(C type) == <our size>`, so
the left number is musl's — read the other way it inverts every one of these.
Recorded rather than quietly fixed because it is the same defect as everything
else in §1010 and §1011: a confident direction stated without re-reading the
number it came from, committed *while writing up* a family of bugs of exactly
that kind.

**Fixed rather than listed: `fd_set`.** 32 bytes against the C library's 128,
so every `select()` read and wrote the front quarter of the caller's own
variable. The cause is worth keeping: `FD_SET_WORDS` was derived from
`FD_SETSIZE`, which is 256 here because `fdtable::MAX_FDS` is. That reasoning is
right about *this system's fd limit* and wrong about *the C library's
structure* — two facts that are the same number in glibc and musl and are not
the same fact. They are now `FD_SET_BITS` (1024, the ABI) and `FD_SETSIZE`
(256, the policy).

### The checker's own classifier was wrong the whole time

Worth its own section because of how it hid.

`compile_c` split clang's output into "assertion failures" and "everything
else" with a regex expecting the message in **quotes**. clang prints it bare:

```text
error: static assertion failed due to requirement 'sizeof(x) == 8': x size
```

So the regex never matched, and every assertion failure since the gate was
written had been falling through to the "(compile error, not an assertion)"
branch — arriving as one opaque blob with a problem count of 1 regardless of
how many assertions had failed.

**The verdict was right and the classification was wrong**, which is the
hardest kind of wrong to notice: the gate refused exactly when it should, so
every run looked correct. It surfaced only when `KNOWN_MISMATCH` needed to ask
*which* type a failure belonged to, and got "none of them" for all of them —
which then reported all five known-bad types as fixed.

Two things came out of it, and the second matters more than the first:

* the parse is now pinned by a self-test that asserts the extracted message
  **exactly** — `got == ["the 1010 bug"]`, not `"the 1010 bug" in stderr`. The
  weaker form passes against the raw blob, so it would have certified the bug,
  which is the same shape as everything else in §1010 and §1011;
* a translation unit that fails to *compile* now suppresses the "this type
  produced no failure, so it must be fixed" sweep, because a unit that stopped
  early reached no verdict on the assertions after the error. Without that,
  one missing header silently declares every known mismatch repaired.

The summary line was also saying "0 mismatches" while the lines above it listed
five. A summary that contradicts its own detail trains the reader to skip the
detail.

### Two of the five fixed, and the same conflation a third time

`sysinfo` and `utmpx` are closed. Both fixes were one field, once the layouts
were **measured** rather than reasoned about — a program compiled with
`zig cc --target=x86_64-linux-musl` and run under WSL, printing every offset.
That took one command and settled four types at once; the two left are left
because they need structural work, not because anything about them is unknown.
Their measured layouts are recorded in `KNOWN_MISMATCH` itself, so the next
attempt starts from numbers rather than from a header.

`sysinfo` produced the kernel/libc conflation for the **third** time in two
days. Its test read:

```rust
// Linux struct sysinfo is 112 bytes on x86_64.
assert_eq!(core::mem::size_of::<Sysinfo>(), 112);
```

That sentence is true. The kernel's `struct sysinfo` ends
`char _f[20-2*sizeof(long)-sizeof(int)]` and comes to 112. musl's userspace one
ends `char __reserved[256]` and comes to 368. Same name, same header name, two
structures — exactly `sigaction` again, and exactly `regmatch_t` again.

Which boundary governs was **checked** this time rather than assumed:
`unistd::sysinfo` fills the caller's structure field by field from
`SYS_CLOCK_MONOTONIC` and a process count, and never hands it to the kernel. So
the only boundary is with C, and musl's number wins. Had it been a struct we
pass *to* the kernel, 112 would have been right and the gate's entry would have
been the error.

### And one I got wrong while fixing it

The first `utmpx` patch *added* musl's trailing `__unused[20]`. The struct
already had it, as `_reserved`, documented "reserved for future use" — which is
what it looks like from this side and not what it is. The result was 416 against
400, caught by the gate on the next run.

The cause is the same one this whole entry keeps circling: the struct was read
as far as the fields the gate had named and no further. Three times in two days
now, and the pattern is specific enough to state as a rule — **when a tool
reports a defect at a location, the unit of reading is the whole declaration,
not the lines the tool pointed at.** The gate catching it in seconds is the
argument for the gate; needing it caught is the argument for the rule.

### KNOWN_MISMATCH is empty again, one tick after it was built

All three remaining types are fixed: `aiocb` reordered to musl's, and the two
System V status structures given a real nested `ipc_perm`. The table that was
built to hold defects across ticks held them for one.

That is the argument for the mechanism rather than against it. The alternative
was to take each failing type back *out* of `abi_layout.rs` until someone got
to it, which loses the measurement and makes "broken" indistinguishable from
"never looked at". The table cost about twenty lines and made the difference
between three recorded defects and three forgotten ones — and its second
ratchet, the one that refuses a stale entry, is what turned each fix into a
push that could not silently leave the exemption behind.

The comment left in the empty table says so: **empty is a state, not a
default.**

### Three things the IPC fix turned up that are not about IPC

**The knowledge was already in the tree.** `posix/src/linux_ipc_perm_types.rs`
has held the correct `ipc_perm` offsets — key 0, uid 4 … mode 20, seq 24, size
48 — the whole time, and `linux_ipc.rs` has held a kernel `ipc64_perm`. The two
structures that needed them simply did not use them. No test could see that,
because no test crossed a C boundary. A constant that is right and unread is
worth exactly as much as one that is wrong.

**The two structures now sit in the same file on purpose.** `IpcPerm` (the C
library's) is declared directly above `Ipc64Perm` (the kernel's). They are the
same 48 bytes and differ inside — the kernel's carries `seq` as a `short` with
padding either side, the C library's an `int`. Keeping them adjacent, each
saying what the other is, is the cheapest available defence against one type
being pressed into both roles, which is precisely how §1010 happened.

**`aiocb`'s doc claimed something unfalsifiable.** It said it "matches the
POSIX `struct aiocb` layout". POSIX cannot settle that: it names the members
and leaves the order to the implementation, so there is no *the* POSIX layout —
only a particular C library's. A claim that cannot be true or false is worse
than a wrong one, because nothing can contradict it. It is now stated as
musl's, with the numbers.

### Coverage is complete: 76 types, and the last 17 were nearly written off

The remaining seventeen looked like kernel wire formats — `open_how`,
`clone_args`, `perf_event_attr`, the capability structs, io_uring — and were
about to be recorded as outside this gate's remit on the strength of musl not
declaring them. **Probing first showed the toolchain declares thirteen of
them.** zig ships the Linux uapi headers alongside musl's, so the same
mechanism checks both.

That changes the rule the gate follows, and the new statement is better than
the old one: **compare against the header that defines the boundary this type
crosses.** musl for a libc type, `<linux/openat2.h>` for `struct open_how`. It
was never really "musl is the oracle"; it was "the other side of the boundary
is the oracle", and musl happened to be the only boundary in view.

### A fourth state, and this one is checkable too

Four of the seventeen have no definition anywhere — `ndbm.h`, `fts.h` and
`linux/sysctl.h` are absent from musl and from the uapi headers. Leaving them
in `BASELINE_UNCOVERED` would be a slow lie: they would sit there for ever
looking like work nobody had got to.

`NO_ORACLE` records them with the reason, and the reason is **verified**. Each
entry names the C type and header it claims cannot be found, and the gate
compiles a probe for it on every run; if the header ever appears, the entry
fails and the type gets a real check. Only one entry is taken on trust —
`CapEntryInfo`, which is SlateOS's own and has no counterpart to probe for.

So the three tables now say three different things, and each is enforced:

| table | means | enforced by |
|---|---|---|
| `KNOWN_MISMATCH` | measured, wrong, recorded | fails if the type starts passing |
| `NO_ORACLE` | looked, nothing to compare with | fails if the C type becomes findable |
| `BASELINE_UNCOVERED` | nobody has looked | may only shrink |

The third is now empty, which is the point.

### `abi_extensible!`, for structs whose size is a version number

`landlock_ruleset_attr` (ours 16, header 24) and `perf_event_attr` (112 against
144) failed, and neither is a defect. Those syscalls take the structure's size
explicitly — `landlock_create_ruleset(attr, size, flags)`, `perf_event_open`
via `attr->size` — precisely so the structure can grow without breaking callers
built against an older header. Ours being *smaller* is an older ABI version,
which the kernel is required to accept.

So `abi_extensible!` asserts `sizeof(C) >= ours` and every named field at `==`.
That still catches the failure that matters — ours being **larger** than the
kernel knows about — and refuses to let a genuinely short structure hide:
`statvfs` was short and `statvfs()` takes no size, so it does not qualify. The
macro's doc says so, because the temptation to reach for it the next time a
size assertion is inconvenient is obvious.

### Three self-inflicted defects in one sitting, all in the checker

Worth listing together, because they have one cause between them.

* **`str.index("BASELINE_UNCOVERED")` matched inside the module docstring**,
  which *mentions* the identifier, and the replacement that followed ate the
  docstring's closing `"""`. The file stopped parsing. Fixed by restoring it
  and re-applying every edit with anchors that are whole top-level statements
  rather than words — a word appears in prose, a statement does not.
* **`BASELINE_UNCOVERED: set[str] = {}` is a `dict`.** The annotation is not
  checked at run time, so the gate died on `set - dict` the moment the baseline
  reached the state it exists to reach. `set()`, with the reason written beside
  it.
* **`COVERED_RE` matched only `abi!`**, so the four types moved to
  `abi_extensible!` immediately reported as *uncovered* — which would have had
  the ratchet demand entries for types it was already checking.

The common cause is a pattern that was right about the case in front of it and
silently wrong about the next one: an identifier that also occurs in prose, an
annotation that is not a constructor, a regex that knows one spelling of a
thing with two. Each was caught within a minute by running the checker, and
none by reading it.

**Against it, honestly:** coverage is now complete — 76 types checked, none
known-bad, five accounted for in `NO_ORACLE` — and "complete" is a claim
about the *set of types*, not about the depth of each. Five are checked by
size alone because ours flattens a union or a nested struct, and one
(`CapEntryInfo`) is taken on trust. The honest summary is that every type
which crosses the boundary is now accounted for, and that accounting is
itself enforced. (This paragraph has been
rewritten three times in one session — 15, 28, 42, 62 — which is the ratchet
working as intended rather than a correction to it. The number in it will keep
going stale; the two ratchets in the script are the copies that cannot.) The most dangerous names are in
that frozen set -- `PthreadMutexT`, `PthreadAttrT`, `CpuSetT`, `SemT` are all
types a C program declares *by value*, where being smaller than musl's means
our writes land past the caller's own object. The counter-argument is that a
ratchet at 15 is strictly better than the nothing that existed yesterday, and
the frozen set is now written down in one place with a note on which entries
are the scary ones -- which is what makes shrinking it a task somebody can pick
up rather than an audit somebody has to invent.
