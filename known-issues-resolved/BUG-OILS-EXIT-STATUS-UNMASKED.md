### BUG-OILS-EXIT-STATUS-UNMASKED. `exit`/`return` did not truncate their argument to 8 bits — 2026-07-27 — ✅ RESOLVED 2026-07-27

**Symptom.** A status that travelled purely in-process kept its full width:
`( exit 256 ); echo $?` printed 256 where bash prints 0, and `f() { return
260; }; f; echo $?` printed 260 where bash prints 4. osh only ever got the mask
for free from the OS at the real process boundary (`osh -c 'exit 256'` did
report 0), so the bug was invisible at top level and appeared only inside a
subshell or a function. Since `$?` is compared against small integers
everywhere, an unmasked value silently breaks any such test — a `return 260`
that a caller checks for `-eq 4` just never matches.

**Root cause.** `userspace/oils/src/interp.rs`, the `"exit"` and `"return"`
builtin arms parsed the argument with `parse::<i32>()` and used the value
directly. Two defects: no `& 0xFF`, and a parse too narrow to accept the wide
integers bash takes (`exit 99999999999` is 255 to bash, not an error).

**Fix.** A shared `parse_exit_status` helper parses as `i64` and masks to the
low 8 bits, used by both builtins. Verified against bash 5 across the sign and
overflow cases: 256→0, 300→44, −1→255, −300→212, 99999999999→255.

**Found by** the `jobs-wait.sh` differential corpus case; covered by the
`exit_and_return_status_masked_to_8_bits` unit test.

**Related, fixed in the same commit:** `wait PID` on an already-reaped job
reported 127 "not a child of this shell". bash remembers a terminated job's
status, so a targeted `wait` is repeatable. Fixed with a `reaped_status`
pid→status map kept *separate* from the job table (a reaped job leaves the
`jobs` listing immediately while `wait` still answers for it, so one table
cannot model both), populated by `wait PID` and `wait -n` alike and cleared by
an argument-less `wait` — bash's purge point.
