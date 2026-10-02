### [A] No program larger than 16 MiB can be started at all, and it is a hard limit rather than the fragmentation I reported -- 2026-09-21
**Status:** FIXED 2026-09-25 (design-decisions.md §959) -- heap allocations over 16 MiB are mapped from vmalloc; the cmake rung now runs instead of skipping. A-Q19 resolved and removed from `open-questions.md`.

**In short:** the kernel cannot start any program bigger than 16 megabytes.
Not *usually* cannot, not *when memory is busy* -- cannot, every time, by
construction. I reported this yesterday as a fragmentation problem that
might come and go. It does not come and go.

**The chain, each link verified rather than inferred:**

| step | fact |
|---|---|
| starting a program | `spawn_process(elf_data: &[u8], ...)` takes the **whole ELF as one contiguous slice** |
| getting that slice | `Vfs::read_file` -> a `Vec<u8>` -> the kernel heap |
| a big heap request | `heap.rs` `large_alloc` -> `frame::alloc_order(order)` |
| the order | `large_order` rounds the frame count **up to the next power of two** |
| the ceiling | `frame.rs` `alloc_inner`: `if order > MAX_ORDER { return Err(InvalidArgument) }`, and `const MAX_ORDER: usize = 10` |

2^10 frames x 16 KiB = **16 MiB**. A request above that is refused
*before* the free lists are consulted, so free memory is irrelevant: the
boot that died had 2.7 GB free.

And the rounding moves the real boundary lower than it looks. A binary of
8 MiB + 1 byte rounds to 1024 frames and demands the **entire 16 MiB
maximum block**. So the practical ladder is: under 8 MiB is comfortable,
8-16 MiB needs the largest block the allocator can ever produce, and above
16 MiB is impossible.

**Three things I wrote yesterday are wrong, and the same mistake produced
all three.**

| I wrote | actually |
|---|---|
| *"python312.zip is 20,498,464 bytes, the same order, and reads fine every boot"* | it is **never read into kernel memory**. `SRC_ZIP` appears in `pathz_missing` -- an existence check -- and CPython reads it itself from ring 3 |
| *"whether a 2048-frame block exists depends on fragmentation"* | order 11 is rejected by a bounds check before any free list is examined |
| *"the failure is nondeterministic, which is the worst kind of panic"* | it is perfectly deterministic |

Every one came from the same move: I compared **file sizes** and concluded
something about **allocation behaviour**, without checking whether the file
I was comparing against is ever allocated. The zip was my control, and my
control was not in the experiment.

**What it means beyond cmake.** This is not a property of the cmake rung; it
is a property of the OS. Any port that produces a binary over 16 MiB cannot
be started, and the failure arrives as a kernel allocation abort rather than
as a diagnosable refusal. `cmake-slateos.elf` at 22,526,200 bytes is simply
the first thing to cross it. The other four spikes are 1.8-10.5 MB and fit.

**The fix is one of three, and none is small.**

| option | cost |
|---|---|
| raise `MAX_ORDER` to 11 or 12 | touches the core frame allocator; a 32 MiB contiguous reserve is a large standing demand on a 5 GiB machine, and the free-list array and every order walk grow with it |
| stop requiring contiguity in `spawn_process` | the honest fix. The ELF is parsed and its segments copied into the new address space; it does not need to be one slice to do that. Touches the loader, which every process start goes through |
| document it as a supported limit | free, and what this entry does for now |

Not attempting the first two on the strength of one blocked self-test.

*(Corrected within the hour: this said the limit "was unrecorded before
today". It was not. `frame.rs` exports `BUDDY_MAX_ORDER` and its doc reads
"Order N = 2^N frames. Order 10 = 1024 frames = 16 MiB" -- the number has
been there, correct, in the allocator's public diagnostics section. What was
missing is the CONSEQUENCE: nothing connected "the largest single allocation
is 16 MiB" to "therefore no executable above 16 MiB can start", and the two
facts sit in different files with no reference between them. A reader of
`spawn_process` has no reason to visit the allocator's diagnostics
constants, and a reader of that constant is thinking about fragmentation
histograms, not the ELF loader.

That is a different defect from an undocumented limit and a more
interesting one: **the fact was present and its implication was not
derivable from where it sat.** Fixed by connecting them -- `spawn_process`
now carries a `# Size ceiling` section naming the limit and citing
`BUDDY_MAX_ORDER`, so the implication is visible at the place it is met
rather than in the file that happens to own the number.)*


**Fixed, 2026-09-25 -- by an option the table above did not have.** None of
the three: the kernel heap now sends any allocation whose buddy order would
exceed `BUDDY_MAX_ORDER` to vmalloc, which builds it from single frames and
maps them contiguously in kernel virtual memory. `spawn_process` still takes
one slice; the slice just no longer has to be physically contiguous, which it
never needed to be. Allocations the buddy allocator can serve are untouched.

Making vmalloc fit to carry the heap took more than the routing. It had one
caller (its own self-test), and four defects that one caller could not see:
mappings created a top-level page-table entry only in whichever address space
was loaded, were charged to that process's memory use, were never flushed from
the TLB when freed, and raced on intermediate-table creation. All four are
fixed and each has a self-test that fails on the old code; §959 has the
detail. The loader rewrite (stream segments from the file instead of holding
it) remains the better memory profile and is now an optimisation, not a
prerequisite.
