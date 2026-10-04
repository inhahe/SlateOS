### [E] The network manager's editor says DHCP is on for an interface whose DHCP nothing reports -- 2026-10-04
**Status:** OPEN until the fix -- lane E's a3089416c, 2026-10-04: the editor's DHCP state is an `Option<bool>`, `None` for an interface the kernel lists -- has had a boot test on `main`; then this moves to `known-issues-resolved/`.

**In short:** for every interface read from the kernel, the IP
Configuration tab shows "DHCP: Enabled", and its editor opens with the
DHCP switch on -- while the Properties tab of the same interface says
DHCP is "Not reported", which is the truth: `/proc/net` does not say
whether DHCP gave an address. With the switch on, the address boxes are
disabled, the DNS list cannot be changed, and Apply refuses, so on a real
machine nothing can be applied until the user turns off a switch that was
never really on.

**Where.** `apps/netmanager/src/main.rs`, `interface_of`: it builds the
editable configuration with `dhcp_enabled: true` and the report with
`dhcp: None`. The two halves of the window read different fields --
`render_tab_ip_config` reads `edit_ip_config.dhcp_enabled`, the
Properties summary reads `dhcp`.

**How to see it.** On SlateOS, open the network manager, pick an
interface, open IP Configuration: "DHCP: Enabled". Properties: "DHCP: Not
reported". Press Edit: the boxes are greyed out.

**The fix (a3089416c).** The switch has a third position, "not
reported", for an interface whose DHCP state is unknown:

- `IpConfig::dhcp_enabled` is an `Option<bool>`, so that an unknown
  cannot be read as either answer by accident. Every interface read from
  the kernel starts at `None`.
- The IP tab reads "DHCP: Not reported", as Properties does. The switch
  is drawn off, as it is for a configuration set by hand.
- Under "not reported" the address boxes and the DNS list can be
  changed, and Apply sends the configuration typed. Typing addresses and
  pressing Apply says what is meant, so nothing is asked. (Asking was
  the first proposal here; it would have put a question between every
  user and every change on a real machine, where nothing is reported.)
- Pressing the switch goes to on, then off. Only "on" disables the
  boxes, and Apply refuses it as before.

Found while converting the address boxes to the toolkit's field
(2026-10-04): the boxes are now disabled under DHCP, which made the
default switch position visible as soon as Edit is pressed, where before
it showed only when Apply refused.
