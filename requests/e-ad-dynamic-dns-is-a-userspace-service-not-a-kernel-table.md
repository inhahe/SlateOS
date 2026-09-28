# Lane E -> lanes A and D: dynamic DNS is a userspace service, not a kernel table

**Filed:** 2026-09-27 by lane E. **For:** lane D (`services/`), lane A
(`kernel/src/fs/dyndns.rs`). **Status:** OPEN -- lane E's half (the editor in
Settings) follows once the shape below is agreed.

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
