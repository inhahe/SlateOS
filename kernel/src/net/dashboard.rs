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
            r#"{{"id":{},"name":"{}","priority":{},"state":"{}","cpu":{}}}"#,
            task.id,
            json_escape(name_str),
            task.priority,
            task.state,  // TaskState implements Display
            task.last_cpu,
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
    <thead><tr><th>ID</th><th>Name</th><th>Priority</th><th>State</th><th>CPU</th></tr></thead>
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
    var [sr,tr,nr,mr] = await Promise.all([
      fetch('/api/status').then(r=>r.json()),
      fetch('/api/tasks').then(r=>r.json()),
      fetch('/api/network').then(r=>r.json()),
      fetch('/api/memory').then(r=>r.json()),
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
        '</td><td>'+badge(t.state)+'</td><td>'+t.cpu+'</td></tr>';
    });
    document.getElementById('task-body').innerHTML=tb;
    var cb=''; nr.tcp_connections.forEach(function(c){
      cb+='<tr><td>'+c.local_port+'</td><td>'+c.remote_ip+':'+c.remote_port+
        '</td><td>'+c.state+'</td></tr>';
    });
    document.getElementById('tcp-body').innerHTML=cb||'<tr><td colspan="3" style="color:#484f58">No active connections</td></tr>';
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
        assert!(handle_api_request("/not-an-api").is_none());
        assert!(handle_api_request("/api/nonexistent").is_none());
        serial_println!("[dashboard]   API routing: OK");
    }

    serial_println!("[dashboard] Self-test PASSED (7 tests)");
    Ok(())
}
