# CLAUDE.md — AI-Assisted OS Development Standards

This is a Rust microkernel OS for x86_64 desktops. The entire codebase is written by AI (Claude). The human operator does not review code line-by-line and does not extensively test. **You are the developer, the reviewer, and the tester.** Act accordingly.

## Project Layout and Key Files

- `design.txt` — full design specification (the "what")
- `roadmap.md` — **the live source of truth for task progress/status** (the "when"). This is the phased task list with dependencies and status; consult and update it when starting or finishing a task.
- `roadmap-detailed.md` — the design reference: an exhaustive, `design.txt`-derived feature inventory (finer-grained than `roadmap.md`, but NOT the status authority — it lags). Annotate it in place with concise status flags (`[x]` done, `[-]` in progress, `[~]`/blocked + a short "blocked by …" note) **without deleting any information**, and only when the status is verified (cross-referenced against `roadmap.md` or the code) — never fabricate done-status, and don't attempt a one-shot reconciliation of every item. See design-decisions.md §13.
- `scheduler.txt`, `ipc.txt`, `memory management.txt` — deep dives on subsystems
- `design-review.txt`, `design desicions.txt`, `other design decisions.txt` — rationale and tradeoffs

Read the relevant design files before implementing any subsystem. Do not guess at requirements when the answer is written down. Where `design.txt` conflicts with `design desicions.txt` or `other design decisions.txt`, `design.txt` wins.

## Build Environment Notes

### C/C++ Compilers (for crates with C dependencies like libz-sys, ring, libgit2-sys)

The machine has Visual Studio and Build Tools installed but they are **not on PATH by default**. You must either run vcvarsall.bat first or set `CC`/`CC_x86_64_slateos` to the full path.

**vcvarsall.bat locations:**
- VS 2022 Community: `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2022 Enterprise: `"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2026 Build Tools: `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"`
- VS 2022 Build Tools: `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvarsall.bat"`

**cl.exe locations (x64 host → x64 target):**
- VS 2022: `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64\cl.exe"`
- VS 2026 BT: `"C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Tools\MSVC\14.50.35717\bin\Hostx64\x64\cl.exe"`

For cross-compiling Rust crates with C dependencies to our custom target (x86_64-slateos), set:
```
CC_x86_64_slateos=cl.exe   (after running vcvarsall.bat x64)
```
Or from bash without vcvarsall, pass the full cl.exe path and ensure the Windows SDK include/lib dirs are also set.

### Rust toolchains

- `nightly-x86_64-pc-windows-gnu` — primary nightly, has `dlltool.exe` issue with newer crates (getrandom v0.3+). Fixed by copying `dlltool.exe` from self-contained dir to `~/.cargo/bin/`, but it may not work for all crates.
- `nightly-x86_64-pc-windows-msvc` — alternative nightly, avoids dlltool issues, requires VS build tools on PATH for host-side C compilation.
- Custom target `toolchain/x86_64-slateos.json` requires `-Zjson-target-spec` (set via `[unstable]` in `.cargo/config.toml`, NOT via env var or CLI flag).

---

## Single Session — Zone Names as Navigation Aid

This repo is now worked by a **single Claude session at a time**. The
multi-session zero-contention architecture (per-zone fragment files,
self-registration patterns, request dropbox) is no longer in force —
edit any file freely. If you need to change a shared trait, change it.
If you need to add a workspace member, add it. There's no other session
to coordinate with.

The zone names below are kept as a **navigation aid only** — they tell
you where in the tree a given subsystem lives. They are no longer
ownership boundaries.

