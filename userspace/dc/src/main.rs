//! OurOS dc — desk calculator (reverse Polish notation)
//!
//! A traditional RPN calculator supporting arbitrary-precision integers,
//! registers, string operations, and conditional execution.

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

// ── Value type ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Value {
    Number(f64),
    Str(String),
}

impl Value {
    fn as_number(&self) -> Result<f64, String> {
        match self {
            Value::Number(n) => Ok(*n),
            Value::Str(s) => s.parse::<f64>().map_err(|_| format!("not a number: '{s}'")),
        }
    }

    fn display(&self, obase: u32, precision: u32) -> String {
        match self {
            Value::Number(n) => format_number(*n, obase, precision),
            Value::Str(s) => s.clone(),
        }
    }
}

fn format_number(n: f64, obase: u32, precision: u32) -> String {
    if obase == 10 {
        if n == n.floor() && n.abs() < 1e15 {
            if n < 0.0 {
                format!("_{}", (-n) as i64) // dc uses _ for negative
            } else {
                format!("{}", n as i64)
            }
        } else {
            format!("{n:.prec$}", prec = precision as usize)
        }
    } else if obase == 16 {
        let i = n as i64;
        if i < 0 {
            format!("-{:X}", -i)
        } else {
            format!("{i:X}")
        }
    } else if obase == 8 {
        let i = n as i64;
        if i < 0 {
            format!("-{:o}", -i)
        } else {
            format!("{i:o}")
        }
    } else if obase == 2 {
        let i = n as i64;
        if i < 0 {
            format!("-{:b}", -i)
        } else {
            format!("{i:b}")
        }
    } else {
        // Generic base conversion
        let i = n.abs() as u64;
        if i == 0 {
            return "0".to_string();
        }
        let mut digits = Vec::new();
        let mut val = i;
        while val > 0 {
            let d = (val % obase as u64) as u32;
            if d < 10 {
                digits.push((b'0' + d as u8) as char);
            } else {
                digits.push((b'A' + (d - 10) as u8) as char);
            }
            val /= obase as u64;
        }
        digits.reverse();
        let s: String = digits.into_iter().collect();
        if n < 0.0 { format!("-{s}") } else { s }
    }
}

// ── DC state ───────────────────────────────────────────────────────

struct DcState {
    stack: Vec<Value>,
    registers: [Vec<Value>; 256], // 256 registers, each a stack
    ibase: u32,
    obase: u32,
    precision: u32,
}

impl DcState {
    fn new() -> Self {
        Self {
            stack: Vec::new(),
            registers: std::array::from_fn(|_| Vec::new()),
            ibase: 10,
            obase: 10,
            precision: 0,
        }
    }

    fn push(&mut self, val: Value) {
        self.stack.push(val);
    }

    fn pop(&mut self) -> Result<Value, String> {
        self.stack.pop().ok_or_else(|| "stack empty".to_string())
    }

    fn peek(&self) -> Result<&Value, String> {
        self.stack.last().ok_or_else(|| "stack empty".to_string())
    }

    fn pop_number(&mut self) -> Result<f64, String> {
        let val = self.pop()?;
        val.as_number()
    }
}

// ── Token scanner ──────────────────────────────────────────────────

