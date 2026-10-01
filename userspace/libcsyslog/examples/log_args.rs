//! `log_args IDENT FACILITY SEVERITY MESSAGE...`: open the log as `IDENT` with
//! `LOG_PID`, at facility number `FACILITY` (0-23, not yet shifted), and log
//! each `MESSAGE` as one message at `SEVERITY` (0-7).
//!
//! It exists for `scripts/syslog-client-check.sh`, which watches what the C
//! library makes of awkward messages -- a `%` that must stay literal, bytes
//! that are not UTF-8 -- at a private `/dev/log`.

use std::process::ExitCode;

fn number(arg: Option<std::ffi::OsString>, what: &str) -> Result<i32, String> {
    let arg = arg.ok_or_else(|| format!("missing {what}"))?;
    let text = arg
        .to_str()
        .ok_or_else(|| format!("{what} is not a number"))?;
    text.parse()
        .map_err(|_| format!("{what} is not a number: {text}"))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args_os().skip(1);
    let ident = args.next().ok_or("missing IDENT")?;
    let facility = number(args.next(), "FACILITY")?;
    let severity = number(args.next(), "SEVERITY")?;
    let facility = facility
        .checked_mul(8)
        .ok_or_else(|| format!("FACILITY out of range: {facility}"))?;
    libcsyslog::openlog(ident.as_encoded_bytes(), libcsyslog::LOG_PID, facility);
    for message in args {
        libcsyslog::syslog(severity, message.as_encoded_bytes());
    }
    libcsyslog::closelog();
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("log_args: {e}");
            ExitCode::from(2)
        }
    }
}
