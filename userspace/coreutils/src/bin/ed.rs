//! ed — the standard line editor.
//!
//! Usage: ed [FILE]
//!   Classic line editor. Commands operate on a buffer of lines.
//!
//! Commands:
//!   NUM           set current line
//!   p / NUM p     print current/specified line
//!   N,M p         print lines N through M
//!   a             append after current line (end with '.')
//!   i             insert before current line (end with '.')
//!   d / N,M d     delete current/specified lines
//!   c / N,M c     change (replace) lines (end with '.')
//!   s/PAT/REPL/   substitute on current line
//!   w [FILE]      write buffer to file
//!   q             quit (warns if unsaved)
//!   Q             quit without saving
//!   f [FILE]      show/set filename
//!   = / NUM =     print line count / specified line number
//!   n / NUM n     print with line numbers
//!   , p           print all lines
//!   $ p           print last line

use std::env;
use std::fs;
use std::io::{self, BufRead};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut buffer: Vec<String> = Vec::new();
    let mut current: usize = 0; // 1-based, 0 = empty
    let mut filename: Option<String> = None;
    let mut modified = false;

    if let Some(path) = args.first() {
        filename = Some(path.clone());
        match fs::read_to_string(path) {
            Ok(content) => {
                buffer = content.lines().map(|l| l.to_string()).collect();
                current = buffer.len();
                let bytes = content.len();
                println!("{bytes}");
            }
            Err(_) => {
                // New file — empty buffer
                println!("0");
            }
        }
    }

    let stdin = io::stdin();

    loop {
        // No prompt in ed (traditional)
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');

        // Parse command
        let (addr_start, addr_end, cmd, arg) = parse_command(line, current, buffer.len());

        match cmd {
            'q' => {
                if modified {
                    println!("?");
                    modified = false; // next 'q' will quit
                } else {
                    break;
                }
            }
            'Q' => break,

            'p' => {
                for i in addr_start..=addr_end {
                    if i >= 1 && i <= buffer.len() {
                        println!("{}", buffer[i - 1]);
                    }
                }
                current = addr_end.min(buffer.len());
            }

            'n' => {
                for i in addr_start..=addr_end {
                    if i >= 1 && i <= buffer.len() {
                        println!("{i}\t{}", buffer[i - 1]);
                    }
                }
                current = addr_end.min(buffer.len());
            }

            'a' => {
                // Append after addr_start
                let insert_at = addr_start;
                let new_lines = read_input_lines(&stdin);
                for (j, new_line) in new_lines.iter().enumerate() {
                    buffer.insert(insert_at + j, new_line.clone());
                }
                current = insert_at + new_lines.len();
                modified = true;
            }

            'i' => {
                let insert_at = if addr_start >= 1 {
                    addr_start - 1
                } else {
                    0
                };
                let new_lines = read_input_lines(&stdin);
                for (j, new_line) in new_lines.iter().enumerate() {
                    buffer.insert(insert_at + j, new_line.clone());
                }
                current = insert_at + new_lines.len();
                modified = true;
            }

            'c' => {
                // Change: delete range, then insert
                if addr_start >= 1 && addr_end <= buffer.len() {
                    let drain_start = addr_start - 1;
                    let drain_end = addr_end;
                    buffer.drain(drain_start..drain_end);
                    let new_lines = read_input_lines(&stdin);
                    for (j, new_line) in new_lines.iter().enumerate() {
                        buffer.insert(drain_start + j, new_line.clone());
                    }
                    current = drain_start + new_lines.len();
                    modified = true;
                } else {
                    println!("?");
                }
            }

            'd' => {
                if addr_start >= 1 && addr_end <= buffer.len() {
                    buffer.drain(addr_start - 1..addr_end);
                    current = if addr_start <= buffer.len() {
                        addr_start
                    } else {
                        buffer.len()
                    };
                    modified = true;
                } else {
                    println!("?");
                }
            }

            's' => {
                // s/pattern/replacement/[g]
                if current >= 1 && current <= buffer.len() {
                    if let Some((pat, repl, global)) = parse_substitute(&arg) {
                        let line = &buffer[current - 1];
                        let new_line = if global {
                            line.replace(&pat, &repl)
                        } else {
                            line.replacen(&pat, &repl, 1)
                        };
                        buffer[current - 1] = new_line;
                        println!("{}", buffer[current - 1]);
                        modified = true;
                    } else {
                        println!("?");
                    }
                } else {
                    println!("?");
                }
            }

            'w' => {
                let path = if arg.is_empty() {
                    filename.clone()
                } else {
                    let p = arg.to_string();
                    filename = Some(p.clone());
                    Some(p)
                };

                match path {
                    Some(p) => {
                        let content: String =
                            buffer.iter().map(|l| format!("{l}\n")).collect();
                        match fs::write(&p, &content) {
                            Ok(()) => {
                                println!("{}", content.len());
                                modified = false;
                            }
                            Err(e) => {
                                println!("? {e}");
                            }
                        }
                    }
                    None => {
                        println!("? no filename");
                    }
                }
            }

            'f' => {
                if !arg.is_empty() {
                    filename = Some(arg.to_string());
                }
                match &filename {
                    Some(f) => println!("{f}"),
                    None => println!("?"),
                }
            }

            '=' => {
                println!("{}", buffer.len());
            }

            '\0' => {
                // Just a line number — print it
                if addr_start >= 1 && addr_start <= buffer.len() {
                    current = addr_start;
                    println!("{}", buffer[current - 1]);
                } else if addr_start > 0 {
                    println!("?");
                }
            }

            _ => {
                println!("?");
            }
        }
    }
}

