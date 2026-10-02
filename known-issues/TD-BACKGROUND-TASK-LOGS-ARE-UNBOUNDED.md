## TD-BACKGROUND-TASK-LOGS-ARE-UNBOUNDED (lane B, 2026-08-17) — **mitigated once; the cause is unfixed**

**In short:** commands we start in the background write everything they print
to a log file on the C: drive, and nothing ever truncates or deletes those
files. One of them reached **40 GB** and filled the system disk, at which point
unrelated things started failing with `could not write to temporary response
file`. Deleting dead logs got the disk from 11 GB free back to 52 GB, but
nothing stops it happening again.

**Where it lives.** `%LOCALAPPDATA%\Temp\claude\<project>\<session>\tasks\*.output`
— one file per backgrounded Bash call, written by the agent harness, never
rotated. On 2026-08-17 the OS project's directory held 2491 of them totalling
43 GB across four sessions, of which a single file (`bf8dfbpnv.output`, 6
Aug, another lane's session) was 40 GB on its own: a command whose output was
neither bounded nor filtered, left running.

**Reproduce.** Background anything that prints without limit — a `cargo build`
with a stuck dependency retrying, a QEMU run with `-d int`, a test loop that
prints per iteration — and leave it. The file grows at the process's write
rate with no ceiling.

**The proper fix, in order of preference:**

1. **Bound the output at the source.** A backgrounded command should end in a
   filter that cannot grow without limit — `| tail -c 10M`, `| grep -E …`, or
   for a known-chatty run `2>&1 | head -5000`. This is the one an agent can
   apply today and costs nothing.
2. **Cap the log.** If the harness ever grows a per-task output limit, use it.
   Not ours to change from here.
3. **Sweep on a schedule.** `find … -name '*.output' -mtime +7 -delete` is what
   was run by hand this time. A weekly sweep would have kept the disk from ever
   reaching 100%, but it treats the symptom.

**Note for whoever reads this next:** the 40 GB file was *another lane's*, and
deleting an eleven-day-old write-once log is safe, but do not delete a `.output`
file for a task that may still be running — read the task list first. Files
under `-mtime +7` are unambiguously dead.
