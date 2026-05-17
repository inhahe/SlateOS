//! System management dashboard — JSON API and HTML frontend.
//!
//! Provides HTTP API endpoints for real-time system monitoring via the
//! httpd server.  The dashboard is accessible at `/dashboard` with live
//! status data from `/api/*` JSON endpoints.
//!
//! ## Endpoints
//!
//! | Path               | Returns                                      |
//! |--------------------|----------------------------------------------|
//! | `/dashboard`       | HTML single-page dashboard with auto-refresh |
//! | `/api/status`      | JSON: uptime, memory, CPU, task counts       |
//! | `/api/tasks`       | JSON: list of active tasks with details       |
//! | `/api/network`     | JSON: interface info, TCP connections, stats  |
//! | `/api/memory`      | JSON: frame allocator, heap, swap stats       |
//! | `/api/httpd`       | JSON: HTTP server stats, recent access log    |
//! | `/api/dns`         | JSON: DNS cache stats (hits, misses, entries) |
//! | `/api/firewall`    | JSON: firewall status, rules, conntrack count |
//! | `/api/bench`       | JSON: benchmark scorecard (pass/fail, targets)  |
//! | `/api/health`      | JSON: aggregated health check for monitoring    |
//! | `/api/ipv6`        | JSON: IPv6 addresses, SLAAC, DHCPv6 status      |
//! | `/api/containers`  | JSON: active container list with details         |
//! | `/api/tcp`         | JSON: TCP stats, per-connection detail, listeners|
//! | `/api/scheduler`   | JSON: per-CPU utilization, context switches      |
//! | `/api/swap`        | JSON: swap/zram devices, compression stats       |
//! | `/api/fs`          | JSON: mount table, block cache stats             |
//! | `/metrics`         | Prometheus text format (~50 metrics) for monitoring |
//!
//! ## Integration
//!
//! The httpd module routes `/api/*` and `/dashboard` paths to
//! `handle_api_request()` before the normal VFS file-serving path.

use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;

use crate::serial_println;

// ---------------------------------------------------------------------------
// JSON helpers (no serde in no_std)
// ---------------------------------------------------------------------------

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                // Control characters: \u00XX
                let code = c as u32;
                out.push_str(&format!("\\u{:04x}", code));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Convert TcpState to a display string, matching netstat.rs conventions.
fn tcp_state_str(state: super::tcp::TcpState) -> &'static str {
    use super::tcp::TcpState;
    match state {
        TcpState::Closed => "CLOSED",
        TcpState::Listen => "LISTEN",
        TcpState::SynSent => "SYN_SENT",
        TcpState::SynReceived => "SYN_RCVD",
        TcpState::Established => "ESTABLISHED",
        TcpState::FinWait1 => "FIN_WAIT_1",
        TcpState::FinWait2 => "FIN_WAIT_2",
        TcpState::TimeWait => "TIME_WAIT",
        TcpState::CloseWait => "CLOSE_WAIT",
        TcpState::LastAck => "LAST_ACK",
    }
}

// ---------------------------------------------------------------------------
// API handler
// ---------------------------------------------------------------------------

/// Handle an API request.  Returns `Some((content_type, body))` if the
/// path is an API endpoint, `None` otherwise.
pub fn handle_api_request(path: &str) -> Option<(String, Vec<u8>)> {
    match path {
        "/dashboard" | "/dashboard/" => {
            Some((String::from("text/html; charset=utf-8"), dashboard_html()))
        }
        "/api/status" => {
            Some((String::from("application/json"), api_status()))
        }
        "/api/tasks" => {
            Some((String::from("application/json"), api_tasks()))
        }
        "/api/network" => {
            Some((String::from("application/json"), api_network()))
        }
        "/api/memory" => {
            Some((String::from("application/json"), api_memory()))
        }
        "/api/httpd" => {
            Some((String::from("application/json"), api_httpd()))
        }
        "/api/dns" => {
            Some((String::from("application/json"), api_dns()))
        }
        "/api/firewall" => {
            Some((String::from("application/json"), api_firewall()))
        }
        "/api/bench" => {
            Some((String::from("application/json"), api_bench()))
        }
        "/api/health" => {
            Some((String::from("application/json"), api_health()))
        }
        "/api/ipv6" => {
            Some((String::from("application/json"), api_ipv6()))
        }
        "/api/containers" => {
            Some((String::from("application/json"), api_containers()))
        }
        "/api/tcp" => {
            Some((String::from("application/json"), api_tcp()))
        }
        "/api/scheduler" => {
            Some((String::from("application/json"), api_scheduler()))
        }
        "/api/swap" => {
            Some((String::from("application/json"), api_swap()))
        }
        "/api/fs" => {
            Some((String::from("application/json"), api_fs()))
        }
        "/metrics" => {
            Some((String::from("text/plain; version=0.0.4; charset=utf-8"), api_metrics()))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// /api/status
// ---------------------------------------------------------------------------

fn api_status() -> Vec<u8> {
    let uptime_ns = crate::hrtimer::now_ns();
    let uptime_secs = uptime_ns / 1_000_000_000;

    // Memory stats from frame allocator.
    let (total_frames, free_frames) = crate::mm::frame::stats()
        .map(|s| (s.total_frames, s.free_frames))
        .unwrap_or((0, 0));
    let used_frames = total_frames.saturating_sub(free_frames);
    let page_size = 16384u64; // 16 KiB pages
    let total_mem = (total_frames as u64).saturating_mul(page_size);
    let used_mem = (used_frames as u64).saturating_mul(page_size);
    let free_mem = total_mem.saturating_sub(used_mem);

    // Task count from scheduler.
    let task_count = crate::sched::task_list().len();

    // Network interface info.
    let iface = crate::net::interface::info();
    let net_stats = crate::net::interface::stats();

    let json = format!(
        concat!(
            r#"{{"uptime_secs":{},"uptime_ns":{},"memory":{{"total_bytes":{},"used_bytes":{},"#,
            r#""free_bytes":{},"total_frames":{},"used_frames":{},"page_size":{}}},"#,
            r#""tasks":{},"network":{{"up":{},"ip":"{}.{}.{}.{}","#,
            r#""mac":"{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}","#,
            r#""rx_bytes":{},"tx_bytes":{},"rx_packets":{},"tx_packets":{}}}}}"#,
        ),
        uptime_secs, uptime_ns,
        total_mem, used_mem, free_mem, total_frames, used_frames, page_size,
        task_count,
        iface.up,
        iface.ip.0[0], iface.ip.0[1], iface.ip.0[2], iface.ip.0[3],
        iface.mac.0[0], iface.mac.0[1], iface.mac.0[2],
        iface.mac.0[3], iface.mac.0[4], iface.mac.0[5],
        net_stats.rx_bytes, net_stats.tx_bytes,
        net_stats.rx_packets, net_stats.tx_packets,
    );

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/tasks
// ---------------------------------------------------------------------------

fn api_tasks() -> Vec<u8> {
    let tasks = crate::sched::task_list();
    let mut json = String::from("[");

    for (i, task) in tasks.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        // Convert the fixed [u8; 32] name to a &str for JSON output.
        let name_bytes = task.name.get(..task.name_len).unwrap_or(&[]);
        let name_str = core::str::from_utf8(name_bytes).unwrap_or("?");

        json.push_str(&format!(
            concat!(
                r#"{{"id":{},"name":"{}","priority":{},"state":"{}","cpu":{},"#,
                r#""total_ticks":{},"schedule_count":{},"total_wait_ticks":{},"#,
                r#""throttled":{}}}"#,
            ),
            task.id,
            json_escape(name_str),
            task.priority,
            task.state,  // TaskState implements Display
            task.last_cpu,
            task.total_ticks,
            task.schedule_count,
            task.total_wait_ticks,
            task.throttled,
        ));
    }

    json.push(']');
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/network
// ---------------------------------------------------------------------------

fn api_network() -> Vec<u8> {
    let iface = crate::net::interface::info();
    let net_stats = crate::net::interface::stats();
    let tcp_conns = crate::net::tcp::all_connections();

    let mut json = String::from("{\"interface\":");
    json.push_str(&format!(
        concat!(
            r#"{{"up":{},"ip":"{}.{}.{}.{}","#,
            r#""gateway":"{}.{}.{}.{}","dns":"{}.{}.{}.{}","#,
            r#""rx_bytes":{},"tx_bytes":{},"rx_packets":{},"tx_packets":{},"#,
            r#""rx_drops":{},"tx_errors":{}}}"#,
        ),
        iface.up,
        iface.ip.0[0], iface.ip.0[1], iface.ip.0[2], iface.ip.0[3],
        iface.gateway.0[0], iface.gateway.0[1], iface.gateway.0[2], iface.gateway.0[3],
        iface.dns.0[0], iface.dns.0[1], iface.dns.0[2], iface.dns.0[3],
        net_stats.rx_bytes, net_stats.tx_bytes,
        net_stats.rx_packets, net_stats.tx_packets,
        net_stats.rx_drops, net_stats.tx_errors,
    ));

    json.push_str(",\"tcp_connections\":[");
    for (i, conn) in tcp_conns.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"local_port":{},"remote_ip":"{}","remote_port":{},"state":"{}"}}"#,
            conn.local_port,
            conn.remote_ip, // IpAddr implements Display
            conn.remote_port,
            tcp_state_str(conn.state),
        ));
    }
    json.push_str("]}");

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/memory
// ---------------------------------------------------------------------------

