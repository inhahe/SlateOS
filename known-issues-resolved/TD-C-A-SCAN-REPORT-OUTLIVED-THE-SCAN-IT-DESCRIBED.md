## `TD-C-A-SCAN-REPORT-OUTLIVED-THE-SCAN-IT-DESCRIBED` -- **FIXED 2026-09-18** (lane C)

**In short:** `apps/netscan` had its invented hosts removed on 2026-09-15. The
*report wrapped around* those hosts stayed, and it was still a claim: pressing
Scan printed "Scanned 254 IPs | 0 hosts up | 0 open ports | 12.3s", put "0
hosts up on 192.168.1.0/24" in the window title, and filed a history entry
timestamped "2026-05-18 12:07:13". Nothing was contacted. **"0 hosts up" is a
finding about the user's network**, and it reached the taskbar.

**Verified before it was changed.**

| | |
|---|---|
| can it reach anything | no -- `Cargo.toml` lists `appearance`, `guitk`, `oswindow`, and nothing else |
| the hosts | `let hosts: Vec<HostResult> = Vec::new();`, correctly, since 2026-09-15 |
| the summary | `render_summary_bar` prints `total_ips_scanned` and `duration_secs`, an address count and an *estimate* |
| the title | `title()` returned `"{} hosts up on {} - Network Scanner"` |
| the history | `self.history.push_front(result)` with `timestamp` a constant derived from the scan id |

**The tell was internal inconsistency.** The same author, in the same file, had
already made `run_traceroute`, `run_whois` and `send_wol` refuse outright and
say why -- *"Cannot trace a route: this program has no network access, so no
packet was sent"*. The scan is the one of the four that was missed, and it is
the one with a durable history behind it.

**Fixed.** `start_scan` parses the target first, so a typo is still named as a
typo rather than being swallowed by the refusal -- the order `run_traceroute`
uses -- and then sets `scan_note` and stops. No result, no history entry, and
the title stays "Network Scanner". The note is drawn where the summary was.

**A removal is finished when the claim is gone, not when the data is.** The
2026-09-15 change deleted the fabricated *hosts* and left every sentence built
around them. That is the same shape as `TD-C-WEATHER-CAN-ONLY-EVER-BE-EMPTY`
predicted and got wrong -- there the removal really had been finished -- so the
shape is real even though that instance was not.

**The sweep this prompted, and its one other hit.** Searching production code
for hardcoded date literals -- the sharpest signal netscan gave -- found 18
across 11 apps once test code was excluded properly with `rustlex.live_code`.
**A naive cut at the last `#[cfg(test)]` reported 50 across 12**, nearly three
times as many, and put 23 of them in `devicemanager` alone, which actually has
one. That is the `production_part` bug in miniature, and a reminder to use the
lexer rather than a split.

Of the 18, thirteen are honest: doc comments in `sysinfo`, `pomodoro`,
`settings` and `devicemanager` recording what a fabricated date *used* to be,
and explanatory strings in `calendar`, `finance`, `habits` and `reminders`
telling the user what the app opened with before the fabrication was removed.
`rssreader`'s five are inside `SAMPLE_RSS`, a fixture deliberately fed through
the real parser.

**One was live: `apps/podcast`.** `listened_at` was the literal
`"2026-05-18 10:00"` at *both* of its assignment sites --
`complete_current_episode` and `record_current_to_history`, each reachable --
and the history panel drew it, so a listening history filled with rows that all
happened at the same minute of the same day. Fixed by reading the clock:
`system_timestamp()` on the pattern `apps/reminders` uses at `system_now`,
returning `None` rather than a fallback date, because a row saying "time
unknown" is awkward and true and one saying 1 January 1970 is neither. 201
tests.

**A fourth scanner was attempted for this class and abandoned, which is worth
one paragraph.** The four existing finders miss it by construction:
`find-silent-incapacity.py` asks whether a program admits it cannot reach
anything, and netscan *does* -- for three of its four operations. The gap is
**internal inconsistency**, so the query was "apps that admit an incapacity and
still claim a result". It found 38, then 34 after blanking comments with
`rustlex.strip_noise(keep_literals=True)` -- whose own doc says an earlier
query had counted doc comments quoting the lines they replaced, which is
exactly the mistake I made before reading it. What is left is almost entirely
**enum labels**: `PeerStatus::Connected`, `EpisodeStatus::Downloaded`,
`"Completed"`. A label naming a state is not a claim that the state was
reached, and telling the two apart needs the code. **netscan was found by
reading it, not by a query**, and no scanner is proposed here.

**Four tests changed rather than deleted**, and the changes are the record:
`test_app_start_scan` asserted `results.is_some()` and `!history.is_empty()`,
both true and both the defect;
`a_scan_reports_no_hosts_because_it_cannot_reach_the_network` checked the hosts
list and not the sentence around it; `the_title_reports_what_the_scan_found`
asserted the title *did* report a finding. A fifth,
`the_scan_refusal_reaches_the_window`, asserts through the render, because
`wol_note` in this same file was written and drawn by nothing for three
commits and only the write-only-field gate noticed. 135 tests.