| Zone | Covers | Typical paths |
|------|--------|---------------|
| **kernel-core** | boot, GDT/IDT, interrupts, memory manager, page allocator, heap, scheduler | `kernel/src/boot/`, `kernel/src/mm/`, `kernel/src/sched/` |
| **kernel-ipc** | syscall dispatch, channels, pipes, shared memory, futexes, io_uring, IOCP | `kernel/src/ipc/`, `kernel/src/syscall/` |
| **kernel-security** | capabilities, process namespaces, CFI setup, IOMMU | `kernel/src/cap/`, `kernel/src/security/` |
| **kernel-process** | process/thread lifecycle, ELF loader, exception handling | `kernel/src/proc/` |
| **drivers** | driver framework, USB, storage, network, keyboard, display, virtio | `drivers/` |
| **fs** | VFS, ext4 port, FAT32, recycle bin, change notifications | `fs/` |
| **net** | TCP/IP stack, UDP, DNS, DHCP, sockets, firewall | `net/` |
| **posix** | POSIX compatibility layer, libc translation | `posix/` |
| **init** | service manager, init, startup sequencing | `init/`, `services/` (bare-metal startup binaries) |
| **shell** | shell, coreutils, terminal emulator | `userspace/shell/`, `userspace/term/` |
| **gui-core** | compositor, DRM/KMS, GPU drivers, 2D drawing | `gui/compositor/`, `gui/gpu/` |
| **gui-toolkit** | widget library, layout engine, styling, clipboard, drag-drop | `gui/toolkit/` |
| **desktop** | window manager, taskbar, start menu, system tray, themes | `gui/desktop/` |
| **apps** | file explorer, process explorer, settings, text editor, etc. | `apps/` |
| **pkg** | package manager, content-addressed store, generations | `pkg/` |
| **bench** | all benchmarks and performance infrastructure | `bench/` |

### Branch Strategy

Work on feature branches and merge to `main` when the work compiles and
passes tests. Before merging:

- `git pull --rebase origin main`
- Run the full test suite
- Only then merge

### CLAUDE.md

Do not edit this file during normal development. The human operator is
normally the only one who edits it. **Exception:** you may edit this
file when the user explicitly tells you to make a specific change to it.
Do not edit it on your own initiative — only on an explicit instruction.

---

## Code Quality — You Are the Only Reviewer

The human will not be reading your code to catch mistakes. Every bug you write ships. Treat every function as if it will run unsupervised on someone's computer managing their data.

### Self-Review Checklist (run mentally before finishing any task)

1. **Does it handle every error path?** No unwrap() in production code. No silent failures. Map errors to meaningful types. Propagate or handle — never swallow.
2. **Is there UB?** Every `unsafe` block must have a `// SAFETY:` comment explaining why the invariants hold. If you can't write the safety comment, the code is wrong.
3. **Does it leak resources?** File handles, memory mappings, channel handles, capability tokens — all must be cleaned up on every exit path, including panics. Use RAII.
4. **Are there data races?** Every shared mutable state must be protected. Document the locking order if multiple locks are involved. Prefer lock-free structures in hot paths.
5. **Does it match the design spec?** Re-read the relevant section of `design.txt` and the roadmap task description. Don't add features that aren't specified. Don't omit features that are.
6. **Would this be obvious to the next session?** The other Claude sessions (or a future you with no memory) will read this code. Clear naming, doc comments on public items, module-level `//!` docs explaining the subsystem's purpose and design.
7. **Are paths and OS-boundary data handled as bytes?** Never force UTF-8 on filesystem paths, environment variables, or pipe data. Use `OsStr`/`Path`/`&[u8]`, not `String`/`&str`. No `from_utf8_lossy` — that's silent data corruption. Our paths allow all bytes except `/` and `\0`.
8. **Are all inputs resolved before crossing trust boundaries?** Path resolution, user lookups, library loading, and capability checks must complete before entering restricted contexts (sandboxes, privilege drops, namespace changes). After crossing, external resolution may load attacker-controlled data.
9. **Are errors propagated, not discarded?** Never use `.ok()`, `.unwrap_or_default()`, or `let _ =` to discard a `Result` without a comment explaining why the failure is safe to ignore. Batch operations must track and report the worst error, not just the last one.

### Coding Conventions

- **Language**: Rust and Python are the two primary languages.
  - **Rust**: kernel, drivers, compositor, performance-critical userspace services (IPC daemon, filesystem services, audio mixer), and anything that needs bare-metal control or `no_std`.
  - **Python (compiled via fastpy)**: non-kernel OS components where development speed matters and native performance is achieved through AOT compilation. Good candidates: package manager, settings/configuration UI, system utilities, backup program, file indexer, installer, build scripts, service discovery, system information explorer.
  - **C**: only when porting existing C code (ext4, Mesa, Chromium, etc.).
  - **Ada/SPARK**: safety-critical driver logic per the design spec.
  - **Rule of thumb**: if it runs in kernel space or is in the performance-critical table below, use Rust. If it's a userspace tool or application, prefer Python via fastpy unless there's a specific reason to use Rust (e.g., tight integration with a Rust library, `no_std` requirement). fastpy compiles to native code at C++ speed — there is no performance penalty for choosing Python.
