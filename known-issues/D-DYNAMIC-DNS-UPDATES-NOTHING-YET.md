## D-DYNAMIC-DNS-UPDATES-NOTHING-YET — the dynamic-DNS service exists, but no hostname is updated on a SlateOS machine yet (lane D, 2026-10-06)

**Status:** OPEN -- five pieces, four of them in other lanes' trees or the operator's hands.

**In short:** dynamic DNS keeps a name such as `myhome.duckdns.org`
pointing at a home network whose address keeps changing. The service that
does it, `services/dyndns`, is written and tested on the development host
against each provider's documented answers (design-decisions §1175). On a
SlateOS machine it would still update nothing:
- the passwords it needs have nowhere to be kept;
- every provider's update is a secure (`https://`) address, which nothing
  here can reach;
- nothing installs the service on the image or starts it.

Its status file says so for each entry. That status is what Settings will
show, so nobody is told a hostname is being kept that is not.

| Missing | Whose | Where it is asked for |
|---|---|---|
| Where services' passwords are kept: every provider but a custom URL needs one | the operator | `open-questions/D-Q4.md` |
| A TLS client that checks certificates, and the root certificates on the image | lane A (or lane D, if lane A hands it over); the certificates are lane D's | `requests/d-a-nothing-in-userspace-can-make-an-https-connection.md` |
| The service on the image and started at boot | lane A (what a boot starts); lane D builds and stages it | `requests/d-a-nothing-on-the-system-image-can-be-started-at-boot.md` |
| Settings' editor over `/etc/dyndns.yaml`, its display of `/run/dyndns.yaml`, and its one call to hand the service a password | lane E (the call waits on D-Q4) | `requests/e-ad-dynamic-dns-is-a-userspace-service-not-a-kernel-table.md` |
| Learning the address from the router (UPnP, NAT-PMP), which a custom URL naming `{ip}` needs, and the port forwards the kernel's table also claimed | lane D | here |

**Checked against the providers themselves: not yet.** The answers the tests
replay are each provider's documented protocol:
- No-IP's and Dynu's dyndns2 codes;
- DuckDNS's `OK` and `KO`;
- FreeDNS's update page;
- Cloudflare's API v4.

None has been seen from the live service. The first run with TLS should
compare each against `dyndnsproviders` before trusting a status.

**What lane D does as they land:**
- **D-Q4 answered:** replace `dyndns::Undecided` with the chosen store, and
  add the call Settings uses to hand over a password.
- **TLS:** the HTTPS refusal in `services/dyndns/src/main.rs` (`Http`) goes,
  the certificates are staged, and the live answers are checked.
- **Boot start:** build `dyndns` for the image, list it in the manifest and
  the startup file, and add a ring-3 rung that runs `dyndns --once` against
  a stand-in provider.

**Where:** `services/dyndns/src/lib.rs` (`Undecided`, the checks, the status
file), `services/dyndns/src/main.rs` (`Http`),
`services/dyndns/providers/src/lib.rs` (each provider).
