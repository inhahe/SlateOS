## TD-C-DYNDNS-PAGE-IS-READ-ONLY

**Date:** 2026-09-09. **Lane:** C.
**Where:** `apps/settings/src/main.rs` — `build_dyndns_page`;
`apps/settings/src/dyndns.rs`; `kernel/src/fs/dyndns.rs`.

**In short:** the new Settings → Network → Dynamic DNS page shows the entries
the machine has, but the user cannot add, edit or remove one from it. Dynamic
DNS keeps a hostname pointing at your home address as your ISP changes it;
right now the only way to set one up is from inside the kernel, so in practice
the page will read "No dynamic-DNS entries are configured" on every machine and
there is no way to make it say anything else.

**Why:** the kernel implements the whole feature — `add_entry`, `remove_entry`,
`set_update_url`, `update_now`, plus UPnP/NAT-PMP forwarding — but exposes none
of it to userspace. There is no dyndns syscall (`grep -rn dyndns kernel/src/sys*`
returns nothing) and `/proc/dyndns` is generated read-only by `gen_dyndns`.
Userspace can therefore observe the state and change nothing about it.

**This is not the page being unfinished.** Rendering an Add button that cannot
add would be the mistake the page was written to avoid — see the test
`the_dyndns_page_invents_no_entries` and the unreachable version in `remote.rs`
that defaults to a fabricated `home.example.com` entry. A read-only page that
is honest about what it can do is the correct intermediate state.

**Proper fix:** a capability-gated syscall pair for add/remove plus one for
`update_now`, and then the page grows an editor. Two things it will need that
exist already and should be used rather than rewritten: the per-provider
credential shapes in `apps/settings/src/remote.rs` (`ProviderSettings`), which
the kernel does not model — it stores one generic `update_url` per entry — and
`apps/settings/src/dyndns.rs`'s `PROVIDERS`, which must stay in step with the
kernel enum either way.

**Trigger to fix:** when the syscall exists. That is lane A's; no request is
filed yet, because the interface should be specified against a real editor
design rather than guessed at now.

**If never fixed:** the page is accurate and inert. Nothing breaks; the feature
is simply unreachable by any user, exactly as it was before the page existed —
the page's value in the meantime is that it stops the *next* reader concluding
from `remote.rs` that dynamic DNS is wired up and working.

**Related:** an entry can also read `Success` while publishing an address the
router no longer has. The page flags that ("address out of date") by comparing
each entry's last published address with the router's external address, because
the status alone does not show it.