fn read_input_lines(stdin: &io::Stdin) -> Vec<String> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        if line == "." {
            break;
        }
        lines.push(line.to_string());
    }
    lines
}

fn parse_command(input: &str, current: usize, total: usize) -> (usize, usize, char, String) {
    let input = input.trim();
    if input.is_empty() {
        return (current + 1, current + 1, '\0', String::new());
    }

    let bytes = input.as_bytes();
    let mut pos = 0;

    // Parse first address
    let addr1 = parse_address(input, &mut pos, current, total);

    // Check for comma (range)
    let addr2 = if pos < bytes.len() && bytes[pos] == b',' {
        pos += 1;
        parse_address(input, &mut pos, current, total)
    } else {
        addr1
    };

    // Parse command character
    let cmd = if pos < bytes.len() {
        let c = bytes[pos] as char;
        pos += 1;
        c
    } else if addr1 != current || input.chars().next().map_or(false, |c| c.is_ascii_digit()) {
        '\0' // just a line number
    } else {
        '\0'
    };

    let arg = input[pos..].trim().to_string();

    (addr1, addr2, cmd, arg)
}

fn parse_address(input: &str, pos: &mut usize, current: usize, total: usize) -> usize {
    let bytes = input.as_bytes();

    while *pos < bytes.len() && bytes[*pos] == b' ' {
        *pos += 1;
    }

    if *pos >= bytes.len() {
        return current;
    }

    match bytes[*pos] {
        b'.' => {
            *pos += 1;
            current
        }
        b'$' => {
            *pos += 1;
            total
        }
        b'0'..=b'9' => {
            let start = *pos;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
            input[start..*pos].parse().unwrap_or(current)
        }
        _ => current,
    }
}

fn parse_substitute(arg: &str) -> Option<(String, String, bool)> {
    // arg starts after 's', e.g., "/pattern/replacement/g"
    let bytes = arg.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let delim = bytes[0];
    let rest = &arg[1..];

    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let rest_bytes = rest.as_bytes();
    let mut i = 0;

    while i < rest_bytes.len() {
        if rest_bytes[i] == b'\\' && i + 1 < rest_bytes.len() {
            current.push(rest_bytes[i + 1] as char);
            i += 2;
        } else if rest_bytes[i] == delim {
            parts.push(current.clone());
            current.clear();
            i += 1;
        } else {
            current.push(rest_bytes[i] as char);
            i += 1;
        }
    }
    parts.push(current);

    if parts.len() < 2 {
        return None;
    }

    let global = parts.get(2).map_or(false, |f| f.contains('g'));
    Some((parts[0].clone(), parts[1].clone(), global))
}
