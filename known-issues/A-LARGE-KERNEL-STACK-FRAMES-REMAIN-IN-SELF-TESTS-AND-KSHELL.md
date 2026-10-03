## A-LARGE-KERNEL-STACK-FRAMES-REMAIN-IN-SELF-TESTS-AND-KSHELL (lane A, 2026-08-17) - **open, low severity**

**In short:** after the two fixes above, the biggest remaining stack frames
in the kernel are no longer on core paths - they are in boot self-tests and
in `kshell` command handlers.  None is currently close to overflowing, but
several are large enough that adding a few locals to one could push it
over, and they are the reason the boot's peak is still 38 280 bytes rather
than something in the low thousands.

Measured with `python scripts/stack-frames.py --profile debug` on the debug
kernel after both fixes landed (top offenders, bytes claimed by the
prologue):

| Function | bytes |
|---|---:|
| `self_test_prctl_dispatch` | 32 160 |
| `kshell::cmd_oci` | 31 760 |
| `linux_fd::self_test` | 28 224 |
| `kshell::cmd_container` | 27 432 |
| `eventlog::self_test` | 26 720 |
| `scfilter::init` | 21 872 |
| `kernel_main` | 19 776 |
| `self_test_numa_sched_landlock` | 19 664 |
| `test_per_cpu_work_stealing` | 19 504 |
| `self_test_legacy_deprecated_syscalls` | 18 832 |
| `PerCpuScheduler::new_const` | 18 752 |
| `net::httpd::self_test` | 17 152 |
| `xhci::init` | 15 504 |
| `oci::build_one_stage_inner` | 15 000 |

Plus some large stack *locals* found by inspection rather than by the tool:

- `kernel/src/audio_notify.rs:133` - `[0u8; 8192]`
- `kernel/src/audio_mixer.rs:420,425` - ~12 KiB combined
- `kernel/src/syscall/linux.rs:80592` - `[0u8; 4096]`
- `kernel/src/kshell.rs:106303-4`

And one core-path remnant: `proc::pcb::fork_create` itself is still 6 328
bytes, because it builds a ~35-field tuple by value.

**Why this is low severity, not zero severity.** These frames are real but
they do not nest: a self-test is called from `kernel_main`'s test runner at
shallow depth, and a `kshell` handler from the shell loop.  The census
after both fixes shows the deepest live stack at 11% and the boot peak at
58%, so nothing is near the canary today.  The risk is that 32 KiB is half
a stack, and the next person to add a buffer to `self_test_prctl_dispatch`
has no warning that they are spending the second half.

**Proper fix.** Not "make the tests smaller" one at a time - the pattern is
the bug.  These functions accumulate dozens of independent test fixtures as
distinct locals in one frame, and at `opt-level = 0` every one of them is
live for the whole function.  Either split each self-test into
`#[inline(never)]` per-case functions so the fixtures' frames are disjoint,
or move the fixtures behind boxed allocation as `FpuState` now is.  The
first is preferable: it costs nothing at runtime and the frames genuinely
do not overlap in time.

**How to reproduce / verify.** `python scripts/stack-frames.py --profile
debug --top 40`, and `--diff BEFORE_ELF AFTER_ELF` to check a change.
Note the profile: these numbers are debug, which is what `boot-test.sh`
builds and therefore what any canary halt will come from.

### Progress - 2026-08-18: four functions split, and what stopped the rest