- **Edition**: Rust 2024 (or latest stable).
- **Formatting**: `rustfmt` defaults. No manual formatting overrides.
- **Linting**: `#![deny(clippy::all, clippy::pedantic)]` in every crate. Suppress individual lints only with a comment explaining why. Additionally, enable these defensive lints in non-test code (allow in `#[cfg(test)]` modules where panicking on bad data is expected):
  ```toml
  [lints.clippy]
  unwrap_used = "warn"
  expect_used = "warn"
  panic = "warn"
  indexing_slicing = "warn"
  arithmetic_side_effects = "warn"
  ```
  In production code: use `?`, `.get()`, `.checked_*`, `TryFrom` instead. Every `unwrap`/`expect` in non-test code is a potential DoS if an attacker can shape the input. If you must discard a `Result` with `.ok()` or `let _ =`, add a comment explaining why this specific failure is safe to ignore.
- **Error handling**: Use `thiserror` for library errors, `anyhow` only in top-level binaries/tests. Kernel code uses its own error enum — no heap-allocating error types in the kernel.
- **Naming**: Rust conventions (snake_case functions, CamelCase types). Syscall functions prefixed with `sys_`. IPC message types suffixed with `Msg`. Capability types suffixed with `Cap`.
- **Comments**: Explain *why*, not *what*. The code shows what; the comment explains the design decision, the invariant being maintained, or the edge case being handled.
- **No TODO without a tracking note**: If you write `// TODO:`, also add a corresponding entry to `todo.txt` with enough context for someone else to act on it.
- **No dangling references**: Never store references or pointers to elements of a container (like `Vec`) across call boundaries where the container could reallocate. Store stable identifiers (IDs, keys) and look them up when needed.
- **Always do the proper fix**: Never write a quick hack "for now." If a fix requires a large refactor, do the refactor. Quick fixes accumulate as tech debt.

### Architectural Discipline

- **Never defer a fundamental fix for a convenient hack.** If the correct solution requires restructuring, restructure now. The "quick fix for now" is never temporary — it becomes permanent and attracts more hacks on top of it. If there is a genuinely good reason to defer a structural change (e.g., a dependency isn't built yet, or the right design requires information you don't have yet), document it in `todo.txt` with what the correct fix is, why it's deferred, and what condition should trigger doing it properly.
- **Watch for band-aid accumulation.** If you find yourself patching around the same issue in multiple places, stop. Go back and redesign the underlying system with full hindsight. The cost of rework now is always less than the cost of a fundamentally flawed foundation later.
- **Document every known bug and limitation.** If you discover a bug and choose to continue with other work before fixing it, write it up in `todo.txt` with enough detail to reproduce and fix it later. Same for known limitations or shortcuts — if you knowingly ship something incomplete, document exactly what's missing and why. Undocumented bugs are invisible bugs; they will bite someone.
- **Keep a running list of unsolved bugs and technical debt in a markdown file.** In addition to `todo.txt`, maintain an `.md` file (e.g. `bugs.md`, `tech-debt.md`, or a combined `known-issues.md`) where unsolved bugs and accumulated technical debt are tracked. Each entry should have enough context to act on later: what the bug or debt is, where in the code it lives, how to reproduce it (for bugs), and what the proper fix looks like (for debt). If the file doesn't exist yet, create it when you first need to log something. **Ideally, though, bugs and tech debt are fixed immediately as they're discovered** — the tracking file is a fallback for when something genuinely can't be addressed in the current task, not a place to defer work that should be done now.

### Unsafe Code Policy

This is a kernel — `unsafe` is unavoidable. Minimize it and isolate it.

- Wrap every `unsafe` operation in a safe abstraction as close to the call site as possible.
- Never use `unsafe` for convenience or performance unless profiling proves it's necessary and there is no safe alternative.
- Every `unsafe` block must have a `// SAFETY:` comment. No exceptions.
- Audit: after implementing any module that uses `unsafe`, re-read every unsafe block and verify the invariants still hold given the final code.

---

## Testing — You Are the Only Tester

The human will not be running test suites or manually testing functionality. If you don't test it, it is untested. Ship nothing untested.

### Testing Requirements

Every module, function, and subsystem must have tests before it is considered done. "It compiles" is not "it works."

#### Unit Tests
- Every public function gets at least one test for the happy path and one for each error/edge case.
- For `unsafe` code: test the boundary conditions specifically (off-by-one, null, max values, alignment).
- Use `#[cfg(test)]` modules in the same file as the code they test.

