## TD-B-WPA-STATE-MACHINE-IS-AN-ISLAND (lane B, 2026-09-10) — open, offered to lane C

**In short:** `userspace/wpa`'s WPA state machine — `SupplicantState`,
`WpaState`, the BSS table and the association transitions, about 500 lines with
21 tests — is now defined and never called. Deleting the two personalities that
fabricated results left it with no production consumer. It is correct code with
nowhere to run.

**Why it was not deleted with them.** The personalities went under
`design-decisions.md` 1006: `wpa_supplicant` printed "initialized successfully"
having opened no socket, and `wpa_cli` answered `status`, `scan` and
`scan_results` from a fresh in-process state object, so it reported no networks
in range having never looked. Both stated facts nothing measured. The state
machine states nothing — it is the one part of that crate that was honest, and
1006 is about commands that lie, not about code with no caller.

**Where it belongs.** `net80211` has a `Transceiver` trait, an association
state machine in `assoc.rs`, and a supplicant in `supplicant.rs` — lane C's
tree, and the place a real one has to live, because it is where the radio is.
This is a second implementation of the same thing on the wrong side of the lane
boundary. Offered to lane C in
`requests/b-c-wpa-state-machine-and-its-tests-are-yours-if-you-want-them.md`.

**If lane C declines**, delete it. An island with a rejected offer is debt with
a decision behind it, which is the difference between this entry and a pile of
unreachable code nobody has looked at.

**Not caught by `scripts/scan-orphan-modules.py`**, which scans lane C's roots
only — the ledger is about `apps/`, `gui/` and `net*/`. Worth knowing that lane
B has no equivalent gate, so this was found by reading rather than by a check,
and a second one would not be reported.
