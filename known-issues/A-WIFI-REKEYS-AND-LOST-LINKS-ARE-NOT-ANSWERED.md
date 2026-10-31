## A-WIFI-REKEYS-AND-LOST-LINKS-ARE-NOT-ANSWERED (lane A, 2026-10-30) — OPEN

**Status:** OPEN -- the rekeys answered since 2026-10-31; a lost link is not
joined again

**In short:** a WiFi card serving the system's sockets for its life must
keep its network. The router's key renewals are now answered; but when the
connection drops -- the router restarts, the signal goes, the router throws
the station off -- nothing joins the network again, and the card stays
without one. The boot test's join run lasts a minute or two and is not
affected; serving WiFi for the system's life (`linuxdrv.serve=mt7925e@...`)
would be.

**Where.** `linuxdrv/emul/lx_wifi.c`: after `lx_wifi_join_network`, the
process leaves nl80211's `mlme` group (so that nothing is kept for a reader
gone), and nothing watches for a disconnection.

**Fixed (2026-10-31): the rekeys.** The joined station is kept for the life
of its association (`lx_wifi_station`, `lx_wifi_keep`): its EAPOL frames
are taken and answered for as long as the process runs -- message 3 sent
again (its message 4 lost on the air) answered again without the keys
installed twice, a new four-way handshake (a pairwise rekey) made with a
nonce of its own -- its keys held apart (a temporary PTK) until a message 3
signed with them arrives, so that a forged message 1 changes nothing -- and
the group key handshake (802.11-2020 12.7.7) answered and the renewed group
key installed. Checked in every boot test's hwsim run
(`lx_wifi_kept_check`): after the handshake the access point resends
message 3, makes a pairwise rekey, a frame crossing each way under the new
keys, then renews its group key, a broadcast crossing under it.

**The proper fix, for what is left.** In serve mode, listen to the `mlme`
group for the life of the process, reading it from the keeper's side: on
a disconnection, or a link lost, scan and join again
(`lx_wifi_join_network`), the network file's PMK kept for it (the file is
dropped as soon as the join has read it, design-decisions 1596), the daemon's for-life wait
(design-decisions 1594) carrying the sockets over the gap.

Trigger: serving a WiFi card for the system's life.