#### Integration Tests
- Every subsystem gets integration tests in `tests/` that exercise the public API as a real caller would.
- IPC tests: send messages across channels, verify delivery, test backpressure, test channel close.
- Scheduler tests: verify priority ordering, preemption, priority inheritance under contention.
- Memory tests: allocate/free patterns, fragmentation behavior, OOM handling.
- Filesystem tests: create/read/write/delete files, concurrent access, full-disk behavior, crash recovery (simulate power loss by truncating the journal).

#### Stress Tests
- Concurrency stress tests for all shared data structures. Run thousands of operations across multiple threads/cores.
- Memory pressure tests: allocate until OOM, verify graceful degradation.
- IPC flooding: send maximum-rate messages, verify no drops or corruption.

#### Boot Tests
- After any change to boot, memory init, or interrupt handling: verify the kernel boots in QEMU. Use the QEMU `-serial stdio` output to confirm reaching expected milestones.
- Automate this with a test script that boots QEMU, waits for a success marker on serial, and exits. Fail the test if the marker doesn't appear within a timeout.

### Running Tests

Always run tests after making changes:
```bash
# Unit and integration tests for the crate you changed
cargo test -p <crate-name>

# Full workspace test before merging
cargo test --workspace

# Boot test (when touching kernel boot path)
./scripts/boot-test.sh

# Benchmarks (when touching performance-critical code)
cargo bench -p <crate-name>
```

If a test fails, fix it before moving on. Do not comment out failing tests.

### `scripts/run-timeout.py` — hang-proof test/command runner

**Use this to run any command that could hang, deadlock, or leave orphans**
— above all `cargo test` (a deadlocked test never exits on its own) and
QEMU boot tests. Do **not** wrap such runs in coreutils `timeout`: `timeout`
only kills the direct child (`cargo`), so the spawned test binaries — and
any grandchildren they spawn (e.g. `std::process::Command` externals) —
survive as orphans that spin for hours and flood output.

`run-timeout.py` fixes this by putting the child in a Windows **Job Object**
with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (POSIX: a process group + SIGKILL).
On timeout, on Ctrl-C, or if the runner itself dies, the **entire process
tree** is torn down atomically — grandchildren included. It streams the
child's output live (no buffering that hides progress) and prints a
heartbeat every `--poll` seconds so a long build never looks like a hang.

```bash
# python scripts/run-timeout.py [--poll SECS] <timeout_secs> <command> [args...]
python scripts/run-timeout.py --poll 20 300 cargo test --target x86_64-pc-windows-gnu
python scripts/run-timeout.py 60 ./scripts/boot-test.sh
```

Exit codes: the child's own code on normal completion; `124` timed out (tree
killed); `125` failed to launch; `130` interrupted. Prefer this over bare
`timeout` for anything that spawns external processes, and always run
potentially-hanging test suites through it in the background so a genuine
deadlock is bounded and can never orphan.

---

## Performance — Benchmark Everything Critical

The design spec calls out that AI tends to write "correct-but-naive" code. This OS must be competitive with Linux, Windows, and macOS in performance-critical paths. The mitigation: **write benchmarks first, then optimize against concrete targets.**

### Performance-Critical Subsystems

These subsystems are on the hot path for virtually every workload. Naive implementations here are not acceptable.

| Subsystem | Why it's critical | Benchmark target (reference) |
|-----------|-------------------|------------------------------|
| **Syscall dispatch** | Every userspace→kernel transition | Linux: ~100ns for getpid. Target: within 2x of Linux. |
| **IPC channel send/recv** | Primary inter-process communication | Fuchsia channel round-trip: ~1-2us. L4 IPC: ~0.5-1us. Target: < 2us round-trip. |
| **Context switch** | Every preemption, every blocking call | Linux: ~1-3us. Target: < 5us. |
| **Page fault handling** | Demand paging, stack growth, CoW | Linux: ~2-5us for anonymous page fault. Target: < 10us. |
| **Physical page alloc/free** | Every mmap, every process start | Linux buddy allocator: ~100-500ns. Target: < 1us. |
| **Heap allocation (kernel)** | Constant kernel bookkeeping | jemalloc small alloc: ~20-50ns. Target: < 200ns for common sizes. |
| **Scheduler pick_next_task** | Every timer tick, every blocking op | Must be O(1) or O(log n). Never O(n) over all tasks. |
| **Futex wait/wake (uncontended)** | Userspace mutex fast path | Linux: uncontended = no syscall (atomic CAS in userspace). Contended wake: ~1-3us. Match this design. |
| **io_uring submission** | High-throughput async I/O | Linux io_uring: ~100-200ns per SQE submission. Target: same order. |
| **IOCP-like completion wait** | Main event loop for all servers/GUI apps | Windows IOCP / Linux epoll: sub-microsecond for ready events. |
| **Interrupt dispatch** | Every keystroke, every packet, every timer | Total ISR latency < 10us. Deferred work via softirq/tasklet equivalent. |
| **VFS path lookup** | Every file open, every path resolution | Linux: cached lookup ~200-500ns per component. Use dcache equivalent. |
| **Filesystem read/write** | All I/O | Compare to ext4 on Linux for sequential and random I/O throughput. Target: within 20% of Linux ext4. |
| **Compositor frame** | Every display refresh | Must composite a full desktop in < 2ms at 4K to not miss 144Hz vsync. |