fn api_memory() -> Vec<u8> {
    let (total_frames, free_frames) = crate::mm::frame::stats()
        .map(|s| (s.total_frames, s.free_frames))
        .unwrap_or((0, 0));
    let used_frames = total_frames.saturating_sub(free_frames);
    let page_size = 16384u64;
    let total_mem = (total_frames as u64).saturating_mul(page_size);
    let used_mem = (used_frames as u64).saturating_mul(page_size);

    let heap = crate::mm::heap::stats();

    let json = format!(
        concat!(
            r#"{{"physical":{{"total_bytes":{},"used_bytes":{},"free_bytes":{},"#,
            r#""total_frames":{},"used_frames":{},"page_size":{}}},"#,
            r#""heap":{{"bytes_in_use":{},"peak_bytes_in_use":{},"#,
            r#""slab_allocs":{},"large_allocs":{},"alloc_failures":{}}}}}"#,
        ),
        total_mem, used_mem, total_mem.saturating_sub(used_mem),
        total_frames, used_frames, page_size,
        heap.bytes_in_use, heap.peak_bytes_in_use,
        heap.slab_allocs, heap.large_allocs, heap.alloc_failures,
    );

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/httpd
// ---------------------------------------------------------------------------

fn api_httpd() -> Vec<u8> {
    use super::httpd;

    let running = httpd::is_running();
    let port = httpd::port();
    let tls_running = httpd::is_tls_running();
    let tls_port = httpd::tls_port();
    let requests = httpd::request_count();
    let not_modified = httpd::not_modified_count();
    let partial = httpd::partial_count();
    let rate_limited = httpd::rate_limited_count();
    let rl_enabled = httpd::rate_limit_enabled();
    let gzip_compressed = httpd::gzip_count();
    let gzip_saved = httpd::gzip_bytes_saved();

    let mut json = format!(
        concat!(
            r#"{{"server":{{"http_running":{},"http_port":{},"#,
            r#""tls_running":{},"tls_port":{}}},"#,
            r#""stats":{{"requests":{},"not_modified_304":{},"partial_206":{},"#,
            r#""rate_limited_429":{},"gzip_compressed":{},"gzip_bytes_saved":{}}},"#,
            r#""rate_limit":{{"enabled":{}}},"#,
            r#""access_log":["#,
        ),
        running, port,
        tls_running, tls_port,
        requests, not_modified, partial,
        rate_limited, gzip_compressed, gzip_saved,
        rl_enabled,
    );

    let entries = httpd::recent_access_log(20);
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"method":"{}","path":"{}","status":{},"body_size":{},"duration_us":{}}}"#,
            json_escape(&e.method),
            json_escape(&e.path),
            e.status,
            e.body_size,
            e.duration_us,
        ));
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/dns
// ---------------------------------------------------------------------------

fn api_dns() -> Vec<u8> {
    let stats = super::dns::cache_stats();

    let json = format!(
        concat!(
            r#"{{"cache":{{"hits":{},"misses":{},"evictions":{},"#,
            r#""entries":{},"capacity":{}}}}}"#,
        ),
        stats.hits, stats.misses, stats.evictions,
        stats.entries, stats.capacity,
    );

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/firewall
// ---------------------------------------------------------------------------

fn api_firewall() -> Vec<u8> {
    let enabled = super::firewall::is_enabled();
    let policy = super::firewall::default_policy();
    let conntrack = super::firewall::conntrack_count();

    let policy_str = match policy {
        super::firewall::DefaultPolicy::Accept => "accept",
        super::firewall::DefaultPolicy::Drop => "drop",
    };

    let (rules, rule_count) = super::firewall::rule_stats();

    let mut json = format!(
        r#"{{"enabled":{},"default_policy":"{}","conntrack_entries":{},"rules":["#,
        enabled,
        policy_str,
        conntrack,
    );

    for i in 0..rule_count {
        if i > 0 {
            json.push(',');
        }
        let r = &rules[i];
        let src = core::str::from_utf8(r.source.get(..r.source_len as usize).unwrap_or(&[]))
            .unwrap_or("?");
        json.push_str(&format!(
            r#"{{"priority":{},"protocol":"{}","action":"{}","direction":"{}","dst_port":{},"source":"{}","matches":{}}}"#,
            r.priority,
            r.protocol,
            r.action,
            r.direction,
            r.dst_port,
            json_escape(src),
            r.matches,
        ));
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/bench
// ---------------------------------------------------------------------------

fn api_bench() -> Vec<u8> {
    let entries = crate::bench::scorecard_snapshot();

    let total = entries.len();
    let passed = entries.iter().filter(|e| e.passed).count();
    let failed = total.saturating_sub(passed);

    let mut json = format!(
        r#"{{"summary":{{"total":{},"passed":{},"failed":{}}},"entries":["#,
        total, passed, failed,
    );

    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"name":"{}","measured_ns":{},"target_ns":{},"passed":{}}}"#,
            json_escape(e.name),
            e.measured_ns,
            e.target_ns,
            e.passed,
        ));
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/ipv6
// ---------------------------------------------------------------------------

