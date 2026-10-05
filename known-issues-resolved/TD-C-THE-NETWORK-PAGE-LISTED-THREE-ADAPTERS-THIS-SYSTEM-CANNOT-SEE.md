## TD-C-THE-NETWORK-PAGE-LISTED-THREE-ADAPTERS-THIS-SYSTEM-CANNOT-SEE -- FIXED 2026-09-15

**In short:** the Settings app's Network page listed `eth0` as connected at
192.168.1.100, `wlan0` as disconnected, and `lo` at 127.0.0.1, each with a
green or grey dot for its link state. This system cannot list its network
interfaces at all -- nothing in the tree asks the kernel what hardware is
present -- so all three were constants in the source. The page now says it
cannot look.

**Date:** 2026-09-15. **Lane:** C. The fourth and last of the fabricated
Settings pages found by sweeping after the update page: Sound (fixed
2026-09-14), Updates, Accounts, Privacy and now Network.

**Why a status page is the worst place for this.** A green dot beside an
interface name and an address is the strongest claim a page can make: it says
*I looked, and this is what is there*. Every other invented control at least
described a setting. This described the machine.

**Two separate reasons the controls went, and the second outlives the first.**
The adapter list cannot be built: `net/` carries a DNS resolver and an HTTP
client and nothing that enumerates interfaces, and there is no service between
a GUI application and the kernel's own. But the address, DNS and proxy fields
would have been dead even with a list, because **nothing reads them**. The
nearest thing to a consumer is `net/dns`, which defines a `ResolverConfig` and
parses `resolv.conf` -- so the *format* exists -- and no program in this tree
loads one. A nameserver typed into that page lived in the window's memory until
it closed. `net/httpclient` takes no proxy at all.

**Removed with the pages:** `NetworkAdapter`, `AdapterType`, `IpConfigMode`,
`adapters`, `selected_adapter`, `ip_config_mode`, `static_ip`,
`static_gateway`, `dns_primary`, `dns_secondary`, `proxy_enabled`,
`proxy_address`, `proxy_port`, `SelectId::Adapter`, `ToggleId::ProxyEnabled`,
`DropdownId::IpConfig`, `render_text_field` and `PageSink::field_row`. 260
lines deleted against 96 added.

**A note on the tests, which is where the real risk was.** Three tests died with
the feature. Six more did not: they clicked an adapter row as a *vehicle* for
testing the event loop -- when a frame is drawn, which events are ours, when
the loop stops -- and the adapter was incidental to all of them. Those were
re-pointed at an account row, which is the same shape of control and is backed
by something. Deleting them along with the feature would have been the easy
mistake and would have quietly removed the coverage of the shipped `run` loop,
which has nothing to do with networking.

**The remaining pages of this app are honest as far as this sweep goes**, but
the sweep was "read every page", not a check anything runs. The shape to look
for is a `Vec` of plausible-looking records built in a constructor: every one of
the five was found that way, in `SettingsState::new`.
