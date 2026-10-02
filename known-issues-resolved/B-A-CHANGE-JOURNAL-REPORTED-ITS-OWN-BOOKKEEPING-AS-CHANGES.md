## B-A-CHANGE-JOURNAL-REPORTED-ITS-OWN-BOOKKEEPING-AS-CHANGES (lane A, 2026-08-22) — FIXED 2026-08-22

**In short:** The filesystem change journal — the thing a backup agent, indexer
or sync daemon asks "what changed?" — reported its own record-keeping files as
changes. Because those files are written *by* the act of answering the
question, every answer created the next one. `changetrack::changes()` could
never return empty, so an agent polling until the filesystem went quiet would
poll forever, doing real disk writes on every round.

**Found by** wiring `fs::changetrack::self_test` into the boot path for the
first time (`B-A-FORTY-ONE-SELF-TESTS-HAD-NEVER-RUN`). It panicked on
`changetrack.rs:614`, "expected 0 changes after advance" — an assertion that had
been correct and unrun since the module was written.

**Mechanism.** `changetrack::query_impl` advances the caller's cursor to
`current_seq` and then persists the cursor table with
`Vfs::write_file("/_CHANGE_CURSORS", …)`. Every `Vfs` write calls
`journal::record` (`vfs.rs:2014` and eight siblings). So the persist is
journalled at a sequence *greater* than the one just handed to the caller, and
the next `changes()` call returns it — then persists again, arming the call
after that. A fixed point does not exist.

`/_JOURNAL` had the same shape one level down: the journal's own auto-flush
writes it via `Vfs::write_file`, re-entering `record` and dirtying the journal
it had just flushed. That one does not recurse — `unflushed` is zeroed before
the write — but it does mean the journal was never actually flushed-clean.

**Fix.** `journal::record_with_old_path` now drops events for the three private
bookkeeping paths (`/_JOURNAL`, `/_CHANGE_CURSORS`, `/_TRASH/_INDEX`) before
taking the lock. Fixing it at the recorder rather than in `changetrack` matters:
the loop was visible to *every* registered cursor, not only the one whose
`changes()` call did the write, so filtering at the query would have left every
other consumer looping.

Matched exactly, not by an `_` prefix: a prefix rule would silently swallow a
real user file called `/_notes.txt`, and losing a genuine change is the more
expensive direction of error. The journal self-test now asserts both halves —
the three internal paths produce no entries and consume no sequence number,
while `/_JOURNAL.bak` is still reported as the ordinary user file it is.

**Severity.** Real, and worse the longer a system runs: `/_TRASH/_INDEX` aside,
both loops burn disk writes proportional to how often anyone asks what changed.
Nothing consumed change-tracking at boot, which is why it was never observed —
the subsystem's only user so far was a self-test that had never been called.
