## B-TERMINAL-THE-PTY-ACCEPTED-KEYSTROKES-AND-THREW-THEM-AWAY (lane C, 2026-08-16) — FIXED

**In short:** If you typed in the terminal faster than the program could read —
or typed at all while a program was busy — the terminal could quietly lose
whole command lines, or hand the program *half* of one and run it. Nothing
reported this. Fixed in `3bcbaecc0`.

**Where:** `apps/terminal/src/pty.rs`, `PtyMaster::write` (cooked-mode arm) and
`PtySlave::set_raw_mode`.

**The defect.** The line discipline delivered completed lines and echo bytes
best-effort and then lied about it:

```rust
CookedAction::FlushLine(line) => {
    // Best-effort write of the completed line.
    // If the channel is full we drop the data
    // (real implementation would block or buffer).
    let _ = inner.master_to_slave.write(&line);
}
```

and every byte was counted as consumed regardless, so `write` returned
`Ok(data.len())`. A caller has no way to retry what it has been told was
delivered.

**Truncation was the worse half.** `ByteChannel::write` writes
`data.len().min(available)` and returns that count — so a nearly-full channel
takes a *prefix* of the line. `let _ =` discarded the remainder, and the child
received a command line with its tail cut off. The shell then runs something
the user never typed. Dropping the line is recoverable by retyping; running
`rm -rf /home/user/proj` as `rm -rf /home/user` is not.

**The fix.** Two bounded pending queues in `PtyInner`, one per direction.
Overflowing lines and echoes are queued, not dropped; every read flushes what
now fits. When the input queue reaches the channel capacity, `PtyMaster::write`
stops consuming and reports a short count — or `BufferFull` when it could take
nothing, which is distinguishable from a successful zero-byte write that a
caller would spin on.

**Ordering had to be fixed with it.** Once a queue exists, a write that goes
straight to the channel jumps ahead of bytes produced earlier. The raw-mode
master write and the slave write now append to the queue whenever it is
non-empty. Without this the child's reply could be displayed above the
characters that prompted it.

**A third defect in the same file: the cooked-mode line buffer was unbounded.**
`cooked_process` pushed every printable byte into `line_buf` and only a line
terminator released it, so input that never contains one — a paste of binary
data, a file piped into a PTY left in cooked mode — grew it until the process
ran out of memory. Now bounded at `MAX_CANON` (4096, as POSIX); past it a
printable byte is refused and echoed as BEL, which is `IMAXBEL`. Editing and
control characters keep working so the user can back out.

**Tests:** `nothing_the_user_typed_is_lost_when_the_channel_fills`,
`a_backlogged_write_reports_a_short_count_rather_than_swallowing_input`,
`slave_output_does_not_overtake_queued_echo`,
`a_mode_switch_does_not_discard_the_partial_line`,
`the_line_buffer_stops_growing_at_max_canon`,
`an_ordinary_line_is_unaffected_by_the_bound`. Injection-verified one defect at
a time: restoring the dropping line write fails exactly the two data-loss
tests; restoring the dropping echo write fails exactly the ordering test.

**Sweep note.** Same pattern as
`B-EXPLORER-A-MOVE-DELETED-SOURCES-IT-HAD-NEVER-COPIED`: a `let _ =` on a call
whose return value *is* the outcome the user cares about. Both were found by
scanning `apps/**/*.rs` for `let _ =` / `.ok();` before the first `#[cfg(test)]`
line. It is worth saying plainly that the comment next to the `let _ =`
correctly described the bug ("we drop the data") and it still shipped — a
documented data-loss path is still a data-loss path, and `todo.txt` is where
that belongs, not a comment nobody greps.
