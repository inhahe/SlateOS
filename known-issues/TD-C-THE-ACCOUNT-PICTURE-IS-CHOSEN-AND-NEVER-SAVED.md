## TD-C-THE-ACCOUNT-PICTURE-IS-CHOSEN-AND-NEVER-SAVED

**In short:** you can pick a picture for your account in Settings, the list
updates, and nothing writes it down. Close the window and the choice is gone.

**Date:** 2026-09-15. **Lane:** C.

`handle_click` sets `account.picture = index` on the in-memory `Vec` and there
is no writer anywhere -- the value reaches no file and no other program. The
account database does carry a real per-account avatar
(`loginusers::Account::avatar`, an identifier string), so the place for the
choice exists; what is missing is a write path to the user database and a
mapping between that identifier and this page's fixed grid of pictures.

**Deliberately not fixed in the same change that wired the accounts up.** The
picker's own logic is correct and carefully tested -- clicking a tile selects
that tile, and the edit lands on the signed-in account rather than the
highlighted one, each pinned by its own test. Deleting a working control and
wiring a data source are separate decisions, and doing both at once would have
meant deciding the second one in the middle of the first. It is recorded here
so the choice is made deliberately rather than by momentum.
