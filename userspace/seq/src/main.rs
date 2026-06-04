//! seq/yes/expr — number sequence, repeated output, and expression evaluation for OurOS
//!
//! Multi-personality binary detected via argv[0]:
//! - `seq`: print a sequence of numbers
//! - `yes`: repeatedly output a string
//! - `expr`: evaluate expressions

use std::env;
use std::io::{self, Write};
use std::process;

// ── Mode detection ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Seq,
    Yes,
    Expr,
}

fn detect_mode(argv0: &str) -> Mode {
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let name = base.strip_suffix(".exe").unwrap_or(base);
    match name.to_lowercase().as_str() {
        "yes" => Mode::Yes,
        "expr" => Mode::Expr,
        _ => Mode::Seq,
    }
}

// ── seq implementation ───────────────────────────────────────────

fn run_seq(args: &[String]) -> i32 {
    let mut separator = "\n".to_string();
    let mut format: Option<String> = None;
    let mut equal_width = false;
    let mut positional: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-s" | "--separator" => {
                i += 1;
                if i < args.len() {
                    separator = args[i].clone();
                }
            }
            "-f" | "--format" => {
                i += 1;
                if i < args.len() {
                    format = Some(args[i].clone());
                }
            }
            "-w" | "--equal-width" => {
                equal_width = true;
            }
            "--help" => {
                println!("Usage: seq [OPTION]... LAST");
                println!("  or:  seq [OPTION]... FIRST LAST");
                println!("  or:  seq [OPTION]... FIRST INCREMENT LAST");
                println!("Print numbers from FIRST to LAST, in steps of INCREMENT.");
                println!();
                println!("Options:");
                println!("  -f, --format=FORMAT  use printf-style FORMAT (default: %g)");
                println!("  -s, --separator=SEP  use SEP to separate numbers (default: \\n)");
                println!("  -w, --equal-width    pad with leading zeros to equal width");
                println!("      --help           display this help and exit");
                println!("      --version        output version information and exit");
                return 0;
            }
            "--version" => {
                println!("seq (OurOS) 0.1.0");
                return 0;
            }
            _ if arg.starts_with("--separator=") => {
                separator = arg.strip_prefix("--separator=").unwrap_or("").to_string();
            }
            _ if arg.starts_with("--format=") => {
                format = Some(arg.strip_prefix("--format=").unwrap_or("").to_string());
            }
            _ => {
                positional.push(arg.clone());
            }
        }
        i += 1;
    }

    let (first, increment, last) = match positional.len() {
        1 => {
            let last = match positional[0].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[0]);
                    return 1;
                }
            };
            (1.0_f64, 1.0_f64, last)
        }
        2 => {
            let first = match positional[0].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[0]);
                    return 1;
                }
            };
            let last = match positional[1].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[1]);
                    return 1;
                }
            };
            let inc = if first <= last { 1.0 } else { -1.0 };
            (first, inc, last)
        }
        3 => {
            let first = match positional[0].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[0]);
                    return 1;
                }
            };
            let inc = match positional[1].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[1]);
                    return 1;
                }
            };
            let last = match positional[2].parse::<f64>() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("seq: invalid floating point argument: '{}'", positional[2]);
                    return 1;
                }
            };
            (first, inc, last)
        }
        0 => {
            eprintln!("seq: missing operand");
            eprintln!("Try 'seq --help' for more information.");
            return 1;
        }
        _ => {
            eprintln!("seq: extra operand '{}'", positional[3]);
            return 1;
        }
    };

    if increment == 0.0 {
        eprintln!("seq: zero increment");
        return 1;
    }

    // Determine decimal places for formatting
    let decimal_places = |s: &str| -> usize {
        if let Some(dot) = s.find('.') {
            s.len() - dot - 1
        } else {
            0
        }
    };

    let max_decimals = positional
        .iter()
        .map(|s| decimal_places(s))
        .max()
        .unwrap_or(0);

    // Determine width for equal-width mode
    let format_number = |n: f64| -> String {
        if let Some(ref fmt) = format {
            // Simple %g/%f/%e support
            if fmt.contains("%g") || fmt.contains("%G") {
                fmt.replace("%g", &format_g(n, max_decimals))
                    .replace("%G", &format_g(n, max_decimals).to_uppercase())
            } else if fmt.contains("%f") || fmt.contains("%F") {
                fmt.replace("%f", &format!("{:.prec$}", n, prec = max_decimals))
                    .replace(
                        "%F",
                        &format!("{:.prec$}", n, prec = max_decimals).to_uppercase(),
                    )
            } else if fmt.contains("%e") || fmt.contains("%E") {
                fmt.replace("%e", &format!("{:e}", n))
                    .replace("%E", &format!("{:E}", n))
            } else {
                format_g(n, max_decimals)
            }
        } else {
            format_g(n, max_decimals)
        }
    };

    // Collect all numbers first for equal-width
    let mut numbers: Vec<f64> = Vec::new();
    let mut current = first;

    if increment > 0.0 {
        while current <= last + increment * 1e-10 {
            numbers.push(current);
            current += increment;
            // Prevent infinite loops from floating point
            if numbers.len() > 10_000_000 {
                break;
            }
        }
    } else {
        while current >= last + increment * 1e-10 {
            numbers.push(current);
            current += increment;
            if numbers.len() > 10_000_000 {
                break;
            }
        }
    }

    let max_width = if equal_width {
        numbers
            .iter()
            .map(|n| format_number(*n).len())
            .max()
            .unwrap_or(0)
    } else {
        0
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (idx, num) in numbers.iter().enumerate() {
        let formatted = format_number(*num);
        let output = if equal_width && formatted.len() < max_width {
            let padding = max_width - formatted.len();
            if let Some(rest) = formatted.strip_prefix('-') {
                format!("-{}{}", "0".repeat(padding), rest)
            } else {
                format!("{}{}", "0".repeat(padding), formatted)
            }
        } else {
            formatted
        };

        if idx > 0 {
            let _ = out.write_all(separator.as_bytes());
        }
        let _ = out.write_all(output.as_bytes());
    }

    if !numbers.is_empty() {
        let _ = out.write_all(b"\n");
    }

    0
}

