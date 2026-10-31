## A-WIFI-REKEYS-AND-LOST-LINKS-ARE-NOT-ANSWERED (lane A, 2026-10-30) — OPEN

**Status:** OPEN

**In short:** once the WiFi driver process has joined its network, it
answers nothing more of the network's key management. Routers renew the
key their broadcast frames go under every so often (an hour is a common
default) by a short handshake of their own, and some renew the station's
own key too; unanswered, the router drops the station. A dropped or lost
connection is not joined again either. The boot test's join run lasts a
minute or two, well inside either, so it passes; a WiFi card serving the
system's sockets for its life (`linuxdrv.serve=mt7925e@...`) would lose
its network within the hour.

**Where.** `linuxdrv/emul/lx_wifi.c`: `lx_wifi_join_at` takes the
handshake's EAPOL frames only until its four-way handshake is done
(`dev_remove_pack` after `lx_wifi_station_handshake`). After that,
`lx_nic.c`'s receive handler passes 802.1X frames on
(`RX_HANDLER_PASS` for `ETH_P_PAE`) and nothing in the process takes them.
Nothing watches nl80211's `mlme` group for a disconnection either.

**The proper fix.** In serve mode, keep the station's supplicant for the
process's life:
- answer the group key handshake (802.11-2020 12.7.7: message 1 carrying
  the new group key, wrapped; message 2 back; the key installed), checking
  each frame's MIC and replay counter as `lx_wifi_supplicant` does;
- answer a new four-way handshake the router starts, installing the new
  pairwise key;
- on a disconnection (`NL80211_CMD_DISCONNECT` from the mlme group) or a
  lost link, scan and join again (`lx_wifi_join_network`), the daemon's
  for-life wait (design-decisions 1594) carrying the sockets over the gap.

Trigger: serving a WiFi card for the system's life, the next step after the
join run passes on the test PC.