### Benchmarking Protocol

1. **Write the benchmark before or alongside the implementation**, not after. Use `criterion` for microbenchmarks. Put benchmarks in `bench/<subsystem>/`.
2. **Record baseline numbers** from Linux/Fuchsia/Windows (from published benchmarks, academic papers, or your own measurements on the dev machine). Store these in `bench/baselines.toml`:
   ```toml
   [syscall_getpid]
   linux_ns = 100
   target_ns = 200
   source = "measured on dev machine, Linux 6.x, same hardware"

   [ipc_channel_roundtrip]
   fuchsia_us = 1.5
   l4_us = 0.8
   target_us = 2.0
   source = "Fuchsia perf docs, L4 published benchmarks"
   ```
3. **Run benchmarks after every change to a critical subsystem.** Compare to baselines. If a change regresses a benchmark by more than 10%, investigate before merging.
4. **Optimize iteratively.** Write the correct version first, benchmark it, then optimize. Use profiling (`perf` equivalent, or manual cycle counting via `rdtsc`) to find the actual bottleneck. Don't guess.
5. **Document optimizations.** When you apply a non-obvious optimization, add a comment explaining what it does, why it helps, and what the benchmark improvement was:
   ```rust
   // OPT: Using per-CPU free lists avoids atomic operations on the global
   // allocator in the common case. Benchmark: page_alloc dropped from 800ns
   // to 150ns (5.3x improvement). See bench/mm/page_alloc.rs.
   ```

### Performance Patterns to Follow

- **Per-CPU data structures** for anything accessed on every syscall/interrupt (allocator free lists, scheduler run queues, IPC queues). Avoid cross-CPU atomic contention.
- **Lock-free fast paths** for high-frequency operations. Futexes should be pure atomic CAS in userspace for the uncontended case. Only enter the kernel on contention.
- **Cache-line alignment** for per-CPU structures and heavily-contended locks. False sharing destroys performance.
- **Avoid heap allocation on hot paths.** Pre-allocate, use slab allocators, or use stack allocation. No `Vec::push` in interrupt handlers.
- **Batching** for I/O operations (io_uring model: submit many, complete many, one syscall).
- **Lazy computation.** Don't compute what you might not need (relatime for atime updates, lazy TLB shootdown, deferred page zeroing).

### Performance Anti-Patterns to Avoid

- **Premature abstraction that prevents optimization.** A trait with dynamic dispatch (`dyn Trait`) on a hot path costs an indirect call (~5-10ns + branch mispredict). Use monomorphization or manual dispatch for hot paths.
- **Holding locks across I/O or long operations.** Design for fine-grained locking. If a critical section could block, redesign.
- **Copying data when you can transfer ownership or use zero-copy.** IPC channels should move pages between address spaces, not copy message contents.
- **Linear scans** in the scheduler, allocator, or event dispatch. These must be O(1) or O(log n).

---

## Studying Existing Implementations

Before implementing any major subsystem, study how proven OSes do it. This is not optional — it is the primary mitigation against writing naive code.

### What to Study and Where

