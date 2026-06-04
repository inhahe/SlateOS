//! join — join lines of two files on a common field.
//!
//! Usage: join [-1 FIELD] [-2 FIELD] [-t SEP] [-a FILENUM] [-o FORMAT] [-e EMPTY] FILE1 FILE2
//!   -1 FIELD    join on this field of FILE1 (default: 1)
//!   -2 FIELD    join on this field of FILE2 (default: 1)
//!   -t SEP      use SEP as the field separator (default: whitespace)
//!   -a FILENUM  also print unpairable lines from file FILENUM (1 or 2)
//!   -o FORMAT   output format: comma-separated list of FILENUM.FIELD
//!               (e.g., "1.1,2.2,1.3") or "auto"
//!   -e EMPTY    replace missing fields with EMPTY string
//!
//! Both files must be sorted on the join field. If FILE is "-", read stdin.
//!
//! Exit codes:
//!   0  success
//!   1  error

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

struct Options {
    field1: usize,       // 0-indexed join field for FILE1
    field2: usize,       // 0-indexed join field for FILE2
    separator: Option<char>,
    unpair1: bool,       // -a 1: print unpairable lines from FILE1
    unpair2: bool,       // -a 2: print unpairable lines from FILE2
    output_spec: Option<Vec<(usize, usize)>>, // (file_num 1|2, field 0-indexed)
    empty: String,
}

fn split_fields(line: &str, sep: Option<char>) -> Vec<&str> {
    match sep {
        Some(c) => line.split(c).collect(),
        None => line.split_whitespace().collect(),
    }
}

fn get_field<'a>(fields: &[&'a str], index: usize, empty: &'a str) -> &'a str {
    if index < fields.len() {
        fields[index]
    } else {
        empty
    }
}

fn output_line(
    fields1: &[&str],
    fields2: &[&str],
    opts: &Options,
    out: &mut impl Write,
) {
    let sep = match opts.separator {
        Some(c) => c.to_string(),
        None => " ".to_string(),
    };

    if let Some(ref spec) = opts.output_spec {
        let parts: Vec<&str> = spec
            .iter()
            .map(|&(file_num, field_idx)| {
                if file_num == 1 {
                    get_field(fields1, field_idx, &opts.empty)
                } else {
                    get_field(fields2, field_idx, &opts.empty)
                }
            })
            .collect();
        let _ = writeln!(out, "{}", parts.join(&sep));
    } else {
        // Default: join field, then remaining fields from FILE1, then FILE2.
        let join_key = get_field(fields1, opts.field1, &opts.empty);
        let mut parts: Vec<&str> = vec![join_key];
        for (i, f) in fields1.iter().enumerate() {
            if i != opts.field1 {
                parts.push(f);
            }
        }
        for (i, f) in fields2.iter().enumerate() {
            if i != opts.field2 {
                parts.push(f);
            }
        }
        let _ = writeln!(out, "{}", parts.join(&sep));
    }
}

fn output_unpaired(
    fields: &[&str],
    file_num: usize,
    opts: &Options,
    out: &mut impl Write,
) {
    let sep = match opts.separator {
        Some(c) => c.to_string(),
        None => " ".to_string(),
    };

    if let Some(ref spec) = opts.output_spec {
        let empty_fields: Vec<&str> = Vec::new();
        let parts: Vec<&str> = spec
            .iter()
            .map(|&(fnum, field_idx)| {
                if fnum == file_num {
                    get_field(fields, field_idx, &opts.empty)
                } else {
                    get_field(&empty_fields, field_idx, &opts.empty)
                }
            })
            .collect();
        let _ = writeln!(out, "{}", parts.join(&sep));
    } else {
        let _ = writeln!(out, "{}", fields.join(&sep));
    }
}

fn parse_output_spec(s: &str) -> Option<Vec<(usize, usize)>> {
    let mut result = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some((file_str, field_str)) = part.split_once('.') {
            let file_num: usize = file_str.parse().ok()?;
            let field_num: usize = field_str.parse().ok()?;
            if !(1..=2).contains(&file_num) || field_num < 1 {
                return None;
            }
            result.push((file_num, field_num - 1)); // convert to 0-indexed
        } else {
            return None;
        }
    }
    Some(result)
}

fn read_lines(reader: Box<dyn Read>) -> Vec<String> {
    let buf = BufReader::new(reader);
    let mut lines = Vec::new();
    for line in buf.lines() {
        match line {
            Ok(l) => lines.push(l),
            Err(e) => {
                eprintln!("join: read error: {e}");
                process::exit(1);
            }
        }
    }
    lines
}

