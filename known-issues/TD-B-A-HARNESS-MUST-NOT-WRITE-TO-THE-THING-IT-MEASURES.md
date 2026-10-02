## TD-B-A-HARNESS-MUST-NOT-WRITE-TO-THE-THING-IT-MEASURES (lane B, 2026-09-11)

`scripts/stat-diff.sh` reported `stat -f -c %a` and `stat -f -c %f` as
differences. They are free-block counts, and **the harness was consuming the
blocks**: it captures each side's stdout and stderr to temporary files, so
writing ours' output allocated blocks on the very filesystem GNU was about to
be asked about, a few milliseconds later.

Demonstrated rather than reasoned about:

    %f before four mktemps and one write : 238861778
    %f after                             : 238861777

Fixed by putting the capture files on `/dev/shm` — tmpfs, a different
filesystem from the fixtures, confirmed by their differing `%i`. The two cases
now pass, which is the check that the diagnosis was right.

**This is the second harness in a day to have put itself into its own
measurement.** `env-diff.sh` had to stop handing the two sides different `PATH`
values, because `env` prints its environment and the harness's scaffolding was
in it. Same shape, different resource: *a harness may not appear in the answer
it is collecting.* Worth checking for in any future harness whose subject
reports on a shared resource — free space, memory, process counts, open file
descriptors.