| Subsystem | Study these | Where to find them |
|-----------|-------------|--------------------|
| Scheduler | Linux EEVDF, BFS/MuQSS (for desktop ideas), Fuchsia fair scheduler | Linux `kernel/sched/`, BFS patch set, Fuchsia `zircon/kernel/sched/` |
| Memory manager | Linux buddy allocator + SLUB, Fuchsia PMM | Linux `mm/`, Fuchsia `zircon/kernel/phys/` |
| IPC | Fuchsia channels, seL4 IPC, L4 family | Fuchsia `zircon/kernel/object/channel_dispatcher.cc`, seL4 source |
| VFS / filesystem | Linux VFS, ext4 | Linux `fs/`, `fs/ext4/` |
| Capability system | Fuchsia handles, seL4 capabilities, Capsicum | Fuchsia `zircon/kernel/object/`, seL4 source |
| I/O scheduler | Linux BFQ | Linux `block/bfq-*` |
| Graphics compositor | wlroots, Smithay (Rust Wayland), KWin | wlroots source, Smithay crate |
| GUI toolkit | Iced, Slint, egui (Rust), Qt (for widget design ideas) | Respective repos |

### How to Study

1. Read the source for the specific algorithm or data structure, not the entire subsystem.
2. Understand the *invariants* and *design tradeoffs*, not just the code.
3. Adapt the approach to our architecture (e.g., Linux's scheduler assumes CFS/EEVDF semantics; ours uses priority round-robin. Take the per-CPU queue and work-stealing design; drop the virtual-runtime fairness math).
4. Cite your references in code comments: `// Based on Linux's buddy allocator (mm/page_alloc.c) with 16KiB base page adaptation.`

---

## Architectural Rules (from the Design Spec)

These are non-negotiable design decisions. Do not deviate.

- **Microkernel**: drivers run in userspace. Only scheduler, memory manager, IPC, capability enforcement, and interrupt routing run in kernel space.
- **16 KiB pages**, not 4 KiB. The entire memory subsystem must be built around this.
- **Capability-based security from day one.** Every kernel object accessed via unforgeable handles. No ambient authority.
- **Channel IPC** (structured messages + capability transfer) as primary IPC. Not file descriptors. Not signals.
- **Specialized syscalls** (Linux style, many syscall numbers) with optional io_uring-style batching.
- **Versioned syscall tables** for ABI stability.
- **No Unix signals for process control.** Use IPC messages for shutdown, etc.
- **Hardware exceptions → language-level exceptions** (SEH-style), not Unix signals.
- **Case-sensitive filesystem.** Forward slash path separator. Allow all characters except `/` and null.
- **ext4 first.** Do not write a custom filesystem. Port existing battle-tested code.
- **Committed memory by default**, lazy allocation opt-in. No silent overcommit.
- **YAML for configuration files**, processed with a library that preserves comments and formatting.
- **No AI features in the OS** (except speech I/O). No ads.
- **No binary logs.** Text-based (JSON-lines) structured logging.

---

## Autonomous Work — Keep Going

The human operator is often away from the computer or asleep. Do not stop and wait for input when the path forward is clear. The roadmap is long, the design files are detailed, and the strategies are laid out.

- **At every moment you must be in exactly one of three states: (1) actively making forward progress on a task, (2) starting a new task you just picked, or (3) blocked waiting on the operator for input you genuinely need before doing anything further.** "Doing nothing" while still pretending to work is never a valid state. A tick whose entire content is "I sampled X and it was already fine, nothing to do" is idling — it produces no artifact, changes no file, and closes no task. If your current line of work has stopped producing changes (e.g. an audit/verification pass that keeps coming back "already correct"), that line of work is **done** — declare it complete and move to the next roadmap task, even a large one. Do not keep re-running a verification loop that yields no edits just to have something to report. If you can't name a blocker (state 3), you are not blocked — pick the next task and start it.
  - **State (3) — genuinely blocked on the operator — is a legitimate state, and in it you SHOULD idle.** When a design decision, an architectural fork, a user-visible policy, or any irreversible/costly-to-reverse choice genuinely needs the operator's input *and there is no other unblocked task to pick up*, the correct behavior is to **stop**: post the question to `open-questions.md` (with options, pros/cons, and your recommendation), tell the operator you're waiting, and then **stop scheduling autonomous wakeups entirely** — do **not** call `ScheduleWakeup`/keep the `/loop` alive. Let the loop end. Waking up repeatedly only to re-report "still waiting on you" is itself a form of idling and clutters the transcript. Resume the loop only when the operator answers. (First, though, make sure you really are fully blocked: if *any* other roadmap task is unblocked, prefer doing that over stopping — stopping is only correct when the operator's input gates *all* forward progress.)
- **Never launch multiple agents in parallel.** Launching multiple agents at the same time causes rate limiting, which stalls the entire session. Do all work sequentially in the foreground — write files directly, one at a time. This applies to all tasks, not just userspace utilities.
- **Work through the roadmap continuously.** When you finish a task, check `roadmap.md` for the next task whose dependencies are met and start it. Don't ask "what should I do next?" when the answer is written down.
- **You do not need user direction to choose the next task — even a big one.** Picking what to work on is your job, not the operator's. When the current thread is done, select the next roadmap task yourself and start it, whether it's a small increment or a large initiative (a new subsystem, a major port, a multi-day feature). Do not stall waiting for the operator to bless your choice of task. The *only* thing you wait on the operator for is a **design decision you genuinely think they may want to weigh in on** — an architectural fork, a user-visible behavior/policy, a tradeoff with no obviously-correct answer, or anything that would be costly to reverse. For those, ask a focused question (and you may proceed on a clearly-correct default if they're away). For everything else — including *which* big task comes next — just decide and go. When such a design decision genuinely blocks all forward progress and the operator is away, document it in `todo.txt` and pick a *different* unblocked task rather than idling.
- **Make judgment calls, but flag them.** If you hit an ambiguity that the design files don't resolve, make a reasonable decision and keep going — but document it in `todo.txt` under a `## Judgment Calls` heading with what you decided, why, and where in the code it affects. The human will review these and may want changes. Write the code so these decisions are easy to reverse.
- **Record design decisions in `design-decisions.md`.** Whenever you make a design decision that has genuine pros and cons on either side — a real tradeoff, not an obviously-correct choice — add an entry to `design-decisions.md` capturing the decision, the alternatives, and the reasoning on each side. (In practice there probably should not be *any* important design decision with pros and cons on both sides that you make on your own without asking the user first — see the "design decision you genuinely think they may want to weigh in on" rule above. This bullet covers the smaller-scale tradeoffs you do resolve yourself.) When the **user** is the one who makes such a decision, ask the user whether they want it added to `design-decisions.md` before recording it — don't assume.
  - **Every `design-decisions.md` entry MUST record who made the call.** Put a `**Decided by:**` field near the top of each entry (right after the date). The attribution is based **solely on who made the *final call*** — never on who first proposed the idea:
    - `Operator` — **the user made the final call.** Use this whenever the decision was presented to the operator and the operator chose, *regardless of whether you or the operator first suggested the option that was picked.* If you proposed the chosen option, or recommended a different one, that does **not** change the attribution to yourself — it stays `Operator`. You may (and should, when relevant) add a parenthetical recording the collaboration: who proposed the option and whether you agreed or disagreed — e.g. `Operator (Claude proposed this option)`, `Operator (Claude recommended the other option; operator overruled)`, or `Operator (operator's own proposal)`.
    - `Claude (autonomous)` — **you made the final call yourself,** without putting it to the operator. This is yours to revisit later (and the operator may overrule it). If the operator pre-approved the general scope/direction but you made the specific call, use `Claude (operator-approved scope)` and note the split.
    This distinction matters: an **Operator** decision is settled policy you should not silently revisit, whereas a **Claude** one is yours to revisit. Never blur the two — and never attribute a decision to yourself merely because you proposed the option the operator ultimately chose.
- **Track decisions that need the operator in `open-questions.md`.** When a decision genuinely needs the human (an architectural fork, a user-visible policy, a tradeoff with no obviously-correct answer) and you've deferred it rather than resolved it, add it to `open-questions.md` — the question, the options with pros/cons, your recommendation if you have one, and where in the code it bites. This is distinct from `design-decisions.md` (decisions already *made*) and from `known-issues.md` (bugs/tech-debt): `open-questions.md` is the operator's decision queue. When the operator answers one, move it to `design-decisions.md` (marked `Decided by: Operator`) and remove it from `open-questions.md`.
- **Push often.** Run `git push` after every completed task, or at minimum after every few meaningful commits. If you've done good work and the machine loses power, that work should not be lost. Never let a long stretch of unpushed commits accumulate.
- **If you're genuinely stuck** — a design contradiction, a hardware issue, a question that truly requires human judgment — document exactly what you need in `todo.txt`, commit and push your progress so far, then stop. But the bar for "genuinely stuck" is high. Most obstacles have answers in the design files or can be resolved with a reasonable default.
- **Default behavior: schedule a 60-second heartbeat wakeup every turn.** Unless work is genuinely blocked on a human decision, at the end of every turn call `ScheduleWakeup` with `delaySeconds=60` (prompt `<<autonomous-loop-dynamic>>` for autonomous roadmap work) so the autonomous loop keeps ticking and forward progress continues. This is the standing default — you do not need the operator to ask for it; re-establish the heartbeat automatically whenever it is not already running. The only exception is when work is genuinely blocked on a human decision — then schedule no wakeup and let the loop end (see the state-(3) rule above). On this project roadmap work almost always exists, so 60s should be the norm; do not back off to longer idle intervals.

---

## Effort & Duration Calibration (measured, not guessed)

Your trained instinct estimates task durations in **human-developer
calendar units**, and is consistently wrong by ~1–2 orders of magnitude
on this project — because the training data is humans, who code far
slower than you. Do **not** answer "how hard/long is X?" or pace your own
work using that instinct. Use the numbers below, which are **measured
from this repo's own history** (4640 commits over 46 days; every session
JSONL entry and every git commit carries an ISO-8601 millisecond
timestamp, so this is observed, not assumed).

**Your actual measured throughput:**
- **~1 committed, tested, logical change every ~5 minutes of active
  work** (median commit spacing 4.7 min; ~12.6 commits per active hour
  across ~331 active hours).
- Concrete recent anchors (single-day chain): a kernel bug fix *with*
  root-cause instrumentation + a regression test + three full QEMU
  boot-test cycles = **~38 min**; a new syscall feature (`execveat` real
  execution incl. `fexecve`) = **~21 min**; a new capability-gated
  syscall pair (`process_vm_readv/writev`) = **~17 min**; a focused
  ring-3 self-test = **~7–10 min**.

**Divide your gut estimate hard. Rough conversion:**

| Your instinct says… | Reality (active work) | You're off by |
|---|---|---|
| "a couple hours" | ~5–20 min | ~10× |
| "a day or two" | ~20–40 min | ~20–40× |
| "a week or two" (a subsystem) | a few focused active hours, spread over 1–3 calendar days | ~20–40× |
| "months" / "~a year" (e.g. a Chromium port) | days-to-weeks of active work *if unblocked* | ≥10×; and the real gate is missing **prerequisites/dependencies**, not your effort |

**Consequences for behavior:**
- When asked how hard/long something is, answer in the *measured* units
  (minutes-to-hours of active work; at most a handful of calendar days
  for genuinely large things) — not human-team weeks/months.
- When self-pacing, treat "big" roadmap tasks as cheap enough to **just
  start**. A "large initiative" here is usually a few active hours, not a
  commitment you must ration or seek permission to begin. (The standing
  rule to get operator go-ahead before a *giant external port* still
  holds — that's about prioritization/prerequisites, not effort cost.)

**Calendar time vs. active time.** The coding is fast; what actually
stretches wall-clock delivery is **downtime, not effort** — overwhelmingly
rate-limit windows. The most recent rate-limit lasted ~2.9–3.1 days (the
history shows one ~75 h and one ~68 h gap). Even counting *all* idle +
rate-limit gaps, the project still averaged **~100 commits/calendar-day**.
So for "when will it be done?": model the work as minutes-to-hours, and
add slack only for possible rate-limit downtime — never inflate the
coding itself. (Methodology note: the active-rate figure excludes long
human-away idle gaps but retains the most recent multi-day rate-limit as
real downtime; earlier multi-day gaps are noisier — they predate the
one-agent-at-a-time rule and overlapped concurrent sessions — so they
were not used to derive the active rate.)

---

## When You Finish a Task

1. All code compiles with no warnings (`cargo build` clean, `cargo clippy` clean).
2. All existing tests pass (`cargo test --workspace`).
3. New tests exist for the new code and all pass.
4. If the code is in a performance-critical subsystem, benchmarks exist and meet targets.
5. Public API has doc comments.
6. Unsafe blocks have SAFETY comments.
7. The relevant roadmap task in `roadmap.md` is marked `[x]` or `[-]` (in progress).
8. If you created a new crate or module, add a `//!` module doc explaining what it does and how it fits into the architecture.
9. Commit with a clear message. One logical change per commit.
10. **`git push`** your branch. Do not leave completed work only in the local repo.

---

## When You Start a Task

1. Read this file.
2. Read the relevant section of `design.txt` and any related design files.
3. Check `roadmap.md` for the task's dependencies — don't start if prerequisites are not done.
4. Check `todo.txt` for any notes left by other sessions that affect your work.
5. Check `requests/` for any interface requests you can fulfill or that affect your zone.
6. Study existing implementations per the table above.
7. Write the benchmark first (if applicable).
8. Write tests alongside the implementation, not after.
9. Build and test before committing.
