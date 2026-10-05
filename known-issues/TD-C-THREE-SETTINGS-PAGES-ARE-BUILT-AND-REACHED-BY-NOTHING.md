## TD-C-THREE-SETTINGS-PAGES-ARE-BUILT-AND-REACHED-BY-NOTHING — TWO OF THREE RESOLVED 2026-09-09

**Status, 2026-09-09.** Two of the three are gone. `snapshots.rs` (2,256
lines) was deleted down to a 313-line `/proc/snapshots` reader, and
`associations.rs` (1,753) was deleted outright after its fallback-handler
logic was ported into `apps/fileassoc` — where it fixed a real bug, two
records that could disagree about the default handler.

**`remote.rs` (1,619 lines) remains, and is deliberately kept.** Both halves
of it hold design that is recorded nowhere else, so deleting it would lose
knowledge rather than remove duplication:

- **The remote-desktop half** — `RemoteDesktopConfig`'s `require_authentication`,
  `allowed_users`, port and encryption level — is the only statement in the
  tree of what the compositor's connection gate should ask. The compositor
  really does listen on a TCP socket and really does accept anyone; see the
  2026-09-09 scope note on `TD-C-ANY-CLIENT-CAN-READ-EVERY-WINDOW-TITLE`.
- **The dynamic-DNS half** was superseded only in part. The kernel owns the
  entry list and the update mechanism, and the Settings page now reads it from
  `/proc/dyndns` — but the kernel models credentials as one generic
  `update_url` per entry, whereas `ProviderSettings` records *which* credentials
  each provider actually needs (NoIP: email + password; DuckDNS: domain + token;
  Dynu: hostname + username + password; FreeDNS: domain + auth token). That
  mapping is real, non-obvious knowledge with no other home until an add-entry
  flow exists. See `TD-C-DYNDNS-PAGE-IS-READ-ONLY`.

**Trigger to delete each half:** the remote-desktop half, when the capability
gate lands and the real config lives wherever the compositor reads it; the
dynamic-DNS half, when the provider credential shapes are ported to whatever
builds an entry — the same port-then-delete that `associations.rs` got, and the
reason that one could be deleted and this one cannot yet.

**Date:** 2026-09-08. **Lane:** C.
**Where:** `apps/settings/src/snapshots.rs` (2,256 lines),
`remote.rs` (1,619), `associations.rs` (1,753). 5,628 lines.

**In short:** the Settings app has three whole pages written — system
snapshots, remote desktop, and which program opens which file type — that the
user cannot reach. They are complete, they compile, and nothing anywhere opens
them. A fourth thing makes it worse than a plain gap: the snapshots page exists
*twice*, once here and once in `main.rs`, and it is the copy in `main.rs` that
the user actually sees.

**The evidence.** `main.rs` declares all three with `mod`, and then refers to
them **zero** times: `grep -c 'snapshots::' main.rs` → 0, and the same for the
other two. Their entry points — `render_snapshots_page`,
`render_remote_page`, `render_associations_page` — have no callers at all.
There is no `SettingsPage::Remote` and no `SettingsPage::Associations` variant,
so those two pages have nowhere in the navigation to be opened *from*.

**Why nothing warned.** Each file opens with a module-level
`#![allow(dead_code)]` — `snapshots.rs:15`, `remote.rs:7`,
`associations.rs:7`. That is the exact mechanism
`TD-C-ALLOW-DEAD-CODE-IS-HIDING-WHOLE-UNWIRED-MODULES` describes, and this is
the largest instance found so far: without it, `cargo build` would have said
these modules were unreachable every time anyone built the Settings app.
The attribute was presumably added for a handful of genuinely-unused helpers
and now covers 5,628 lines.

**The duplicate is the part to decide first.** `SettingsPage::Snapshots`
exists and is drawn by `SettingsState::build_snapshots_page` in `main.rs` —
a *different* implementation from `snapshots::render_snapshots_page`. So there
are two snapshot pages, and any change made to the one the user cannot see is
work thrown away.

**Investigated 2026-09-08, and the answer is "neither, as they stand".** The
first guess — keep the reachable one, delete the other — is wrong in both
directions:

* The **reachable** page renders a hardcoded array:
  `("Gen 42", "2026-05-17 09:00", "Current")`, `("Gen 41", …)`. It is a
  mockup. 238 lines.