fn api_ipv6() -> Vec<u8> {
    use crate::net::icmpv6;
    use crate::net::dhcpv6;
    use crate::net::ipv6::Ipv6Addr;

    let link_local = {
        let iface = crate::net::interface::info();
        if iface.up {
            Some(Ipv6Addr::from_mac_link_local(&iface.mac))
        } else {
            None
        }
    };

    let ra_received = icmpv6::ra_received();
    let global_addr = icmpv6::slaac_global_addr();
    let (slaac_addrs, slaac_count) = icmpv6::slaac_addresses();
    let rdnss = icmpv6::slaac_rdnss();
    let router = if ra_received { Some(icmpv6::slaac_router()) } else { None };

    let dhcpv6 = dhcpv6::stats();

    let mut json = String::from(r#"{"link_local":"#);
    if let Some(ll) = link_local {
        json.push_str(&format!(r#""{}""#, ll));
    } else {
        json.push_str("null");
    }

    json.push_str(r#","slaac":{"ra_received":"#);
    json.push_str(if ra_received { "true" } else { "false" });

    json.push_str(r#","addresses":["#);
    for i in 0..slaac_count {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"addr":"{}","prefix_len":{}}}"#,
            slaac_addrs[i].0,
            slaac_addrs[i].1,
        ));
    }
    json.push(']');

    if let Some(r) = router {
        json.push_str(&format!(r#","router":"{}""#, r));
    }
    if let Some(d) = rdnss {
        json.push_str(&format!(r#","rdnss":"{}""#, d));
    }
    json.push('}');

    // DHCPv6 section.
    json.push_str(&format!(
        concat!(
            r#","dhcpv6":{{"state":"{}","has_address":{},"#,
            r#""solicits_sent":{},"requests_sent":{},"info_requests":{},"#,
            r#""replies":{},"errors":{}"#,
        ),
        json_escape(dhcpv6.state),
        dhcpv6.has_address,
        dhcpv6.solicits_sent,
        dhcpv6.requests_sent,
        dhcpv6.info_requests_sent,
        dhcpv6.replies_received,
        dhcpv6.errors,
    ));
    if let Some(addr) = dhcpv6.address {
        json.push_str(&format!(r#","address":"{}""#, addr));
    }
    if let Some(dns) = dhcpv6.dns_server {
        json.push_str(&format!(r#","dns":"{}""#, dns));
    }
    json.push_str("}}");

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/containers
// ---------------------------------------------------------------------------

fn api_containers() -> Vec<u8> {
    // The container subsystem may not be initialized during early boot.
    // Check safely by trying to lock the table and inspecting the Option.
    if !crate::container::is_initialized() {
        return br#"{"active_count":0,"containers":[]}"#.to_vec();
    }

    let containers = crate::container::list();
    let count = crate::container::active_count();

    let mut json = format!(r#"{{"active_count":{},"containers":["#, count);

    for (i, (id, name, state)) in containers.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let state_str = match state {
            crate::container::ContainerState::Created => "created",
            crate::container::ContainerState::Running => "running",
            crate::container::ContainerState::Stopped => "stopped",
            crate::container::ContainerState::Failed => "failed",
        };

        json.push_str(&format!(
            r#"{{"id":{},"name":"{}","state":"{}""#,
            id,
            json_escape(name),
            state_str,
        ));

        // Enrich with full info if available.
        if let Some(info) = crate::container::info(*id) {
            json.push_str(&format!(
                r#","pid_ns":{},"net_ns":{},"cgroup_id":{},"nr_procs":{}"#,
                info.pid_ns,
                info.net_ns,
                info.cgroup_id,
                info.nr_procs,
            ));
        }

        json.push('}');
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/tcp — detailed TCP connection and listener info
// ---------------------------------------------------------------------------

fn api_tcp() -> Vec<u8> {
    let tcp_stats = super::tcp::stats();
    let tcp_conns = super::tcp::all_connections();
    let (listeners, listener_count) = super::tcp::all_listeners();

    let mut json = String::from("{\"stats\":");
    json.push_str(&format!(
        concat!(
            r#"{{"active":{},"established":{},"syn_sent":{},"#,
            r#""time_wait":{},"close_wait":{},"listeners":{},"#,
            r#""rx_bytes":{},"tx_bytes":{}}}"#,
        ),
        tcp_stats.active_connections, tcp_stats.established,
        tcp_stats.syn_sent, tcp_stats.time_wait, tcp_stats.close_wait,
        tcp_stats.listeners, tcp_stats.total_rx_bytes, tcp_stats.total_tx_bytes,
    ));

    json.push_str(",\"connections\":[");
    for (i, conn) in tcp_conns.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            concat!(
                r#"{{"local_port":{},"remote_ip":"{}","remote_port":{},"#,
                r#""state":"{}","ns_id":{},"srtt_us":{},"rto_ms":{},"#,
                r#""cwnd":{},"ssthresh":{},"snd_wnd":{},"eff_mss":{},"#,
                r#""rx_buffered":{},"tx_buffered":{},"#,
                r#""ecn":{},"sack":{},"wscale":{},"ts":{},"#,
                r#""keepalive":{},"nagle":{}}}"#,
            ),
            conn.local_port,
            conn.remote_ip,
            conn.remote_port,
            tcp_state_str(conn.state),
            conn.ns_id,
            conn.srtt_ns / 1000, // convert to microseconds for readability
            conn.rto_ns / 1_000_000, // convert to milliseconds
            conn.cwnd, conn.ssthresh, conn.snd_wnd, conn.eff_mss,
            conn.rx_buffered, conn.tx_buffered,
            conn.ecn_ok, conn.sack_ok, conn.wscale_ok, conn.ts_ok,
            conn.keepalive, conn.nagle,
        ));
    }

    json.push_str("],\"listeners\":[");
    for i in 0..listener_count {
        if i > 0 {
            json.push(',');
        }
        if let Some(l) = listeners.get(i) {
            json.push_str(&format!(
                r#"{{"port":{},"backlog_used":{},"backlog_max":{}}}"#,
                l.port, l.backlog_used, l.backlog_max,
            ));
        }
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/scheduler — detailed scheduler stats and per-CPU data
// ---------------------------------------------------------------------------

fn api_scheduler() -> Vec<u8> {
    use core::fmt::Write;

    let stats = crate::sched::sched_stats();
    let num_cpus = stats.num_cpus;

    let mut json = String::with_capacity(1024);
    json.push_str(&format!(
        concat!(
            r#"{{"num_cpus":{},"total_ctx_switches":{},"#,
            r#""total_work_steals":{},"tasks_spawned":{},"#,
            r#""tasks_exited":{},"load_avg_x100":{}"#,
        ),
        num_cpus, stats.total_ctx_switches,
        stats.total_work_steals, stats.total_tasks_spawned,
        stats.total_tasks_exited, stats.load_avg_x100,
    ));

    json.push_str(",\"cpus\":[");
    for cpu in 0..num_cpus {
        if cpu > 0 {
            json.push(',');
        }
        let (total, idle) = stats.cpu_ticks.get(cpu).copied().unwrap_or((0, 0));
        let ctx = stats.ctx_switches.get(cpu).copied().unwrap_or(0);
        let vol = stats.voluntary_switches.get(cpu).copied().unwrap_or(0);
        let pre = stats.preemptions.get(cpu).copied().unwrap_or(0);
        // Compute utilization percentage (0-100) from total vs idle ticks.
        let util_pct = if total > 0 {
            total.saturating_sub(idle).saturating_mul(100) / total
        } else {
            0
        };
        let _ = write!(json,
            concat!(
                r#"{{"cpu":{},"total_ticks":{},"idle_ticks":{},"#,
                r#""utilization_pct":{},"ctx_switches":{},"#,
                r#""voluntary":{},"preemptions":{}}}"#,
            ),
            cpu, total, idle, util_pct, ctx, vol, pre,
        );
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/swap — swap/zram device info and compression stats
// ---------------------------------------------------------------------------

fn api_swap() -> Vec<u8> {
    use crate::mm::swap;

    let available = swap::is_available();
    let free = swap::free_slots();
    let used = swap::used_slots();
    let reclaimable = swap::reclaimable_count();
    let (total_bytes, used_bytes, _device_count) = swap::summary();
    let compression = swap::compression_stats();
    let devices = swap::list_devices();

    let ratio = compression.ratio_percent();
    let saved = compression.bytes_saved();

    let mut json = format!(
        concat!(
            r#"{{"available":{},"total_bytes":{},"used_bytes":{},"#,
            r#""free_slots":{},"used_slots":{},"reclaimable_pages":{},"#,
            r#""compression":{{"compressed_bytes":{},"uncompressed_bytes":{},"#,
            r#""compressed_pages":{},"uncompressed_pages":{},"#,
            r#""ratio_pct":{},"bytes_saved":{}}}"#,
        ),
        available, total_bytes, used_bytes,
        free, used, reclaimable,
        compression.compressed_bytes, compression.uncompressed_bytes,
        compression.compressed_count, compression.uncompressed_count,
        ratio, saved,
    );

    json.push_str(",\"devices\":[");
    for (i, dev) in devices.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str(&format!(
            r#"{{"name":"{}","type":"{}","total_slots":{},"used_slots":{},"priority":{}}}"#,
            json_escape(&dev.name),
            dev.device_type,
            dev.total_slots,
            dev.used_slots,
            dev.priority,
        ));
    }

    json.push_str("]}");
    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/fs
// ---------------------------------------------------------------------------

/// Filesystem API endpoint: mount table and block cache statistics.
///
/// Returns JSON with:
/// - `mounts`: array of mounted filesystems with path, type, and options
/// - `cache`: block cache statistics (hits, misses, reads, writes, etc.)
fn api_fs() -> Vec<u8> {
    use core::fmt::Write;

    let mounts = crate::fs::vfs::Vfs::mounts_full();
    let cache = crate::fs::cache::stats();

    let mut json = String::with_capacity(512);
    json.push_str(r#"{"mounts":["#);

    for (i, (path, fs_type, opts)) in mounts.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(
            json,
            concat!(
                r#"{{"path":"{}","fs_type":"{}","read_only":{},"#,
                r#""noatime":{},"noexec":{},"nosuid":{}}}"#,
            ),
            json_escape(path),
            json_escape(fs_type),
            opts.read_only,
            opts.noatime,
            opts.noexec,
            opts.nosuid,
        );
    }

    let hit_rate_pct = if cache.hits.saturating_add(cache.misses) > 0 {
        cache.hits.saturating_mul(100) / cache.hits.saturating_add(cache.misses)
    } else {
        0
    };

    let _ = write!(
        json,
        concat!(
            r#"],"cache":{{"reads":{},"hits":{},"misses":{},"#,
            r#""writes":{},"writebacks":{},"readaheads":{},"#,
            r#""expired_flushes":{},"entries_used":{},"#,
            r#""entries_dirty":{},"capacity":{},"hit_rate_pct":{}}}}}"#,
        ),
        cache.reads,
        cache.hits,
        cache.misses,
        cache.writes,
        cache.writebacks,
        cache.readaheads,
        cache.expired_flushes,
        cache.entries_used,
        cache.entries_dirty,
        cache.capacity,
        hit_rate_pct,
    );

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /api/health
// ---------------------------------------------------------------------------

/// Aggregated health check for external monitoring tools.
///
/// Returns a single JSON object with an overall `status` field
/// ("ok", "degraded", or "critical") and individual check results.
/// Designed to be polled by uptime monitors and orchestration systems.
fn api_health() -> Vec<u8> {
    // Memory health: critical if >95% used, degraded if >85%.
    let (total_frames, free_frames) = crate::mm::frame::stats()
        .map(|s| (s.total_frames, s.free_frames))
        .unwrap_or((0, 0));
    let used_frames = total_frames.saturating_sub(free_frames);
    let mem_pct = if total_frames > 0 {
        used_frames.saturating_mul(100) / total_frames
    } else {
        0
    };
    let mem_status = if mem_pct > 95 { "critical" }
        else if mem_pct > 85 { "degraded" }
        else { "ok" };

    // Network health: check interface is up and has an IP.
    let iface = crate::net::interface::info();
    let net_up = iface.up && iface.ip.0 != [0, 0, 0, 0];
    let net_status = if net_up { "ok" } else { "degraded" };

    // HTTP server health.
    let httpd_running = super::httpd::is_running();
    let httpd_status = if httpd_running { "ok" } else { "degraded" };

    // Task count (sanity — zero tasks is impossible during normal operation).
    let task_count = crate::sched::task_list().len();
    let tasks_status = if task_count > 0 { "ok" } else { "critical" };

    // Filesystem health: check block cache dirty ratio.
    let bcache = crate::fs::cache::stats();
    let fs_dirty_pct = if bcache.capacity > 0 {
        bcache.entries_dirty.saturating_mul(100) / bcache.capacity
    } else {
        0
    };
    let fs_status = if fs_dirty_pct > 90 { "critical" }
        else if fs_dirty_pct > 70 { "degraded" }
        else { "ok" };

    // Uptime (seconds).
    let uptime_secs = crate::hrtimer::now_ns() / 1_000_000_000;

    // Overall status: worst of all individual checks.
    let overall = if mem_status == "critical" || tasks_status == "critical"
                  || fs_status == "critical"
    {
        "critical"
    } else if mem_status == "degraded" || net_status == "degraded"
           || httpd_status == "degraded" || fs_status == "degraded"
    {
        "degraded"
    } else {
        "ok"
    };

    let json = format!(
        concat!(
            r#"{{"status":"{}","uptime_secs":{},"checks":{{"#,
            r#""memory":{{"status":"{}","used_pct":{}}},"#,
            r#""network":{{"status":"{}","up":{}}},"#,
            r#""httpd":{{"status":"{}","running":{}}},"#,
            r#""tasks":{{"status":"{}","count":{}}},"#,
            r#""filesystem":{{"status":"{}","dirty_pct":{}}}}}}}"#,
        ),
        overall, uptime_secs,
        mem_status, mem_pct,
        net_status, net_up,
        httpd_status, httpd_running,
        tasks_status, task_count,
        fs_status, fs_dirty_pct,
    );

    json.into_bytes()
}

// ---------------------------------------------------------------------------
// /metrics (Prometheus text format)
// ---------------------------------------------------------------------------

/// Prometheus-compatible metrics endpoint (text/plain, version 0.0.4).
///
/// Exports key system metrics in Prometheus exposition format for
/// integration with monitoring stacks (Prometheus, Grafana, etc.).
/// Currently exposes ~50 metrics covering system, memory, heap, tasks,
/// network, TCP, HTTP, DNS, swap/zram, scheduler, firewall, containers,
/// block cache, and per-CPU utilization.
fn api_metrics() -> Vec<u8> {
    use super::httpd;
    use core::fmt::Write;

    // Pre-allocate generously — avoids multiple reallocs for ~40 metrics.
    let mut t = String::with_capacity(6144);

    // -- Uptime ---------------------------------------------------------------
    let uptime_secs = crate::hrtimer::now_ns() / 1_000_000_000;
    prom_gauge(&mut t, "os_uptime_seconds",
        "System uptime in seconds.", uptime_secs);

    // -- Physical memory ------------------------------------------------------
    let (total_frames, free_frames) = crate::mm::frame::stats()
        .map(|s| (s.total_frames, s.free_frames))
        .unwrap_or((0, 0));
    let used_frames = total_frames.saturating_sub(free_frames);
    let page_size = 16384u64;
    let total_mem = (total_frames as u64).saturating_mul(page_size);
    let used_mem = (used_frames as u64).saturating_mul(page_size);

    prom_gauge(&mut t, "os_memory_total_bytes",
        "Total physical memory in bytes.", total_mem);
    prom_gauge(&mut t, "os_memory_used_bytes",
        "Used physical memory in bytes.", used_mem);
    prom_gauge(&mut t, "os_memory_frames_total",
        "Total physical frames.", total_frames as u64);
    prom_gauge(&mut t, "os_memory_frames_used",
        "Used physical frames.", used_frames as u64);

    // -- Kernel heap ----------------------------------------------------------
    let heap = crate::mm::heap::stats();
    prom_gauge(&mut t, "os_heap_bytes_in_use",
        "Kernel heap bytes in use.", heap.bytes_in_use);
    prom_gauge(&mut t, "os_heap_peak_bytes",
        "Peak kernel heap usage.", heap.peak_bytes_in_use);

    // -- Tasks ----------------------------------------------------------------
    let task_count = crate::sched::task_list().len() as u64;
    prom_gauge(&mut t, "os_tasks_total", "Active task count.", task_count);

    // -- Network interface (L2) -----------------------------------------------
    let net_stats = crate::net::interface::stats();
    prom_counter(&mut t, "os_net_rx_bytes_total",
        "Network bytes received.", net_stats.rx_bytes);
    prom_counter(&mut t, "os_net_tx_bytes_total",
        "Network bytes transmitted.", net_stats.tx_bytes);
    prom_counter(&mut t, "os_net_rx_packets_total",
        "Network packets received.", net_stats.rx_packets);
    prom_counter(&mut t, "os_net_tx_packets_total",
        "Network packets transmitted.", net_stats.tx_packets);

    // -- TCP ------------------------------------------------------------------
    let tcp = super::tcp::stats();
    prom_gauge(&mut t, "os_tcp_connections_active",
        "Active TCP connections (any state except Closed).",
        tcp.active_connections as u64);
    prom_gauge(&mut t, "os_tcp_connections_established",
        "TCP connections in ESTABLISHED state.",
        tcp.established as u64);
    prom_gauge(&mut t, "os_tcp_connections_syn_sent",
        "TCP connections in SYN_SENT state.",
        tcp.syn_sent as u64);
    prom_gauge(&mut t, "os_tcp_connections_time_wait",
        "TCP connections in TIME_WAIT state.",
        tcp.time_wait as u64);
    prom_gauge(&mut t, "os_tcp_connections_close_wait",
        "TCP connections in CLOSE_WAIT state.",
        tcp.close_wait as u64);
    prom_gauge(&mut t, "os_tcp_listeners",
        "Active TCP listeners.", tcp.listeners as u64);
    prom_counter(&mut t, "os_tcp_rx_bytes_total",
        "TCP receive buffer bytes across all connections.",
        tcp.total_rx_bytes as u64);
    prom_counter(&mut t, "os_tcp_tx_bytes_total",
        "TCP transmit buffer bytes across all connections.",
        tcp.total_tx_bytes as u64);

    // -- HTTP -----------------------------------------------------------------
    prom_counter(&mut t, "os_http_requests_total",
        "HTTP requests served.", httpd::request_count());
    prom_counter(&mut t, "os_http_304_total",
        "HTTP 304 Not Modified responses.", httpd::not_modified_count());
    prom_counter(&mut t, "os_http_206_total",
        "HTTP 206 Partial Content responses.", httpd::partial_count());
    prom_counter(&mut t, "os_http_429_total",
        "HTTP 429 Rate Limited responses.", httpd::rate_limited_count());
    prom_counter(&mut t, "os_http_gzip_total",
        "Gzip-compressed responses served.", httpd::gzip_count());
    prom_counter(&mut t, "os_http_gzip_bytes_saved_total",
        "Bytes saved by gzip compression.", httpd::gzip_bytes_saved());

    // -- DNS ------------------------------------------------------------------
    let dns = super::dns::cache_stats();
    prom_counter(&mut t, "os_dns_cache_hits_total",
        "DNS cache hits.", dns.hits);
    prom_counter(&mut t, "os_dns_cache_misses_total",
        "DNS cache misses.", dns.misses);
    prom_gauge(&mut t, "os_dns_cache_entries",
        "Current DNS cache entries.", dns.entries);

    // -- Swap / zram ----------------------------------------------------------
    let swap = crate::mm::swap::compression_stats();
    prom_gauge(&mut t, "os_swap_compressed_bytes",
        "Compressed size of swapped pages (actual storage).",
        swap.compressed_bytes);
    prom_gauge(&mut t, "os_swap_uncompressed_bytes",
        "Logical (uncompressed) size of swapped pages.",
        swap.uncompressed_bytes);
    prom_gauge(&mut t, "os_swap_compressed_pages",
        "Number of pages stored with compression.",
        swap.compressed_count);
    prom_gauge(&mut t, "os_swap_uncompressed_pages",
        "Number of pages stored uncompressed (incompressible).",
        swap.uncompressed_count);

    // -- Scheduler ------------------------------------------------------------
    let sched = crate::sched::sched_stats();
    prom_counter(&mut t, "os_sched_context_switches_total",
        "Total context switches across all CPUs.",
        sched.total_ctx_switches);
    prom_counter(&mut t, "os_sched_work_steals_total",
        "Total work-stealing operations.", sched.total_work_steals);
    prom_counter(&mut t, "os_sched_tasks_spawned_total",
        "Total tasks spawned since boot.", sched.total_tasks_spawned);
    prom_counter(&mut t, "os_sched_tasks_exited_total",
        "Total tasks exited since boot.", sched.total_tasks_exited);
    // Load average is stored ×100 (e.g. 150 = load 1.50).
    // Emit as integer ×100 — Prometheus can divide in queries.
    prom_gauge(&mut t, "os_sched_load_avg_x100",
        "System load average times 100 (150 = 1.50).",
        sched.load_avg_x100);

    // -- Per-CPU utilization --------------------------------------------------
    // Emit (total_ticks, idle_ticks) per online CPU as labeled counters.
    // Prometheus consumers compute utilization as:
    //   1 - rate(os_cpu_idle_ticks[5m]) / rate(os_cpu_total_ticks[5m])
    let _ = write!(t,
        "# HELP os_cpu_total_ticks Total scheduler ticks per CPU.\n\
         # TYPE os_cpu_total_ticks counter\n");
    for cpu in 0..sched.num_cpus {
        if let Some(&(total, _idle)) = sched.cpu_ticks.get(cpu) {
            let _ = write!(t, "os_cpu_total_ticks{{cpu=\"{}\"}} {}\n", cpu, total);
        }
    }
    let _ = write!(t,
        "# HELP os_cpu_idle_ticks Idle scheduler ticks per CPU.\n\
         # TYPE os_cpu_idle_ticks counter\n");
    for cpu in 0..sched.num_cpus {
        if let Some(&(_total, idle)) = sched.cpu_ticks.get(cpu) {
            let _ = write!(t, "os_cpu_idle_ticks{{cpu=\"{}\"}} {}\n", cpu, idle);
        }
    }

    // -- Firewall -------------------------------------------------------------
    prom_gauge(&mut t, "os_firewall_conntrack_entries",
        "Active firewall connection tracking entries.",
        super::firewall::conntrack_count() as u64);

    // -- Containers -----------------------------------------------------------
    let ct_active = if crate::container::is_initialized() {
        crate::container::active_count() as u64
    } else {
        0
    };
    prom_gauge(&mut t, "os_containers_active",
        "Active container count.", ct_active);

    // -- Block cache -----------------------------------------------------------
    let bcache = crate::fs::cache::stats();
    prom_counter(&mut t, "os_bcache_reads_total",
        "Block cache read requests.", bcache.reads);
    prom_counter(&mut t, "os_bcache_hits_total",
        "Block cache hits.", bcache.hits);
    prom_counter(&mut t, "os_bcache_misses_total",
        "Block cache misses.", bcache.misses);
    prom_counter(&mut t, "os_bcache_writes_total",
        "Block cache write requests.", bcache.writes);
    prom_counter(&mut t, "os_bcache_writebacks_total",
        "Block cache dirty writebacks.", bcache.writebacks);
    prom_gauge(&mut t, "os_bcache_entries_used",
        "Block cache entries in use.", bcache.entries_used);
    prom_gauge(&mut t, "os_bcache_entries_dirty",
        "Block cache dirty entries.", bcache.entries_dirty);
    prom_gauge(&mut t, "os_bcache_capacity",
        "Block cache capacity.", bcache.capacity);

    t.into_bytes()
}

// ---------------------------------------------------------------------------
// Prometheus text-format helpers
// ---------------------------------------------------------------------------

/// Emit a gauge metric (HELP + TYPE + value) into the buffer.
fn prom_gauge(buf: &mut String, name: &str, help: &str, value: impl core::fmt::Display) {
    use core::fmt::Write;
    let _ = write!(buf,
        "# HELP {n} {h}\n# TYPE {n} gauge\n{n} {v}\n",
        n = name, h = help, v = value,
    );
}

/// Emit a counter metric (HELP + TYPE + value) into the buffer.
fn prom_counter(buf: &mut String, name: &str, help: &str, value: impl core::fmt::Display) {
    use core::fmt::Write;
    let _ = write!(buf,
        "# HELP {n} {h}\n# TYPE {n} counter\n{n} {v}\n",
        n = name, h = help, v = value,
    );
}

// ---------------------------------------------------------------------------
// HTML dashboard
// ---------------------------------------------------------------------------

fn dashboard_html() -> Vec<u8> {
    let html = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>System Dashboard</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
       background: #0d1117; color: #c9d1d9; padding: 20px; }
h1 { color: #58a6ff; margin-bottom: 20px; font-size: 24px; }
h2 { color: #8b949e; margin-bottom: 10px; font-size: 16px; text-transform: uppercase;
     letter-spacing: 0.5px; }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
        gap: 16px; margin-bottom: 20px; }
.card { background: #161b22; border: 1px solid #30363d; border-radius: 8px;
        padding: 16px; }
.stat { display: flex; justify-content: space-between; padding: 6px 0;
        border-bottom: 1px solid #21262d; }
.stat:last-child { border-bottom: none; }
.stat-label { color: #8b949e; }
.stat-value { color: #f0f6fc; font-weight: 600; font-variant-numeric: tabular-nums; }
.stat-value.ok { color: #3fb950; }
.stat-value.warn { color: #d29922; }
.bar { height: 8px; background: #21262d; border-radius: 4px; margin-top: 8px; }
.bar-fill { height: 100%; border-radius: 4px; transition: width 0.5s; }
.bar-fill.mem { background: #1f6feb; }
.bar-fill.warn { background: #d29922; }
.bar-fill.crit { background: #f85149; }
table { width: 100%; border-collapse: collapse; font-size: 13px; }
th { text-align: left; color: #8b949e; padding: 8px 6px; border-bottom: 1px solid #30363d;
     font-weight: 500; }
td { padding: 6px; border-bottom: 1px solid #21262d; font-variant-numeric: tabular-nums; }
tr:hover td { background: #1c2128; }
.refresh { color: #484f58; font-size: 12px; float: right; }
.badge { display: inline-block; padding: 2px 8px; border-radius: 12px; font-size: 11px;
         font-weight: 600; }
.badge-run { background: #0d3117; color: #3fb950; }
.badge-idle { background: #1c1e23; color: #8b949e; }
.badge-blk { background: #341a10; color: #d29922; }
</style>
</head>
<body>
<h1>System Dashboard <span class="refresh" id="refresh">updating...</span></h1>

<div class="grid">
  <div class="card" id="overview">
    <h2>System</h2>
    <div id="sys-stats"></div>
  </div>
  <div class="card" id="mem-card">
    <h2>Memory</h2>
    <div id="mem-stats"></div>
  </div>
  <div class="card" id="net-card">
    <h2>Network</h2>
    <div id="net-stats"></div>
  </div>
</div>

<div class="card" style="margin-bottom:16px">
  <h2>Tasks</h2>
  <table>
    <thead><tr><th>ID</th><th>Name</th><th>Pri</th><th>State</th><th>CPU</th><th>Ticks</th><th>Sched</th></tr></thead>
    <tbody id="task-body"></tbody>
  </table>
</div>

<div class="card">
  <h2>TCP Connections</h2>
  <table>
    <thead><tr><th>Local Port</th><th>Remote</th><th>State</th></tr></thead>
    <tbody id="tcp-body"></tbody>
  </table>
</div>

<div class="grid" style="margin-top:16px">
  <div class="card">
    <h2>HTTP Server</h2>
    <div id="httpd-stats"></div>
  </div>
  <div class="card">
    <h2>DNS Cache</h2>
    <div id="dns-stats"></div>
  </div>
  <div class="card">
    <h2>Firewall</h2>
    <div id="fw-stats"></div>
  </div>
</div>

<div class="grid" style="margin-top:16px">
  <div class="card">
    <h2>IPv6</h2>
    <div id="ipv6-stats"></div>
  </div>
  <div class="card">
    <h2>Containers</h2>
    <div id="ct-stats"></div>
  </div>
  <div class="card">
    <h2>Swap / zram</h2>
    <div id="swap-stats"></div>
  </div>
  <div class="card">
    <h2>Filesystem</h2>
    <div id="fs-stats"></div>
  </div>
</div>

<div class="grid" style="margin-top:16px">
  <div class="card">
    <h2>TCP Stack</h2>
    <div id="tcp-stats"></div>
  </div>
  <div class="card">
    <h2>Scheduler</h2>
    <div id="sched-stats"></div>
  </div>
</div>

<div class="card" style="margin-top:16px;margin-bottom:16px">
  <h2>TCP Listeners</h2>
  <table>
    <thead><tr><th>Port</th><th>Backlog</th><th>Capacity</th></tr></thead>
    <tbody id="listener-body"></tbody>
  </table>
</div>

<div class="card" style="margin-bottom:16px">
  <h2>Mount Table</h2>
  <table>
    <thead><tr><th>Path</th><th>Type</th><th>Options</th></tr></thead>
    <tbody id="mount-body"></tbody>
  </table>
</div>

<div class="card" style="margin-bottom:16px">
  <h2>Benchmarks <span id="bench-summary" style="font-size:12px;color:#8b949e;text-transform:none;letter-spacing:0"></span></h2>
  <table>
    <thead><tr><th>Benchmark</th><th>Measured</th><th>Target</th><th>Result</th></tr></thead>
    <tbody id="bench-body"></tbody>
  </table>
</div>

<div class="card">
  <h2>Recent HTTP Requests</h2>
  <table>
    <thead><tr><th>Method</th><th>Path</th><th>Status</th><th>Size</th><th>Time</th></tr></thead>
    <tbody id="httpd-log"></tbody>
  </table>
</div>

<script>
function fmt(b) {
  if (b >= 1073741824) return (b/1073741824).toFixed(1)+' GiB';
  if (b >= 1048576) return (b/1048576).toFixed(1)+' MiB';
  if (b >= 1024) return (b/1024).toFixed(1)+' KiB';
  return b+' B';
}
function uptimeFmt(s) {
  var d=Math.floor(s/86400), h=Math.floor(s%86400/3600), m=Math.floor(s%3600/60);
  return (d>0?d+'d ':'')+(h>0?h+'h ':'')+(m>0?m+'m ':'')+(s%60)+'s';
}
function stat(label, value, cls) {
  return '<div class="stat"><span class="stat-label">'+label+'</span>'+
    '<span class="stat-value'+(cls?' '+cls:'')+'">'+value+'</span></div>';
}
function badge(state) {
  var s=state.toLowerCase(), c='badge-idle';
  if(s==='running')c='badge-run'; else if(s.indexOf('block')>=0||s==='waiting')c='badge-blk';
  return '<span class="badge '+c+'">'+state+'</span>';
}
function bar(pct, cls) {
  var c=cls||'mem'; if(pct>90)c='crit'; else if(pct>70)c='warn';
  return '<div class="bar"><div class="bar-fill '+c+'" style="width:'+pct+'%"></div></div>';
}

async function update() {
  try {
    var [sr,tr,nr,mr,hr,dr,fr,br,v6r,ctr,tcpr,schr,swr,fsr] = await Promise.all([
      fetch('/api/status').then(r=>r.json()),
      fetch('/api/tasks').then(r=>r.json()),
      fetch('/api/network').then(r=>r.json()),
      fetch('/api/memory').then(r=>r.json()),
      fetch('/api/httpd').then(r=>r.json()),
      fetch('/api/dns').then(r=>r.json()),
      fetch('/api/firewall').then(r=>r.json()),
      fetch('/api/bench').then(r=>r.json()),
      fetch('/api/ipv6').then(r=>r.json()),
      fetch('/api/containers').then(r=>r.json()),
      fetch('/api/tcp').then(r=>r.json()),
      fetch('/api/scheduler').then(r=>r.json()),
      fetch('/api/swap').then(r=>r.json()),
      fetch('/api/fs').then(r=>r.json()),
    ]);
    var memPct = sr.memory.total_bytes>0 ?
      Math.round(sr.memory.used_bytes*100/sr.memory.total_bytes) : 0;
    document.getElementById('sys-stats').innerHTML =
      stat('Uptime', uptimeFmt(sr.uptime_secs)) +
      stat('Tasks', sr.tasks) +
      stat('IP', sr.network.ip, sr.network.up?'ok':'') +
      stat('MAC', sr.network.mac);
    document.getElementById('mem-stats').innerHTML =
      stat('Used', fmt(sr.memory.used_bytes)+' / '+fmt(sr.memory.total_bytes), memPct>90?'warn':'') +
      stat('Free', fmt(sr.memory.free_bytes)) +
      stat('Frames', sr.memory.used_frames+' / '+sr.memory.total_frames) +
      stat('Heap', fmt(mr.heap.bytes_in_use)+' (peak: '+fmt(mr.heap.peak_bytes_in_use)+')') +
      bar(memPct);
    document.getElementById('net-stats').innerHTML =
      stat('RX', fmt(nr.interface.rx_bytes)+' ('+nr.interface.rx_packets+' pkts)') +
      stat('TX', fmt(nr.interface.tx_bytes)+' ('+nr.interface.tx_packets+' pkts)') +
      stat('Drops', nr.interface.rx_drops+' RX / '+nr.interface.tx_errors+' TX errors',
           (nr.interface.rx_drops+nr.interface.tx_errors)>0?'warn':'') +
      stat('Gateway', nr.interface.gateway) +
      stat('DNS', nr.interface.dns);
    var tb=''; tr.forEach(function(t){
      tb+='<tr><td>'+t.id+'</td><td>'+t.name+'</td><td>'+t.priority+
        '</td><td>'+badge(t.state)+'</td><td>'+t.cpu+'</td><td>'+
        t.total_ticks+'</td><td>'+t.schedule_count+'</td></tr>';
    });
    document.getElementById('task-body').innerHTML=tb;
    var cb=''; nr.tcp_connections.forEach(function(c){
      cb+='<tr><td>'+c.local_port+'</td><td>'+c.remote_ip+':'+c.remote_port+
        '</td><td>'+c.state+'</td></tr>';
    });
    document.getElementById('tcp-body').innerHTML=cb||'<tr><td colspan="3" style="color:#484f58">No active connections</td></tr>';
    document.getElementById('httpd-stats').innerHTML =
      stat('HTTP', hr.server.http_running?'Running (port '+hr.server.http_port+')':'Stopped', hr.server.http_running?'ok':'') +
      stat('HTTPS', hr.server.tls_running?'Running (port '+hr.server.tls_port+')':'Stopped', hr.server.tls_running?'ok':'') +
      stat('Requests', hr.stats.requests) +
      stat('304 Not Modified', hr.stats.not_modified_304, hr.stats.not_modified_304>0?'ok':'') +
      stat('206 Partial', hr.stats.partial_206) +
      stat('429 Rate Limited', hr.stats.rate_limited_429, hr.stats.rate_limited_429>0?'warn':'') +
      stat('Gzip Compressed', hr.stats.gzip_compressed, hr.stats.gzip_compressed>0?'ok':'') +
      stat('Gzip Saved', fmt(hr.stats.gzip_bytes_saved), hr.stats.gzip_bytes_saved>0?'ok':'') +
      stat('Rate Limiting', hr.rate_limit.enabled?'Enabled':'Disabled');
    var lb=''; hr.access_log.slice().reverse().forEach(function(e){
      var sc=e.status>=400?'warn':(e.status===304?'ok':'');
      var dur=e.duration_us<1000?(e.duration_us+'\u00b5s'):(e.duration_us<1000000?((e.duration_us/1000).toFixed(1)+'ms'):((e.duration_us/1000000).toFixed(2)+'s'));
      lb+='<tr><td>'+e.method+'</td><td>'+e.path+'</td><td><span class="stat-value'+(sc?' '+sc:'')+'">'+e.status+'</span></td><td>'+fmt(e.body_size)+'</td><td>'+dur+'</td></tr>';
    });
    document.getElementById('httpd-log').innerHTML=lb||'<tr><td colspan="5" style="color:#484f58">No requests yet</td></tr>';
    var hitRate=dr.cache.hits+dr.cache.misses>0?Math.round(dr.cache.hits*100/(dr.cache.hits+dr.cache.misses))+'%':'n/a';
    document.getElementById('dns-stats').innerHTML =
      stat('Entries', dr.cache.entries+' / '+dr.cache.capacity) +
      stat('Hits', dr.cache.hits, dr.cache.hits>0?'ok':'') +
      stat('Misses', dr.cache.misses) +
      stat('Hit Rate', hitRate, hitRate!=='n/a'?'ok':'') +
      stat('Evictions', dr.cache.evictions, dr.cache.evictions>0?'warn':'');
    document.getElementById('fw-stats').innerHTML =
      stat('Status', fr.enabled?'Enabled':'Disabled', fr.enabled?'ok':'') +
      stat('Default Policy', fr.default_policy) +
      stat('Rules', fr.rules.length) +
      stat('Conntrack', fr.conntrack_entries);
    var v6h = stat('Link-Local', v6r.link_local||'none');
    if(v6r.slaac.ra_received){
      v6r.slaac.addresses.forEach(function(a){v6h+=stat('Global', a.addr+'/'+a.prefix_len, 'ok');});
      if(v6r.slaac.router)v6h+=stat('Router', v6r.slaac.router);
      if(v6r.slaac.rdnss)v6h+=stat('RDNSS', v6r.slaac.rdnss);
    } else {
      v6h+=stat('SLAAC', 'No RA received');
    }
    v6h+=stat('DHCPv6', v6r.dhcpv6.state, v6r.dhcpv6.has_address?'ok':'');
    if(v6r.dhcpv6.address)v6h+=stat('DHCPv6 Addr', v6r.dhcpv6.address, 'ok');
    if(v6r.dhcpv6.dns)v6h+=stat('DHCPv6 DNS', v6r.dhcpv6.dns);
    document.getElementById('ipv6-stats').innerHTML=v6h;
    var cth='';
    if(ctr.active_count===0){cth=stat('Status','No active containers');}
    else{cth=stat('Active',ctr.active_count);
      ctr.containers.forEach(function(c){var sc=c.state==='running'?'ok':(c.state==='failed'?'warn':'');cth+=stat(c.name,c.state+(c.nr_procs!==undefined?' ('+c.nr_procs+' procs)':''),sc);});
    }
    document.getElementById('ct-stats').innerHTML=cth;
    // Swap card.
    var swh='';
    if(!swr.available){swh=stat('Status','Swap not available');}
    else{
      swh=stat('Used', fmt(swr.used_bytes)+' / '+fmt(swr.total_bytes));
      var swPct=swr.total_bytes>0?Math.round(swr.used_bytes*100/swr.total_bytes):0;
      swh+=bar(swPct);
      swh+=stat('Slots', swr.used_slots+' / '+(swr.used_slots+swr.free_slots));
      swh+=stat('Reclaimable', swr.reclaimable_pages+' pages');
      var c=swr.compression;
      if(c.compressed_pages>0||c.uncompressed_pages>0){
        swh+=stat('Compressed', fmt(c.compressed_bytes)+' ('+c.compressed_pages+' pages)');
        swh+=stat('Uncompressed', fmt(c.uncompressed_bytes)+' logical');
        swh+=stat('Ratio', c.ratio_pct+'%', c.ratio_pct<80?'ok':'');
        swh+=stat('Saved', fmt(c.bytes_saved), c.bytes_saved>0?'ok':'');
      }
      swr.devices.forEach(function(d){
        swh+=stat(d.name+' ('+d.type+')', d.used_slots+'/'+d.total_slots+' slots, pri='+d.priority);
      });
    }
    document.getElementById('swap-stats').innerHTML=swh;
    // TCP stats card.
    var ts=tcpr.stats;
    document.getElementById('tcp-stats').innerHTML =
      stat('Active', ts.active, ts.active>0?'ok':'') +
      stat('Established', ts.established, ts.established>0?'ok':'') +
      stat('SYN Sent', ts.syn_sent, ts.syn_sent>0?'warn':'') +
      stat('TIME_WAIT', ts.time_wait) +
      stat('CLOSE_WAIT', ts.close_wait, ts.close_wait>0?'warn':'') +
      stat('Listeners', ts.listeners) +
      stat('RX Buffered', fmt(ts.rx_bytes)) +
      stat('TX Buffered', fmt(ts.tx_bytes));
    // TCP listeners table.
    var lb2=''; tcpr.listeners.forEach(function(l){
      lb2+='<tr><td>'+l.port+'</td><td>'+l.backlog_used+'</td><td>'+l.backlog_max+'</td></tr>';
    });
    document.getElementById('listener-body').innerHTML=lb2||'<tr><td colspan="3" style="color:#484f58">No active listeners</td></tr>';
    // Scheduler card.
    var loadAvg=(schr.load_avg_x100/100).toFixed(2);
    var schh=stat('CPUs', schr.num_cpus) +
      stat('Load Avg', loadAvg, parseFloat(loadAvg)>schr.num_cpus?'warn':'ok') +
      stat('Ctx Switches', schr.total_ctx_switches) +
      stat('Work Steals', schr.total_work_steals) +
      stat('Tasks Spawned', schr.tasks_spawned) +
      stat('Tasks Exited', schr.tasks_exited);
    schr.cpus.forEach(function(c){
      var uc=c.utilization_pct>90?'warn':(c.utilization_pct>0?'ok':'');
      schh+=stat('CPU '+c.cpu, c.utilization_pct+'% ('+c.ctx_switches+' ctx, '+c.preemptions+' preempt)', uc);
    });
    document.getElementById('sched-stats').innerHTML=schh;
    // Filesystem card.
    var fc=fsr.cache;
    var fcHitPct=fc.hits+fc.misses>0?Math.round(fc.hits*100/(fc.hits+fc.misses)):0;
    var fsh=stat('Mounts', fsr.mounts.length) +
      stat('Cache Entries', fc.entries_used+' / '+fc.capacity) +
      stat('Dirty', fc.entries_dirty, fc.entries_dirty>0?'warn':'') +
      stat('Reads', fc.reads) +
      stat('Hit Rate', fcHitPct+'%', fcHitPct>80?'ok':(fc.reads>0?'warn':'')) +
      stat('Writebacks', fc.writebacks) +
      stat('Readaheads', fc.readaheads) +
      stat('Expired Flushes', fc.expired_flushes);
    document.getElementById('fs-stats').innerHTML=fsh;
    // Mount table.
    var mb=''; fsr.mounts.forEach(function(m){
      var o=[];
      if(m.read_only)o.push('ro'); else o.push('rw');
      if(m.noatime)o.push('noatime');
      if(m.noexec)o.push('noexec');
      if(m.nosuid)o.push('nosuid');
      mb+='<tr><td>'+m.path+'</td><td>'+m.fs_type+'</td><td>'+o.join(', ')+'</td></tr>';
    });
    document.getElementById('mount-body').innerHTML=mb||'<tr><td colspan="3" style="color:#484f58">No mounts</td></tr>';
    function nsFmt(ns){if(ns>=1000000)return (ns/1000000).toFixed(1)+'ms';if(ns>=1000)return (ns/1000).toFixed(1)+'us';return ns+'ns';}
    var bs=br.summary;
    document.getElementById('bench-summary').textContent=bs.total>0?
      '('+bs.passed+'/'+bs.total+' passed'+(bs.failed>0?', '+bs.failed+' failed':'')+')'
      :'(no data)';
    var bb=''; br.entries.forEach(function(e){
      var cls=e.passed?'ok':'warn';
      bb+='<tr><td>'+e.name+'</td><td><span class="stat-value">'+nsFmt(e.measured_ns)+
        '</span></td><td>'+nsFmt(e.target_ns)+'</td><td><span class="stat-value '+cls+'">'+
        (e.passed?'PASS':'FAIL')+'</span></td></tr>';
    });
    document.getElementById('bench-body').innerHTML=bb||'<tr><td colspan="4" style="color:#484f58">No benchmark data yet</td></tr>';
    document.getElementById('refresh').textContent='updated '+new Date().toLocaleTimeString();
  } catch(e) {
    document.getElementById('refresh').textContent='error: '+e.message;
  }
}
update(); setInterval(update, 3000);
</script>
</body>
</html>"#;

    Vec::from(html.as_bytes())
}

// ---------------------------------------------------------------------------
// Benchmark helpers (public for bench module)
// ---------------------------------------------------------------------------

/// Generate the /api/status JSON response.  Exposed for benchmarking.
///
/// Measures the cost of collecting system state (uptime, memory, CPU)
/// and serializing it to JSON.
#[inline(never)]
pub fn bench_api_status() -> Vec<u8> {
    api_status()
}

/// Generate the /api/health JSON response.  Exposed for benchmarking.
///
/// Measures the cost of the aggregated health check across all subsystems.
#[inline(never)]
pub fn bench_api_health() -> Vec<u8> {
    api_health()
}

/// Generate the Prometheus /metrics text response.  Exposed for benchmarking.
///
/// Measures the cost of formatting ~40 Prometheus metrics (including
/// per-CPU labeled metrics) with TYPE/HELP annotations.
#[inline(never)]
pub fn bench_api_metrics() -> Vec<u8> {
    api_metrics()
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Dashboard module self-test.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[dashboard] Running self-test...");

    // Test 1: JSON escape.
    {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("he\"lo"), "he\\\"lo");
        assert_eq!(json_escape("line\nnew"), "line\\nnew");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        serial_println!("[dashboard]   JSON escape: OK");
    }

    // Test 2: API status returns valid JSON-ish bytes.
    {
        let status = api_status();
        assert!(!status.is_empty());
        // Should start with '{' and end with '}'.
        assert_eq!(status[0], b'{');
        assert_eq!(status[status.len().saturating_sub(1)], b'}');
        serial_println!("[dashboard]   API status: OK ({} bytes)", status.len());
    }

    // Test 3: API tasks returns valid JSON array.
    {
        let tasks = api_tasks();
        assert!(!tasks.is_empty());
        assert_eq!(tasks[0], b'[');
        assert_eq!(tasks[tasks.len().saturating_sub(1)], b']');
        serial_println!("[dashboard]   API tasks: OK ({} bytes)", tasks.len());
    }

    // Test 4: API network returns valid JSON.
    {
        let net = api_network();
        assert!(!net.is_empty());
        assert_eq!(net[0], b'{');
        assert_eq!(net[net.len().saturating_sub(1)], b'}');
        serial_println!("[dashboard]   API network: OK ({} bytes)", net.len());
    }

    // Test 5: API memory returns valid JSON.
    {
        let mem = api_memory();
        assert!(!mem.is_empty());
        assert_eq!(mem[0], b'{');
        assert_eq!(mem[mem.len().saturating_sub(1)], b'}');
        serial_println!("[dashboard]   API memory: OK ({} bytes)", mem.len());
    }

    // Test 6: Dashboard HTML is non-empty and looks like HTML.
    {
        let html = dashboard_html();
        assert!(html.len() > 100);
        assert!(html.starts_with(b"<!DOCTYPE html>"));
        serial_println!("[dashboard]   Dashboard HTML: OK ({} bytes)", html.len());
    }

    // Test 7: handle_api_request routes correctly.
    {
        assert!(handle_api_request("/dashboard").is_some());
        assert!(handle_api_request("/api/status").is_some());
        assert!(handle_api_request("/api/tasks").is_some());
        assert!(handle_api_request("/api/network").is_some());
        assert!(handle_api_request("/api/memory").is_some());
        assert!(handle_api_request("/api/httpd").is_some());
        assert!(handle_api_request("/api/dns").is_some());
        assert!(handle_api_request("/api/firewall").is_some());
        assert!(handle_api_request("/api/bench").is_some());
        assert!(handle_api_request("/api/health").is_some());
        assert!(handle_api_request("/api/ipv6").is_some());
        assert!(handle_api_request("/api/containers").is_some());
        assert!(handle_api_request("/api/tcp").is_some());
        assert!(handle_api_request("/api/scheduler").is_some());
        assert!(handle_api_request("/api/swap").is_some());
        assert!(handle_api_request("/api/fs").is_some());
        assert!(handle_api_request("/metrics").is_some());
        assert!(handle_api_request("/not-an-api").is_none());
        assert!(handle_api_request("/api/nonexistent").is_none());
        serial_println!("[dashboard]   API routing: OK");
    }

    // Test 8: API httpd returns valid JSON with expected structure.
    {
        let httpd = api_httpd();
        assert!(!httpd.is_empty());
        assert_eq!(httpd[0], b'{');
        assert_eq!(httpd[httpd.len().saturating_sub(1)], b'}');
        let httpd_str = core::str::from_utf8(&httpd).unwrap_or("");
        assert!(httpd_str.contains("\"server\""));
        assert!(httpd_str.contains("\"stats\""));
        assert!(httpd_str.contains("\"access_log\""));
        assert!(httpd_str.contains("\"gzip_compressed\""));
        assert!(httpd_str.contains("\"gzip_bytes_saved\""));
        serial_println!("[dashboard]   API httpd: OK ({} bytes)", httpd.len());
    }

    // Test 9: API dns returns valid JSON with cache stats.
    {
        let dns = api_dns();
        assert!(!dns.is_empty());
        assert_eq!(dns[0], b'{');
        assert_eq!(dns[dns.len().saturating_sub(1)], b'}');
        let dns_str = core::str::from_utf8(&dns).unwrap_or("");
        assert!(dns_str.contains("\"cache\""));
        assert!(dns_str.contains("\"hits\""));
        assert!(dns_str.contains("\"capacity\""));
        serial_println!("[dashboard]   API dns: OK ({} bytes)", dns.len());
    }

    // Test 10: API firewall returns valid JSON.
    {
        let fw = api_firewall();
        assert!(!fw.is_empty());
        assert_eq!(fw[0], b'{');
        assert_eq!(fw[fw.len().saturating_sub(1)], b'}');
        let fw_str = core::str::from_utf8(&fw).unwrap_or("");
        assert!(fw_str.contains("\"enabled\""));
        assert!(fw_str.contains("\"default_policy\""));
        assert!(fw_str.contains("\"rules\""));
        serial_println!("[dashboard]   API firewall: OK ({} bytes)", fw.len());
    }

    // Test 11: API bench returns valid JSON with expected structure.
    {
        let bench = api_bench();
        assert!(!bench.is_empty());
        assert_eq!(bench[0], b'{');
        assert_eq!(bench[bench.len().saturating_sub(1)], b'}');
        let bench_str = core::str::from_utf8(&bench).unwrap_or("");
        assert!(bench_str.contains("\"summary\""));
        assert!(bench_str.contains("\"total\""));
        assert!(bench_str.contains("\"passed\""));
        assert!(bench_str.contains("\"failed\""));
        assert!(bench_str.contains("\"entries\""));
        serial_println!("[dashboard]   API bench: OK ({} bytes)", bench.len());
    }

    // Test 12: API health returns valid JSON with status and checks.
    {
        let health = api_health();
        assert!(!health.is_empty());
        assert_eq!(health[0], b'{');
        assert_eq!(health[health.len().saturating_sub(1)], b'}');
        let health_str = core::str::from_utf8(&health).unwrap_or("");
        assert!(health_str.contains("\"status\""));
        assert!(health_str.contains("\"checks\""));
        assert!(health_str.contains("\"memory\""));
        assert!(health_str.contains("\"network\""));
        assert!(health_str.contains("\"httpd\""));
        assert!(health_str.contains("\"tasks\""));
        assert!(health_str.contains("\"filesystem\""));
        assert!(health_str.contains("\"used_pct\""));
        assert!(health_str.contains("\"dirty_pct\""));
        // Status must be one of the three valid values.
        assert!(
            health_str.contains("\"ok\"")
            || health_str.contains("\"degraded\"")
            || health_str.contains("\"critical\"")
        );
        serial_println!("[dashboard]   API health: OK ({} bytes)", health.len());
    }

    // Test 13: Prometheus metrics endpoint returns valid exposition format.
    {
        let metrics = api_metrics();
        assert!(!metrics.is_empty());
        let metrics_str = core::str::from_utf8(&metrics).unwrap_or("");
        // Original metrics — TYPE and HELP annotations.
        assert!(metrics_str.contains("# TYPE os_uptime_seconds gauge"));
        assert!(metrics_str.contains("# HELP os_memory_total_bytes"));
        assert!(metrics_str.contains("os_http_requests_total "));
        assert!(metrics_str.contains("os_dns_cache_entries "));
        assert!(metrics_str.contains("os_tasks_total "));
        assert!(metrics_str.contains("os_net_rx_bytes_total "));
        // New TCP metrics.
        assert!(metrics_str.contains("# TYPE os_tcp_connections_active gauge"));
        assert!(metrics_str.contains("os_tcp_connections_established "));
        assert!(metrics_str.contains("os_tcp_listeners "));
        assert!(metrics_str.contains("os_tcp_rx_bytes_total "));
        // New swap/zram metrics.
        assert!(metrics_str.contains("# TYPE os_swap_compressed_bytes gauge"));
        assert!(metrics_str.contains("os_swap_uncompressed_bytes "));
        assert!(metrics_str.contains("os_swap_compressed_pages "));
        // New scheduler metrics.
        assert!(metrics_str.contains("# TYPE os_sched_context_switches_total counter"));
        assert!(metrics_str.contains("os_sched_load_avg_x100 "));
        assert!(metrics_str.contains("os_sched_tasks_spawned_total "));
        // Per-CPU metrics (at least CPU 0 must exist).
        assert!(metrics_str.contains("os_cpu_total_ticks{cpu=\"0\"}"));
        assert!(metrics_str.contains("os_cpu_idle_ticks{cpu=\"0\"}"));
        // Firewall conntrack.
        assert!(metrics_str.contains("os_firewall_conntrack_entries "));
        // Block cache metrics.
        assert!(metrics_str.contains("# TYPE os_bcache_reads_total counter"));
        assert!(metrics_str.contains("os_bcache_hits_total "));
        assert!(metrics_str.contains("os_bcache_entries_used "));
        assert!(metrics_str.contains("os_bcache_capacity "));
        // Each metric line should end with a number (no trailing whitespace).
        // Lines with labels like {cpu="0"} have the value after the closing brace.
        let has_numeric_values = metrics_str.lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .all(|l| l.split_whitespace().last().map_or(false, |v| v.parse::<u64>().is_ok()));
        assert!(has_numeric_values, "All metric lines must have numeric values");
        // Count total metric families (# TYPE lines).
        let type_lines = metrics_str.lines()
            .filter(|l| l.starts_with("# TYPE "))
            .count();
        assert!(type_lines >= 43,
            "Expected at least 43 metric families, got {}", type_lines);
        serial_println!("[dashboard]   Prometheus metrics: OK ({} bytes, {} families)",
            metrics.len(), type_lines);
    }

    // Test 14: API ipv6 returns valid JSON with expected fields.
    {
        let ipv6 = api_ipv6();
        assert!(!ipv6.is_empty());
        assert_eq!(ipv6[0], b'{');
        assert_eq!(ipv6[ipv6.len().saturating_sub(1)], b'}');
        let ipv6_str = core::str::from_utf8(&ipv6).unwrap_or("");
        assert!(ipv6_str.contains("\"link_local\""));
        assert!(ipv6_str.contains("\"slaac\""));
        assert!(ipv6_str.contains("\"dhcpv6\""));
        assert!(ipv6_str.contains("\"state\""));
        serial_println!("[dashboard]   API ipv6: OK ({} bytes)", ipv6.len());
    }

    // Test 15: API containers returns valid JSON with expected fields.
    {
        let ct = api_containers();
        assert!(!ct.is_empty());
        assert_eq!(ct[0], b'{');
        assert_eq!(ct[ct.len().saturating_sub(1)], b'}');
        let ct_str = core::str::from_utf8(&ct).unwrap_or("");
        assert!(ct_str.contains("\"active_count\""));
        assert!(ct_str.contains("\"containers\""));
        serial_println!("[dashboard]   API containers: OK ({} bytes)", ct.len());
    }

    // Test 16: API tcp returns valid JSON with stats, connections, listeners.
    {
        let tcp = api_tcp();
        assert!(!tcp.is_empty());
        assert_eq!(tcp[0], b'{');
        assert_eq!(tcp[tcp.len().saturating_sub(1)], b'}');
        let tcp_str = core::str::from_utf8(&tcp).unwrap_or("");
        assert!(tcp_str.contains("\"stats\""));
        assert!(tcp_str.contains("\"connections\""));
        assert!(tcp_str.contains("\"listeners\""));
        assert!(tcp_str.contains("\"active\""));
        assert!(tcp_str.contains("\"established\""));
        serial_println!("[dashboard]   API tcp: OK ({} bytes)", tcp.len());
    }

    // Test 17: API scheduler returns valid JSON with per-CPU data.
    {
        let sched = api_scheduler();
        assert!(!sched.is_empty());
        assert_eq!(sched[0], b'{');
        assert_eq!(sched[sched.len().saturating_sub(1)], b'}');
        let sched_str = core::str::from_utf8(&sched).unwrap_or("");
        assert!(sched_str.contains("\"num_cpus\""));
        assert!(sched_str.contains("\"total_ctx_switches\""));
        assert!(sched_str.contains("\"cpus\""));
        assert!(sched_str.contains("\"utilization_pct\""));
        assert!(sched_str.contains("\"preemptions\""));
        serial_println!("[dashboard]   API scheduler: OK ({} bytes)", sched.len());
    }

    // Test 18: API swap returns valid JSON with compression stats.
    {
        let swap = api_swap();
        assert!(!swap.is_empty());
        assert_eq!(swap[0], b'{');
        assert_eq!(swap[swap.len().saturating_sub(1)], b'}');
        let swap_str = core::str::from_utf8(&swap).unwrap_or("");
        assert!(swap_str.contains("\"available\""));
        assert!(swap_str.contains("\"compression\""));
        assert!(swap_str.contains("\"devices\""));
        assert!(swap_str.contains("\"reclaimable_pages\""));
        serial_println!("[dashboard]   API swap: OK ({} bytes)", swap.len());
    }

    // Test 19: API fs returns valid JSON with mounts and cache.
    {
        let fs = api_fs();
        assert!(!fs.is_empty());
        assert_eq!(fs[0], b'{');
        assert_eq!(fs[fs.len().saturating_sub(1)], b'}');
        let fs_str = core::str::from_utf8(&fs).unwrap_or("");
        assert!(fs_str.contains("\"mounts\""));
        assert!(fs_str.contains("\"cache\""));
        assert!(fs_str.contains("\"reads\""));
        assert!(fs_str.contains("\"hits\""));
        assert!(fs_str.contains("\"misses\""));
        assert!(fs_str.contains("\"capacity\""));
        assert!(fs_str.contains("\"hit_rate_pct\""));
        // At least one mount should exist (rootfs).
        assert!(fs_str.contains("\"path\""));
        assert!(fs_str.contains("\"fs_type\""));
        serial_println!("[dashboard]   API fs: OK ({} bytes)", fs.len());
    }

    serial_println!("[dashboard] Self-test PASSED (19 tests)");
    Ok(())
}