fn open_input(path: &str) -> Box<dyn Read> {
    if path == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("join: {path}: {e}");
                process::exit(1);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options {
        field1: 0,
        field2: 0,
        separator: None,
        unpair1: false,
        unpair2: false,
        output_spec: None,
        empty: String::new(),
    };
    let mut files: Vec<String> = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-1" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -1 requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => opts.field1 = n - 1,
                    _ => {
                        eprintln!("join: invalid field number: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-2" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -2 requires an argument");
                    process::exit(1);
                }
                match args[i].parse::<usize>() {
                    Ok(n) if n >= 1 => opts.field2 = n - 1,
                    _ => {
                        eprintln!("join: invalid field number: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-t" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -t requires an argument");
                    process::exit(1);
                }
                opts.separator = args[i].chars().next();
            }
            "-a" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -a requires an argument");
                    process::exit(1);
                }
                match args[i].as_str() {
                    "1" => opts.unpair1 = true,
                    "2" => opts.unpair2 = true,
                    _ => {
                        eprintln!("join: invalid file number for -a: {}", args[i]);
                        process::exit(1);
                    }
                }
            }
            "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -o requires an argument");
                    process::exit(1);
                }
                if args[i] != "auto" {
                    match parse_output_spec(&args[i]) {
                        Some(spec) => opts.output_spec = Some(spec),
                        None => {
                            eprintln!("join: invalid output format: {}", args[i]);
                            process::exit(1);
                        }
                    }
                }
            }
            "-e" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("join: option -e requires an argument");
                    process::exit(1);
                }
                opts.empty = args[i].clone();
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("join: unknown option: {arg}");
                process::exit(1);
            }
            _ => {
                files.push(args[i].clone());
            }
        }
        i += 1;
    }

    if files.len() != 2 {
        eprintln!("join: exactly two files required");
        eprintln!("Usage: join [-1 FIELD] [-2 FIELD] [-t SEP] FILE1 FILE2");
        process::exit(1);
    }

    let lines1 = read_lines(open_input(&files[0]));
    let lines2 = read_lines(open_input(&files[1]));

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Merge-join: both files should be sorted on the join field.
    let mut idx1 = 0;
    let mut idx2 = 0;

    while idx1 < lines1.len() && idx2 < lines2.len() {
        let f1 = split_fields(&lines1[idx1], opts.separator);
        let f2 = split_fields(&lines2[idx2], opts.separator);

        let key1 = get_field(&f1, opts.field1, "");
        let key2 = get_field(&f2, opts.field2, "");

        match key1.cmp(key2) {
            std::cmp::Ordering::Less => {
                if opts.unpair1 {
                    output_unpaired(&f1, 1, &opts, &mut out);
                }
                idx1 += 1;
            }
            std::cmp::Ordering::Greater => {
                if opts.unpair2 {
                    output_unpaired(&f2, 2, &opts, &mut out);
                }
                idx2 += 1;
            }
            std::cmp::Ordering::Equal => {
                // Collect all lines from FILE2 with the same key for
                // many-to-many joins.
                let save_idx2 = idx2;
                while idx2 < lines2.len() {
                    let f2_cur = split_fields(&lines2[idx2], opts.separator);
                    let key2_cur = get_field(&f2_cur, opts.field2, "");
                    if key2_cur != key1 {
                        break;
                    }
                    idx2 += 1;
                }

                // For each line in FILE1 with this key, pair with all
                // matching FILE2 lines.
                while idx1 < lines1.len() {
                    let f1_cur = split_fields(&lines1[idx1], opts.separator);
                    let key1_cur = get_field(&f1_cur, opts.field1, "");
                    if key1_cur != key1 {
                        break;
                    }
                    for j in save_idx2..idx2 {
                        let f2_cur = split_fields(&lines2[j], opts.separator);
                        output_line(&f1_cur, &f2_cur, &opts, &mut out);
                    }
                    idx1 += 1;
                }
            }
        }
    }

    // Print remaining unpairable lines.
    if opts.unpair1 {
        while idx1 < lines1.len() {
            let f1 = split_fields(&lines1[idx1], opts.separator);
            output_unpaired(&f1, 1, &opts, &mut out);
            idx1 += 1;
        }
    }
    if opts.unpair2 {
        while idx2 < lines2.len() {
            let f2 = split_fields(&lines2[idx2], opts.separator);
            output_unpaired(&f2, 2, &opts, &mut out);
            idx2 += 1;
        }
    }
}
