## 1215. The speed test measures against public test servers, over plain HTTP, when Start is pressed

**Date:** 2026-09-26
**Lane:** E
**Decided by:** Claude (autonomous) -- Claude's to revisit

**In short:** The speed test used to invent its results, then showed none.
It now measures the connection: it times TCP connections to a public test
server for the round trip, then downloads (and, where the server allows,
uploads) for a set time and counts the bytes. The servers are ones their
operators publish for exactly this -- Tele2, Hetzner, Linode, Vultr and
thinkbroadband -- and nothing is contacted until Start is pressed, and then
only the server selected.

### The choices

| Question | Chosen | Alternative | Why |
|---|---|---|---|
| Servers | eight public test-file servers, named with who runs them and where | speedtest.net's protocol and server list | Ookla's protocol is proprietary and its terms restrict third-party clients; these servers publish their files for public testing |
| Transport | plain HTTP | HTTPS | nothing here can check a server's identity (see E-Q2); a speed test sends nothing of the user's, only bytes to count |
| Latency | TCP connect time, 20 probes | ICMP ping | an application can open TCP connections and nothing lower; a handshake is one round trip with no server work in it. A probe that fails is counted as a failed probe -- the result's "packet loss" became "failed probes", since TCP hides lost packets |
| The figure | the average rate after the first fifth of each phase | the average of the whole phase | a connection starts slow and doubles; counting the climb reports a slower line than there is |
| No upload address | "not measured" | zero | zero says the line cannot send |

**Where it lives:** `apps/speedtest/src/net.rs` (the run), `real_servers()`
and `SpeedTestUI::apply` in `apps/speedtest/src/main.rs`.

**How to reverse:** `default_servers()` returning an empty list makes Start
report "No server is selected" and contact nothing.
