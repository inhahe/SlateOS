## TD-C-THE-VPN-MANAGER-REPORTED-CONNECTING-AND-KEPT-A-LOG-OF-SESSIONS-THAT-NEVER-HAPPENED -- FIXED 2026-09-15

**In short:** clicking Connect in the VPN manager turned the profile green,
said "Connected successfully", showed a local address of 10.8.0.2 and a latency
of 42 ms, and started a session clock. No tunnel was established, because
nothing in this operating system can establish one. The window also opened on a
log of past sessions that never happened.

**Date:** 2026-09-15. **Lane:** C. Found by
`scripts/find-claimed-acts.py`.

**This one has a consequence the others do not.** Every fabrication fixed in
this sweep costs the user time or trust. This one costs them privacy: **a
person who believes their traffic is inside a tunnel behaves as though it is.**
That is the whole purpose of the program, and it is the one belief it could
create falsely.

The code said so itself:

```rust
// Simulate immediate connection success for UI purposes
if let Some(conn) = self.connection_for_mut(pid) {
    conn.status = ConnectionStatus::Connected;
}
self.add_log(&name, "Connected successfully", LogLevel::Info);
```

**Someone had already been here, and cleaned the wrong half.** `advance`
carries a careful paragraph refusing to move the byte counters, because
"traffic on a tunnel it is not carrying would be a number invented to look
busy". The fabricated *readings* were removed and the *claim* was left
standing — the same partial audit that left `apps/sysinfo` on a FIXED list
with three lying controls, and the same shape as `apps/procexplorer`, where
`refresh()` was wired to the real `/proc` and `main()` went on seeding invented
processes. **A fabrication has two halves — the numbers and the sentence — and
fixing either one leaves a program that still lies.**

**There is no VPN in this tree and there is not going to be one soon.** `net/`
holds `dns` and `httpclient`. There is no tunnel device, no WireGuard, no
IPsec, nothing that could carry a packet through anything. So this is not
"unfinished"; it is a capability the system does not have, which is why the
new status is `Unavailable` rather than `Error`. **A status that invites the
user to try again is its own small lie** when trying again cannot help.

**What changed:**

* `connect()` sets `ConnectionStatus::Unavailable`, logs a warning, and returns
  `Err("No VPN client on this system: the profile was checked, but no tunnel
  was established")`. `quick_connect` and `reconnect` delegate to it, so both
  became honest without being touched.
* The invented `local_ip = "10.8.0.2"` and `latency_ms = 42` are gone — an
  address nothing assigned and a round trip nothing measured, drawn in the
  details pane beside the real fields with no way to tell them apart.
* The log starts empty. It had opened on "Connected to vpn.company.com",
  "Assigned IP 10.8.0.2", "Handshake completed with peer", "Connection timed
  out". **A log is where a user looks to find out what actually happened**,
  which makes an invented one a fabricated account of their own machine rather
  than decoration.
* The export writes through `safeio::write_str_atomically` rather than
  `fs::write`, which truncates the target before writing: an interrupted
  export would have left neither the old profiles nor the new ones. It also
  had no test at all — the one door this program has, unpinned.

**Twenty-one tests went red at once, and that is the measurement worth
keeping.** Every test of `disconnect`, the session statistics, the uptime clock
and the status sort order reached its starting state by calling `connect` and
unwrapping. **Twenty-four call sites asserted their own setup through the very
claim that was false**, so the moment `connect` stopped pretending, most of the
crate's connection suite collapsed. Nothing was wrong with those tests as tests
— the machinery they cover is correct — but none of them could have caught this
defect, because all of them depended on it.

They now reach that state through `connected_for_testing`, a `#[cfg(test)]`
method that places the state by hand. The name is the point: no production path
calls it, and a reader of any test can see in one word that the connection was
put there rather than arrived at.

**A consequence worth stating plainly:** with nothing able to connect, the
Reconnect button is conditional on a state the system cannot reach, so it
cannot appear in a shipping build at all. That is honest rather than broken,
and `reconnect_is_only_offered_once_there_is_a_connection_to_reconnect` now
pins both halves — that Connect does not make it appear, and that a
hand-placed connection does.

**Still open, and it is the same app:** `sample_profiles()` is called from
`VpnManager::new()`, so the window still opens on three invented profiles
("Work VPN" at vpn.company.com, "Personal WG", "Travel VPN") that the user
never created. Unlike the log, these are configuration rather than history, and
unlike `procexplorer`'s seed they cannot be removed in one line — `new()` is
used by all 210 tests, so the migration is a test-wide change of the same size
as the one above. The next commit is that migration: `new()` opens empty,
`with_sample_profiles()` becomes `#[cfg(test)]`, and the window says "no
profiles yet — import one" the way `apps/renamer` says "Ctrl+O to choose a
folder".

**Verified by sabotage**, five claims, each broken with an edit that still
compiles: the tunnel claimed again, the invented address restored, the invented
latency restored. All five went red. The harness also reported honestly that
**nothing pins the atomic write** — testing that needs fault injection this
crate has no way to do, and the export test says so at the test rather than
leaving the gap silent.
