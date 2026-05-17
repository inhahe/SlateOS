//! uptime — tell how long the system has been running.
//!
//! Usage: uptime
//!   Reads /proc/uptime for system uptime information.

use std::fs;

fn main() {
    // Try reading /proc/uptime (format: "seconds.fraction idle_seconds.fraction")
    match fs::read_to_string("/proc/uptime") {
        Ok(content) => {
            let parts: Vec<&str> = content.split_whitespace().collect();
            if let Some(secs_str) = parts.first() {
                let total_secs: f64 = secs_str.parse().unwrap_or(0.0);
                let days = (total_secs / 86400.0) as u64;
                let hours = ((total_secs % 86400.0) / 3600.0) as u64;
                let mins = ((total_secs % 3600.0) / 60.0) as u64;

                print!("up ");
                if days > 0 {
                    print!("{days} day{}, ", if days == 1 { "" } else { "s" });
                }
                println!("{hours:02}:{mins:02}");
            } else {
                println!("up (unknown)");
            }
        }
        Err(_) => {
            println!("uptime: cannot read /proc/uptime");
        }
    }
}