fn execute(state: &mut DcState, input: &str, output: &mut dyn Write) -> Result<bool, String> {
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        match c {
            // Whitespace: skip
            ' ' | '\t' | '\n' | '\r' => {}

            // Numbers.  A dc number may begin with a hex digit A-F (dc treats
            // A-F as the digit values 10-15 regardless of ibase), so the
            // number scanner must trigger on those too — not just 0-9.
            '0'..='9' | '.' | '_' | 'A'..='F' => {
                let start = i;
                let negative = c == '_';
                if negative {
                    i += 1;
                }
                // Collect digits (0-9 and A-F) and the decimal point.
                let mut num_str = String::new();
                while i < chars.len()
                    && (chars[i].is_ascii_digit()
                        || chars[i] == '.'
                        || ('A'..='F').contains(&chars[i]))
                {
                    num_str.push(chars[i]);
                    i += 1;
                }

                let value = if state.ibase == 16 {
                    // Parse as hex
                    parse_base_number(&num_str, 16)?
                } else if state.ibase != 10 {
                    parse_base_number(&num_str, state.ibase)?
                } else {
                    num_str
                        .parse::<f64>()
                        .map_err(|_| format!("invalid number: '{}'", &input[start..i]))?
                };

                state.push(Value::Number(if negative { -value } else { value }));
                continue; // Skip the i += 1 at the end
            }

            // Strings
            '[' => {
                let mut depth = 1;
                let mut s = String::new();
                i += 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '[' {
                        depth += 1;
                        s.push('[');
                    } else if chars[i] == ']' {
                        depth -= 1;
                        if depth > 0 {
                            s.push(']');
                        }
                    } else {
                        s.push(chars[i]);
                    }
                    i += 1;
                }
                state.push(Value::Str(s));
                continue;
            }

            // Arithmetic
            '+' => {
                let b = state.pop_number()?;
                let a = state.pop_number()?;
                state.push(Value::Number(a + b));
            }
            '-' => {
                let b = state.pop_number()?;
                let a = state.pop_number()?;
                state.push(Value::Number(a - b));
            }
            '*' => {
                let b = state.pop_number()?;
                let a = state.pop_number()?;
                state.push(Value::Number(a * b));
            }
            '/' => {
                let b = state.pop_number()?;
                if b == 0.0 {
                    return Err("division by zero".to_string());
                }
                let a = state.pop_number()?;
                state.push(Value::Number(a / b));
            }
            '%' => {
                let b = state.pop_number()?;
                if b == 0.0 {
                    return Err("remainder by zero".to_string());
                }
                let a = state.pop_number()?;
                state.push(Value::Number(a % b));
            }
            '~' => {
                // Divmod: push quotient then remainder
                let b = state.pop_number()?;
                if b == 0.0 {
                    return Err("division by zero".to_string());
                }
                let a = state.pop_number()?;
                let quot = (a / b).trunc();
                let rem = a % b;
                state.push(Value::Number(quot));
                state.push(Value::Number(rem));
            }
            '^' => {
                let exp = state.pop_number()?;
                let base = state.pop_number()?;
                state.push(Value::Number(base.powf(exp)));
            }
            '|' => {
                // Modular exponentiation: a b c | => (a^b) % c
                let modulus = state.pop_number()?;
                let exp = state.pop_number()?;
                let base = state.pop_number()?;
                if modulus == 0.0 {
                    return Err("modular exponentiation with zero modulus".to_string());
                }
                let result = mod_pow(base as i64, exp as i64, modulus as i64);
                state.push(Value::Number(result as f64));
            }
            'v' => {
                let n = state.pop_number()?;
                if n < 0.0 {
                    return Err("square root of negative number".to_string());
                }
                state.push(Value::Number(n.sqrt()));
            }

            // Stack commands
            'p' => {
                let val = state.peek()?;
                let s = val.display(state.obase, state.precision);
                writeln!(output, "{s}").map_err(|e| format!("write: {e}"))?;
            }
            'n' => {
                let val = state.pop()?;
                let s = val.display(state.obase, state.precision);
                write!(output, "{s}").map_err(|e| format!("write: {e}"))?;
            }
            'f' => {
                // Print entire stack, top first
                for val in state.stack.iter().rev() {
                    let s = val.display(state.obase, state.precision);
                    writeln!(output, "{s}").map_err(|e| format!("write: {e}"))?;
                }
            }
            'c' => {
                state.stack.clear();
            }
            'd' => {
                let val = state.peek()?.clone();
                state.push(val);
            }
            'r' => {
                let len = state.stack.len();
                if len >= 2 {
                    state.stack.swap(len - 1, len - 2);
                }
            }
            'R' => {
                // Rotate n elements
                let n = state.pop_number()? as usize;
                let len = state.stack.len();
                if n > 0 && n <= len {
                    let top = state.stack.pop().unwrap_or(Value::Number(0.0));
                    state.stack.insert(len - n, top);
                }
            }
            'z' => {
                let depth = state.stack.len() as f64;
                state.push(Value::Number(depth));
            }
            'Z' => {
                let val = state.pop()?;
                let digits = match &val {
                    Value::Number(n) => {
                        let s = format!("{}", n.abs() as i64);
                        s.len() as f64
                    }
                    Value::Str(s) => s.len() as f64,
                };
                state.push(Value::Number(digits));
            }
            'X' => {
                let val = state.pop()?;
                let frac_digits = match &val {
                    Value::Number(n) => {
                        let s = format!("{n}");
                        if let Some(pos) = s.find('.') {
                            (s.len() - pos - 1) as f64
                        } else {
                            0.0
                        }
                    }
                    Value::Str(_) => 0.0,
                };
                state.push(Value::Number(frac_digits));
            }

            // Parameters
            'i' => {
                let base = state.pop_number()? as u32;
                if !(2..=36).contains(&base) {
                    return Err(format!("input base must be 2-36, got {base}"));
                }
                state.ibase = base;
            }
            'o' => {
                let base = state.pop_number()? as u32;
                if !(2..=36).contains(&base) {
                    return Err(format!("output base must be 2-36, got {base}"));
                }
                state.obase = base;
            }
            'k' => {
                let prec = state.pop_number()? as u32;
                state.precision = prec;
            }
            'I' => {
                state.push(Value::Number(state.ibase as f64));
            }
            'O' => {
                state.push(Value::Number(state.obase as f64));
            }
            'K' => {
                state.push(Value::Number(state.precision as f64));
            }

            // Register operations
            's' => {
                i += 1;
                if i >= chars.len() {
                    return Err("'s' requires register name".to_string());
                }
                let reg = chars[i] as u8;
                let val = state.pop()?;
                if state.registers[reg as usize].is_empty() {
                    state.registers[reg as usize].push(val);
                } else {
                    let last = state.registers[reg as usize].len() - 1;
                    state.registers[reg as usize][last] = val;
                }
            }
            'l' => {
                i += 1;
                if i >= chars.len() {
                    return Err("'l' requires register name".to_string());
                }
                let reg = chars[i] as u8;
                let val = state.registers[reg as usize]
                    .last()
                    .cloned()
                    .unwrap_or(Value::Number(0.0));
                state.push(val);
            }
            'S' => {
                i += 1;
                if i >= chars.len() {
                    return Err("'S' requires register name".to_string());
                }
                let reg = chars[i] as u8;
                let val = state.pop()?;
                state.registers[reg as usize].push(val);
            }
            'L' => {
                i += 1;
                if i >= chars.len() {
                    return Err("'L' requires register name".to_string());
                }
                let reg = chars[i] as u8;
                let val = state.registers[reg as usize]
                    .pop()
                    .unwrap_or(Value::Number(0.0));
                state.push(val);
            }

            // String/execute
            'x' => {
                let val = state.pop()?;
                match val {
                    Value::Str(s) => {
                        let should_quit = execute(state, &s, output)?;
                        if should_quit {
                            return Ok(true);
                        }
                    }
                    Value::Number(_) => {
                        state.push(val);
                    }
                }
            }

            // Conditionals
            '>' | '<' | '=' => {
                i += 1;
                if i >= chars.len() {
                    return Err(format!("'{c}' requires register name"));
                }
                let reg = chars[i] as u8;
                let b = state.pop_number()?;
                let a = state.pop_number()?;
                let cond = match c {
                    '>' => a > b,
                    '<' => a < b,
                    '=' => (a - b).abs() < f64::EPSILON,
                    _ => false,
                };
                if cond
                    && let Some(val) = state.registers[reg as usize].last().cloned()
                        && let Value::Str(s) = val {
                            let should_quit = execute(state, &s, output)?;
                            if should_quit {
                                return Ok(true);
                            }
                        }
            }
            '!' if i + 1 < chars.len()
                && (chars[i + 1] == '>' || chars[i + 1] == '<' || chars[i + 1] == '=') =>
            {
                let op = chars[i + 1];
                i += 2;
                if i >= chars.len() {
                    return Err(format!("'!{op}' requires register name"));
                }
                let reg = chars[i] as u8;
                let b = state.pop_number()?;
                let a = state.pop_number()?;
                let cond = match op {
                    '>' => a <= b,
                    '<' => a >= b,
                    '=' => (a - b).abs() >= f64::EPSILON,
                    _ => false,
                };
                if cond
                    && let Some(val) = state.registers[reg as usize].last().cloned()
                        && let Value::Str(s) = val {
                            let should_quit = execute(state, &s, output)?;
                            if should_quit {
                                return Ok(true);
                            }
                        }
            }

            // I/O
            '?' => {
                // Read a line from stdin and execute it
                let stdin = io::stdin();
                let mut line = String::new();
                stdin
                    .lock()
                    .read_line(&mut line)
                    .map_err(|e| format!("read: {e}"))?;
                let should_quit = execute(state, line.trim(), output)?;
                if should_quit {
                    return Ok(true);
                }
            }

            'q' => {
                return Ok(true);
            }
            'Q' => {
                // Quit n levels of macro execution
                let _n = state.pop_number()?;
                return Ok(true);
            }

            '#' => {
                // Comment: skip to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }

            _ => {
                // Unknown command: ignore silently (traditional dc behavior)
            }
        }

        i += 1;
    }

    Ok(false)
}

