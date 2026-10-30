## 1593. A WiFi driver process drives Linux's 802.11 stack through nl80211

**Date:** 2026-10-30 · **Decided by:** Claude (operator-approved scope) -- WiFi through Linux's own drivers is roadmap 2.1's (§1577); how the layer drives Linux's 802.11 stack is Claude's call, under §1578 · **Lane:** A

**In short:** The test PC's WiFi card runs Linux's own driver (mt7925e) in a
driver process, on top of Linux's own 802.11 stack: mac80211, the part that
talks to the card, and cfg80211, the part that is told "scan", "join this
network", "use this key". On Linux, programs tell cfg80211 those things
through a message protocol called nl80211, the one wpa_supplicant and iwd
use. The driver process has no message sockets, so the layer stands where
those programs would and speaks nl80211 to cfg80211 directly in the process.
The first use is a scan; the second, joining a network -- on simulated
radios, where the layer also plays the access point's own program
(hostapd), answering the radio that asks to join; the third, joining a
network protected by a passphrase (WPA2), the layer making the exchange
that agrees the keys as both sides.

### The decision

1. **The layer is generic netlink's one user in the process**
   (`linuxdrv/emul/lx_netlink.c`). A family registers as on Linux and is
   kept by name. The layer builds a request as a socket would send one and
   runs it as genetlink runs a command: the attributes parsed by the
   command's policy, then the family's `pre_doit`, the handler and the
   `post_doit`. Replies come back to the layer. A message multicast to a
   group the layer listens to is queued for it; any other message, and the
   routing netlink's notifications, are freed as a send no one receives.
   So is a message to the layer's own port that answers none of its
   requests -- nl80211 sends a management frame the layer registered for
   that way, to the port that registered -- except that it is queued for
   the layer (`lx_genl_unicast`), up to 64, a reply told from it by the
   request's sequence number. Permission flags (`GENL_ADMIN_PERM`) are not
   checked: the process's rights are the capabilities the kernel gave it.
2. **The WiFi task speaks nl80211** (`linuxdrv/emul/lx_wifi.c`): with
   "scan", it waits for the interface mac80211 makes once the card is
   registered, brings it up, sends `NL80211_CMD_TRIGGER_SCAN` -- passive,
   with no SSID, so the card only listens -- waits for the "scan" group's
   `NEW_SCAN_RESULTS`, and says each network cfg80211 found.
3. **Joining an open network** (`lx_wifi_join`): the station is told
   `NL80211_CMD_CONNECT`, and cfg80211's own connection manager
   authenticates and associates. On simulated radios the access point is
   the layer's too, and does what hostapd does for an open network: it
   registers for authentication and association frames
   (`NL80211_CMD_REGISTER_FRAME`), answers each (`NL80211_CMD_FRAME`), adds
   the station with the rates it asked for (`NEW_STATION`), and lets it
   through the port (`SET_STATION`, authorized) -- there being no 802.1X
   to wait for.
4. **Joining a WPA2-PSK network** (`lx_wifi_handshake`, keys and frames in
   `emul/lx_wpa.c`): the station connects with WPA2's settings and its port
   closed; the four-way handshake travels as EAPOL frames over the network
   devices (ethertype 0x888e, which mac80211 lets through a closed port);
   each side's keys go in with `NEW_KEY` and its port opens with
   `SET_STATION`. The layer makes the handshake itself, as wpa_supplicant
   (and, on simulated radios, hostapd) would: PBKDF2, the PRF, the MIC and
   the key wrap over Linux's SHA-1 and AES libraries, CCMP by Linux's own
   crypto API (`stacks/crypto.manifest`) wherever the card does not do it.
5. **The kernel names the card** with `linuxdrv.wifi=BB:DD.F`, as
   `linuxdrv.card` names an Ethernet card (§1590's hardware mode): a
   MediaTek network controller of subclass 80 that no driver holds. Without
   it the boot test runs the process with no card, which still runs
   cfg80211's, nl80211's and mac80211's start-up -- what QEMU, which has no
   such card, can check.

### Alternatives

- **Calling cfg80211's internals directly** (building a
  `cfg80211_scan_request` and calling the driver's scan op, as nl80211's
  own handler does) -- no dispatcher, but the layer would reach into
  `net/wireless/core.h` and `rdev-ops.h`, which change from one Linux
  release to the next, and every command would be a copy of nl80211's
  handler. nl80211 is Linux's stable ABI for exactly these requests.
- **Linux's own netlink sockets** (`net/netlink`, with the socket layer
  under it) -- the dispatch exactly Linux's, but sockets, their buffers and
  their accounting for a process whose one user is the layer itself; the
  stand-in is two hundred lines.
- **Active scanning** (probe requests for the wildcard SSID) -- quicker on
  the 2.4 GHz band, but it transmits; a first run on hardware should only
  listen.
- **hostapd's own code for the access point** -- what a real access point
  runs, but a userspace program over sockets, for a role the layer plays
  only so that a station has something to join where there is no network:
  for an open network it is two answers and two station commands.
- **wpa_supplicant's own code for the handshake** -- the reference, and
  WPA3 and enterprise networks with it, but a BSD-licensed program of tens
  of thousands of lines over sockets, its own event loop and drivers; a
  port is its own task. WPA2-PSK's handshake is four frames and five
  primitives, each checked against a published vector; WPA3 (SAE) would
  reopen this.

### Checked by

The boot test runs the mt7925e process with no card, which must start
Linux's 802.11 stack and exit 0 (`linuxdrv::wifi_runs`), and Linux's
`mac80211_hwsim` with two simulated radios (`linuxdrv::hwsim_run`): the
first made an open access point through nl80211 (`SET_INTERFACE`,
`START_AP` with a beacon the layer builds), the second's passive scan
having to find it, the second then joining it -- its carrier on, the
access point holding it as a station -- and a frame crossing the network
each way, payload and sender as sent, in clear on the air (hwsim's
monitor interface); then the network made WPA2-PSK, the second joining it
again, the handshake made, and a frame crossing each way protected on the
air, its payload never in clear. Before any of it, the run checks the
crypto API's CCM against RFC 3610 and the handshake's primitives against
802.11's and RFC 3394's vectors -- both ends share the code, so a mistake
the same at each would pass a join unnoticed. On the test PC,
`scripts/pikvm.py stage --add-cmdline "linuxdrv.wifi=06:00.0"` runs it on
the RZ717, which must scan and exit 0, the networks it found on the screen.
