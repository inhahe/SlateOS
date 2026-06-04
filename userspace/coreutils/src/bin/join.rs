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

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
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

/// Parse join's argv into `(Options, files)`.  Returns an error string
/// suitable for `eprintln!("join: {e}")`.  This is pure; it does no I/O.
fn parse_args(args: &[String]) -> Result<(Options, Vec<String>), String> {
    let mut opts = Options::default();
    let mut files: Vec<String> = Vec::new();
    let mut i: usize = 0;

    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        match arg.as_str() {
            "-1" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -1 requires an argument".to_string())?;
                let n = v.parse::<usize>().map_err(|_| format!("invalid field number: {v}"))?;
                if n == 0 {
                    return Err(format!("invalid field number: {v}"));
                }
                opts.field1 = n.saturating_sub(1);
            }
            "-2" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -2 requires an argument".to_string())?;
                let n = v.parse::<usize>().map_err(|_| format!("invalid field number: {v}"))?;
                if n == 0 {
                    return Err(format!("invalid field number: {v}"));
                }
                opts.field2 = n.saturating_sub(1);
            }
            "-t" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -t requires an argument".to_string())?;
                opts.separator = v.chars().next();
            }
            "-a" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -a requires an argument".to_string())?;
                match v.as_str() {
                    "1" => opts.unpair1 = true,
                    "2" => opts.unpair2 = true,
                    other => return Err(format!("invalid file number for -a: {other}")),
                }
            }
            "-o" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -o requires an argument".to_string())?;
                if v != "auto" {
                    let spec = parse_output_spec(v).ok_or_else(|| format!("invalid output format: {v}"))?;
                    opts.output_spec = Some(spec);
                }
            }
            "-e" => {
                i = i.saturating_add(1);
                let v = args.get(i).ok_or_else(|| "option -e requires an argument".to_string())?;
                opts.empty = v.clone();
            }
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown option: {other}"));
            }
            other => files.push(other.to_string()),
        }
        i = i.saturating_add(1);
    }

    Ok((opts, files))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (opts, files) = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("join: {e}");
            process::exit(1);
        }
    };

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
                    for line2 in lines2.iter().take(idx2).skip(save_idx2) {
                        let f2_cur = split_fields(line2, opts.separator);
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- split_fields ----------------

    #[test]
    fn split_whitespace_default() {
        let fields = split_fields("a b  c\td", None);
        assert_eq!(fields, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn split_explicit_separator() {
        let fields = split_fields("a,b,,c", Some(','));
        assert_eq!(fields, vec!["a", "b", "", "c"]);
    }

    #[test]
    fn split_empty_line() {
        assert!(split_fields("", None).is_empty());
        assert_eq!(split_fields("", Some(',')), vec![""]);
    }

    // ---------------- get_field ----------------

    #[test]
    fn get_field_in_range() {
        let fields = vec!["a", "b", "c"];
        assert_eq!(get_field(&fields, 0, "X"), "a");
        assert_eq!(get_field(&fields, 2, "X"), "c");
    }

    #[test]
    fn get_field_out_of_range_uses_empty() {
        let fields = vec!["a", "b"];
        assert_eq!(get_field(&fields, 5, "MISSING"), "MISSING");
    }

    // ---------------- parse_output_spec ----------------

    #[test]
    fn output_spec_single() {
        assert_eq!(parse_output_spec("1.1"), Some(vec![(1, 0)]));
    }

    #[test]
    fn output_spec_multiple() {
        assert_eq!(
            parse_output_spec("1.1,2.2,1.3"),
            Some(vec![(1, 0), (2, 1), (1, 2)])
        );
    }

    #[test]
    fn output_spec_invalid_no_dot() {
        assert_eq!(parse_output_spec("11"), None);
    }

    #[test]
    fn output_spec_invalid_file_num() {
        assert_eq!(parse_output_spec("3.1"), None);
        assert_eq!(parse_output_spec("0.1"), None);
    }

    #[test]
    fn output_spec_invalid_field_zero() {
        assert_eq!(parse_output_spec("1.0"), None);
    }

    #[test]
    fn output_spec_with_spaces() {
        assert_eq!(parse_output_spec(" 1.1 , 2.2 "), Some(vec![(1, 0), (2, 1)]));
    }

    // ---------------- parse_args ----------------

    #[test]
    fn args_defaults_when_empty() {
        let (opts, files) = parse_args(&s(&[])).unwrap();
        assert_eq!(opts, Options::default());
        assert!(files.is_empty());
    }

    #[test]
    fn args_files_only() {
        let (opts, files) = parse_args(&s(&["a.txt", "b.txt"])).unwrap();
        assert_eq!(opts.field1, 0);
        assert_eq!(files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn args_field_numbers() {
        let (opts, _) = parse_args(&s(&["-1", "2", "-2", "3", "a", "b"])).unwrap();
        assert_eq!(opts.field1, 1);
        assert_eq!(opts.field2, 2);
    }

    #[test]
    fn args_separator() {
        let (opts, _) = parse_args(&s(&["-t", ",", "a", "b"])).unwrap();
        assert_eq!(opts.separator, Some(','));
    }

    #[test]
    fn args_unpair_one_and_two() {
        let (opts, _) = parse_args(&s(&["-a", "1", "-a", "2", "a", "b"])).unwrap();
        assert!(opts.unpair1 && opts.unpair2);
    }

    #[test]
    fn args_unpair_invalid() {
        let err = parse_args(&s(&["-a", "3", "a", "b"])).unwrap_err();
        assert!(err.contains("invalid file number"));
    }

    #[test]
    fn args_output_spec() {
        let (opts, _) = parse_args(&s(&["-o", "1.1,2.2", "a", "b"])).unwrap();
        assert_eq!(opts.output_spec, Some(vec![(1, 0), (2, 1)]));
    }

    #[test]
    fn args_output_spec_auto_keeps_default() {
        let (opts, _) = parse_args(&s(&["-o", "auto", "a", "b"])).unwrap();
        assert_eq!(opts.output_spec, None);
    }

    #[test]
    fn args_empty_string() {
        let (opts, _) = parse_args(&s(&["-e", "NULL", "a", "b"])).unwrap();
        assert_eq!(opts.empty, "NULL");
    }

    #[test]
    fn args_missing_value_errors() {
        for flag in ["-1", "-2", "-t", "-a", "-o", "-e"] {
            let err = parse_args(&s(&[flag])).unwrap_err();
            assert!(err.contains(flag) || err.contains("requires"));
        }
    }

    #[test]
    fn args_invalid_field_zero_errors() {
        let err = parse_args(&s(&["-1", "0", "a", "b"])).unwrap_err();
        assert!(err.contains("invalid field number"));
    }

    #[test]
    fn args_unknown_option_errors() {
        let err = parse_args(&s(&["-z", "a", "b"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    // ---------------- output_line ----------------

    fn capture_line(fields1: &[&str], fields2: &[&str], opts: &Options) -> String {
        let mut buf: Vec<u8> = Vec::new();
        output_line(fields1, fields2, opts, &mut buf);
        String::from_utf8(buf).unwrap()
    }

    fn capture_unpaired(fields: &[&str], file_num: usize, opts: &Options) -> String {
        let mut buf: Vec<u8> = Vec::new();
        output_unpaired(fields, file_num, opts, &mut buf);
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn output_line_default_format() {
        // join field is field1, then other fields of FILE1, then FILE2.
        let opts = Options::default();
        let line = capture_line(&["k", "v1"], &["k", "v2"], &opts);
        assert_eq!(line, "k v1 v2\n");
    }

    #[test]
    fn output_line_with_separator() {
        let opts = Options {
            separator: Some(','),
            ..Options::default()
        };
        let line = capture_line(&["k", "v1"], &["k", "v2"], &opts);
        assert_eq!(line, "k,v1,v2\n");
    }

    #[test]
    fn output_line_with_output_spec() {
        let opts = Options {
            separator: Some(','),
            output_spec: Some(vec![(2, 1), (1, 0)]),
            ..Options::default()
        };
        let line = capture_line(&["k", "v1"], &["k", "v2"], &opts);
        // Spec: file2 field 1 (= "v2"), file1 field 0 (= "k").
        assert_eq!(line, "v2,k\n");
    }

    #[test]
    fn output_unpaired_default() {
        let opts = Options::default();
        let line = capture_unpaired(&["k", "v1", "extra"], 1, &opts);
        assert_eq!(line, "k v1 extra\n");
    }

    #[test]
    fn output_unpaired_with_spec_fills_empty_for_other_file() {
        let opts = Options {
            separator: Some(','),
            empty: "NULL".to_string(),
            output_spec: Some(vec![(1, 0), (2, 1), (1, 1)]),
            ..Options::default()
        };
        let line = capture_unpaired(&["k", "v1"], 1, &opts);
        // File 1 fields available; for (2, 1) it falls back to empty.
        assert_eq!(line, "k,NULL,v1\n");
    }
}