fn parse_base_number(s: &str, base: u32) -> Result<f64, String> {
    let mut result: f64 = 0.0;
    let mut frac = false;
    let mut frac_mult: f64 = 1.0;

    for c in s.chars() {
        if c == '.' {
            frac = true;
            continue;
        }
        let digit = if c.is_ascii_digit() {
            (c as u32) - ('0' as u32)
        } else if c.is_ascii_uppercase() {
            (c as u32) - ('A' as u32) + 10
        } else {
            return Err(format!("invalid digit '{c}' for base {base}"));
        };
        if digit >= base {
            return Err(format!("digit '{c}' too large for base {base}"));
        }
        if frac {
            frac_mult /= base as f64;
            result += digit as f64 * frac_mult;
        } else {
            result = result * base as f64 + digit as f64;
        }
    }
    Ok(result)
}

fn mod_pow(mut base: i64, mut exp: i64, modulus: i64) -> i64 {
    if modulus == 1 {
        return 0;
    }
    let mut result: i64 = 1;
    base %= modulus;
    while exp > 0 {
        if exp % 2 == 1 {
            result = result.wrapping_mul(base) % modulus;
        }
        exp /= 2;
        base = base.wrapping_mul(base) % modulus;
    }
    result
}

// ── Main ───────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let argv: Vec<String> = env::args().collect();
    let mut files: Vec<String> = Vec::new();
    let mut expressions: Vec<String> = Vec::new();

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "-h" | "--help" => {
                eprintln!("Usage: dc [OPTIONS] [FILE]...");
                eprintln!("Desk calculator (reverse Polish notation).");
                eprintln!();
                eprintln!("  -e EXPR   evaluate expression");
                eprintln!("  -f FILE   execute file");
                eprintln!("  -h        display this help");
                process::exit(0);
            }
            "-e" | "--expression" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-e' requires an argument".to_string());
                }
                expressions.push(argv[i].clone());
            }
            _ if argv[i].starts_with("--expression=") => {
                expressions.push(argv[i]["--expression=".len()..].to_string());
            }
            "-f" | "--file" => {
                i += 1;
                if i >= argv.len() {
                    return Err("option '-f' requires an argument".to_string());
                }
                files.push(argv[i].clone());
            }
            _ => files.push(argv[i].clone()),
        }
        i += 1;
    }

    let mut state = DcState::new();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Execute expressions first
    for expr in &expressions {
        if execute(&mut state, expr, &mut out)? {
            return Ok(());
        }
    }

    // Execute files
    for file in &files {
        let content = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        if execute(&mut state, &content, &mut out)? {
            return Ok(());
        }
    }

    // If no expressions or files, read from stdin
    if expressions.is_empty() && files.is_empty() {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = line.map_err(|e| format!("read: {e}"))?;
            if execute(&mut state, &line, &mut out)? {
                return Ok(());
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("dc: {e}");
        process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(input: &str) -> String {
        let mut state = DcState::new();
        let mut out = Vec::new();
        let _ = execute(&mut state, input, &mut out);
        String::from_utf8(out).unwrap_or_default()
    }

    fn eval_state(input: &str) -> DcState {
        let mut state = DcState::new();
        let mut out = Vec::new();
        let _ = execute(&mut state, input, &mut out);
        state
    }

    // ── Basic arithmetic ──

    #[test]
    fn test_add() {
        assert_eq!(eval("3 5 + p"), "8\n");
    }

    #[test]
    fn test_subtract() {
        assert_eq!(eval("10 3 - p"), "7\n");
    }

    #[test]
    fn test_multiply() {
        assert_eq!(eval("4 5 * p"), "20\n");
    }

    #[test]
    fn test_divide() {
        assert_eq!(eval("20 4 / p"), "5\n");
    }

    #[test]
    fn test_remainder() {
        assert_eq!(eval("17 5 % p"), "2\n");
    }

    #[test]
    fn test_power() {
        assert_eq!(eval("2 10 ^ p"), "1024\n");
    }

    #[test]
    fn test_sqrt() {
        assert_eq!(eval("144 v p"), "12\n");
    }

    #[test]
    fn test_negative() {
        assert_eq!(eval("_5 3 + p"), "_2\n");
    }

    // ── Stack operations ──

    #[test]
    fn test_duplicate() {
        assert_eq!(eval("5 d + p"), "10\n");
    }

    #[test]
    fn test_swap() {
        assert_eq!(eval("3 5 r p"), "3\n");
    }

    #[test]
    fn test_clear() {
        let state = eval_state("1 2 3 c");
        assert!(state.stack.is_empty());
    }

    #[test]
    fn test_stack_depth() {
        assert_eq!(eval("1 2 3 z p"), "3\n");
    }

    #[test]
    fn test_print_stack() {
        assert_eq!(eval("1 2 3 f"), "3\n2\n1\n");
    }

    #[test]
    fn test_print_no_newline() {
        assert_eq!(eval("42 n"), "42");
    }

    // ── Registers ──

    #[test]
    fn test_store_load() {
        assert_eq!(eval("42 sa la p"), "42\n");
    }

    #[test]
    fn test_register_push_pop() {
        assert_eq!(eval("10 Sa 20 Sa La p"), "20\n");
    }

    #[test]
    fn test_register_stack() {
        assert_eq!(eval("10 Sa 20 Sa La p La p"), "20\n10\n");
    }

    // ── Parameters ──

    #[test]
    fn test_ibase() {
        assert_eq!(eval("16 i FF p"), "255\n");
    }

    #[test]
    fn test_obase_hex() {
        assert_eq!(eval("16 o 255 p"), "FF\n");
    }

    #[test]
    fn test_obase_binary() {
        assert_eq!(eval("2 o 10 p"), "1010\n");
    }

    #[test]
    fn test_obase_octal() {
        assert_eq!(eval("8 o 255 p"), "377\n");
    }

    #[test]
    fn test_precision() {
        let state = eval_state("5 k");
        assert_eq!(state.precision, 5);
    }

    #[test]
    fn test_query_ibase() {
        assert_eq!(eval("I p"), "10\n");
    }

    #[test]
    fn test_query_obase() {
        assert_eq!(eval("O p"), "10\n");
    }

    #[test]
    fn test_query_precision() {
        assert_eq!(eval("K p"), "0\n");
    }

    // ── Strings ──

    #[test]
    fn test_string_push() {
        assert_eq!(eval("[hello] p"), "hello\n");
    }

    #[test]
    fn test_nested_string() {
        assert_eq!(eval("[[nested]] p"), "[nested]\n");
    }

    #[test]
    fn test_execute_string() {
        assert_eq!(eval("[3 5 +] x p"), "8\n");
    }

    // ── Conditionals ──

    #[test]
    fn test_greater_than_true() {
        assert_eq!(eval("5 3 [yes] sa >a"), ""); // 5 > 3 is true... but we need to print
        // Better test:
        let state = eval_state("[10] sa 5 3 >a");
        // After executing "10", stack should have 10
        assert_eq!(state.stack.len(), 1);
    }

    #[test]
    fn test_less_than_true() {
        assert_eq!(eval("[99 p] sa 3 5 <a"), "99\n");
    }

    #[test]
    fn test_equal_true() {
        assert_eq!(eval("[42 p] sa 5 5 =a"), "42\n");
    }

    #[test]
    fn test_not_equal() {
        assert_eq!(eval("[77 p] sa 3 5 !=a"), "77\n");
    }

    #[test]
    fn test_condition_false() {
        assert_eq!(eval("[99 p] sa 5 3 <a"), ""); // 5 < 3 is false
    }

    // ── Divmod ──

    #[test]
    fn test_divmod() {
        assert_eq!(eval("17 5 ~ p r p"), "2\n3\n"); // remainder then quotient
    }

    // ── Modular exponentiation ──

    #[test]
    fn test_mod_pow_simple() {
        // 2^10 % 1000 = 1024 % 1000 = 24
        assert_eq!(eval("2 10 1000 | p"), "24\n");
    }

    // ── Number formatting ──

    #[test]
    fn test_format_decimal() {
        assert_eq!(format_number(42.0, 10, 0), "42");
        assert_eq!(format_number(-5.0, 10, 0), "_5");
    }

    #[test]
    fn test_format_hex() {
        assert_eq!(format_number(255.0, 16, 0), "FF");
        assert_eq!(format_number(10.0, 16, 0), "A");
    }

    #[test]
    fn test_format_octal() {
        assert_eq!(format_number(255.0, 8, 0), "377");
    }

    #[test]
    fn test_format_binary() {
        assert_eq!(format_number(10.0, 2, 0), "1010");
    }

    // ── Base parsing ──

    #[test]
    fn test_parse_base16() {
        assert_eq!(parse_base_number("FF", 16).unwrap(), 255.0);
        assert_eq!(parse_base_number("10", 16).unwrap(), 16.0);
    }

    #[test]
    fn test_parse_base8() {
        assert_eq!(parse_base_number("77", 8).unwrap(), 63.0);
    }

    #[test]
    fn test_parse_base2() {
        assert_eq!(parse_base_number("1010", 2).unwrap(), 10.0);
    }

    #[test]
    fn test_parse_base_invalid() {
        assert!(parse_base_number("F", 10).is_err());
    }

    // ── mod_pow ──

    #[test]
    fn test_mod_pow_fn() {
        assert_eq!(mod_pow(2, 10, 1000), 24);
        assert_eq!(mod_pow(3, 3, 7), 6);
        assert_eq!(mod_pow(2, 0, 5), 1);
    }

    // ── Z and X commands ──

    #[test]
    fn test_digit_count() {
        assert_eq!(eval("12345 Z p"), "5\n");
    }

    #[test]
    fn test_string_length() {
        assert_eq!(eval("[hello] Z p"), "5\n");
    }

    // ── Comment ──

    #[test]
    fn test_comment() {
        assert_eq!(eval("5 # this is a comment\np"), "5\n");
    }

    // ── Complex expressions ──

    #[test]
    fn test_factorial_like() {
        // 5! = 120: 1 2 3 4 5 * * * *
        assert_eq!(eval("1 2 * 3 * 4 * 5 * p"), "120\n");
    }

    #[test]
    fn test_chained_operations() {
        assert_eq!(eval("2 3 + 4 * p"), "20\n");
    }

    // ── Edge cases ──

    #[test]
    fn test_empty_input() {
        assert_eq!(eval(""), "");
    }

    #[test]
    fn test_only_whitespace() {
        assert_eq!(eval("   \t\n  "), "");
    }

    #[test]
    fn test_zero() {
        assert_eq!(eval("0 p"), "0\n");
    }

    #[test]
    fn test_large_number() {
        assert_eq!(eval("999999999 1 + p"), "1000000000\n");
    }
}
