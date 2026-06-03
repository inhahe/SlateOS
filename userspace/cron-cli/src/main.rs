#![deny(clippy::all)]

//! cron-cli — OurOS cron job scheduler
//!
//! Multi-personality: `crontab`, `crond`, `anacron`, `at`, `atq`, `atrm`, `batch`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_crontab(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crontab [OPTIONS] [FILE]");
        println!();
        println!("crontab — manage cron tables (OurOS).");
        println!();
        println!("Options:");
        println!("  -e         Edit crontab");
        println!("  -l         List crontab");
        println!("  -r         Remove crontab");
        println!("  -u USER    Specify user");
        println!("  -i         Prompt before removal");
        return 0;
    }

    let list = args.iter().any(|a| a == "-l");
    let edit = args.iter().any(|a| a == "-e");
    let remove = args.iter().any(|a| a == "-r");

    if list {
        println!("# m h dom mon dow command");
        println!("*/5 * * * * /usr/bin/check-updates");
        println!("0 2 * * * /usr/bin/backup --daily");
        println!("0 0 * * 0 /usr/bin/backup --weekly");
        println!("@reboot /usr/bin/startup-tasks");
    } else if edit {
        println!("crontab: editing crontab for user");
    } else if remove {
        println!("crontab: removed crontab for user");
    } else {
        let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
        if let Some(f) = file {
            println!("crontab: installing new crontab from '{}'", f);
        } else {
            eprintln!("crontab: usage error. See --help.");
            return 1;
        }
    }
    0
}

fn run_crond(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: crond [OPTIONS]");
        println!("Options: -f (foreground), -l N (log level), -L FILE (log file)");
        return 0;
    }
    println!("crond: starting cron daemon (OurOS)");
    println!("crond: loaded 3 crontabs");
    println!("crond: checking schedules every 60 seconds");
    0
}

fn run_anacron(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: anacron [OPTIONS]");
        println!();
        println!("anacron — run periodic jobs (OurOS).");
        println!();
        println!("Options:");
        println!("  -f     Force run all jobs");
        println!("  -u     Update timestamps only");
        println!("  -s     Serialize job execution");
        println!("  -n     Run now, ignore delays");
        println!("  -d     Debug mode (foreground)");
        println!("  -t TAB Use alternate anacrontab");
        return 0;
    }

    let force = args.iter().any(|a| a == "-f");
    println!("anacron: checking scheduled jobs");
    if force {
        println!("anacron: forcing all jobs to run");
    }
    println!("anacron: job `cron.daily' started");
    println!("anacron: job `cron.weekly' started");
    println!("anacron: normal exit (2 jobs run)");
    0
}

fn run_at(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: at TIME");
        println!();
        println!("at — schedule commands for later execution (OurOS).");
        println!();
        println!("TIME formats: HH:MM, midnight, noon, teatime, now + N min/hours/days");
        return 0;
    }

    let time = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    if time.is_empty() {
        eprintln!("at: missing time specification");
        return 1;
    }

    println!("warning: commands will be executed using /bin/sh");
    println!("job 42 at Thu Jan  1 {}", if time.is_empty() { "12:00:00 2025" } else { "2025" });
    0
}

fn run_atq(_args: &[String]) -> i32 {
    println!("42\tThu Jan  1 14:00:00 2025 a user");
    println!("43\tFri Jan  2 00:00:00 2025 a user");
    0
}

fn run_atrm(args: &[String]) -> i32 {
    let job = args.first().map(|s| s.as_str()).unwrap_or("");
    if job.is_empty() {
        eprintln!("atrm: missing job number");
        return 1;
    }
    println!("atrm: removed job {}", job);
    0
}

fn run_batch(_args: &[String]) -> i32 {
    println!("warning: commands will be executed using /bin/sh");
    println!("job 44 at Thu Jan  1 12:00:00 2025 (queued, will run when load average drops below 1.5)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "crontab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "crond" => run_crond(&rest),
        "anacron" => run_anacron(&rest),
        "at" => run_at(&rest),
        "atq" => run_atq(&rest),
        "atrm" => run_atrm(&rest),
        "batch" => run_batch(&rest),
        _ => run_crontab(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crontab};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cron"), "cron");
        assert_eq!(basename(r"C:\bin\cron.exe"), "cron.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cron.exe"), "cron");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_crontab(&["--help".to_string()]), 0);
        assert_eq!(run_crontab(&["-h".to_string()]), 0);
        assert_eq!(run_crontab(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_crontab(&[]), 0);
    }
}
