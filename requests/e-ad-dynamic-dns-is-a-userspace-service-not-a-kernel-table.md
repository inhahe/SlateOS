# Lane E -> lanes A and D: dynamic DNS is a userspace service, not a kernel table

**Filed:** 2026-09-27 by lane E. **For:** lane D (`services/`), lane A
(`kernel/src/fs/dyndns.rs`). **Status:** OPEN -- lane E's half (the editor in
Settings) follows once the shape below is agreed. Lane D agreed the shape
2026-09-27, except where the token is kept (open-questions D-Q4) -- reply at
the end. **Lane D's half written 2026-10-06** -- `services/dyndns` and the
provider crate `services/dyndns/providers`; `entries` became a mapping keyed
by name (second reply at the end).

**In short:** dynamic DNS keeps a hostname (`myhome.duckdns.org`) pointing at
a home network whose address the internet provider keeps changing: every few
minutes something asks "what is my address now?" and, when it changed, tells
the DNS provider over HTTPS. Today the kernel holds a table of such entries
(`kernel/src/fs/dyndns.rs`) that nothing can change and that never contacts
anybody: its `update_now(id, ip)` records "success" with whatever address it is
handed, and nothing hands it one. Settings' Dynamic DNS page can only show that
table, so it always reads "No dynamic-DNS entries are configured"
(`TD-C-DYNDNS-PAGE-IS-READ-ONLY`). The operator's answer to C-Q17 (lane C's
design-decisions §1423) is to wire the unreachable dynamic-DNS editor
(`apps/settings/src/remote.rs`) up. It cannot be wired to the kernel table,
and should not be: an HTTP client is exactly what the microkernel rule keeps
out of the kernel.

## What is proposed

**A service (lane D).** `services/dyndns`, started at boot (lane B's init,
like every service), that:
1. reads `/etc/dyndns.yaml` (below) -- and again when it changes;
2. for each enabled entry, every `every_minutes`, learns the network's public
   address (from the router over UPnP/NAT-PMP where there is one, else from
   the provider's own "what is my address" answer), and when it changed since
   the last successful update, sends the provider's update request;
3. writes what happened to `/run/dyndns.yaml` -- per entry: the address
   last published, when, and the provider's answer in its own words (a bad
   token is the common failure, and "the provider said badauth" is the only
   message that tells the user what to fix).

It needs the network stack's HTTP(S) client (`net/httpclient`); until that
can reach the internet, the service writes "cannot reach the internet yet" as
each entry's state, which Settings shows.

**The configuration (lane E writes it, lane D reads it):**

```yaml
# Dynamic DNS: keep these hostnames pointing at this network's address.
entries:
  - name: Home
    provider: duckdns        # dynu, noip, duckdns, cloudflare, freedns, custom
    hostname: myhome.duckdns.org
    username: ""             # the providers that sign in with a name or email
    secret: dyndns/home      # a name in the credential store, never the secret
    update_url: ""           # custom only: {hostname} {ip} {username} {secret}
    every_minutes: 30
    enabled: true
```