* The **unreachable** one is backed by a genuine model — `SnapshotId`,
  `BlockHash::compute`, `SnapshotIncludes`, `SnapshotManager`, a
  copy-on-write block store, snapshot trees. 2,256 lines. But
  `SnapshotManager::new()` builds it **in memory** and nothing in the file
  reads a byte from disk, so it too displays only what the process itself
  put there.

**And the real thing exists, done, in the kernel.** `roadmap.md` line 2434:
`fs::snapshot` — "CAS-backed point-in-time directory tree snapshots with
create/restore/delete/diff/list; branching (parent→child tree), selective
include/exclude filters, metadata preservation" — marked `[x]`, with a
`fssnapshot` kshell command and **`/proc/snapshots`**. So `snapshots.rs` is a
2,256-line userspace reimplementation of a kernel subsystem that was already
finished, and the page the user sees is a picture of neither.

**The format is settled, so the fix is not open-ended.** `gen_snapshots()` in
`kernel/src/fs/procfs.rs` emits a fixed-column table:

```
Filesystem snapshots: <n>

  ID  NAME                  PATH                               FILES         BYTES  PARENT
```

with `PARENT` as an id or `-`, and the path **octal-escaped**
(`mangle_mount_field`) precisely so a root path containing a newline cannot
forge a row.

That escaping is what makes splitting on newlines *safe* — one line is one
snapshot, guaranteed, because no raw newline can reach the output. (An earlier
draft of this paragraph said the reverse: that a reader "must not assume one
line is one snapshot". That was backwards, and worth correcting in place rather
than quietly, because a reader who believed it would write a more complicated
parser to defend against something the kernel already prevents. What the reader
*does* owe is the un-escaping, on the path field, after splitting.)

**So the work is:** read `/proc/snapshots`, render *that*, delete the mockup,
and delete or drastically reduce `snapshots.rs` to whatever the page still
needs that the kernel does not provide. Do not "choose between the two pages" —
that framing, which the first version of this entry used, assumes one of them
shows real snapshots and neither does.

**First two thirds done 2026-09-08.** `snapshots::system_snapshots()` reads
`/proc/snapshots`, and `build_snapshots_page` renders it; the four invented
rows are gone, and a machine with no snapshots now says so. `SnapshotRow::path`
is `Vec<u8>` — the escaping exists to carry bytes a text table otherwise
could not, and a `String` there would undo that with a lossy conversion in the
one field where a wrong answer names a different directory. The page's label
converts lossily and says why, at the one point where a person has to read it.

The parser anchors from **both ends** rather than by column, and that is not
defensiveness for its own sake: it is what makes it correct against the two
malformed shapes reported in
`requests/c-a-proc-snapshots-escapes-the-path-but-not-the-name.md` — a name
with a space in it, and a name longer than its column. Both are covered by
tests here, so if lane A escapes the name the tests keep passing and the parser
does not need to change.

**And the model is gone, same day.** `snapshots.rs` is **2,256 → 313 lines**:
the reader, `format_size`, and their tests. Removing the
`#![allow(dead_code)]` *first* is what made the deletion safe — the compiler
named all **41** unreachable items, so the cut was made from its list rather
than by eye.

**30 passing tests were deleted with it**, and that is the right outcome rather
than a cost to regret: they tested `SnapshotManager`, `BlockHash`,
`SnapshotIncludes` and the rest — a userspace reimplementation of a kernel
subsystem that should not exist. A test suite over code that should be deleted
is an argument for keeping it, which is exactly the trap. The eight tests that
matter — the ones over the format the kernel actually emits — all remain.

**The other two, checked the same day, and neither is a delete either.** Both
turn out to be the colorpicker shape again — the unreachable copy holds
something the reachable one does not — so "it duplicates a working app,
therefore remove it" is wrong for both.