The prescribed fix ("split each self-test into `#[inline(never)]` per-case
functions") is now mechanised as `scripts/split-frames.py` and applied to the
four functions it fits.  Measured on the debug kernel:

| Function | before | after (peak) | cases |
|---|---:|---:|---:|
| `self_test_prctl_dispatch` | 32 160 | **4 064** | 23 |
| `self_test_legacy_deprecated_syscalls` | 18 832 | **3 312** | 34 |
| `net::httpd::self_test` | 17 152 | **11 152** | 8 |
| `self_test_numa_sched_landlock` | 19 664 | **16 768** | 3 |

That is 52 512 bytes off the four peaks, and `self_test_prctl_dispatch` -
previously the largest frame in the kernel - is now a rounding error.
**`kshell::cmd_oci` (31 760) is the new top offender.**

**"Peak" means `outer + deepest case`, and that distinction is the whole
point.** The per-symbol ranking is misleading in the flattering direction
after a split: it reports the shrunken outer frame and lists the case
functions as separate symbols, so a split that achieved nothing looks like a
large win.  `linux_fd::self_test` is the cautionary case - the ranking called
it 28 224 -> 18 896, but its one case claims 9 344, so what is really on the
stack while that case runs is 28 240, i.e. **16 bytes worse than before**.
`scripts/stack-frames.py --peak` now reports this grouping directly; use it,
not the ranking, to judge a split.

**Three splits were applied, measured and reverted** on that basis:

| Function | peak before | peak after | verdict |
|---|---:|---:|---|
| `self_test_seccomp_ptrace_clone3` | 13 888 | 13 056 | -6%, not worth 35 lines |
| `self_test_remap_ioprio_futex2` | 13 712 | 13 552 | -1%, noise |
| `linux_fd::self_test` | 28 224 | 28 240 | a regression |

The common cause is *coverage*: the cases only spanned 73-722 lines of a much
longer function, so most of the frame stayed outside them.  A partial split
buys nothing, because the peak is set by whatever the outer frame still holds.

**What is out of the transformer's scope, and why** - these need hand work, not
a better tool:

- ~~`kshell::cmd_oci` (31 760) and `kshell::cmd_container` (27 432) are
  `match cmd { "inspect" => { .. }, .. }`.  The arms read outer locals
  (`parts`), so each needs its fixtures threaded through as parameters -
  a judgement call about what that interface should be.~~  **Done the next
  day by `--arms`; see the 2026-08-18 progress note below.**  The threading
  turned out not to be a judgement call at all: the compiler names the
  fixtures for you, one `E0434` at a time.
- `eventlog::self_test` (26 720), `self_test_sysv_ipc_mqueue` (15 808) and
  `self_test_bpf_perf_keyring` (15 568) have **flat** bodies: a long run of
  `let a = SyscallArgs { .. }; if dispatch(..) { fail }` with no block
  structure at all.  Splitting them means *inventing* the case boundaries,
  which is a decision about what the test's units are.
- `scfilter::init` (21 872), `test_per_cpu_work_stealing` (19 504),
  `PerCpuScheduler::new_const` (18 752) and `xhci::init` (15 504) are not
  self-tests and have no case structure to exploit.

**Two properties make the rewrite safe to apply mechanically**, and both are
worth knowing before hand-splitting anything else:

- A nested `fn` cannot capture, so a case that reads an outer local is
  `E0434` at compile time, never a silent miscompile.  This fired on the first
  real attempt (`self_test_remap_ioprio_futex2` shares one futex word across
  its cases); `--param NAME:TYPE` threads such a fixture through explicitly.
- `case()?` reproduces `return Err(..)` exactly - both abandon the whole
  self-test - but it does *not* reproduce `return Ok(())`, which used to end
  the self-test and would now end only the case, letting every later case run.
  The transformer refuses a case containing any non-`Err` return rather than
  rewrite it.  No case in the tree hit this, but nothing else about the
  rewrite can change behaviour, so it is the one thing worth refusing over.

`python scripts/split-frames.py --check` runs a regression suite of synthetic
inputs, one per trap the transformer actually fell into against real source:
the wrapped-`if` brace rustfmt puts on its own line, a block closed by
`} else {`, a `}` inside a comment (`sched::{set,copy}_task_name`), braces in
strings and nested block comments, a multi-line raw string (refused, because
re-indenting would change its value), and the `return Ok(())` case above.

### Progress - 2026-08-18: the four kshell dispatchers, via `--arms`

The note above called the `match`-arm shape a judgement call and left it for
hand work.  That was wrong, and cheaply so: the arms *are* the cases, and the
only thing that made them different was syntax.  `split-frames.py --arms`
handles them, and the four biggest dispatchers came out as follows.

| Function | before | after (peak) | outer | cases |
|---|---:|---:|---:|---:|
| `kshell::cmd_oci` | 31 760 | **18 512** | 208 | 12 |
| `kshell::cmd_container` | 27 432 | **5 000** | 160 | 33 |
| `kshell::cmd_firewall` | 14 032 | **5 760** | 192 | 13 |
| `kshell::cmd_mmtune` | 12 512 | **4 496** | 2 352 | 19 |

51 968 bytes off the peaks, on top of the 52 512 from the self-tests.  Note the
`outer` column: 160-208 bytes.  That is the difference from the three splits
that had to be reverted - a dispatcher's arms cover *all* of it, so nothing is
left behind in the outer frame to set the peak.  Coverage was the whole story
there and it is the whole story here.

**The safety argument is not the same one**, and this is the part worth
carrying forward.  A dispatcher arm rejects bad arguments with a bare
`return;`, meaning *leave the command*.  Inside a nested `fn` that becomes
*leave this case*, and the two coincide only when there is nothing after the
match for control to fall back into.  So `--arms` asserts two things rather
than assuming them: the function returns unit, and the match is its **last**
statement.  Both hold for every kshell dispatcher; neither is checked by the
compiler, which is exactly why they are asserted.

Everything else stayed the compiler's job, and it did it:

- **`E0434` named the fixtures.** The first build failed with eight of them,
  identifying `cmd` (shared by `cmd_firewall`'s `allow`/`allow6` arms) and
  `args` (`cmd_container`). Adding `--param cmd:&str --param args:&str` and
  rebuilding was the entire "judgement call" the earlier note anticipated.
- **`unused_variables` caught the use-test's one false positive.**
  `--param cmd:&str` matched `image.config.cmd`, a *field*, and passed a `cmd`
  no arm used.  `_used_params` now ignores a name after a `.` - except after
  `..`, where `a..cmd` is a real use - and `--check` has a case for it.
- **`clippy::needless_borrow` caught the one hand-fix.**  `cmd_container`'s
  network arm already called `cmd_container_network(&parts)`, which was right
  when `parts` was a `Vec` and a double borrow once it arrives as a slice.

Clippy against a pristine `kshell.rs` (stash the one file, run, unstash):
18 235 -> 18 235 diagnostics, changed kinds 0.

**What is left, and why the tool cannot take it.**  `cmd_oci`'s peak is still
18 304, and it is now one single case: the `"run" | "create"` arm, 1 178 of the
function's 1 691 lines.  Its body is *flat* - a long sequential build-up of one
container configuration, where the locals genuinely do all stay live - so it is
in the same class as `eventlog::self_test`, and shrinking it means restructuring
what it computes, not moving braces.

### Progress - 2026-08-18: the flat bodies, via `--flat`, and one comment that lied

The note above called the flat shape "*inventing* the case boundaries, which is
a decision about what the test's units are".  That was wrong for the same
reason the `--arms` note was wrong: the boundaries are already there.  The
author of `eventlog::self_test` separated the cases with a blank line and a
`// Test 7: ...` header - they simply are not separated with *braces*, which is
all the brace-walker could see.  `--flat` reads the blank lines, so it recovers
the author's own units rather than imposing any.

| Function | before | after (peak) | outer | cases |
|---|---:|---:|---:|---:|
| `proc::linux_fd::self_test` | 28 224 | **18 928** | 8 256 | 2 |
| `eventlog::self_test` | 26 720 | **17 872** | 4 848 | 11 |
| `net::httpd::self_test` | 11 152 | **8 352** | 128 | 16 |
| `self_test_sysv_ipc_mqueue` | (top-40) | **7 232** | 64 | 6 |
| `self_test_bpf_perf_keyring` | (top-40) | **6 000** | 224 | 24 |

(The httpd row read **3 248** when this section was first written, and that was
wrong - see the `--peak` defect in the next section. Its cases were themselves
split, and `--peak` was pairing only one level of nesting. Corrected here to
the figure the fixed tool reports; the "before" column was never affected,
because none of these functions had nested cases before being split.)

`linux_fd::self_test` is the one the earlier note recorded as a *regression*
(28 224 -> 28 240 by `--peak`, from a one-case split).  It is now 18 928, and
the tool refuses a one-case split outright rather than reporting the flattering
per-symbol number: with a single case the peak stays `outer + case`, which is
what it already was, so two cases is the arithmetic minimum at which `max` can
beat `sum`.

Three analysis defects had to be fixed before any of this worked, each hiding
the next, and all three are the same mistake - **judging a name without judging
its position**:

- `container()` refused to descend into a lone bare block unless it held two
  brace-*cases*.  `self_test_sysv_ipc_mqueue` wraps its whole 1 874-line body
  in one such block containing no block at all, so `--flat` saw one statement
  where there are nine paragraphs.
- Every paragraph after the first appeared to read the previous one's `a`,
  because `let a = SyscallArgs { .. };` *mentions* `a` and the use-test could
  not tell a declaration from a use.  That line opens all 96 cases of that
  function.
- `if let Some(ev) = q.first() { ev.namespace_str() }` mentions `ev` twice, and
  the second is the binding the first made.  This welded nine independent
  paragraphs of `eventlog::self_test` into two clusters.

The rule that resolves all three: a mention is a read of an outer name when it
stands *before* the point a `let` in that statement brings the name into scope
- which is the end of the **initialiser**, not the end of the pattern.  That is
the same rule that makes the right-hand `a` of `let a = a + 1;` the outer one,
and it falls out for free.

Paragraphs whose locals genuinely cross into each other are **coalesced**
rather than skipped: a crossing local does not mean unsplittable, it means the
blank line is in the wrong place (`let sops_ptr = ..` declared in one
paragraph, dereferenced in the next).  The merge is bounded so it cannot
swallow the whole body - a local declared at the top and used at the bottom
links the first group to the last, and merging there would produce one case
whose peak is `outer + case`, i.e. the no-op again.  Such a paragraph is left
in the outer frame, which is where a body-scope local belongs.

**And one that was not a self-test at all.**  `scfilter::init` - 21 872 bytes,
the largest frame in the kernel once the tests were split - was twenty lines
long and carried a comment explaining that it used no stack:

> `EMPTY` is a `const`, so the all-empty table lives in read-only static
> memory; `Box::new` copies it straight to the heap without first constructing
> a ~19 KiB temporary on the kernel stack.

Being a `const` is what *guarantees* that temporary.  A `const` has no address
and is inlined at each use site; `Box::new` takes its argument by value; so at
`opt-level = 0` the ~19 KiB `FilterTable` is built in `init`'s frame and then
memcpy'd to the heap.  `EMPTY` is now a `static` - one fixed address - and the
table is copied from it straight into an uninitialised box.  `init` is now
under 480 bytes, and no longer ranks in the top five frames of its own module.

This is worth generalising: **a comment asserting a performance property is not
evidence of it.**  This one was specific, plausible, and exactly backwards, and
it survived because nobody measured the function it described.  The same shape
was found in `scfilter::check`'s doc comment earlier (see the "History" section
there, where a `~5 ns` estimate was typeset as a measurement).

**Where this leaves the census.**  The kernel's largest single frame is now
`kernel_main` at 19 776, where before today's work it was
`proc::linux_fd::self_test` at 28 224 - and before this whole thread,
`self_test_prctl_dispatch` at 32 160.  The remaining top entries are
`test_per_cpu_work_stealing` (19 504), `PerCpuScheduler::new_const` (18 752)
and `cmd_oci`'s run arm (18 304), none of which the tool can take:

- `cmd_oci`'s run arm was re-examined with `--flat` after the tool was taught
  to handle unit-returning functions (`--at LINE` picks it out of the 78
  functions now named `case` in `kshell.rs`).  It has **two** paragraphs and
  both cross: it is 126 locals of option parsing in one `while` loop, and the
  locals really are all live.  This is a restructuring job, not a splitting
  one, exactly as the previous note concluded.
- `test_per_cpu_work_stealing` has one paragraph; a one-case split is a no-op.
- `PerCpuScheduler::new_const` is a `const fn` building a large fixed array,
  with no case structure of any kind.

Verified end to end: `cargo build` clean, `cargo clippy --message-format short`
exit 0, `split-frames.py --check` 21/21, and two full boot tests - one for the
five self-test splits, one for `scfilter::init` - both PASSED with every gate
green.

### Progress - 2026-08-18: `kernel_main`, 19 776 -> 10 976

`kernel_main` was the largest frame left, and it is the one frame that is
certainly live under every init routine and every self-test it calls - so
whatever it claims is subtracted from the headroom of everything below it, for
the whole of boot. It is 5 600 lines and 477 paragraphs, none of them nested,
which is precisely the shape `--flat` exists for.

Pointing the tool at it turned up **four more defects**, three of them the same
mistake as the three before: judging a name without judging where it stands.

- **A path qualifier is not a local read.** The body opens with
  `if let Some(ref fb) = boot_info.framebuffer`, binding an `fb` that dies two
  lines later - and then calls `fb::init()` and, 5 000 lines further down,
  `fb::self_test()`. Counting those as reads of the local kept its binding
  live across 395 of the 477 paragraphs and coalesced them into a single
  17 KiB case: 6 cases, peak 19 776 -> 18 720, a 5% "split". Three shapes are
  spelled exactly like a local and can never be one - `x.name` (a field),
  `name::init` (a qualifier), `core::name` (a segment) - and excluding them is
  not a heuristic trading safety for yield, which matters because
  *under*-approximating reads is the one direction that can produce a wrong
  boundary rather than a missed one. With the rule in place: 36 cases, peak
  **10 976** (outer 7 264 + deepest case 3 712). The field half of this rule
  already existed, in a different function, used for a different purpose.
- **An item is not a local.** A `const`, `static`, `fn` or `use` written in a
  body is scoped to its *block*, and a nested `fn` may freely name one from an
  enclosing block. So moving an item into a case is not the `E0434` that
  guards locals - nothing about the paragraph looks wrong at all - it is an
  `E0425` in some *other* case entirely. `kernel_main` declares
  `static INIT_ELF: &[u8] = include_bytes!(..)` in one paragraph and writes it
  to the filesystem five paragraphs later; the first attempt produced six such
  errors across three cases. Items are also two-sided, unlike locals: a `fn`
  is callable above its own declaration, so the forward-growing walk cannot
  express the constraint and items are welded first.
- **A diverging body's last paragraph is in tail position.** In a `-> !`
  function the final statement must itself diverge, and
  `{ fn case() {..} case(); }` evaluates to `()`. That is
  `error[E0308]: expected !, found ()`, and it costs exactly one case to avoid:
  the paragraph is folded into the tail, whose reads keep alive the locals it
  needs.

The fourth was in the *measuring* tool, and is the worst of them:

- **`--peak` paired one level of `fn case` and stopped.** A case big enough to
  be worth splitting again nests, and `net::httpd::self_test` is exactly that.
  The report said its peak was 3 248 - outer 128 plus a 3 120 case - when that
  case calls a nested case claiming a further 5 104. Its true peak is 8 352,
  and the table above has been corrected. A shallow peak is worse than no peak,
  because it is the number a split is *judged* by: it flatters every split that
  needed a second pass, which is every split that mattered. `--peak` also never
  listed `kernel_main` at all, because an `#[unsafe(no_mangle)]` parent sits in
  the symbol table under its plain name while its cases stay under the mangled
  path, so the prefix match found no parent. The one function the report was
  built to watch was the one function it silently omitted.

That last one belongs beside the `scfilter` comment above, as the same failure
in a different register: **a tool that answers a question you did not ask is
indistinguishable from one that answers the one you did.** The `scfilter`
comment claimed a property nobody measured; `--peak` measured a property nobody
checked the definition of. Both read as evidence.

**Where this leaves the census.** The kernel's largest frame is now
`test_per_cpu_work_stealing` at 19 504, with `PerCpuScheduler::new_const`
(18 752), `cmd_oci`'s run arm (18 512 by peak) and `proc::linux_fd::self_test`
(18 928 by peak) behind it. Before this whole thread it was
`self_test_prctl_dispatch` at 32 160. The three the tool cannot take are
unchanged and are all restructuring jobs rather than splitting ones:

- `cmd_oci`'s run arm: two paragraphs, both crossing - 126 locals of option
  parsing in one `while` loop, all genuinely live.
- `test_per_cpu_work_stealing`: one paragraph; a one-case split is a no-op.
- `PerCpuScheduler::new_const`: a `const fn` building a large fixed array.

Verified: `cargo build` clean with zero warnings, `split-frames.py --check`
23/23 (two new cases - a `const` read three paragraphs later, and a diverging
tail), and a full boot test.

### Progress - 2026-08-18: the floor is now the standard library

Eight more `kshell` dispatchers, seven more self-tests, and one function that
was never an `opt-level = 0` artifact at all.  The census below is the whole
point of this section: **the kernel has essentially stopped being the tallest
thing on its own stack.**

| Function | before | after (peak) | how |
|---|---:|---:|---|
| `audio_mixer::mix_output` | 13 168 | off the top-22 | moved to a static |
| `self_test_remap_ioprio_futex2` | 13 712 | **6 000** | `--flat`, 9 cases |
| `sched::backend::self_test` | 13 680 | **6 160** | `--flat`, 8 cases |
| `mm::alloc_trace::self_test` | 13 520 | **~4 550** | `--flat`, 8 cases |
| `mm::tlb_gather::self_test` | 13 168 | **4 416** | block, 5 cases |
| `fs::fat::self_test` | 11 952 | **3 168** | `--flat`, 24 cases |
| `fs::progmgr::self_test` | 11 264 | **6 608** | `--flat`, 5 cases |
| `kshell::cmd_progmgr` | 11 280 | **2 240** | `--arms`, 23 cases |
| `kshell::cmd_netsettings` | 11 240 | **2 944** | `--arms`, 21 cases |
| `kshell::cmd_schedtune` | 11 088 | **4 224** | `--arms`, 17 cases |
| `kshell::cmd_capsettings` | 10 976 | off the top-25 | `--arms`, 22 cases |
| `kshell::cmd_filepicker` | 10 672 | off the top-25 | `--arms`, 19 cases |
| `kshell::cmd_tasksched` | 10 656 | **2 224** | `--arms`, 19 cases |
| `kshell::cmd_fsearch` | 10 656 | **2 864** | `--arms`, 7 cases |
| `kshell::cmd_useracct` | 10 432 | **2 496** | `--arms`, 23 cases |

Prologues claiming >= 4 KiB: 470 -> 459.

**Stop when you reach 13 088.**  The raw ranking now reads:

| bytes | symbol |
|---:|---|
| 13 344 | `kshell::cmd_oci::case` (the `"run" \| "create"` arm) |
| **13 088** | **`alloc::collections::btree::node::Handle::insert_recursing`** |
| 12 448 | `net::ssh::init` |
| 12 432 | `fs::vfs::self_test` |
| 12 416 | `fs::xz::lzma2_decode` |
| 12 288 | `fs::handle::self_test` |
| 11 920 | `self_test_eventfd_futex_pkey` |
| **11 648** | **`alloc::...::BalancingContext::bulk_steal_left`** |
| **11 616** | **`alloc::...::BalancingContext::bulk_steal_right`** |

Three of the top nine frames are `alloc`'s B-tree monomorphisations.  They are
not ours to split, they are on the stack whenever any `BTreeMap` insert
rebalances, and no amount of work on kernel functions moves them.  **This is a
floor.**  Driving a kernel function from 12 KiB to 4 KiB below it buys headroom
on that function's own path - which is worth having, see the reversal below -
but it no longer moves the kernel's peak.  Only `cmd_oci::case` still does.

**A prediction that was wrong, recorded because the reasoning was reasonable.**
All eight dispatchers in the table above already sat *below* the 13 088 floor,
so by the peak argument alone the split could not help and was pure churn - the
same reasoning that correctly reverted four earlier splits.  I expected to
revert the batch.  The measurement said 4-5x, not the 1-6% those reverts were
made of.  The lesson is not "always split": it is that *below the floor* and
*not worth doing* are different claims, and only the first follows from the
floor.  Measure, then decide.

**`audio_mixer::mix_output` was a different animal entirely** and is worth
separating from everything else here.  Its 13 168 bytes were not `opt-level=0`
slop - they were `[i32; 2048]` plus `[u8; 4096]`, two real buffers that do not
shrink under `-O`.  Splitting could never have touched it.  It is now a
lock-guarded static, which also supplied the mutual exclusion the function had
always assumed and never had (two CPUs would each have summed a private
accumulator and then both written the same output slice).  **When a frame
resists splitting, check whether it is actually data before assuming the tool
is at fault** - `audio_notify.rs:133`'s `[0u8; 8192]`, still open, is the same
shape.

**Two transformer defects fixed**, both of the same kind as the earlier three -
the tool silently emitting code that could not compile:

- `--arms` never checked that a fixture the arms read was actually threaded in.
  Every dispatcher binds `parts` in its preamble and reads it in nearly every
  arm, so omitting `--param` did not fail here and there; it made the file
  uncompilable.  Eight dispatchers split in one batch surfaced as 320 errors
  258 seconds later, with the real cause - a missing argument to the script -
  nowhere in the output.  It now refuses up front and names the local.
- The leading *fixture* paragraph, skipped by `--flat` and correctly left in
  the outer frame, was not added to the set of outer locals, so every cluster
  below it was judged as though it had never existed.  36 `E0434`s.

`split-frames.py --check` is now 26/26.

**What is left.**  `cmd_oci::case` (13 344) is the only frame above the floor
that is ours.  It is 1 178 lines of sequential container-configuration build-up
with ~126 genuinely-live locals, so it is a restructuring job, not a splitting
one.  Below it, `fs::vfs::self_test` (12 432) and `fs::handle::self_test`
(12 288) are refused by the transformer for a good reason - each contains a
bare `return` that `case()?` would silently change the meaning of - and need
hand splitting.  `net::ssh::init` (12 448) and `fs::procfs::generate` (11 072)
have no case structure at all.

### Progress - 2026-08-18: the kernel is now entirely below the stdlib floor

`cmd_oci`'s `run`/`create` arm - the "restructuring job, not a splitting one"
the paragraph above left as the last item above the floor - is done, by hand
rather than by `split-frames.py`.  **With it, the largest prologue in the kernel
is `alloc`'s own `Handle::insert_recursing` at 13 088 bytes.  Every remaining
kernel function is below it.**  That is the terminal state for this line of
work as originally framed: there is no longer any kernel frame whose shrinking
would lower the kernel's peak.

| | before | after |
|---|---:|---:|
| largest single frame on the `oci run` path | 13 344 | **4 048** |
| peak over that path (`cmd_oci` -> `oci_run` -> `OciRunFlags::parse`) | 19 640 | **10 344** |
| `cmd_oci` as `--peak` reports it | 13 344 | **2 432** |
| prologues claiming >= 4 KiB | 459 | **458** |

Nine siblings along the container lifecycle: `prepare_rootfs`, `build_config`,
`report_created`, `bind_network`, `mount_jail`, `apply_created_state`,
`merge_env`, `effective_workdir` / `effective_uid_gid`, `launch_init`.  Largest
of them is `launch_init` at 2 160.

**Two things worth keeping from how it was done.**

- **The transformer was the wrong tool and that was clear in advance.**
  `--arms` splits *at a `match`*; this arm's cost was not in a match, it was in
  700 lines of straight-line configuration where every local really is live at
  the end.  The split had to follow the container's lifecycle, which no
  structural rule can infer.  Attempting it with `--arms` anyway would have
  produced one arm containing everything.
- **It was moved by script, not retyped.**  `build/split-oci-run.py` (untracked;
  `build/` is git-ignored) extracts each paragraph by line range, re-indents it,
  and applies a list of asserted renames.  The reason is not tedium: the arm is
  ~700 lines of console output whose exact wording is the shell's user
  interface, and retyping it is precisely how wording drifts without anyone
  noticing.  Every rename is asserted to have matched at least once, so a range
  that moved fails in the script rather than 700 lines away in rustc.

**Two structural changes fell out and were kept**, both because they are
clearer than what they replace, not merely smaller: the 23-binding
`let OciRunFlags { .. } = ` destructure goes back to a single `f` threaded as
`&OciRunFlags`, and `match container::create` becomes a `let ... else` early
return, which de-nests 440 lines by a level.

**What is left after this** is all below 13 088 and therefore does not move the
kernel's peak - it buys headroom on its own call path only, which is still worth
having (see the wrong prediction recorded above) but is no longer urgent:
`net::ssh::init` (12 448), `fs::vfs::self_test` (12 432), `fs::xz::lzma2_decode`
(12 416), `fs::handle::self_test` (12 288), `fs::procfs::generate` (11 072),
`fs::ext4::fsck::fsck_ext4` (10 656), and the ~9.6-10.2 KiB kshell dispatchers
`cmd_sysinfo`, `cmd_focusassist`, `cmd_autostart`, `cmd_wallpaper`, `cmd_nc`,
`cmd_appnotify`, `cmd_parental`.  On the `oci run` path specifically the
dominant term is now `OciRunFlags::parse` at 6 088.