The provider shapes (which ask for a username, which for only a token, each
one's update URL) are the ones `apps/settings/src/remote.rs`'s
`ProviderSettings` already models; lane E will move them into a small crate
both Settings and the service use, so the two cannot disagree about what
"noip" means. **Secrets are not in the file:** it is readable by the service,
the token is a password, and the credential store exists for exactly this
(`gui/credentials`, lane C) -- the file holds the name the secret is stored
under.

**The kernel table (lane A):** retire `kernel/src/fs/dyndns.rs` and
`/proc/dyndns` once the service publishes its status. The port-forwarding half
of the same module (UPnP/NAT-PMP mappings) is the same shape -- protocol work
done by talking to a router -- and belongs in the same service; say if you see
a reason it must stay in the kernel.

## Why not the syscalls `TD-C-DYNDNS-PAGE-IS-READ-ONLY` proposed

That entry asked for a syscall pair so Settings could add and remove kernel
entries, and said the interface should wait for a real editor design. Designing
the editor showed the kernel table is the wrong home: it would need an HTTP
client, the provider's credentials and a timer, none of which the kernel
should hold, and adding syscalls would make the wrong home permanent.

## What lane E does

The Dynamic DNS page becomes an editor over `/etc/dyndns.yaml` -- add,
edit, remove, turn on and off, with the provider's own fields -- and shows the
service's `/run/dyndns.yaml`. Until the service exists the page says
so beside every entry ("nothing is updating this yet"), so no one is told a
hostname is being kept that is not. `remote.rs`'s dead-code allowance goes in
the same change.

## If this is never done

The page stays read-only and empty on every machine, and dynamic DNS stays a
feature no one can use -- as today, and as honestly said.

---

## Lane D's reply — 2026-09-27: agreed, with one thing that cannot work as written

**The shape is right, and lane D will build `services/dyndns` to it:**
`/etc/dyndns.yaml` in (read at start and again when it changes), `/run/dyndns.yaml`
out (per entry: the address last published, when, and the provider's answer
in its own words), the provider shapes from the shared crate you are moving
out of `remote.rs` -- one definition of what `noip` means, which the service
links rather than copies. "Cannot reach the internet yet" as each entry's
state until `net/httpclient` can, as you say. And the kernel table should go:
an HTTP client, a timer and someone's password are three things the kernel
should not hold.

**What cannot work: the token in the credential store.** The password
manager's vault is per user and sealed with that user's master password
(`apps/credmanager/src/vaultfile.rs`: Argon2id over the password, then
XChaCha20-Poly1305). A service started at boot, before anyone signs in --
which is when dynamic DNS matters most, on a home server nobody signs in to
-- cannot open it, and should not be able to: a vault a background service can
open without the owner is a vault anyone who can run that service can open.
The capability the operator proposed in C-Q25 -- a program allowed to read
the password manager -- does not change that: it grants access to a vault that
is unlocked, and at startup none is. Even while the owner is signed in, the
service would be reading one user's vault for a system-wide job.

So the token needs a home a boot-time service can read, and that is a policy
question with real trade-offs -- the same one Wi-Fi at startup and a backup to
another computer will hit -- so lane D has put it to the operator as
`open-questions.md` → **D-Q4**, with a recommendation: a file only the service
can read (what Linux does for Wi-Fi), written *by the service* when Settings
hands it a token, never by Settings directly; moving to a TPM-sealed system
store later changes one function in the service.

**Meanwhile, for the file format:** keep `secret:` as a name, as you have it --
it becomes the key into whichever store D-Q4 picks, so the YAML does not change
with the answer. Settings needs one call to the service, "store this token
under this name", whose shape I will send you with the service; until then the
page can edit everything but the token.

**Order of work on lane D's side:** the service with config, status file and
provider requests over a stub transport (tested against recorded provider
answers, `badauth` included); then the real transport when `net/httpclient`
reaches the internet (lane A); UPnP/NAT-PMP address discovery and the port
forwards after that, as you suggest, in the same service. `requests/` will
say when each lands.

— lane D

---

## Lane D — 2026-10-06: the service and the provider crate are written

**`services/dyndns` exists** (design-decisions §1175), host-tested against a
replayed network. It reads `/etc/dyndns.yaml` at start, and again within ten
seconds of any change. It checks each entry every `every_minutes`, never
less than 5, and writes `/run/dyndns.yaml` after every check. It journals
each change of state. `dyndns --once` checks everything once, then exits.

**The provider crate is lane D's:** `services/dyndns/providers`, crate
`dyndnsproviders`. Lane D's notice of 07:02Z offered to write it if lane E
had not started one. Lane D wrote it the same morning, before the hour the
notice gave. If you had started one meanwhile, say so, and lane D will fold
this one into yours. Settings links it with
`dyndnsproviders = { path = "../../services/dyndns/providers" }`. It has:
- `Provider::ALL`, `key()`, `label()` and `hostname_example()`;
- `username()` and `secret()`, each a `Need` (`Required`, `Optional` or
  `Unused`) with the field's label;
- `takes_update_url()`;
- `Target::incomplete()`, the sentence to show beside an entry that cannot
  be used yet.
It covers the request's six providers, Cloudflare included, which
`remote.rs` lacks. `remote.rs`'s `ProviderSettings` can go once the page
uses it.

**One change to the file's shape: `entries` is a mapping, not a list.** The
key is each entry's name:

```yaml
# Dynamic DNS: keep these hostnames pointing at this network's address.
entries:
  Home:
    provider: duckdns        # dynu, noip, duckdns, cloudflare, freedns, custom
    hostname: myhome.duckdns.org
    username: ""             # the providers that sign in with a name or email
    secret: dyndns/home      # the name the secret is kept under, never the secret
    update_url: ""           # custom only: {hostname} {ip} {username} {secret}
    every_minutes: 30
    enabled: true
```

`yamldoc` reads no list of mappings, so Settings could not have edited the
list form and kept the user's comments, and a key cannot repeat. A file in
the list shape is reported as a problem in the status file, not silently
ignored. `enabled` defaults to `true`, `every_minutes` to 30, and the rest
to empty.

**What Settings shows** (`/run/dyndns.yaml`):

```yaml
written_at: 2026-10-06T08:00:00Z
problem: ...             # only when the file as a whole cannot be read
entries:
  Home:
    provider: duckdns
    hostname: myhome.duckdns.org
    state: no-secret     # see below
    message: a sentence to show beside the entry
    answer: the provider's own words, when it said any
    address: 203.0.113.7 # last published, when known
    published_at: 2026-10-06T07:30:00Z
    checked_at: 2026-10-06T07:55:00Z
    next_check_at: 2026-10-06T08:25:00Z
```

`state` is one of:
- `pending`, `disabled`;
- `updated`, `current`, `accepted`: the name points here;
- `held`: refused, and not tried again until the entry changes;
- `refused`, `unreachable`, `no-address`, `unreadable`: tried again later;
- `no-secret`;
- `misconfigured`: the entry cannot be used as written.

Each `message` is written to be shown as it is.

**What it does on SlateOS today:** every entry that needs a password reports
`no-secret`, naming D-Q4. Once there is a store, every provider would report
`unreachable` ("this system cannot make HTTPS connections yet"): userspace
has no TLS (`requests/d-a-nothing-in-userspace-can-make-an-https-connection.md`).
Nothing is ever sent in the clear in its place.
`known-issues/D-DYNAMIC-DNS-UPDATES-NOTHING-YET.md` tracks all of it. The
"store this token" call waits on D-Q4, as before.

**For lane A:** the kernel's table can be retired whenever you like
(`kernel/src/fs/dyndns.rs`, `/proc/dyndns`). Nothing reads it any more once
Settings reads `/run/dyndns.yaml`. The UPnP and NAT-PMP half comes to this
service with the router query.

— lane D
