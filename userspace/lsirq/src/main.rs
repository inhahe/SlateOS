//! Slate OS IRQ information display utility.
//!
//! Multi-personality binary providing:
//! - **lsirq** — display information about system interrupts
//!
//! Reads interrupt statistics from /proc/interrupts and /proc/softirqs.

#![deny(clippy::all)]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct IrqInfo {
    /// IRQ number or name.
    irq: String,
    /// Total count across all CPUs.
    total: u64,
    /// Per-CPU counts (used for detailed per-CPU output modes).
    _per_cpu: Vec<u64>,
    /// Chip name (e.g., "IO-APIC", "PCI-MSI").
    chip_name: String,
    /// Hardware IRQ number.
    hwirq: String,
    /// Description / action name.
    name: String,
}

struct LsirqOpts {
    json: bool,
    pairs: bool,
    noheadings: bool,
    softirq: bool,
    sort_by: SortField,
    columns: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum SortField {
    Irq,
    Total,
    Name,
}

// ============================================================================
// Data collection
// ============================================================================

fn parse_interrupts() -> (Vec<String>, Vec<IrqInfo>) {
    let content = match fs::read_to_string("/proc/interrupts") {
        Ok(c) => c,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    let mut lines = content.lines();
    let header = match lines.next() {
        Some(h) => h,
        None => return (Vec::new(), Vec::new()),
    };

    // Parse CPU names from header.
    let cpus: Vec<String> = header
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    let num_cpus = cpus.len();
    let mut irqs = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        // First field is IRQ number (with trailing ':').
        let irq_str = parts[0].trim_end_matches(':').to_string();

        // Next num_cpus fields are counts.
        let mut per_cpu = Vec::new();
        let mut total = 0u64;
        let mut idx = 1;
        for _ in 0..num_cpus {
            if idx < parts.len() {
                let count: u64 = parts[idx].parse().unwrap_or(0);
                per_cpu.push(count);
                total += count;
                idx += 1;
            }
        }

        // Remaining fields are chip name and description.
        let (chip_name, name) = if idx < parts.len() {
            let chip = parts[idx].to_string();
            idx += 1;
            // Skip optional hwirq field.
            let hwirq = if idx < parts.len() && parts[idx].chars().all(|c| c.is_ascii_digit() || c == '-') {
                let h = parts[idx].to_string();
                idx += 1;
                h
            } else {
                String::new()
            };
            let desc: String = parts[idx..].join(" ");
            let _ = hwirq;
            (chip, desc)
        } else {
            (String::new(), String::new())
        };

        irqs.push(IrqInfo {
            irq: irq_str,
            total,
            _per_cpu: per_cpu,
            chip_name,
            hwirq: String::new(),
            name,
        });
    }

    (cpus, irqs)
}

fn parse_softirqs() -> (Vec<String>, Vec<IrqInfo>) {
    let content = match fs::read_to_string("/proc/softirqs") {
        Ok(c) => c,
        Err(_) => return (Vec::new(), Vec::new()),
    };

    let mut lines = content.lines();
    let header = match lines.next() {
        Some(h) => h,
        None => return (Vec::new(), Vec::new()),
    };

    let cpus: Vec<String> = header
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    let num_cpus = cpus.len();
    let mut irqs = Vec::new();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].trim_end_matches(':').to_string();
        let mut per_cpu = Vec::new();
        let mut total = 0u64;

        for i in 1..=num_cpus {
            if i < parts.len() {
                let count: u64 = parts[i].parse().unwrap_or(0);
                per_cpu.push(count);
                total += count;
            }
        }

        irqs.push(IrqInfo {
            irq: name.clone(),
            total,
            _per_cpu: per_cpu,
            chip_name: "softirq".to_string(),
            hwirq: String::new(),
            name,
        });
    }

    (cpus, irqs)
}

// ============================================================================
// Output
// ============================================================================

fn default_columns() -> Vec<String> {
    vec!["IRQ".into(), "TOTAL".into(), "NAME".into()]
}

fn column_value(irq: &IrqInfo, col: &str) -> String {
    match col.to_uppercase().as_str() {
        "IRQ" => irq.irq.clone(),
        "TOTAL" => irq.total.to_string(),
        "NAME" | "DESC" | "DESCRIPTION" => irq.name.clone(),
        "CHIP" | "CHIPNAME" => irq.chip_name.clone(),
        "HWIRQ" => irq.hwirq.clone(),
        _ => String::new(),
    }
}

fn print_table(out: &mut io::StdoutLock<'_>, irqs: &[IrqInfo], opts: &LsirqOpts) {
    let cols = if opts.columns.is_empty() { default_columns() } else { opts.columns.clone() };

    let mut widths: Vec<usize> = cols.iter().map(|c| c.len()).collect();
    for irq in irqs {
        for (i, col) in cols.iter().enumerate() {
            let val = column_value(irq, col);
            if val.len() > widths[i] {
                widths[i] = val.len();
            }
        }
    }

    if !opts.noheadings {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let _ = write!(out, "{:>width$}", col, width = widths[i]);
        }
        let _ = writeln!(out);
    }

    for irq in irqs {
        for (i, col) in cols.iter().enumerate() {
            if i > 0 { let _ = write!(out, " "); }
            let val = column_value(irq, col);
            let _ = write!(out, "{:>width$}", val, width = widths[i]);
        }
        let _ = writeln!(out);
    }
}