| | duplicates | but holds, uniquely |
|---|---|---|
| ~~`associations.rs`~~ **deleted 2026-09-08** | `apps/fileassoc` | ~~**fallback handlers**~~ — ported first, as `FileType::handler_history`. The rest was checked item by item before deleting: `fileassoc` has `search`/`search_in_category` for the filtering, and config-line serialisation this file never had. The module doc's third claim, "per-extension icons", was a field set once at construction and **never read** — a promise the code did not keep, so nothing to preserve. |
| `remote.rs` (1,619) | `apps/remotedesktop` — 5,199 lines, likewise a real app | **DynDNS** — but *not* uniquely, as an earlier version of this row claimed. `kernel/src/fs/dyndns.rs` implements it (584 lines: `add_entry`, `list_entries`, `set_enabled`, `set_interval`, `set_update_url`, `update_now`, plus UPnP/NAT-PMP forwarding) and publishes `/proc/dyndns`. See below. |

So the shape of the work is the same for both, and it is not deletion:

1. Move the unique part to the application that owns the domain — fallback
   handlers into `apps/fileassoc`, remote-desktop settings into
   `apps/remotedesktop`.
2. ~~Decide where DynDNS belongs.~~ **Already decided, in the roadmap.**
   `roadmap-detailed.md` line 2531: "DynDNS setup helper **in settings**
   (prefer free services, especially dynu.net)", and `design.txt` line 1301 asks
   for it. So `remote.rs`'s DynDNS half is a planned feature sitting in the
   right application — it has simply never been given a page. Nearly written up
   as an open question before checking; the roadmap had answered it.

   **And it is the snapshots case a second time, which I found only by
   re-auditing my own greps.** `kernel/src/fs/dyndns.rs` implements dynamic
   DNS — 584 lines, with UPnP/NAT-PMP port forwarding beside it — and its own
   module doc states the architecture it expects:

   ```text
   Settings panel → Network → Dynamic DNS
     → dyndns::list_providers() → configured providers
   ```

   That settings panel is `remote.rs`, unreachable, carrying a parallel
   in-memory model. `/proc/dyndns` publishes the stats, the detected router,
   and a table of entries (ID, NAME, PROVIDER, HOSTNAME, STATUS, IP).

   **Two earlier claims in this entry were wrong and are corrected above:**
   that `remote.rs` held the only dynamic-DNS configuration in the tree (the
   grep behind it excluded `kernel/`), and that wiring it needs an HTTP
   transport first (`dyndns::update_now` is the kernel's job, not the panel's).

   **So the work is the same shape as the snapshots page:** read
   `/proc/dyndns`, render that, give it a `SettingsPage`, and delete the
   in-memory model. It is *not* blocked, which is the opposite of what this
   entry said an hour ago.
3. *Then* delete the husk.

**Porting the fallback design found a live bug in the app it moved to.**
`AssociationRegistry::remove_app` deleted every association naming the removed
application and stopped there, which is wrong twice over. It orphaned the file
types — uninstalling an editor left every `.rs`, `.toml` and `.log` with no
handler at all, though the user had a perfectly good previous one. And it
updated only **one of the two records** of "what opens this": `associations`
lost the entry while `file_types[ext].default_app_id` went on naming the
removed application, so the registry disagreed with itself and which answer a
caller got depended on which map it asked.

No test caught the second because each map was only ever checked on its own.
Three of the five new tests fail against the old `remove_app`.

**The recurring lesson, now four for four.** Every module in this entry that
looked like a straightforward deletion turned out to hold something the
reachable code lacked: the toolkit's HSV was the *correct* implementation, the
snapshots model was redundant only because a *kernel* subsystem covers it, and
these two each carry a feature that exists nowhere else. Unreachable is not the
same as worthless, and the check that keeps finding this is cheap: diff the
feature lists before deleting, not after.

**Found by** the palette conversion. Every audit until then read only
`main.rs`, so these three files were never looked at; they were found by
grepping every `.rs` under `apps/*/src` for the twenty Catppuccin hex values,
and each turned out to carry its own copy of the palette under the comment
*"Theme colors (same Catppuccin Mocha palette as main settings)"*. They have
since been converted to read the palette, so this is dead code that is at
least no longer *divergent* dead code.

**What to do, in order.**

1. Decide which snapshots page is the real one, delete the other.
2. Decide whether Remote Desktop and File Associations are pages the Settings
   app should have. If yes, add the `SettingsPage` variants and wire them; if
   no, delete the modules. `apps/remotedesktop` already exists as a separate
   application, which is an argument that `remote.rs` duplicates *that* too and
   should go.
3. Remove the three `#![allow(dead_code)]` attributes, so that the next module
   to become unreachable says so.
