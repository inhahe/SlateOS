## TD-C-SIXTY-THREE-FIXTURE-FUNCTIONS-ARE-CALLED-FROM-PRODUCTION

**In short:** about thirty applications fill their windows from functions named
`sample_*`, `seed_sample_*`, `mock_*` or `simulate_*` — and those functions are
called by the real program, not by tests. The weather app invents weather, the
network scanner invents hosts, the partition manager invents disks, the
undelete tool invents recoverable files, and the benchmark reports scores that
are constants in the source. The six fabrications fixed on 2026-09-15 were not
six; they were the ones that happened to be found first.

**Date:** 2026-09-15. **Lane:** C.

**How they were found.** Grep for a production `fn` whose name is fixture
vocabulary (`sample`, `seed`, `demo`, `mock`, `fake`, `stub`, `simulate`,
`example`), excluding anything inside `#[cfg(test)]`, then keep only those that
are **called from a non-test line**. That last filter is what makes the list
worth reading: a `pub fn sample()` that only tests call is a fixture in the
wrong module, which is untidy. One the program calls is a fabrication.

77 definitions matched the name; 63 are called from production. The count
excludes signal-processing vocabulary

**63 is a floor, and the method is why.** Lane B's objection, and it is right:
`simulate_integer_benchmark` was caught because somebody named it honestly. The
same function called `measure_integer_throughput` is invisible to a name grep,
and there is no reason to think the careless cases are the ones that got the
candid names — if anything the opposite. Do not let 63 settle in anyone's head
as the size of the class; it is the size of what one keyhole showed.

**The probe that does not depend on the name, which lane B proposed and which
costs one test per suspect: vary the input, assert the output varies.** A
benchmark that returns the same score on an idle machine and on a loaded one is
refuted in a single measurement, whatever the function is called. A netscan
reporting the same open ports against two different hosts, an undelete listing
the same recoverable files on two different volumes — same probe. It tests the
property that actually matters, that the output is a function of the world,
rather than the property the name suggests. `apps/benchmark`'s
`a_longer_piece_of_work_is_measured_as_longer` is that probe in its
deterministic form. — `sample_rate`, `bits_per_sample`,
`sample_count`, `record_sample`, and `sample` in `gui/compositor/src/blur.rs`
and `gui/imagecodec/src/png.rs`, which are all the other meaning of the word.

**PAID OFF 2026-09-15: `apps/benchmark`, all sixteen.** Four CPU and four
memory tests are measured; one disk test is measured and four report why they
are not; three graphics tests are measured and renamed. Details below, and the
commits carry the numbers. That is 16 of the 63.

**The one verified in detail, because it is the worst and it is instructive.**
`apps/benchmark` has sixteen `simulate_*` functions and `run_cpu_benchmark`
calls them. `simulate_integer_benchmark` performs 500,000 real iterations of
integer arithmetic — and then discards the result and returns `5200.0`, plus or
minus twelve depending on the parity of the accumulator. Its own comment
explains the perturbation: *"to prevent const-folding"*. So the machinery that
makes the number look computed is deliberate, the CPU time is really spent, and
the score has nothing to do with the machine it ran on.

The file says so at the top — *"for initial development we compute
deterministic scores"* — and **nothing the user sees says so**. That is exactly
the shape lane B found in `patch -l` the same day: the field's doc comment says
"accepted and currently inert" and `--help` says "Match ignoring whitespace."
A rationale written where only the maintainer reads it does not reach the
person being told a number.

A benchmark is the worst possible host for this defect, because its entire
output is a claim about *this* machine, and the number is designed to be
compared with other people's.

**The list, by what the fabrication is about.** Unverified beyond the name-and-
call-site test — treat each as a claim to check, not a finding:

| what it invents | where |
|---|---|
| measurements | `benchmark` (16), `speedtest` (`simulate_probe`) |
| hardware | `devicemanager`, `partmanager` (`sample_disks`), `soundrecorder` (`mock_devices`), `netmanager` (5) |
| the network | `netscan` (`simulate_host_scan`, `simulated_hostname`, `simulated_banner`, `simulate_traceroute`, `simulate_whois`), `vpnmanager` (3), `ircclient` |
| the user's data | `email` (`seed_sample_mail`), `notes`, `calendar`, `reminders`, `spreadsheet`, `slides`, `ebook`, `videoplayer` (4), `torrent`, `podcast`, `renamer`, `mediaconvert`, `rssreader`, `screenrecorder` |
| recoverability | `undelete` (`simulated_partitions`, `simulated_recycle_bin`) |
| system state | `systemrestore` (`simulate_create`, `simulate_restore`), `archivemanager` |

`gui/desktop/src/language_settings.rs`'s three `example` functions are almost
certainly a false positive — a language's *example text* is example text.

**Which to do first, and why not alphabetically.** By what believing the
fabrication costs:

1. **`benchmark` and `speedtest`** — a number you would act on, and compare.
   The CPU and memory ones are *genuinely measurable today*: the work is
   already performed, and only the clock is missing. `std::time::Instant` is
   available. This is a fix, not a deletion.
2. **`undelete` and `partmanager`** — these describe what is recoverable and
   what is on a disk, immediately before a user does something irreversible.
3. **`netscan`** — an invented open port is a security conclusion.
4. The rest, where the cost is a user's time and trust rather than an action.

**What "fixed" means.** Three outcomes, in order of preference, and the choice
is per case rather than per app: **measure it** where the data is obtainable
(`benchmark`'s CPU scores, and the photo manager's EXIF once it could open a
file); **wire it** where a real source exists in the tree (the Accounts page
onto `gui/loginusers`); **say so** where neither is possible (Sound, Updates,
Privacy, Network). What is never right is leaving it, and what is never enough
is a comment — three of the six fixed today had one.