fn print_json(out: &mut io::StdoutLock<'_>, irqs: &[IrqInfo]) {
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"interrupts\": [");
    for (i, irq) in irqs.iter().enumerate() {
        let comma = if i + 1 < irqs.len() { "," } else { "" };
        let _ = writeln!(out,
            "    {{\"irq\": \"{}\", \"total\": {}, \"chip_name\": \"{}\", \"name\": \"{}\"}}{comma}",
            irq.irq, irq.total, irq.chip_name, irq.name
        );
    }
    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
}

fn print_pairs(out: &mut io::StdoutLock<'_>, irqs: &[IrqInfo]) {
    for irq in irqs {
        let _ = writeln!(out,
            "IRQ=\"{}\" TOTAL=\"{}\" CHIP=\"{}\" NAME=\"{}\"",
            irq.irq, irq.total, irq.chip_name, irq.name
        );
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut opts = LsirqOpts {
        json: false,
        pairs: false,
        noheadings: false,
        softirq: false,
        sort_by: SortField::Irq,
        columns: Vec::new(),
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: lsirq [options]");
                println!();
                println!("Display information about system interrupts.");
                println!();
                println!("Options:");
                println!("  -J, --json         JSON output");
                println!("  -P, --pairs        Key=value output");
                println!("  -n, --noheadings   No headers");
                println!("  -s, --softirq      Show softirqs instead of hardware IRQs");
                println!("  -S, --sort COLUMN  Sort by column (irq, total, name)");
                println!("  -o, --output COLS  Columns (IRQ,TOTAL,NAME,CHIP)");
                println!("  -h, --help         Show this help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lsirq {VERSION}");
                process::exit(0);
            }
            "-J" | "--json" => opts.json = true,
            "-P" | "--pairs" => opts.pairs = true,
            "-n" | "--noheadings" => opts.noheadings = true,
            "-s" | "--softirq" => opts.softirq = true,
            "-S" | "--sort" => {
                i += 1;
                if i < args.len() {
                    opts.sort_by = match args[i].to_lowercase().as_str() {
                        "total" | "count" => SortField::Total,
                        "name" | "desc" => SortField::Name,
                        _ => SortField::Irq,
                    };
                }
            }
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    opts.columns = args[i].split(',').map(|s| s.trim().to_uppercase()).collect();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let (_cpus, mut irqs) = if opts.softirq {
        parse_softirqs()
    } else {
        parse_interrupts()
    };

    // Sort.
    match opts.sort_by {
        SortField::Total => irqs.sort_by_key(|b| std::cmp::Reverse(b.total)),
        SortField::Name => irqs.sort_by(|a, b| a.name.cmp(&b.name)),
        SortField::Irq => {} // Already in natural order.
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.json {
        print_json(&mut out, &irqs);
    } else if opts.pairs {
        print_pairs(&mut out, &irqs);
    } else {
        print_table(&mut out, &irqs, &opts);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_irq(irq: &str, total: u64, name: &str) -> IrqInfo {
        IrqInfo {
            irq: irq.to_string(),
            total,
            _per_cpu: vec![total],
            chip_name: "IO-APIC".to_string(),
            hwirq: String::new(),
            name: name.to_string(),
        }
    }

    #[test]
    fn test_column_value_irq() {
        let irq = make_irq("0", 1000, "timer");
        assert_eq!(column_value(&irq, "IRQ"), "0");
        assert_eq!(column_value(&irq, "TOTAL"), "1000");
        assert_eq!(column_value(&irq, "NAME"), "timer");
        assert_eq!(column_value(&irq, "CHIP"), "IO-APIC");
    }

    #[test]
    fn test_column_value_case_insensitive() {
        let irq = make_irq("1", 500, "keyboard");
        assert_eq!(column_value(&irq, "irq"), "1");
        assert_eq!(column_value(&irq, "total"), "500");
    }

    #[test]
    fn test_column_value_unknown() {
        let irq = make_irq("0", 0, "test");
        assert_eq!(column_value(&irq, "UNKNOWN"), "");
    }

    #[test]
    fn test_default_columns() {
        let cols = default_columns();
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0], "IRQ");
    }

    #[test]
    fn test_sort_field_equality() {
        assert_eq!(SortField::Irq, SortField::Irq);
        assert_ne!(SortField::Total, SortField::Name);
    }

    #[test]
    fn test_parse_interrupts_no_crash() {
        let (_cpus, _irqs) = parse_interrupts();
    }

    #[test]
    fn test_parse_softirqs_no_crash() {
        let (_cpus, _irqs) = parse_softirqs();
    }

    #[test]
    fn test_sort_by_total() {
        let mut irqs = [
            make_irq("0", 100, "a"),
            make_irq("1", 500, "b"),
            make_irq("2", 200, "c"),
        ];
        irqs.sort_by_key(|irq| std::cmp::Reverse(irq.total));
        assert_eq!(irqs[0].irq, "1");
        assert_eq!(irqs[1].irq, "2");
        assert_eq!(irqs[2].irq, "0");
    }

    #[test]
    fn test_sort_by_name() {
        let mut irqs = [
            make_irq("0", 0, "timer"),
            make_irq("1", 0, "keyboard"),
            make_irq("2", 0, "mouse"),
        ];
        irqs.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(irqs[0].name, "keyboard");
        assert_eq!(irqs[1].name, "mouse");
        assert_eq!(irqs[2].name, "timer");
    }

    #[test]
    fn test_irq_info_clone() {
        let irq = make_irq("8", 42, "rtc");
        let cloned = irq.clone();
        assert_eq!(cloned.irq, "8");
        assert_eq!(cloned.total, 42);
    }
}
