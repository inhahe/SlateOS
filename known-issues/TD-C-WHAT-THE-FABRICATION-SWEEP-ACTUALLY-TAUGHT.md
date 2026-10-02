## TD-C-WHAT-THE-FABRICATION-SWEEP-ACTUALLY-TAUGHT -- INDEX, 2026-09-15

**In short:** on one day, thirty-nine application and library directories in
lane C turned out to be telling the user things the program had no way to know.
The individual findings are in the twelve entries above. This one is for what
they have in common, because the per-app entries cannot carry it and the next
person will meet the pattern before they meet any particular app.

**Date:** 2026-09-15. **Lane:** C. **Status:** INDEX -- nothing here is
outstanding work; it is the reasoning, kept where it can be found.

### How to rank them

Not by how much of a program is invented. **By what the program tells the user
to believe, and by what believing it costs.** A `--help` line and a window are
the same thing. On that axis the ordering that fell out was:

1. **Acts reported but not performed, where the act is irreversible.** undelete
   reporting files recovered, partmanager reporting a format applied,
   devicemanager's Uninstall, mediaconvert's Completed. The false *success* is
   worse than the false failure, every time: someone told a thing failed keeps
   looking, and someone told it succeeded stops -- and may delete the original.
2. **Records that outlive the session.** screenrecorder filed a history entry
   naming a path and a size; podcast added file sizes to a disk total;
   remotedesktop wrote `success: true` before the attempt. These are consulted
   later, when the thing they describe is gone.
3. **Evidence in an argument with a third party.** speedtest's 450 Mbps is
   what somebody checks before deciding whether the connection they pay for is
   the one they get.
4. **Diagnostic tools.** See below -- these deserve their own rule.
5. **Everything else.**

### For a diagnostic tool, invented data is worse than for a display tool

**Because what the user is looking for is an absence or an anomaly, and
fabricated data is neither.** devicemanager and startupmanager are the same bug
by this measure: a device manager is where somebody goes *when hardware is not
working*, and an invented inventory cannot be missing anything. Every device in
it was Working. A startup manager is where somebody goes to find what runs at
login that they did not put there; every entry was a benign system component.

**A wrong forecast is a wrong fact. A wrong startup list is a wrong
conclusion.**

### Absent is not empty, and the empty state is sometimes the dangerous one

Removing a fabrication leaves a hole, and a hole is read as an answer. Three
increasingly sharp forms:

* **An empty list reads as "none found".** An empty partition list says the
  machine has no disks; an empty scan says the network is quiet.
* **For a measurement, the empty value is a specific and alarming reading.**
  0 Mbps says the line is dead. `00:00:00` of recording space says the disk is
  full. Hence `--:--:--` and "unknown, not zero".
* **For an alert channel, silence is read as an all-clear -- and that failure
  is deferred to the exact situation the feature existed for.** weather's
  fabricated "Thunderstorm Watch" was a one-day problem; teaching the user that
  the app *has* an alert channel outlives it. Same shape as podcast's
  Downloaded mark, which fails on a plane, and reminders' empty list, which
  says "nothing is due".

### A fabrication is a claim about something the program cannot observe

Bundled content is not a claim, and this is the distinction that stopped the
sweep from becoming mechanical. `apps/ebook` ships three books and **keeps
them**: a title claims nothing about a disk, the prose reads, nobody is misled.
`apps/spreadsheet` keeps its Item/Price/Qty/Total example for the same reason.

*(Amended 2026-09-27, lane E: `apps/ebook` no longer ships them. Once it could
open real books its library became the user's own list, and the books then
failed the slot test below -- "The Clockwork Garden" beside a user's own
books is a record in the slot where theirs go. They are test fixtures now,
and a first run opens on an empty library that says how to fill it.)*

Two tests separate the cases, and both came from being wrong first:

* **The path test.** A filename is a claim that a file exists. `left.rs`,
  `sample.db`, `/music/song.flac`, `/home/user/Documents` -- all retired.
