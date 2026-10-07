## D-POSIX-SA-ONSTACK-HANDLER-MOVED-TO-THE-TOP-TWICE — a handler the kernel started on the alternate stack was moved to its top again, over the kernel's saved context and libc's own dispatch (lane D, 2026-09-30) — **Status: FIXED 2026-09-30 (`posix/src/signal.rs`)**

**In short:** a program that registers an alternate signal stack
(`sigaltstack`) and installs a handler with `SA_ONSTACK` had that handler
crash, or return into garbage, whenever the signal came through the kernel
-- another process's `kill`, a timer, `^C` at the terminal -- and the
handler used more than a few words of stack. The kernel builds such a
signal's frame on the alternate stack, as it should since lane A's
87ef09d0b; libc, not noticing it was there already, moved the handler to the
top of that same stack a second time, where its frames overwrote the
kernel's saved registers, the trampoline's saved pointer to them and the
dispatch's return address.

**Why:** `altstack_entry` decided "already on it" by a flag that only
libc's own switch sets, and nothing of libc's runs before a handler the
kernel starts there. The kernel's `altstack_top_for` asks the stack pointer,
and its comment says the two must agree. `ctest-altstack` could not see it:
it sent every signal with `raise`, which dispatches in-process and never
meets the kernel's frame -- written when the kernel still built every frame
on the interrupted stack, as its header said.

**Fixed 2026-09-30.** "On the alternate stack" is the flag or the stack
pointer being inside the region, as the kernel asks it
(`on_the_alt_stack`): `altstack_entry` no longer moves a handler that is
already there, and `sigaltstack` reports `SS_ONSTACK`, and refuses a change
with `EPERM`, while one the kernel started there runs. `ctest-altstack`
gained checks 43-49, which send their signals with `kill(0, sig)` in a
process group of their own -- the kernel delivers them as the call returns
-- to a handler using 8 KiB of stack.

**Where:** `posix/src/signal.rs` (`altstack_entry`, `on_the_alt_stack`,
`sigaltstack`); `services/ctest-altstack/main.c`.
