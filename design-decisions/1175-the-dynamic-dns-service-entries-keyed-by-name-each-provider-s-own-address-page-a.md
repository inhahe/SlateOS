## 1175. The dynamic-DNS service: entries keyed by name, each provider's own address page, a refusal that would repeat holds the entry, and HTTPS or nothing

**Date:** 2026-10-06
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** dynamic DNS keeps a name such as `myhome.duckdns.org`
pointing at a home network whose address the internet provider keeps
changing. `services/dyndns` now does that, for each entry Settings writes to
`/etc/dyndns.yaml`, and reports to `/run/dyndns.yaml` (lane E's
`requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md`).
Five choices had more than one reasonable answer. Here are the answers, and why:
- entries are written as a mapping, not a list;
- the address is learned from each provider's own page;
- a refused update is not repeated until the entry changes;
- a password is never sent without encryption;
- an unchanged address is still sent every 25 days.
What each provider is lives in `services/dyndns/providers` (crate
`dyndnsproviders`), which Settings links too.

**1. Entries are a mapping keyed by name, not the list the request sketched.**

| Option | *What changes* | For | Against |
|---|---|---|---|
| **A. `entries:` maps a name to its settings** (chosen) | `Home:` with its settings indented under it | `yamldoc`, the library every configuration file here goes through so that Settings keeps a user's comments, reads it; a name cannot be given twice, and the status file is keyed by the same names | not the shape the request wrote |
| B. A list of entries, each with `name:` | `- name: Home` ... | the request's shape | `yamldoc` does not read a list of mappings: Settings could not edit the file without losing its comments, which is `yamldoc`'s whole reason to exist; two entries could share a name |

A file in the list shape is reported as a whole-file problem in the status
file, rather than read as no entries, so a file written to the old sketch
says what is wrong.

**2. The public address is learned from the provider's own page.** These are
No-IP's `ip1.dynupdate.no-ip.com`, Dynu's `checkip.dynu.com` and
Cloudflare's `cdn-cgi/trace`. DuckDNS and FreeDNS read the address off the
update request itself, so for them nothing is learned and the update says
nothing. No third party is ever asked: an entry for one provider sends
nothing to another. The alternative, one "what is my address" service for
every entry, would tell a company the user never chose that this network
exists, at every check. A custom URL that names `{ip}` waits for the router
query (UPnP or NAT-PMP, still to be written) and says so in its status.

**3. A refusal that asking again would only repeat holds the entry until the
entry changes.** These are dyndns2's `badauth`, `nohost`, `notfqdn`,
`numhost`, `abuse`, `badagent` and `!donator`, DuckDNS's `KO`, FreeDNS's
unknown token, and a 401, 403 or other 4xx. The provider's own trouble
(`911`, `dnserr`, a 5xx, a 429) waits 30 minutes, as the dyndns2 protocol
asks. Everything else (unreachable, no address, an unreadable answer) is
tried again at the entry's interval. The alternative, retrying everything
each interval, is what gets a client blocked: No-IP and Dynu turn a stream
of `badauth` into `abuse`. Rereading the configuration releases a hold for
any entry it changed, since the change may be the fix. Restarting the
service releases them all. The secret changing should release it too, but
there is no store for secrets to watch yet (D-Q4).

**4. HTTPS or nothing.** Every provider's update is an `https://` URL and
carries the password. Nothing on SlateOS speaks TLS yet, so the service
refuses those requests with that reason, which Settings shows. It never
retries them over plain HTTP. The address pages it asks over plain HTTP (No-IP's, Dynu's) carry no secret.
What a provider answers is shown in its own words, with the entry's secret
blanked out first, since the status file is readable by everyone.

**5. An unchanged address is published again after 25 days.** That is
`ddclient`'s `max-interval` default, well inside the 30 days after which
some providers retire a hostname nothing has updated, and it covers a record
someone changed by hand at the provider.

**Secrets:** read through one trait, `dyndns::Secrets`, whose only
implementation answers that there is no store yet and names D-Q4. An entry
that needs a password waits and says why. When D-Q4 is answered, that
implementation changes and nothing else does.

**Revisit:** 1, if `yamldoc` learns lists of mappings and Settings would
rather write a list. 4, when TLS exists: the refusal goes, and the live
providers are checked against the answers the tests replay, which are their
documented ones.
