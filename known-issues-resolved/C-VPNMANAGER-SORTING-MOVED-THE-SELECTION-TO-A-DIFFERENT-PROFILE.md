## C-VPNMANAGER-SORTING-MOVED-THE-SELECTION-TO-A-DIFFERENT-PROFILE (lane C, 2026-08-26) — FIXED 2026-08-26

**In short:** in the VPN Manager window, changing the sort order silently moved
the highlight onto a *different* VPN profile than the one the user had picked —
whichever profile happened to land on the row number the old one had been on.
Everything the window then did (Connect, Disconnect, Remove, and every on/off
switch on the detail panel) acted on that other profile. Removing the wrong VPN
profile is not a cosmetic bug.

**Where it lived:** `apps/vpnmanager/src/main.rs`, `VpnManager::sort_profiles`.
`selected_profile` is an `Option<usize>` — an *index* into `self.profiles` — and
`sort_profiles` reorders that vector in place. Nothing re-pointed the index
afterwards.

**Why it went unnoticed:** the sort order could not be changed. The toolbar drew
`Sort: Name` as a label and nothing clicked it, so `set_sort_order` had no
caller outside the tests, and the tests all asserted on `profiles` rather than
on `selected_profile`. The bug existed for as long as the control that triggers
it was a picture.

**The fix:** `sort_profiles` now records the selected profile's **id** before
sorting and looks the index back up afterwards. The list is small and the sort
is not on any hot path, so a linear search costs nothing worth measuring.

**Regression test:** `sorting_keeps_the_selection_on_the_profile_the_user_chose`
— it clicks a row, clicks the Sort control twice, and asserts both that the
row number *did* change (or the test proves nothing) and that the selected id
did not. Confirmed to fail when the id re-lookup is mutated out.

**The general shape, for the other 122 unwired programs:** a selection stored as
an index into a list the program itself re-orders is a bug waiting for the
control that re-orders it to be wired up. Store the id.
