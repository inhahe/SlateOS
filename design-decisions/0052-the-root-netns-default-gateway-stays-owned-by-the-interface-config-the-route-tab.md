## 52. The root netns default gateway stays owned by the interface config; the route table holds only non-default routes, and `resolve_next_hop` consults the table first then falls back to the interface gateway

**Date:** 2026-07-02
**Decided by:** Claude (autonomous) — completes TD18 follow-up (b) (route-table
write syscalls). Clearly-correct default with no operator fork needed: it adds
a capability to an existing subsystem without changing established semantics.

**Context.** The kernel already had a full per-namespace routing table
(`netns::add_route`/`remove_route`/`route_lookup` with longest-prefix-match,
`routes`), but two things were missing for the *root* namespace: (1) no syscall
exposed it to userspace, so `ip route add`/`route add` for non-default routes
hard-errored; and (2) `net::ipv4::resolve_next_hop`'s root branch ignored the
table entirely, using only `interface::info().gateway`. The new
`SYS_NET_ROUTE_ADD`/`_DEL`/`_LIST` (857/858/859) expose the table, and the root
branch now consults `route_lookup(ROOT_NS, dst)` before the interface fallback.

**The decision.** There are two plausible homes for the *default* route
(`0.0.0.0/0`):

- **(A, chosen)** Keep the default gateway in the interface config
  (`SYS_NET_IF_CONFIG` GATEWAY field). The route table holds only *specific*
  (non-default) routes. `resolve_next_hop` tries the table first (specific
  routes win by longest-prefix-match), and if nothing matches falls back to the
  interface gateway for the implicit default + connected delivery. `ip route add
  default via X` / `route add default gw X` continue to write the interface
  gateway (already wired in follow-up (a)); only non-default routes touch the
  table.
- **(B, rejected)** Make the route table the single source of truth, with
  `default via X` inserting a `0.0.0.0/0` table entry and the interface
  `gateway` field becoming a derived cache (or removed).

**Why A.** (1) No migration/reconciliation: the default-gateway semantics from
follow-up (a) and every existing `resolve_next_hop` path are unchanged, so this
is purely additive and backward-compatible — an empty table behaves exactly as
before. (2) The display tools already synthesize the default route from
`SYS_NET_IF_INFO` separately from listed routes, so keeping the two sources
distinct matches what userspace already renders. (3) It avoids two writers
racing on the same `0.0.0.0/0` slot. **Cost of A:** the default route is not a
row in the route table, so a naive `route -n` merge must union the interface
default with the table (the tools already do this). **Cost of B:** a larger,
riskier refactor touching `configure()`, `resolve_next_hop`, and every place
that reads `info().gateway`, for a mostly-cosmetic unification. Revisit B only
if we later need multiple default routes or per-route metrics on the default.