fn format_g(n: f64, precision: usize) -> String {
    if precision == 0 {
        // Integer-like
        if n == n.floor() && n.abs() < 1e15 {
            format!("{}", n as i64)
        } else {
            format!("{}", n)
        }
    } else {
        format!("{:.prec$}", n, prec = precision)
    }
}

// ── yes implementation ───────────────────────────────────────────

fn run_yes(args: &[String]) -> i32 {
    if args.first().map(|s| s.as_str()) == Some("--help") {
        println!("Usage: yes [STRING]...");
        println!("Repeatedly output a line with all specified STRING(s), or 'y'.");
        return 0;
    }
    if args.first().map(|s| s.as_str()) == Some("--version") {
        println!("yes (OurOS) 0.1.0");
        return 0;
    }

    let output = if args.is_empty() {
        "y".to_string()
    } else {
        args.join(" ")
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Use a buffered approach for performance
    let line = format!("{}\n", output);
    let bytes = line.as_bytes();

    // Build a large buffer to reduce write syscalls
    let mut buf = Vec::with_capacity(8192);
    while buf.len() + bytes.len() <= 8192 {
        buf.extend_from_slice(bytes);
    }

    loop {
        if out.write_all(&buf).is_err() {
            break;
        }
    }

    0
}

// ── expr implementation ──────────────────────────────────────────

/// Token types for expr
#[derive(Debug, Clone, PartialEq)]
enum ExprToken {
    Number(i64),
    Str(String),
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Not,
    LParen,
    RParen,
    Colon,
    Match,
    Substr,
    Index,
    Length,
}

fn tokenize_expr(args: &[String]) -> Vec<ExprToken> {
    let mut tokens = Vec::new();

    for arg in args {
        let tok = match arg.as_str() {
            "+" => ExprToken::Plus,
            "-" => ExprToken::Minus,
            "*" => ExprToken::Multiply,
            "/" => ExprToken::Divide,
            "%" => ExprToken::Modulo,
            "=" => ExprToken::Eq,
            "!=" => ExprToken::Ne,
            "<" => ExprToken::Lt,
            "<=" => ExprToken::Le,
            ">" => ExprToken::Gt,
            ">=" => ExprToken::Ge,
            "&" => ExprToken::And,
            "|" => ExprToken::Or,
            "!" => ExprToken::Not,
            "(" => ExprToken::LParen,
            ")" => ExprToken::RParen,
            ":" => ExprToken::Colon,
            "match" => ExprToken::Match,
            "substr" => ExprToken::Substr,
            "index" => ExprToken::Index,
            "length" => ExprToken::Length,
            _ => {
                if let Ok(n) = arg.parse::<i64>() {
                    ExprToken::Number(n)
                } else {
                    ExprToken::Str(arg.clone())
                }
            }
        };
        tokens.push(tok);
    }

    tokens
}

/// Expr value: either integer or string
#[derive(Debug, Clone, PartialEq)]
enum ExprValue {
    Int(i64),
    Str(String),
}

impl ExprValue {
    fn as_int(&self) -> Option<i64> {
        match self {
            ExprValue::Int(n) => Some(*n),
            ExprValue::Str(s) => s.parse::<i64>().ok(),
        }
    }

    fn as_str(&self) -> String {
        match self {
            ExprValue::Int(n) => n.to_string(),
            ExprValue::Str(s) => s.clone(),
        }
    }

    fn is_null_or_zero(&self) -> bool {
        match self {
            ExprValue::Int(n) => *n == 0,
            ExprValue::Str(s) => s.is_empty(),
        }
    }
}

struct ExprParser {
    tokens: Vec<ExprToken>,
    pos: usize,
}

impl ExprParser {
    fn new(tokens: Vec<ExprToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&ExprToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<ExprToken> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: &ExprToken) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Parse: or_expr
    fn parse(&mut self) -> Result<ExprValue, String> {
        self.parse_or()
    }

    /// or_expr: and_expr ('|' and_expr)*
    fn parse_or(&mut self) -> Result<ExprValue, String> {
        let mut left = self.parse_and()?;

        while self.peek() == Some(&ExprToken::Or) {
            self.next();
            let right = self.parse_and()?;
            left = if !left.is_null_or_zero() { left } else { right };
        }

        Ok(left)
    }

    /// and_expr: cmp_expr ('&' cmp_expr)*
    fn parse_and(&mut self) -> Result<ExprValue, String> {
        let mut left = self.parse_compare()?;

        while self.peek() == Some(&ExprToken::And) {
            self.next();
            let right = self.parse_compare()?;
            left = if !left.is_null_or_zero() && !right.is_null_or_zero() {
                left
            } else {
                ExprValue::Int(0)
            };
        }

        Ok(left)
    }

    /// cmp_expr: add_expr (('='|'!='|'<'|'<='|'>'|'>=') add_expr)?
    fn parse_compare(&mut self) -> Result<ExprValue, String> {
        let left = self.parse_add()?;

        let op = match self.peek() {
            Some(ExprToken::Eq) => Some(ExprToken::Eq),
            Some(ExprToken::Ne) => Some(ExprToken::Ne),
            Some(ExprToken::Lt) => Some(ExprToken::Lt),
            Some(ExprToken::Le) => Some(ExprToken::Le),
            Some(ExprToken::Gt) => Some(ExprToken::Gt),
            Some(ExprToken::Ge) => Some(ExprToken::Ge),
            _ => None,
        };

        if let Some(op) = op {
            self.next();
            let right = self.parse_add()?;

            // Try numeric comparison first, fall back to string
            let result = match (left.as_int(), right.as_int()) {
                (Some(l), Some(r)) => match op {
                    ExprToken::Eq => l == r,
                    ExprToken::Ne => l != r,
                    ExprToken::Lt => l < r,
                    ExprToken::Le => l <= r,
                    ExprToken::Gt => l > r,
                    ExprToken::Ge => l >= r,
                    _ => false,
                },
                _ => {
                    let ls = left.as_str();
                    let rs = right.as_str();
                    match op {
                        ExprToken::Eq => ls == rs,
                        ExprToken::Ne => ls != rs,
                        ExprToken::Lt => ls < rs,
                        ExprToken::Le => ls <= rs,
                        ExprToken::Gt => ls > rs,
                        ExprToken::Ge => ls >= rs,
                        _ => false,
                    }
                }
            };

            Ok(ExprValue::Int(if result { 1 } else { 0 }))
        } else {
            Ok(left)
        }
    }

    /// add_expr: mul_expr (('+' | '-') mul_expr)*
    fn parse_add(&mut self) -> Result<ExprValue, String> {
        let mut left = self.parse_mul()?;

        loop {
            match self.peek() {
                Some(ExprToken::Plus) => {
                    self.next();
                    let right = self.parse_mul()?;
                    let l = left.as_int().ok_or("non-integer argument")?;
                    let r = right.as_int().ok_or("non-integer argument")?;
                    left = ExprValue::Int(l + r);
                }
                Some(ExprToken::Minus) => {
                    self.next();
                    let right = self.parse_mul()?;
                    let l = left.as_int().ok_or("non-integer argument")?;
                    let r = right.as_int().ok_or("non-integer argument")?;
                    left = ExprValue::Int(l - r);
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// mul_expr: colon_expr (('*' | '/' | '%') colon_expr)*
    fn parse_mul(&mut self) -> Result<ExprValue, String> {
        let mut left = self.parse_colon()?;

        loop {
            match self.peek() {
                Some(ExprToken::Multiply) => {
                    self.next();
                    let right = self.parse_colon()?;
                    let l = left.as_int().ok_or("non-integer argument")?;
                    let r = right.as_int().ok_or("non-integer argument")?;
                    left = ExprValue::Int(l * r);
                }
                Some(ExprToken::Divide) => {
                    self.next();
                    let right = self.parse_colon()?;
                    let l = left.as_int().ok_or("non-integer argument")?;
                    let r = right.as_int().ok_or("non-integer argument")?;
                    if r == 0 {
                        return Err("division by zero".to_string());
                    }
                    left = ExprValue::Int(l / r);
                }
                Some(ExprToken::Modulo) => {
                    self.next();
                    let right = self.parse_colon()?;
                    let l = left.as_int().ok_or("non-integer argument")?;
                    let r = right.as_int().ok_or("non-integer argument")?;
                    if r == 0 {
                        return Err("division by zero".to_string());
                    }
                    left = ExprValue::Int(l % r);
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// colon_expr: primary (':' primary)?
    fn parse_colon(&mut self) -> Result<ExprValue, String> {
        let left = self.parse_primary()?;

        if self.peek() == Some(&ExprToken::Colon) {
            self.next();
            let right = self.parse_primary()?;
            // Pattern match: left : right
            let text = left.as_str();
            let pattern = right.as_str();
            let matched = simple_regex_match(&text, &pattern);
            Ok(matched)
        } else {
            Ok(left)
        }
    }

    /// primary: NUMBER | STRING | '(' expr ')' | 'match' str pat |
    ///          'substr' str pos len | 'index' str chars | 'length' str
    fn parse_primary(&mut self) -> Result<ExprValue, String> {
        match self.peek().cloned() {
            Some(ExprToken::LParen) => {
                self.next();
                let val = self.parse()?;
                if !self.expect(&ExprToken::RParen) {
                    return Err("missing ')'".to_string());
                }
                Ok(val)
            }
            Some(ExprToken::Match) => {
                self.next();
                let text = self.parse_primary()?;
                let pattern = self.parse_primary()?;
                Ok(simple_regex_match(&text.as_str(), &pattern.as_str()))
            }
            Some(ExprToken::Substr) => {
                self.next();
                let text = self.parse_primary()?;
                let pos = self.parse_primary()?;
                let len = self.parse_primary()?;
                let s = text.as_str();
                let p = pos.as_int().unwrap_or(0) as usize;
                let l = len.as_int().unwrap_or(0) as usize;
                if p == 0 || p > s.len() {
                    Ok(ExprValue::Str(String::new()))
                } else {
                    let start = p - 1; // 1-based to 0-based
                    let end = (start + l).min(s.len());
                    Ok(ExprValue::Str(s[start..end].to_string()))
                }
            }
            Some(ExprToken::Index) => {
                self.next();
                let text = self.parse_primary()?;
                let chars = self.parse_primary()?;
                let s = text.as_str();
                let search_chars = chars.as_str();
                let pos = s
                    .chars()
                    .position(|c| search_chars.contains(c))
                    .map(|p| (p + 1) as i64)
                    .unwrap_or(0);
                Ok(ExprValue::Int(pos))
            }
            Some(ExprToken::Length) => {
                self.next();
                let text = self.parse_primary()?;
                Ok(ExprValue::Int(text.as_str().len() as i64))
            }
            Some(ExprToken::Number(n)) => {
                self.next();
                Ok(ExprValue::Int(n))
            }
            Some(ExprToken::Str(s)) => {
                self.next();
                Ok(ExprValue::Str(s))
            }
            Some(ExprToken::Not) => {
                self.next();
                let val = self.parse_primary()?;
                Ok(ExprValue::Int(if val.is_null_or_zero() { 1 } else { 0 }))
            }
            None => Err("missing operand".to_string()),
            Some(tok) => Err(format!("syntax error near '{:?}'", tok)),
        }
    }
}

/// Simple regex match for expr's : operator
/// Anchored at start. Returns matched substring if there's a \( \) group,
/// or the length of the match otherwise.
fn simple_regex_match(text: &str, pattern: &str) -> ExprValue {
    // expr's regex is always anchored at the start
    let pat_chars: Vec<char> = pattern.chars().collect();

    if let Some((matched_len, group)) = regex_match_expr(&pat_chars, 0, text, 0) {
        if let Some(g) = group {
            ExprValue::Str(g)
        } else {
            ExprValue::Int(matched_len as i64)
        }
    } else {
        // No match
        if pattern.contains("\\(") {
            ExprValue::Str(String::new())
        } else {
            ExprValue::Int(0)
        }
    }
}

/// Returns (match_length, optional_group_capture)
fn regex_match_expr(
    pat: &[char],
    pi: usize,
    text: &str,
    ti: usize,
) -> Option<(usize, Option<String>)> {
    if pi >= pat.len() {
        return Some((ti, None));
    }

    let text_bytes = text.as_bytes();

    // Check for group: \( ... \)
    if pi + 1 < pat.len() && pat[pi] == '\\' && pat[pi + 1] == '(' {
        // Find matching \)
        let group_start = ti;
        if let Some(close) = find_group_close(pat, pi + 2) {
            let inner = &pat[pi + 2..close];
            let rest = &pat[close + 2..]; // skip \)

            // Try all possible match lengths for the inner pattern. The inner
            // subexpression must consume the *entire* candidate substring
            // [group_start..end] (anchored, exact fit) — otherwise a short
            // pattern like `abc` would falsely "match" a longer slice such as
            // `abcdef` by only matching its prefix, capturing too much.
            for end in (group_start..=text.len()).rev() {
                let sub_text = &text[group_start..end];
                if let Some((consumed, _)) = regex_match_expr(inner, 0, sub_text, 0)
                    && consumed == sub_text.len()
                    && let Some((full_len, _)) = regex_match_expr(rest, 0, text, end)
                {
                    let _ = full_len;
                    return Some((end, Some(text[group_start..end].to_string())));
                }
            }
            return None;
        }
    }

    // Escaped char
    if pi + 1 < pat.len() && pat[pi] == '\\' && !matches!(pat[pi + 1], '(' | ')') {
        let escaped = pat[pi + 1];
        let next_pi = pi + 2;
        let has_star = next_pi < pat.len() && pat[next_pi] == '*';

        if has_star {
            // \X*
            let mut count = 0;
            while ti + count < text.len() && text_bytes[ti + count] as char == escaped {
                count += 1;
            }
            for n in (0..=count).rev() {
                if let Some(r) = regex_match_expr(pat, next_pi + 1, text, ti + n) {
                    return Some(r);
                }
            }
            return None;
        } else if ti < text.len() && text_bytes[ti] as char == escaped {
            return regex_match_expr(pat, next_pi, text, ti + 1);
        } else {
            return None;
        }
    }

    // Dot
    if pat[pi] == '.' {
        let has_star = pi + 1 < pat.len() && pat[pi + 1] == '*';
        if has_star {
            let mut count = 0;
            while ti + count < text.len() {
                count += 1;
            }
            for n in (0..=count).rev() {
                if let Some(r) = regex_match_expr(pat, pi + 2, text, ti + n) {
                    return Some(r);
                }
            }
            return None;
        } else if ti < text.len() {
            return regex_match_expr(pat, pi + 1, text, ti + 1);
        } else {
            return None;
        }
    }

    // Character class [...]
    if pat[pi] == '[' {
        let (negate, class_end, chars) = parse_bracket_class(pat, pi);
        let next_pi = class_end + 1;
        let has_star = next_pi < pat.len() && pat[next_pi] == '*';
        let matches_char = |c: char| -> bool {
            let found = chars.iter().any(|&(lo, hi)| c >= lo && c <= hi);
            if negate { !found } else { found }
        };

        if has_star {
            let mut count = 0;
            while ti + count < text.len() && matches_char(text_bytes[ti + count] as char) {
                count += 1;
            }
            for n in (0..=count).rev() {
                if let Some(r) = regex_match_expr(pat, next_pi + 1, text, ti + n) {
                    return Some(r);
                }
            }
            return None;
        } else if ti < text.len() && matches_char(text_bytes[ti] as char) {
            return regex_match_expr(pat, next_pi, text, ti + 1);
        } else {
            return None;
        }
    }

    // $ at end
    if pat[pi] == '$' && pi + 1 == pat.len() {
        if ti == text.len() {
            return Some((ti, None));
        }
        return None;
    }

    // Literal character
    let ch = pat[pi];
    let has_star = pi + 1 < pat.len() && pat[pi + 1] == '*';

    if has_star {
        let mut count = 0;
        while ti + count < text.len() && text_bytes[ti + count] as char == ch {
            count += 1;
        }
        for n in (0..=count).rev() {
            if let Some(r) = regex_match_expr(pat, pi + 2, text, ti + n) {
                return Some(r);
            }
        }
        None
    } else if ti < text.len() && text_bytes[ti] as char == ch {
        regex_match_expr(pat, pi + 1, text, ti + 1)
    } else {
        None
    }
}

fn find_group_close(pat: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i + 1 < pat.len() {
        if pat[i] == '\\' && pat[i + 1] == ')' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_bracket_class(pat: &[char], start: usize) -> (bool, usize, Vec<(char, char)>) {
    let mut i = start + 1;
    let negate = i < pat.len() && pat[i] == '^';
    if negate {
        i += 1;
    }

    let mut ranges = Vec::new();

    if i < pat.len() && pat[i] == ']' {
        ranges.push((']', ']'));
        i += 1;
    }

    while i < pat.len() && pat[i] != ']' {
        if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
            ranges.push((pat[i], pat[i + 2]));
            i += 3;
        } else {
            ranges.push((pat[i], pat[i]));
            i += 1;
        }
    }

    (negate, i, ranges)
}

fn run_expr(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("expr: missing operand");
        eprintln!("Try 'expr --help' for more information.");
        return 2;
    }

    if args.first().map(|s| s.as_str()) == Some("--help") {
        println!("Usage: expr EXPRESSION");
        println!("Evaluate EXPRESSION and print result.");
        println!();
        println!("Operations (in order of increasing precedence):");
        println!("  ARG1 | ARG2       return ARG1 if not null/zero, else ARG2");
        println!("  ARG1 & ARG2       return ARG1 if both non-null/non-zero, else 0");
        println!("  ARG1 OP ARG2      comparison: = != < <= > >=");
        println!("  ARG1 + ARG2       arithmetic addition");
        println!("  ARG1 - ARG2       arithmetic subtraction");
        println!("  ARG1 * ARG2       arithmetic multiplication");
        println!("  ARG1 / ARG2       arithmetic division");
        println!("  ARG1 % ARG2       arithmetic remainder");
        println!("  STRING : REGEX    anchored pattern match");
        println!("  match STRING RE   same as STRING : RE");
        println!("  substr STR P L    substring of STR, P is 1-based");
        println!("  index STR CHARS   index of first CHARS char in STR, or 0");
        println!("  length STR        length of STR");
        println!("  ( EXPRESSION )    grouping");
        return 0;
    }

    if args.first().map(|s| s.as_str()) == Some("--version") {
        println!("expr (OurOS) 0.1.0");
        return 0;
    }

    let tokens = tokenize_expr(args);
    let mut parser = ExprParser::new(tokens);

    match parser.parse() {
        Ok(val) => {
            println!("{}", val.as_str());
            if val.is_null_or_zero() { 1 } else { 0 }
        }
        Err(e) => {
            eprintln!("expr: {}", e);
            2
        }
    }
}

// ── main ─────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = detect_mode(args.first().map(|s| s.as_str()).unwrap_or("seq"));

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let exit_code = match mode {
        Mode::Seq => run_seq(&rest),
        Mode::Yes => run_yes(&rest),
        Mode::Expr => run_expr(&rest),
    };

    process::exit(exit_code);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Mode detection
    #[test]
    fn test_detect_seq() {
        assert_eq!(detect_mode("seq"), Mode::Seq);
        assert_eq!(detect_mode("/usr/bin/seq"), Mode::Seq);
    }

    #[test]
    fn test_detect_yes() {
        assert_eq!(detect_mode("yes"), Mode::Yes);
        assert_eq!(detect_mode("/bin/yes"), Mode::Yes);
    }

    #[test]
    fn test_detect_expr() {
        assert_eq!(detect_mode("expr"), Mode::Expr);
        assert_eq!(detect_mode("/usr/bin/expr"), Mode::Expr);
    }

    // format_g tests
    #[test]
    fn test_format_g_integer() {
        assert_eq!(format_g(5.0, 0), "5");
        assert_eq!(format_g(100.0, 0), "100");
        assert_eq!(format_g(-3.0, 0), "-3");
    }

    #[test]
    fn test_format_g_decimal() {
        assert_eq!(format_g(1.5, 1), "1.5");
        assert_eq!(format_g(1.25, 2), "1.25");
    }

    // expr tokenization
    #[test]
    fn test_tokenize_number() {
        let tokens = tokenize_expr(&["42".to_string()]);
        assert_eq!(tokens, vec![ExprToken::Number(42)]);
    }

    #[test]
    fn test_tokenize_ops() {
        let tokens = tokenize_expr(&["+".to_string(), "-".to_string(), "*".to_string()]);
        assert_eq!(
            tokens,
            vec![ExprToken::Plus, ExprToken::Minus, ExprToken::Multiply]
        );
    }

    #[test]
    fn test_tokenize_comparison() {
        let tokens = tokenize_expr(&["=".to_string(), "!=".to_string(), "<".to_string()]);
        assert_eq!(tokens, vec![ExprToken::Eq, ExprToken::Ne, ExprToken::Lt]);
    }

    #[test]
    fn test_tokenize_string() {
        let tokens = tokenize_expr(&["hello".to_string()]);
        assert_eq!(tokens, vec![ExprToken::Str("hello".to_string())]);
    }

    // expr evaluation
    #[test]
    fn test_expr_addition() {
        let tokens = tokenize_expr(&["3".to_string(), "+".to_string(), "4".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(7));
    }

    #[test]
    fn test_expr_subtraction() {
        let tokens = tokenize_expr(&["10".to_string(), "-".to_string(), "3".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(7));
    }

    #[test]
    fn test_expr_multiplication() {
        let tokens = tokenize_expr(&["6".to_string(), "*".to_string(), "7".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(42));
    }

    #[test]
    fn test_expr_division() {
        let tokens = tokenize_expr(&["15".to_string(), "/".to_string(), "3".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(5));
    }

    #[test]
    fn test_expr_modulo() {
        let tokens = tokenize_expr(&["17".to_string(), "%".to_string(), "5".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(2));
    }

    #[test]
    fn test_expr_comparison_eq() {
        let tokens = tokenize_expr(&["5".to_string(), "=".to_string(), "5".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(1));
    }

    #[test]
    fn test_expr_comparison_ne() {
        let tokens = tokenize_expr(&["5".to_string(), "!=".to_string(), "3".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(1));
    }

    #[test]
    fn test_expr_comparison_lt() {
        let tokens = tokenize_expr(&["3".to_string(), "<".to_string(), "5".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(1));
    }

    #[test]
    fn test_expr_or_nonzero() {
        let tokens = tokenize_expr(&["5".to_string(), "|".to_string(), "0".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(5));
    }

    #[test]
    fn test_expr_or_zero() {
        let tokens = tokenize_expr(&["0".to_string(), "|".to_string(), "3".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(3));
    }

    #[test]
    fn test_expr_and_both_nonzero() {
        let tokens = tokenize_expr(&["5".to_string(), "&".to_string(), "3".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(5));
    }

    #[test]
    fn test_expr_and_one_zero() {
        let tokens = tokenize_expr(&["5".to_string(), "&".to_string(), "0".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(0));
    }

    #[test]
    fn test_expr_length() {
        let tokens = tokenize_expr(&["length".to_string(), "hello".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(5));
    }

    #[test]
    fn test_expr_substr() {
        let tokens = tokenize_expr(&[
            "substr".to_string(),
            "hello".to_string(),
            "2".to_string(),
            "3".to_string(),
        ]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Str("ell".to_string()));
    }

    #[test]
    fn test_expr_index() {
        let tokens = tokenize_expr(&["index".to_string(), "hello".to_string(), "lo".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(3)); // 'l' is at position 3
    }

    #[test]
    fn test_expr_index_not_found() {
        let tokens = tokenize_expr(&["index".to_string(), "hello".to_string(), "xyz".to_string()]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(0));
    }

    // expr regex match
    #[test]
    fn test_expr_match_literal() {
        let result = simple_regex_match("abcdef", "abc");
        assert_eq!(result, ExprValue::Int(3));
    }

    #[test]
    fn test_expr_match_dot() {
        let result = simple_regex_match("abcdef", "a.c");
        assert_eq!(result, ExprValue::Int(3));
    }

    #[test]
    fn test_expr_match_star() {
        let result = simple_regex_match("aabbb", "a*b*");
        assert_eq!(result, ExprValue::Int(5));
    }

    #[test]
    fn test_expr_match_group() {
        let result = simple_regex_match("abcdef", "\\(abc\\)");
        assert_eq!(result, ExprValue::Str("abc".to_string()));
    }

    #[test]
    fn test_expr_match_no_match() {
        let result = simple_regex_match("abcdef", "xyz");
        assert_eq!(result, ExprValue::Int(0));
    }

    #[test]
    fn test_expr_match_dot_star() {
        let result = simple_regex_match("hello world", ".*");
        assert_eq!(result, ExprValue::Int(11));
    }

    // ExprValue tests
    #[test]
    fn test_value_is_null_zero() {
        assert!(ExprValue::Int(0).is_null_or_zero());
        assert!(ExprValue::Str(String::new()).is_null_or_zero());
        assert!(!ExprValue::Int(1).is_null_or_zero());
        assert!(!ExprValue::Str("hello".to_string()).is_null_or_zero());
    }

    #[test]
    fn test_value_as_int() {
        assert_eq!(ExprValue::Int(42).as_int(), Some(42));
        assert_eq!(ExprValue::Str("42".to_string()).as_int(), Some(42));
        assert_eq!(ExprValue::Str("hello".to_string()).as_int(), None);
    }

    #[test]
    fn test_value_as_str() {
        assert_eq!(ExprValue::Int(42).as_str(), "42");
        assert_eq!(ExprValue::Str("hello".to_string()).as_str(), "hello");
    }

    // Parens
    #[test]
    fn test_expr_parens() {
        let tokens = tokenize_expr(&[
            "(".to_string(),
            "3".to_string(),
            "+".to_string(),
            "4".to_string(),
            ")".to_string(),
            "*".to_string(),
            "2".to_string(),
        ]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(14));
    }

    // Division by zero
    #[test]
    fn test_expr_division_by_zero() {
        let tokens = tokenize_expr(&["5".to_string(), "/".to_string(), "0".to_string()]);
        let mut parser = ExprParser::new(tokens);
        assert!(parser.parse().is_err());
    }

    // Precedence
    #[test]
    fn test_expr_precedence() {
        // 2 + 3 * 4 = 14 (not 20)
        let tokens = tokenize_expr(&[
            "2".to_string(),
            "+".to_string(),
            "3".to_string(),
            "*".to_string(),
            "4".to_string(),
        ]);
        let mut parser = ExprParser::new(tokens);
        let result = parser.parse().unwrap();
        assert_eq!(result, ExprValue::Int(14));
    }

    // Bracket class
    #[test]
    fn test_bracket_class() {
        let result = simple_regex_match("abc", "[abc]*");
        assert_eq!(result, ExprValue::Int(3));
    }

    #[test]
    fn test_bracket_range() {
        let result = simple_regex_match("xyz", "[a-z]*");
        assert_eq!(result, ExprValue::Int(3));
    }

    #[test]
    fn test_negated_bracket() {
        let result = simple_regex_match("ABC", "[^a-z]*");
        assert_eq!(result, ExprValue::Int(3));
    }
}
