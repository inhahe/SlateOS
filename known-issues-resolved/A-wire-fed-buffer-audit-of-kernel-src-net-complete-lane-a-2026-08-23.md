## Wire-fed buffer audit of `kernel/src/net` — complete (lane A, 2026-08-23)

**In short:** after finding two places where a remote machine could make the
kernel allocate memory forever, I checked every other place in the networking
code that stores incoming data, to see whether the same mistake was hiding
anywhere else. It was not. Ten such buffers exist; two were broken and are now
fixed, and the other eight were already correct. This entry records the list so
nobody has to repeat the search.

**Status:** DONE. No action outstanding.

### Method

The failure being hunted is not a lint and not a crash. It is: *a buffer fed
from the network that has no ceiling, or that has one but cannot say so.* The
three questions from the SSH entry above, applied to every file in
`kernel/src/net`:

1. Can a length or offset field be rejected in a way that leaves the bytes
   buffered and the connection open?
2. Is there an accumulating buffer fed from the wire with no absolute cap?
3. Where two sibling parsers exist (plain/encrypted, v4/v6, client/server), do
   their return types and their bounds agree? A divergence is where the weaker
   one is wrong.

Question 3 is the cheap one and it earns its place: both bugs found were a
sibling pair where one half was right and the other was not.

### Every wire-fed buffer, and its bound

| Buffer | Bound | Verdict |
|---|---|---|
| `ssh.rs` `Session::recv_buf` | *none* | **was broken** — fixed, `MAX_RECV_BUF` |
| `tls.rs` `hs_buf` (handshake reassembly) | u24, i.e. 16 MiB | **was broken** — fixed, `MAX_HANDSHAKE_MSG_SIZE` |
| `tcp.rs` `TcpConnection::rx_buffer` | `MAX_RX_BUFFER`, clamped at every append, and it drives the advertised window | correct |
| `tcp.rs` `TcpConnection::tx_buffer` | fed by the local application, not the wire | n/a |
| `udp.rs` `rx_queue` / `rx_queue_v6` | `MAX_QUEUED` (64), silent drop — right for UDP | correct, and the two agree |
| `veth.rs` `rx_queue` | `VETH_QUEUE_DEPTH`, returns `ChannelFull` and counts the drop | correct |
| `frag.rs` v4 and v6 reassembly `buffer` | `MAX_PAYLOAD_V4`/`_V6` checked *before* `resize`, 8 slots, 30 s/60 s timeouts | correct, and the two agree |
| `telnet.rs` `line_buf` | `MAX_LINE_LEN` | correct |
| `ssh.rs` `Channel::line_buf` | 1024 | correct, though see below |
| `tls.rs` `recv_buf` (client and server) | holds one record's unread remainder; the reader drains before it refills, so ≤ `MAX_PLAINTEXT_SIZE` | correct, and the two agree |

`firewall.rs` has no accumulating buffer at all — its conntrack tables are
fixed-size arrays, and its v4 and v6 paths call the *same* `extract_ports`
helper, so they cannot drift apart. That is the structure the rest of this
table is trying to achieve by inspection.

### The one nit left

`ssh.rs`'s `Channel::line_buf` caps at a bare `1024` rather than a named
constant. It is a correct bound, so this is not a bug and is deliberately not
being "fixed" into a churn commit — but if that file is touched again, the
literal should become a `const` next to `MAX_RECV_BUF`. A magic number is a
bound nobody can find when they go looking for bounds, which is exactly the
search this entry documents.

### What did not work

Grepping for the *symptom* found nothing, twice. Both bugs are invisible to
`cargo clippy`: no index, no unwrap, no arithmetic. `net/ssh.rs` was sitting on
the sweep list with 104 clippy sites and the denial of service was not one of
them. The audit that worked was reading each framing layer and asking what a
peer gains by lying about a length — which is a different activity from
lowering a warning count, and the warning count is not a proxy for it.