* **The slot test.** A fixture becomes a fabrication when it occupies the slot
  where the user's own record goes. A notebook called "Work", a card called
  "Implement dark mode toggle", a contact called Alice Anderson, a check-in on
  a dated day. Indistinguishable from yours a week later.

### A careful fixture is harder to notice than a careless one

Three of them had the *harmful* part handled correctly and the claim itself
unexamined:

| app | the care taken |
|---|---|
| `reminders` | due dates relative to `now`, so "overdue" stayed true as the clock moved |
| `habits` | check-ins spread to a plausible 70% rather than a flat 100% |
| `contacts` | `+1-555-01xx` and `example.com` -- reserved, non-routable |

Every one of those identified a real hazard and handled it well. **The polish is
what makes the data read as something that was thought about, and therefore
meant.** Stale dates, a flat streak or a routable number would have been
questioned sooner.

### "An empty window looks broken" -- seven authors, none reaching the remedy

The same reasoning, in seven different people's words: `videoplayer`'s "so the
first window is not an empty black rectangle", `torrent`'s "a client that opens
on an empty list looks broken rather than idle", `photomanager`'s "so the first
window is not an empty grid", `filesearch`'s "until a real index exists this is
what there is to search", `kanban`'s "until a store on disk exists this is what
there is to show", `musicplayer`'s -- the sharpest -- "an empty library would
read as a broken player rather than an unimplemented one", and `reminders`'.

**All seven were right about the symptom and none reached the remedy.** An
empty window does look broken. The answer is to **say why it is empty**, not to
fill it. Seven people getting the first half and missing the second is not
seven mistakes; it is one idea that does not occur to people, which is the only
reason this section exists.

### Three smaller rules, each learned by getting it wrong

* **A fix that promises a capability the program does not have is the same
  defect, pointed one step further into the future.** `finance`'s banner first
  read "add an account and a transaction, and every figure below will be
  yours". There is no control that adds an account. The compiler found it.
* **A distinction can be drawn correctly at the data layer and collapsed in a
  format string.** `diskimager`'s `PartitionTable` separated `None` from
  `Unknown` with a doc comment explaining why it mattered; the drive panel
  printed `partitions.len()` regardless, so an unread table read as "0
  partitions" -- a blank drive, the one you overwrite. Lane A found it.
* **An outcome written at the start of an attempt is not a record of what
  happened.** `remotedesktop` wrote `success: true` the moment Connect was
  pressed. That would be a defect with a working network.

### On the tooling, and on trusting it

`scripts/find-reachable-fixtures.py` asks who *calls* a builder rather than
what it is named, which is the question that separates a fixture from a
fabrication. It found `netscan`'s three remaining inventions hours after that
same file was declared fixed.

**And it nearly lost its best find to me.** I was assembling its EXEMPT list
from the report by name -- `sample_rate`, `bits_per_sample`, `sample_pixel` --
all obviously fine. `apps/magnifier` computed screen colours as
`x*7 + y*13 % 256` and magnified the result **for someone who had opened a
magnifier because they cannot check the screen by looking.** It was the most
consequential finding of the day and it sat in the report looking exactly like
the false positives beside it. The caution is now in the file, beside the list.

Two render-invariant tests caught banners I added -- `magnifier`'s containment
pass and `contacts`' bounds check. Both read *commands* rather than pixels,
which is precisely why a clip could not hide an overrun from them.

### One measurement, which came out cleaner than expected

**Fixture placement predicts the blast radius exactly.** Of the applications
whose fixture was wired into the *constructor*, every one broke between 6 and
66 tests when it was removed. Of those that called it from `main()` --
`email`, `videoplayer`, `musicplayer`, `filediff`, `reminders`, `notes`,
`kanban`, `spreadsheet` -- **every one broke zero.**

The fix is otherwise identical. A fixture in the constructor is handed to every
test whether it wants it or not; a fixture in `main` has to be asked for. **The
cost of removing a fabrication is almost entirely in where the call sat, not in
what it produced.**
