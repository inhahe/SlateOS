## 53. Firewall write syscalls (860–864) mirror the kernel Rule model exactly; the `fw` tool's richer on-disk format skips (with a warning) any rule the kernel cannot represent rather than pushing a broader rule

**Date:** 2026-07-02
**Decided by:** Claude (autonomous) — completes TD18 follow-up (b) (firewall
write syscalls). Additive capability on an existing subsystem; no operator fork.

**Context.** The kernel already had a full per-namespace packet-filtering
firewall (`net::firewall`: `Rule { active, direction, action, protocol, src_ip,
src_prefix, dst_port, priority, match_count }`, global + per-ns tables, packet
path via `check_inbound_ns`/`check_outbound_ns`, reads served by
`/proc/net/firewall`). No syscall exposed the *write* path, so `fw enable`,
`fw allow/deny`, `fw policy`, `fw delete`, `fw reset` could only edit the local
`/etc/fw.rules` file and never touched the running kernel — the old `fw_ioctl`
stub returned `ENOSYS`. The new `SYS_NET_FW_ENABLE`/`_SET_POLICY`/`_ADD_RULE`/
`_DEL_RULE`/`_FLUSH` (860–864, all root-gated, operating on the caller's netns
with root ns == the global firewall) close that gap.

**The decision — ABI shape.** `ADD_RULE` takes a fixed 12-byte binary record
(`[direction, action, protocol, src_prefix, dst_port:u16le, priority:u16le,
src_ip:4]`) rather than a text line. Binary avoids a parser in the kernel
syscall path (the kernel has no reason to reparse the human format), keeps the
decode branch-simple (destructure the array by value — no indexing), and mirrors
the `Rule` fields 1:1. `ENABLE`/`SET_POLICY`/`DEL_RULE` are scalar-only; `FLUSH`
takes no args. Reads stay on `/proc/net/firewall` (no read syscall), matching
the route-syscall precedent (§52) where listing has both a syscall and procfs
but control is the syscall's job.

**The decision — model mismatch handling.** The `fw` tool's on-disk rule format
is richer than the kernel model: it carries `src_port` and `dst_ip` dimensions
the kernel `Rule` has no field for. Two options:

- **(A, chosen)** When a rule constrains `src_port` or `dst_ip`, the tool
  **skips** pushing it to the kernel and prints a warning; the rule is still
  saved to `/etc/fw.rules` (so no user data is lost and a future richer kernel
  model could honour it). `to_kernel_record` returns `None` for such rules.
- **(B, rejected)** Drop the unrepresentable dimension and push the rule anyway
  (e.g. ignore `dst_ip`, matching all destinations).

**Why A.** Silently widening a rule (B) is a security footgun: an operator who
wrote "allow from 10.0.0.5 to 10.0.0.10:80" would get "allow from 10.0.0.5 to
*:80" installed in the kernel — strictly more permissive than intended, exactly
the wrong direction for a firewall to err. A explicitly refuses to install a
rule it cannot honour and tells the operator, which is fail-safe. **Cost of A:**
the kernel ruleset can diverge from the file (some file rules aren't installed);
the tool's warning makes this visible, and `fw list` reads kernel state so the
divergence is observable. Revisit if/when the kernel `Rule` gains `src_port`/
`dst_ip` fields — then A's skipped rules become representable with no ABI change
on the enable/policy/del/flush syscalls (only the ADD record grows).

**Positional delete correctness.** Because unrepresentable rules are never
pushed, the kernel index of a rule ≠ its position in the tool's list. `fw
delete N` computes the kernel index as the count of *representable* rules before
position N and only issues `DEL_RULE` if the target rule was itself pushed —
avoiding an off-by-one that would delete the wrong kernel rule.
