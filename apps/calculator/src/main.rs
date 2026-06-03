//! OurOS Calculator
//!
//! Graphical scientific calculator with:
//! - Standard and Scientific modes (toggle between them)
//! - Proper operator precedence via recursive descent parser
//! - History of last 20 calculations
//! - Memory operations (M+, M-, MR, MC, MS)
//! - Degree/Radian toggle for trigonometric functions
//! - Keyboard shortcuts (numpad, Enter=equals, Escape=clear)
//! - Comprehensive error handling (division by zero, overflow, invalid input)
//!
//! Uses the guitk library for UI rendering.

#[allow(unused_imports)]
use guitk::color::Color;
#[allow(unused_imports)]
use guitk::event::{Event, EventResult, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
#[allow(unused_imports)]
use guitk::layout::{FlexAlign, FlexDirection, FlexItem, FlexJustify, SizeConstraint};
#[allow(unused_imports)]
use guitk::render::RenderTree;
#[allow(unused_imports)]
use guitk::style::{Borders, CornerRadii, Edges, FontWeight, Style, TextAlign};
#[allow(unused_imports)]
use guitk::widget::{Widget, WidgetId, WidgetTree};

use std::collections::VecDeque;
use std::f64::consts::{E, PI};

// ============================================================================
// Calculator modes
// ============================================================================

/// The calculator can operate in Standard or Scientific mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcMode {
    Standard,
    Scientific,
}

/// Angle unit for trigonometric functions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AngleUnit {
    Degrees,
    Radians,
}

// ============================================================================
// Expression parser — token types
// ============================================================================

/// Tokens produced by the lexer for the expression parser.
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Plus,
    Minus,
    Multiply,
    Divide,
    Modulo,
    Power,
    LeftParen,
    RightParen,
    Func(MathFunc),
}

/// Built-in mathematical functions recognized by the parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MathFunc {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Ln,
    Log10,
    Sqrt,
    Abs,
    Floor,
    Ceil,
    Exp,
    Factorial,
}

// ============================================================================
// Lexer
// ============================================================================

/// Tokenize an expression string into a sequence of tokens.
///
/// Returns `None` if the input contains unrecognized characters or
/// malformed numbers.
fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let ch = *chars.get(i)?;

        // Skip whitespace.
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Numbers (including decimals).
        if ch.is_ascii_digit() || ch == '.' {
            let start = i;
            let mut has_dot = ch == '.';
            i += 1;
            while i < len {
                let c = *chars.get(i)?;
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == '.' && !has_dot {
                    has_dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            let num_str: String = chars[start..i].iter().collect();
            let value = num_str.parse::<f64>().ok()?;
            tokens.push(Token::Number(value));
            continue;
        }

        // Operators and parentheses.
        match ch {
            '+' => { tokens.push(Token::Plus); i += 1; }
            '-' => { tokens.push(Token::Minus); i += 1; }
            '*' => { tokens.push(Token::Multiply); i += 1; }
            '/' => { tokens.push(Token::Divide); i += 1; }
            '%' => { tokens.push(Token::Modulo); i += 1; }
            '^' => { tokens.push(Token::Power); i += 1; }
            '(' => { tokens.push(Token::LeftParen); i += 1; }
            ')' => { tokens.push(Token::RightParen); i += 1; }
            _ if ch.is_ascii_alphabetic() => {
                // Parse identifiers (function names, constants).
                let start = i;
                while i < len && chars.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match word.as_str() {
                    "sin" => tokens.push(Token::Func(MathFunc::Sin)),
                    "cos" => tokens.push(Token::Func(MathFunc::Cos)),
                    "tan" => tokens.push(Token::Func(MathFunc::Tan)),
                    "asin" => tokens.push(Token::Func(MathFunc::Asin)),
                    "acos" => tokens.push(Token::Func(MathFunc::Acos)),
                    "atan" => tokens.push(Token::Func(MathFunc::Atan)),
                    "ln" => tokens.push(Token::Func(MathFunc::Ln)),
                    "log" => tokens.push(Token::Func(MathFunc::Log10)),
                    "sqrt" => tokens.push(Token::Func(MathFunc::Sqrt)),
                    "abs" => tokens.push(Token::Func(MathFunc::Abs)),
                    "floor" => tokens.push(Token::Func(MathFunc::Floor)),
                    "ceil" => tokens.push(Token::Func(MathFunc::Ceil)),
                    "exp" => tokens.push(Token::Func(MathFunc::Exp)),
                    "fact" => tokens.push(Token::Func(MathFunc::Factorial)),
                    "pi" => tokens.push(Token::Number(PI)),
                    "e" => tokens.push(Token::Number(E)),
                    _ => return None, // Unknown identifier.
                }
            }
            _ => return None, // Unrecognized character.
        }
    }

    Some(tokens)
}

// ============================================================================
// Recursive descent parser
// ============================================================================

/// Parser state: consumes tokens left-to-right and builds an evaluated result.
///
/// Grammar (by descending precedence):
///
/// ```text
/// expr     = term (('+' | '-') term)*
/// term     = power (('*' | '/' | '%') power)*
/// power    = unary ('^' power)?          // right-associative
/// unary    = ('-' | '+') unary | call
/// call     = FUNC '(' expr ')' | primary
/// primary  = NUMBER | '(' expr ')'
/// ```
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    angle_unit: AngleUnit,
}

impl Parser {
    fn new(tokens: Vec<Token>, angle_unit: AngleUnit) -> Self {
        Self {
            tokens,
            pos: 0,
            angle_unit,
        }
    }

    /// Peek at the current token without consuming it.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Consume the current token and advance.
    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Evaluate the entire expression, returning the result or an error string.
    fn parse(&mut self) -> Result<f64, &'static str> {
        let result = self.expr()?;
        if self.pos < self.tokens.len() {
            return Err("Unexpected token");
        }
        Ok(result)
    }

    /// expr = term (('+' | '-') term)*
    fn expr(&mut self) -> Result<f64, &'static str> {
        let mut left = self.term()?;

        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    let right = self.term()?;
                    left += right;
                }
                Some(Token::Minus) => {
                    self.advance();
                    let right = self.term()?;
                    left -= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// term = power (('*' | '/' | '%') power)*
    fn term(&mut self) -> Result<f64, &'static str> {
        let mut left = self.power()?;

        loop {
            match self.peek() {
                Some(Token::Multiply) => {
                    self.advance();
                    let right = self.power()?;
                    left *= right;
                }
                Some(Token::Divide) => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        return Err("Division by zero");
                    }
                    left /= right;
                }
                Some(Token::Modulo) => {
                    self.advance();
                    let right = self.power()?;
                    if right == 0.0 {
                        return Err("Division by zero");
                    }
                    left %= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    /// power = unary ('^' power)?   — right-associative
    fn power(&mut self) -> Result<f64, &'static str> {
        let base = self.unary()?;

        if matches!(self.peek(), Some(Token::Power)) {
            self.advance();
            let exponent = self.power()?; // Right-associative recursion.
            let result = base.powf(exponent);
            if result.is_infinite() || result.is_nan() {
                return Err("Overflow");
            }
            Ok(result)
        } else {
            Ok(base)
        }
    }

    /// unary = ('-' | '+') unary | call
    fn unary(&mut self) -> Result<f64, &'static str> {
        match self.peek() {
            Some(Token::Minus) => {
                // Distinguish unary minus from subtraction by checking if this is
                // the start of the expression or follows an operator/left-paren.
                self.advance();
                let val = self.unary()?;
                Ok(-val)
            }
            Some(Token::Plus) => {
                self.advance();
                self.unary()
            }
            _ => self.call(),
        }
    }

    /// call = FUNC '(' expr ')' | primary
    fn call(&mut self) -> Result<f64, &'static str> {
        if let Some(Token::Func(func)) = self.peek().cloned() {
            self.advance();
            // Expect '(' after function name.
            if !matches!(self.peek(), Some(Token::LeftParen)) {
                return Err("Expected '(' after function");
            }
            self.advance(); // consume '('
            let arg = self.expr()?;
            if !matches!(self.peek(), Some(Token::RightParen)) {
                return Err("Missing ')'");
            }
            self.advance(); // consume ')'
            self.apply_func(func, arg)
        } else {
            self.primary()
        }
    }

    /// primary = NUMBER | '(' expr ')'
    fn primary(&mut self) -> Result<f64, &'static str> {
        match self.peek().cloned() {
            Some(Token::Number(n)) => {
                self.advance();
                Ok(n)
            }
            Some(Token::LeftParen) => {
                self.advance(); // consume '('
                let val = self.expr()?;
                if !matches!(self.peek(), Some(Token::RightParen)) {
                    return Err("Missing ')'");
                }
                self.advance(); // consume ')'
                Ok(val)
            }
            Some(_) => Err("Unexpected token"),
            None => Err("Unexpected end of expression"),
        }
    }

    /// Apply a mathematical function to its argument.
    fn apply_func(&self, func: MathFunc, arg: f64) -> Result<f64, &'static str> {
        let result = match func {
            MathFunc::Sin => {
                let a = self.to_radians(arg);
                a.sin()
            }
            MathFunc::Cos => {
                let a = self.to_radians(arg);
                a.cos()
            }
            MathFunc::Tan => {
                let a = self.to_radians(arg);
                let cos_val = a.cos();
                if cos_val.abs() < 1e-15 {
                    return Err("Undefined (tan)");
                }
                a.tan()
            }
            MathFunc::Asin => {
                if !(-1.0..=1.0).contains(&arg) {
                    return Err("Domain error (asin)");
                }
                self.radians_to_user_unit(arg.asin())
            }
            MathFunc::Acos => {
                if !(-1.0..=1.0).contains(&arg) {
                    return Err("Domain error (acos)");
                }
                self.radians_to_user_unit(arg.acos())
            }
            MathFunc::Atan => self.radians_to_user_unit(arg.atan()),
            MathFunc::Ln => {
                if arg <= 0.0 {
                    return Err("Domain error (ln)");
                }
                arg.ln()
            }
            MathFunc::Log10 => {
                if arg <= 0.0 {
                    return Err("Domain error (log)");
                }
                arg.log10()
            }
            MathFunc::Sqrt => {
                if arg < 0.0 {
                    return Err("Domain error (sqrt)");
                }
                arg.sqrt()
            }
            MathFunc::Abs => arg.abs(),
            MathFunc::Floor => arg.floor(),
            MathFunc::Ceil => arg.ceil(),
            MathFunc::Exp => {
                let r = arg.exp();
                if r.is_infinite() {
                    return Err("Overflow");
                }
                r
            }
            MathFunc::Factorial => {
                if arg < 0.0 || arg.fract() != 0.0 {
                    return Err("Domain error (fact)");
                }
                let n = arg as u64;
                if n > 170 {
                    return Err("Overflow (fact)");
                }
                factorial(n)
            }
        };

        if result.is_nan() || result.is_infinite() {
            return Err("Math error");
        }
        Ok(result)
    }

    /// Convert an angle from the user's selected unit to radians.
    fn to_radians(&self, angle: f64) -> f64 {
        match self.angle_unit {
            AngleUnit::Radians => angle,
            AngleUnit::Degrees => angle.to_radians(),
        }
    }

    /// Convert an angle from radians to the user's selected unit.
    // Renamed from `from_radians` to satisfy `wrong_self_convention`
    // (from_* should not take `&self`).
    fn radians_to_user_unit(&self, radians: f64) -> f64 {
        match self.angle_unit {
            AngleUnit::Radians => radians,
            AngleUnit::Degrees => radians.to_degrees(),
        }
    }
}

/// Compute n! for non-negative integers.
fn factorial(n: u64) -> f64 {
    let mut result: f64 = 1.0;
    for i in 2..=n {
        result *= i as f64;
    }
    result
}

/// Public entry point: parse and evaluate an expression string.
///
/// Returns either the computed `f64` result or an error message.
fn evaluate(expression: &str, angle_unit: AngleUnit) -> Result<f64, &'static str> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return Err("Empty expression");
    }

    let tokens = tokenize(trimmed).ok_or("Invalid input")?;
    if tokens.is_empty() {
        return Err("Empty expression");
    }

    let mut parser = Parser::new(tokens, angle_unit);
    parser.parse()
}

// ============================================================================
// History entry
// ============================================================================

/// A record of one completed calculation.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub expression: String,
    pub result: String,
}

// ============================================================================
// Calculator state
// ============================================================================

/// Maximum number of history entries kept.
const MAX_HISTORY: usize = 20;

/// Complete calculator application state.
pub struct Calculator {
    /// Current expression being built.
    pub expression: String,
    /// Display text (result or error after pressing '=').
    pub display: String,
    /// Whether the display is showing a result (next digit input resets).
    pub showing_result: bool,
    /// Current mode: Standard or Scientific.
    pub mode: CalcMode,
    /// Angle unit for trig functions.
    pub angle_unit: AngleUnit,
    /// Memory register.
    pub memory: f64,
    /// Whether memory contains a stored value.
    pub memory_set: bool,
    /// Calculation history (newest first).
    pub history: VecDeque<HistoryEntry>,
    /// Whether the history panel is visible.
    pub show_history: bool,
    /// Parenthesis nesting depth (for display feedback).
    pub paren_depth: i32,
}

impl Default for Calculator {
    fn default() -> Self {
        Self::new()
    }
}

impl Calculator {
    /// Create a new calculator in Standard mode.
    pub fn new() -> Self {
        Self {
            expression: String::new(),
            display: String::from("0"),
            showing_result: false,
            mode: CalcMode::Standard,
            angle_unit: AngleUnit::Degrees,
            memory: 0.0,
            memory_set: false,
            history: VecDeque::new(),
            show_history: false,
            paren_depth: 0,
        }
    }

    // ======================================================================
    // Input actions
    // ======================================================================

    /// Append a digit character ('0'-'9') to the expression.
    pub fn input_digit(&mut self, digit: char) {
        if self.showing_result {
            self.expression.clear();
            self.display.clear();
            self.showing_result = false;
        }
        self.expression.push(digit);
        self.update_display();
    }

    /// Append a decimal point.
    pub fn input_decimal(&mut self) {
        if self.showing_result {
            self.expression.clear();
            self.expression.push('0');
            self.showing_result = false;
        }
        // Avoid double decimal in the current number token.
        let last_number = self.current_number_token();
        if !last_number.contains('.') {
            self.expression.push('.');
        }
        self.update_display();
    }

    /// Append an operator (+, -, *, /).
    pub fn input_operator(&mut self, op: char) {
        self.showing_result = false;
        // Replace trailing operator if the user changes their mind.
        let trimmed = self.expression.trim_end();
        if let Some(last) = trimmed.chars().last()
            && "+-*/%".contains(last) && op != '-' {
                // Replace the last operator (but allow unary minus after another op).
                let end = trimmed.len() - last.len_utf8();
                self.expression.truncate(end);
            }
        self.expression.push(' ');
        self.expression.push(op);
        self.expression.push(' ');
        self.update_display();
    }

    /// Append a named function call (e.g., "sin(").
    pub fn input_function(&mut self, name: &str) {
        if self.showing_result {
            // Wrap the previous result so user can do sin(prev_result).
            let prev = self.display.clone();
            self.expression.clear();
            self.expression.push_str(name);
            self.expression.push('(');
            self.expression.push_str(&prev);
            self.showing_result = false;
        } else {
            self.expression.push_str(name);
            self.expression.push('(');
        }
        self.paren_depth += 1;
        self.update_display();
    }

    /// Insert a constant value (pi or e).
    pub fn input_constant(&mut self, name: &str) {
        if self.showing_result {
            self.expression.clear();
            self.showing_result = false;
        }
        self.expression.push_str(name);
        self.update_display();
    }

    /// Open a parenthesis.
    pub fn input_open_paren(&mut self) {
        if self.showing_result {
            self.expression.clear();
            self.showing_result = false;
        }
        self.expression.push('(');
        self.paren_depth += 1;
        self.update_display();
    }

    /// Close a parenthesis (only if one is open).
    pub fn input_close_paren(&mut self) {
        if self.paren_depth > 0 {
            self.expression.push(')');
            self.paren_depth -= 1;
            self.update_display();
        }
    }

    /// Negate the current value (toggle sign).
    pub fn input_negate(&mut self) {
        if self.showing_result {
            // Negate the displayed result.
            if self.display.starts_with('-') {
                self.display.remove(0);
                self.expression = self.display.clone();
            } else if self.display != "0" {
                self.display.insert(0, '-');
                self.expression = self.display.clone();
            }
            self.showing_result = false;
        } else {
            // Wrap the current expression fragment in negation.
            // Simple approach: prepend "(-" and add a ")" later when evaluated.
            let current = self.expression.clone();
            self.expression.clear();
            self.expression.push_str("-(");
            self.expression.push_str(&current);
            self.expression.push(')');
        }
        self.update_display();
    }

    /// Compute a percentage of the accumulated value.
    pub fn input_percent(&mut self) {
        // Evaluate what we have so far and divide by 100.
        match evaluate(&self.expression, self.angle_unit) {
            Ok(val) => {
                let pct = val / 100.0;
                self.expression = format_result(pct);
                self.display = format_result(pct);
            }
            Err(_) => {
                self.display = String::from("Error");
            }
        }
        self.showing_result = true;
        self.update_display();
    }

    /// Delete the last character (backspace).
    pub fn input_backspace(&mut self) {
        if self.showing_result {
            return; // Backspace does nothing on a result.
        }
        if let Some(ch) = self.expression.pop() {
            if ch == '(' {
                self.paren_depth -= 1;
            } else if ch == ')' {
                self.paren_depth += 1;
            }
            // Also trim trailing whitespace left by operator spacing.
            while self.expression.ends_with(' ') {
                self.expression.pop();
            }
        }
        self.update_display();
    }

    /// Clear the current entry (CE) without clearing history.
    pub fn clear_entry(&mut self) {
        self.expression.clear();
        self.display = String::from("0");
        self.showing_result = false;
        self.paren_depth = 0;
    }

    /// Clear everything (C).
    pub fn clear_all(&mut self) {
        self.clear_entry();
    }

    /// Evaluate the current expression and display the result.
    pub fn calculate(&mut self) {
        if self.expression.trim().is_empty() {
            return;
        }

        // Auto-close any open parentheses.
        while self.paren_depth > 0 {
            self.expression.push(')');
            self.paren_depth -= 1;
        }

        let expr_display = self.expression.clone();
        match evaluate(&self.expression, self.angle_unit) {
            Ok(result) => {
                let formatted = format_result(result);
                self.display = formatted.clone();
                self.expression = formatted;
                self.showing_result = true;

                // Add to history.
                self.history.push_front(HistoryEntry {
                    expression: expr_display,
                    result: self.display.clone(),
                });
                if self.history.len() > MAX_HISTORY {
                    self.history.pop_back();
                }
            }
            Err(msg) => {
                self.display = format!("Error: {msg}");
                self.showing_result = true;
            }
        }
    }

    // ======================================================================
    // Memory operations
    // ======================================================================

    /// Store current value in memory (MS).
    pub fn memory_store(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory = val;
            self.memory_set = true;
        }
    }

    /// Recall memory value (MR).
    pub fn memory_recall(&mut self) {
        if self.memory_set {
            let s = format_result(self.memory);
            if self.showing_result {
                self.expression.clear();
                self.showing_result = false;
            }
            self.expression.push_str(&s);
            self.update_display();
        }
    }

    /// Add current value to memory (M+).
    pub fn memory_add(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory += val;
            self.memory_set = true;
        }
    }

    /// Subtract current value from memory (M-).
    pub fn memory_subtract(&mut self) {
        if let Ok(val) = evaluate(&self.expression, self.angle_unit) {
            self.memory -= val;
            self.memory_set = true;
        }
    }

    /// Clear memory (MC).
    pub fn memory_clear(&mut self) {
        self.memory = 0.0;
        self.memory_set = false;
    }

    // ======================================================================
    // Mode toggles
    // ======================================================================

    /// Toggle between Standard and Scientific modes.
    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            CalcMode::Standard => CalcMode::Scientific,
            CalcMode::Scientific => CalcMode::Standard,
        };
    }

    /// Toggle between Degrees and Radians.
    pub fn toggle_angle_unit(&mut self) {
        self.angle_unit = match self.angle_unit {
            AngleUnit::Degrees => AngleUnit::Radians,
            AngleUnit::Radians => AngleUnit::Degrees,
        };
    }

    /// Toggle the history panel.
    pub fn toggle_history(&mut self) {
        self.show_history = !self.show_history;
    }

    // ======================================================================
    // Helpers
    // ======================================================================

    /// Update the display text to reflect the current expression.
    fn update_display(&mut self) {
        if self.expression.is_empty() {
            self.display = String::from("0");
        } else {
            self.display = self.expression.clone();
        }
    }

    /// Extract the last number token being typed (for decimal-point checking).
    fn current_number_token(&self) -> String {
        let mut num = String::new();
        for ch in self.expression.chars().rev() {
            if ch.is_ascii_digit() || ch == '.' {
                num.push(ch);
            } else {
                break;
            }
        }
        num.chars().rev().collect()
    }
}

/// Format a floating-point result for display.
///
/// Uses up to 10 significant digits, strips trailing zeros, and handles
/// very large/small numbers with scientific notation.
fn format_result(value: f64) -> String {
    if value.is_nan() {
        return String::from("NaN");
    }
    if value.is_infinite() {
        return if value > 0.0 {
            String::from("Infinity")
        } else {
            String::from("-Infinity")
        };
    }

    // Check if the value is effectively an integer.
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }

    // General formatting: up to 10 significant digits.
    let s = format!("{:.10}", value);
    // Trim trailing zeros after the decimal point.
    let trimmed = s.trim_end_matches('0');
    let trimmed = trimmed.trim_end_matches('.');
    String::from(trimmed)
}

// ============================================================================
// UI building
// ============================================================================

/// Color palette for the calculator UI.
struct Colors;

#[allow(dead_code)]
impl Colors {
    const WINDOW_BG: Color = Color::rgba(240, 240, 240, 255);
    const DISPLAY_BG: Color = Color::rgba(255, 255, 255, 255);
    const DISPLAY_BORDER: Color = Color::rgba(180, 180, 180, 255);
    const DISPLAY_TEXT: Color = Color::rgba(20, 20, 20, 255);
    const EXPR_TEXT: Color = Color::rgba(120, 120, 120, 255);
    const BTN_NUM_BG: Color = Color::rgba(250, 250, 250, 255);
    const BTN_OP_BG: Color = Color::rgba(230, 230, 240, 255);
    const BTN_EQUALS_BG: Color = Color::rgba(0, 120, 215, 255);
    const BTN_EQUALS_FG: Color = Color::WHITE;
    const BTN_FUNC_BG: Color = Color::rgba(220, 230, 245, 255);
    const BTN_CLEAR_BG: Color = Color::rgba(255, 230, 230, 255);
    const BTN_MEMORY_BG: Color = Color::rgba(230, 245, 230, 255);
    const BTN_MODE_BG: Color = Color::rgba(200, 220, 255, 255);
    const HISTORY_BG: Color = Color::rgba(248, 248, 252, 255);
    const HISTORY_EXPR: Color = Color::rgba(100, 100, 100, 255);
    const HISTORY_RESULT: Color = Color::rgba(20, 20, 20, 255);
    const MEMORY_INDICATOR: Color = Color::rgba(0, 150, 0, 255);
}

/// Create a styled button widget with the given label and background color.
fn calc_button(label: &str, bg: Color) -> Widget {
    let fg = if bg.r < 100 && bg.g < 100 {
        Color::WHITE
    } else {
        Color::BLACK
    };

    Widget::button(label)
        .with_style(Style {
            background: bg,
            foreground: fg,
            padding: Edges::symmetric(8.0, 4.0),
            border: Borders::all(1.0, Color::from_hex(0xBBBBBB)),
            border_radius: CornerRadii::all(4.0),
            font_size: 14.0,
            font_weight: FontWeight::Regular,
            min_width: Some(48.0),
            min_height: Some(36.0),
            ..Style::default()
        })
        .with_flex_grow(1.0)
}

/// Create a small-label button (for scientific mode functions).
fn func_button(label: &str) -> Widget {
    calc_button(label, Colors::BTN_FUNC_BG)
        .with_style(Style {
            background: Colors::BTN_FUNC_BG,
            foreground: Color::BLACK,
            padding: Edges::symmetric(6.0, 2.0),
            border: Borders::all(1.0, Color::from_hex(0xBBBBBB)),
            border_radius: CornerRadii::all(4.0),
            font_size: 11.0,
            font_weight: FontWeight::Regular,
            min_width: Some(42.0),
            min_height: Some(32.0),
            ..Style::default()
        })
}

/// Build the display area showing the expression and current result.
fn build_display(calc: &Calculator) -> Widget {
    let expr_label = Widget::label(&calc.expression)
        .with_style(Style {
            foreground: Colors::EXPR_TEXT,
            font_size: 12.0,
            padding: Edges::symmetric(2.0, 8.0),
            text_align: TextAlign::Right,
            ..Style::default()
        });

    let result_label = Widget::label(&calc.display)
        .with_style(Style {
            foreground: Colors::DISPLAY_TEXT,
            font_size: 24.0,
            font_weight: FontWeight::Bold,
            padding: Edges::symmetric(4.0, 8.0),
            text_align: TextAlign::Right,
            ..Style::default()
        });

    Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_style(Style {
            background: Colors::DISPLAY_BG,
            border: Borders::all(1.0, Colors::DISPLAY_BORDER),
            border_radius: CornerRadii::all(6.0),
            padding: Edges::all(4.0),
            margin: Edges::symmetric(4.0, 4.0),
            min_height: Some(70.0),
            ..Style::default()
        })
        .with_child(expr_label)
        .with_child(result_label)
}

/// Build the mode/status bar showing current mode, angle unit, and memory status.
fn build_status_bar(calc: &Calculator) -> Widget {
    let mode_text = match calc.mode {
        CalcMode::Standard => "Standard",
        CalcMode::Scientific => "Scientific",
    };
    let angle_text = match calc.angle_unit {
        AngleUnit::Degrees => "DEG",
        AngleUnit::Radians => "RAD",
    };

    let mode_btn = calc_button(mode_text, Colors::BTN_MODE_BG)
        .with_style(Style {
            background: Colors::BTN_MODE_BG,
            font_size: 11.0,
            padding: Edges::symmetric(4.0, 8.0),
            border: Borders::all(1.0, Color::from_hex(0xBBBBBB)),
            border_radius: CornerRadii::all(4.0),
            min_width: Some(72.0),
            min_height: Some(24.0),
            ..Style::default()
        });

    let angle_btn = calc_button(angle_text, Colors::BTN_MODE_BG)
        .with_style(Style {
            background: Colors::BTN_MODE_BG,
            font_size: 11.0,
            padding: Edges::symmetric(4.0, 8.0),
            border: Borders::all(1.0, Color::from_hex(0xBBBBBB)),
            border_radius: CornerRadii::all(4.0),
            min_width: Some(42.0),
            min_height: Some(24.0),
            ..Style::default()
        });

    let mut status = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(4.0)
        .with_style(Style {
            padding: Edges::symmetric(2.0, 4.0),
            ..Style::default()
        })
        .with_child(mode_btn)
        .with_child(angle_btn);

    if calc.memory_set {
        let mem_label = Widget::label("M")
            .with_style(Style {
                foreground: Colors::MEMORY_INDICATOR,
                font_size: 11.0,
                font_weight: FontWeight::Bold,
                padding: Edges::symmetric(4.0, 6.0),
                ..Style::default()
            });
        status = status.with_child(mem_label);
    }

    let history_btn = calc_button("Hist", Colors::BTN_MODE_BG)
        .with_style(Style {
            background: Colors::BTN_MODE_BG,
            font_size: 11.0,
            padding: Edges::symmetric(4.0, 8.0),
            border: Borders::all(1.0, Color::from_hex(0xBBBBBB)),
            border_radius: CornerRadii::all(4.0),
            min_width: Some(42.0),
            min_height: Some(24.0),
            ..Style::default()
        });

    status.with_child(history_btn)
}

/// Build the memory button row (MC, MR, M+, M-, MS).
fn build_memory_row() -> Widget {
    Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("MC", Colors::BTN_MEMORY_BG))
        .with_child(calc_button("MR", Colors::BTN_MEMORY_BG))
        .with_child(calc_button("M+", Colors::BTN_MEMORY_BG))
        .with_child(calc_button("M-", Colors::BTN_MEMORY_BG))
        .with_child(calc_button("MS", Colors::BTN_MEMORY_BG))
}

/// Build the scientific function rows (trig, log, etc.).
fn build_scientific_rows() -> Vec<Widget> {
    let row1 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(func_button("sin"))
        .with_child(func_button("cos"))
        .with_child(func_button("tan"))
        .with_child(func_button("("))
        .with_child(func_button(")"));

    let row2 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(func_button("asin"))
        .with_child(func_button("acos"))
        .with_child(func_button("atan"))
        .with_child(func_button("x^y"))
        .with_child(func_button("mod"));

    let row3 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(func_button("ln"))
        .with_child(func_button("log"))
        .with_child(func_button("sqrt"))
        .with_child(func_button("exp"))
        .with_child(func_button("n!"));

    let row4 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(func_button("abs"))
        .with_child(func_button("floor"))
        .with_child(func_button("ceil"))
        .with_child(func_button("pi"))
        .with_child(func_button("e"));

    vec![row1, row2, row3, row4]
}

/// Build the standard numeric/operator keypad.
fn build_standard_keypad() -> Vec<Widget> {
    let row1 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("CE", Colors::BTN_CLEAR_BG))
        .with_child(calc_button("C", Colors::BTN_CLEAR_BG))
        .with_child(calc_button("\u{232B}", Colors::BTN_OP_BG)) // Backspace symbol
        .with_child(calc_button("/", Colors::BTN_OP_BG));

    let row2 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("7", Colors::BTN_NUM_BG))
        .with_child(calc_button("8", Colors::BTN_NUM_BG))
        .with_child(calc_button("9", Colors::BTN_NUM_BG))
        .with_child(calc_button("*", Colors::BTN_OP_BG));

    let row3 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("4", Colors::BTN_NUM_BG))
        .with_child(calc_button("5", Colors::BTN_NUM_BG))
        .with_child(calc_button("6", Colors::BTN_NUM_BG))
        .with_child(calc_button("-", Colors::BTN_OP_BG));

    let row4 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("1", Colors::BTN_NUM_BG))
        .with_child(calc_button("2", Colors::BTN_NUM_BG))
        .with_child(calc_button("3", Colors::BTN_NUM_BG))
        .with_child(calc_button("+", Colors::BTN_OP_BG));

    let row5 = Widget::container()
        .with_flex_direction(FlexDirection::Row)
        .with_gap(2.0)
        .with_style(Style {
            padding: Edges::symmetric(1.0, 4.0),
            ..Style::default()
        })
        .with_child(calc_button("\u{00B1}", Colors::BTN_OP_BG))  // Plus-minus sign
        .with_child(calc_button("0", Colors::BTN_NUM_BG))
        .with_child(calc_button(".", Colors::BTN_NUM_BG))
        .with_child(calc_button("=", Colors::BTN_EQUALS_BG));

    vec![row1, row2, row3, row4, row5]
}

/// Build the history panel showing recent calculations.
fn build_history_panel(calc: &Calculator) -> Widget {
    let title = Widget::label("History")
        .with_style(Style {
            font_size: 13.0,
            font_weight: FontWeight::Bold,
            foreground: Color::from_hex(0x333333),
            padding: Edges::symmetric(4.0, 8.0),
            ..Style::default()
        });

    let mut panel = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_gap(2.0)
        .with_style(Style {
            background: Colors::HISTORY_BG,
            border: Borders::all(1.0, Color::from_hex(0xCCCCCC)),
            border_radius: CornerRadii::all(4.0),
            padding: Edges::all(4.0),
            margin: Edges::symmetric(2.0, 4.0),
            min_height: Some(100.0),
            max_height: Some(200.0),
            ..Style::default()
        })
        .with_child(title);

    if calc.history.is_empty() {
        let empty_msg = Widget::label("No history yet")
            .with_style(Style {
                foreground: Color::GRAY,
                font_size: 11.0,
                padding: Edges::symmetric(4.0, 8.0),
                ..Style::default()
            });
        panel = panel.with_child(empty_msg);
    } else {
        for entry in calc.history.iter().take(10) {
            let expr_label = Widget::label(&entry.expression)
                .with_style(Style {
                    foreground: Colors::HISTORY_EXPR,
                    font_size: 10.0,
                    padding: Edges::symmetric(1.0, 8.0),
                    ..Style::default()
                });
            let result_label = Widget::label(&format!("= {}", entry.result))
                .with_style(Style {
                    foreground: Colors::HISTORY_RESULT,
                    font_size: 12.0,
                    font_weight: FontWeight::SemiBold,
                    padding: Edges::symmetric(1.0, 8.0),
                    ..Style::default()
                });
            let row = Widget::container()
                .with_flex_direction(FlexDirection::Column)
                .with_style(Style {
                    padding: Edges::symmetric(2.0, 0.0),
                    ..Style::default()
                })
                .with_child(expr_label)
                .with_child(result_label);

            panel = panel.with_child(row);
        }
    }

    panel
}

/// Build the full calculator widget tree.
pub fn build_ui(calc: &Calculator) -> Widget {
    let mut root = Widget::container()
        .with_flex_direction(FlexDirection::Column)
        .with_gap(2.0)
        .with_style(Style {
            background: Colors::WINDOW_BG,
            padding: Edges::all(4.0),
            ..Style::default()
        });

    // Status bar (mode toggle, angle unit, memory indicator).
    root = root.with_child(build_status_bar(calc));

    // Display area.
    root = root.with_child(build_display(calc));

    // Memory row.
    root = root.with_child(build_memory_row());

    // Scientific function rows (only in Scientific mode).
    if calc.mode == CalcMode::Scientific {
        for row in build_scientific_rows() {
            root = root.with_child(row);
        }
    }

    // Standard keypad.
    for row in build_standard_keypad() {
        root = root.with_child(row);
    }

    // History panel (toggled).
    if calc.show_history {
        root = root.with_child(build_history_panel(calc));
    }

    root
}

// ============================================================================
// Event dispatch — map button clicks and key presses to calculator actions
// ============================================================================

/// Handle a button press by its label text.
pub fn handle_button(calc: &mut Calculator, label: &str) {
    match label {
        "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
            if let Some(digit) = label.chars().next() {
                calc.input_digit(digit);
            }
        }
        "." => calc.input_decimal(),
        "+" => calc.input_operator('+'),
        "-" => calc.input_operator('-'),
        "*" => calc.input_operator('*'),
        "/" => calc.input_operator('/'),
        "mod" => calc.input_operator('%'),
        "=" => calc.calculate(),
        "C" => calc.clear_all(),
        "CE" => calc.clear_entry(),
        "\u{232B}" => calc.input_backspace(), // Backspace symbol
        "\u{00B1}" => calc.input_negate(),    // Plus-minus sign
        "%" => calc.input_percent(),
        "(" => calc.input_open_paren(),
        ")" => calc.input_close_paren(),
        "sin" => calc.input_function("sin"),
        "cos" => calc.input_function("cos"),
        "tan" => calc.input_function("tan"),
        "asin" => calc.input_function("asin"),
        "acos" => calc.input_function("acos"),
        "atan" => calc.input_function("atan"),
        "ln" => calc.input_function("ln"),
        "log" => calc.input_function("log"),
        "sqrt" => calc.input_function("sqrt"),
        "exp" => calc.input_function("exp"),
        "abs" => calc.input_function("abs"),
        "floor" => calc.input_function("floor"),
        "ceil" => calc.input_function("ceil"),
        "n!" => calc.input_function("fact"),
        "x^y" => calc.input_operator('^'),
        "pi" => calc.input_constant("pi"),
        "e" => calc.input_constant("e"),
        "MC" => calc.memory_clear(),
        "MR" => calc.memory_recall(),
        "M+" => calc.memory_add(),
        "M-" => calc.memory_subtract(),
        "MS" => calc.memory_store(),
        "Standard" | "Scientific" => calc.toggle_mode(),
        "DEG" | "RAD" => calc.toggle_angle_unit(),
        "Hist" => calc.toggle_history(),
        _ => {} // Unknown button — ignore.
    }
}

/// Handle a keyboard event and translate it to calculator actions.
///
/// Returns `true` if the key was handled.
pub fn handle_key(calc: &mut Calculator, key: &KeyEvent) -> bool {
    if !key.pressed {
        return false;
    }

    // Digit keys (both main keyboard and numpad produce text events).
    if let Some(ch) = key.text {
        match ch {
            '0'..='9' => { calc.input_digit(ch); return true; }
            '.' => { calc.input_decimal(); return true; }
            '+' => { calc.input_operator('+'); return true; }
            '-' => { calc.input_operator('-'); return true; }
            '*' => { calc.input_operator('*'); return true; }
            '/' => { calc.input_operator('/'); return true; }
            '%' => { calc.input_percent(); return true; }
            '^' => { calc.input_operator('^'); return true; }
            '(' => { calc.input_open_paren(); return true; }
            ')' => { calc.input_close_paren(); return true; }
            _ => {}
        }
    }

    match key.key {
        Key::Enter => { calc.calculate(); true }
        Key::Escape => { calc.clear_all(); true }
        Key::Backspace => { calc.input_backspace(); true }
        Key::Delete => { calc.clear_entry(); true }
        _ => false,
    }
}

// ============================================================================
// Entry point
// ============================================================================

/// Application window dimensions.
const WINDOW_WIDTH: f32 = 320.0;
const WINDOW_HEIGHT_STANDARD: f32 = 400.0;
const WINDOW_HEIGHT_SCIENTIFIC: f32 = 560.0;

fn main() {
    let calc = Calculator::new();

    // Build initial UI.
    let root = build_ui(&calc);
    let height = match calc.mode {
        CalcMode::Standard => WINDOW_HEIGHT_STANDARD,
        CalcMode::Scientific => WINDOW_HEIGHT_SCIENTIFIC,
    };
    let mut tree = WidgetTree::new(root, WINDOW_WIDTH, height);
    tree.layout();

    // In a real OurOS environment this would enter the compositor event loop.
    // For now, demonstrate that the UI builds and the expression evaluator works.
    //
    // The calculator is ready for integration with the OurOS compositor once
    // the window-management and event-dispatch infrastructure is in place.
    // The event loop would look like:
    //
    // ```
    // loop {
    //     let event = compositor.wait_event();
    //     match event {
    //         Event::CloseRequested => break,
    //         Event::Key(key) => { handle_key(&mut calc, &key); }
    //         Event::Mouse(mouse) => {
    //             // Hit-test buttons and dispatch via handle_button()
    //         }
    //         Event::Resize { width, height } => { tree.resize(width as f32, height as f32); }
    //         _ => {}
    //     }
    //     let root = build_ui(&calc);
    //     tree = WidgetTree::new(root, WINDOW_WIDTH, height);
    //     tree.layout();
    //     let render_tree = tree.render();
    //     compositor.submit(render_tree);
    // }
    // ```
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ----------------------------------------------------------------
    // Expression evaluator tests
    // ----------------------------------------------------------------

    #[test]
    fn test_basic_addition() {
        let result = evaluate("2 + 3", AngleUnit::Degrees);
        assert_eq!(result, Ok(5.0));
    }

    #[test]
    fn test_operator_precedence() {
        // 2 + 3 * 4 = 14 (not 20).
        let result = evaluate("2 + 3 * 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(14.0));
    }

    #[test]
    fn test_operator_precedence_complex() {
        // 1 + 2 * 3 + 4 = 1 + 6 + 4 = 11
        let result = evaluate("1 + 2 * 3 + 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(11.0));
    }

    #[test]
    fn test_parentheses() {
        let result = evaluate("(2 + 3) * 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(20.0));
    }

    #[test]
    fn test_nested_parentheses() {
        let result = evaluate("((2 + 3) * (4 - 1))", AngleUnit::Degrees);
        assert_eq!(result, Ok(15.0));
    }

    #[test]
    fn test_division() {
        let result = evaluate("10 / 4", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.5));
    }

    #[test]
    fn test_division_by_zero() {
        let result = evaluate("5 / 0", AngleUnit::Degrees);
        assert_eq!(result, Err("Division by zero"));
    }

    #[test]
    fn test_modulo() {
        let result = evaluate("17 % 5", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.0));
    }

    #[test]
    fn test_modulo_by_zero() {
        let result = evaluate("5 % 0", AngleUnit::Degrees);
        assert_eq!(result, Err("Division by zero"));
    }

    #[test]
    fn test_power() {
        let result = evaluate("2 ^ 10", AngleUnit::Degrees);
        assert_eq!(result, Ok(1024.0));
    }

    #[test]
    fn test_power_right_associative() {
        // 2^3^2 should be 2^(3^2) = 2^9 = 512, not (2^3)^2 = 64
        let result = evaluate("2 ^ 3 ^ 2", AngleUnit::Degrees);
        assert_eq!(result, Ok(512.0));
    }

    #[test]
    fn test_unary_minus() {
        let result = evaluate("-5", AngleUnit::Degrees);
        assert_eq!(result, Ok(-5.0));
    }

    #[test]
    fn test_unary_minus_in_expression() {
        let result = evaluate("3 + -2", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_double_negation() {
        let result = evaluate("--5", AngleUnit::Degrees);
        assert_eq!(result, Ok(5.0));
    }

    #[test]
    fn test_pi_constant() {
        let result = evaluate("pi", AngleUnit::Degrees);
        assert_eq!(result, Ok(PI));
    }

    #[test]
    fn test_e_constant() {
        let result = evaluate("e", AngleUnit::Degrees);
        assert_eq!(result, Ok(E));
    }

    // ----------------------------------------------------------------
    // Trigonometric function tests
    // ----------------------------------------------------------------

    #[test]
    fn test_sin_degrees() {
        let result = evaluate("sin(90)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_cos_radians() {
        let result = evaluate("cos(0)", AngleUnit::Radians);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_tan_degrees() {
        let result = evaluate("tan(45)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_asin() {
        let result = evaluate("asin(1)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 90.0).abs() < 1e-10);
    }

    #[test]
    fn test_asin_domain_error() {
        let result = evaluate("asin(2)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (asin)"));
    }

    // ----------------------------------------------------------------
    // Logarithmic and exponential tests
    // ----------------------------------------------------------------

    #[test]
    fn test_ln() {
        let result = evaluate("ln(1)", AngleUnit::Degrees);
        assert_eq!(result, Ok(0.0));
    }

    #[test]
    fn test_ln_e() {
        let result = evaluate("ln(e)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_ln_domain_error() {
        let result = evaluate("ln(0)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (ln)"));
    }

    #[test]
    fn test_log10() {
        let result = evaluate("log(100)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_sqrt() {
        let result = evaluate("sqrt(144)", AngleUnit::Degrees);
        assert_eq!(result, Ok(12.0));
    }

    #[test]
    fn test_sqrt_domain_error() {
        let result = evaluate("sqrt(-1)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (sqrt)"));
    }

    #[test]
    fn test_exp() {
        let result = evaluate("exp(0)", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_abs() {
        let result = evaluate("abs(-42)", AngleUnit::Degrees);
        assert_eq!(result, Ok(42.0));
    }

    #[test]
    fn test_floor() {
        let result = evaluate("floor(3.7)", AngleUnit::Degrees);
        assert_eq!(result, Ok(3.0));
    }

    #[test]
    fn test_ceil() {
        let result = evaluate("ceil(3.2)", AngleUnit::Degrees);
        assert_eq!(result, Ok(4.0));
    }

    // ----------------------------------------------------------------
    // Factorial tests
    // ----------------------------------------------------------------

    #[test]
    fn test_factorial_zero() {
        let result = evaluate("fact(0)", AngleUnit::Degrees);
        assert_eq!(result, Ok(1.0));
    }

    #[test]
    fn test_factorial_five() {
        let result = evaluate("fact(5)", AngleUnit::Degrees);
        assert_eq!(result, Ok(120.0));
    }

    #[test]
    fn test_factorial_negative() {
        let result = evaluate("fact(-1)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (fact)"));
    }

    #[test]
    fn test_factorial_non_integer() {
        let result = evaluate("fact(3.5)", AngleUnit::Degrees);
        assert_eq!(result, Err("Domain error (fact)"));
    }

    #[test]
    fn test_factorial_overflow() {
        let result = evaluate("fact(171)", AngleUnit::Degrees);
        assert_eq!(result, Err("Overflow (fact)"));
    }

    // ----------------------------------------------------------------
    // Nested function tests
    // ----------------------------------------------------------------

    #[test]
    fn test_nested_functions() {
        // sqrt(abs(-16)) = sqrt(16) = 4
        let result = evaluate("sqrt(abs(-16))", AngleUnit::Degrees);
        assert_eq!(result, Ok(4.0));
    }

    #[test]
    fn test_function_with_expression_arg() {
        // sqrt(3 + 1) = sqrt(4) = 2
        let result = evaluate("sqrt(3 + 1)", AngleUnit::Degrees);
        assert_eq!(result, Ok(2.0));
    }

    // ----------------------------------------------------------------
    // Calculator state tests
    // ----------------------------------------------------------------

    #[test]
    fn test_calculator_digit_input() {
        let mut calc = Calculator::new();
        calc.input_digit('1');
        calc.input_digit('2');
        calc.input_digit('3');
        assert_eq!(calc.expression, "123");
        assert_eq!(calc.display, "123");
    }

    #[test]
    fn test_calculator_expression_with_ops() {
        let mut calc = Calculator::new();
        calc.input_digit('2');
        calc.input_operator('+');
        calc.input_digit('3');
        calc.input_operator('*');
        calc.input_digit('4');
        calc.calculate();
        assert_eq!(calc.display, "14"); // Correct precedence.
    }

    #[test]
    fn test_calculator_clear() {
        let mut calc = Calculator::new();
        calc.input_digit('5');
        calc.input_digit('5');
        calc.clear_all();
        assert_eq!(calc.display, "0");
        assert_eq!(calc.expression, "");
    }

    #[test]
    fn test_calculator_backspace() {
        let mut calc = Calculator::new();
        calc.input_digit('1');
        calc.input_digit('2');
        calc.input_digit('3');
        calc.input_backspace();
        assert_eq!(calc.expression, "12");
    }

    #[test]
    fn test_calculator_memory() {
        let mut calc = Calculator::new();
        calc.input_digit('4');
        calc.input_digit('2');
        calc.memory_store();
        assert!(calc.memory_set);
        assert_eq!(calc.memory, 42.0);

        calc.clear_all();
        calc.memory_recall();
        assert_eq!(calc.expression, "42");

        calc.clear_all();
        calc.input_digit('8');
        calc.memory_add();
        assert_eq!(calc.memory, 50.0);

        calc.clear_all();
        calc.input_digit('5');
        calc.memory_subtract();
        assert_eq!(calc.memory, 45.0);

        calc.memory_clear();
        assert!(!calc.memory_set);
        assert_eq!(calc.memory, 0.0);
    }

    #[test]
    fn test_calculator_history() {
        let mut calc = Calculator::new();
        calc.input_digit('2');
        calc.input_operator('+');
        calc.input_digit('2');
        calc.calculate();
        assert_eq!(calc.history.len(), 1);
        assert_eq!(calc.history.front().map(|h| h.result.as_str()), Some("4"));
    }

    #[test]
    fn test_calculator_history_max() {
        let mut calc = Calculator::new();
        for i in 0..(MAX_HISTORY + 5) {
            calc.expression = format!("{i}");
            calc.calculate();
        }
        assert_eq!(calc.history.len(), MAX_HISTORY);
    }

    #[test]
    fn test_calculator_mode_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.mode, CalcMode::Standard);
        calc.toggle_mode();
        assert_eq!(calc.mode, CalcMode::Scientific);
        calc.toggle_mode();
        assert_eq!(calc.mode, CalcMode::Standard);
    }

    #[test]
    fn test_calculator_angle_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.angle_unit, AngleUnit::Degrees);
        calc.toggle_angle_unit();
        assert_eq!(calc.angle_unit, AngleUnit::Radians);
        calc.toggle_angle_unit();
        assert_eq!(calc.angle_unit, AngleUnit::Degrees);
    }

    #[test]
    fn test_calculator_decimal() {
        let mut calc = Calculator::new();
        calc.input_digit('3');
        calc.input_decimal();
        calc.input_digit('1');
        calc.input_digit('4');
        assert_eq!(calc.expression, "3.14");
        // Double decimal should be ignored.
        calc.input_decimal();
        assert_eq!(calc.expression, "3.14");
    }

    #[test]
    fn test_calculator_result_resets_on_digit() {
        let mut calc = Calculator::new();
        calc.input_digit('5');
        calc.calculate();
        assert!(calc.showing_result);
        calc.input_digit('3');
        assert!(!calc.showing_result);
        assert_eq!(calc.expression, "3");
    }

    // ----------------------------------------------------------------
    // Format tests
    // ----------------------------------------------------------------

    #[test]
    fn test_format_integer() {
        assert_eq!(format_result(42.0), "42");
    }

    #[test]
    fn test_format_decimal() {
        assert_eq!(format_result(3.25), "3.25");
    }

    #[test]
    fn test_format_negative() {
        assert_eq!(format_result(-7.0), "-7");
    }

    #[test]
    fn test_format_nan() {
        assert_eq!(format_result(f64::NAN), "NaN");
    }

    #[test]
    fn test_format_infinity() {
        assert_eq!(format_result(f64::INFINITY), "Infinity");
    }

    // ----------------------------------------------------------------
    // UI building test
    // ----------------------------------------------------------------

    #[test]
    fn test_ui_builds_standard() {
        let calc = Calculator::new();
        let root = build_ui(&calc);
        // Should have children (status bar, display, memory, keypad rows).
        assert!(!root.children.is_empty());
    }

    #[test]
    fn test_ui_builds_scientific() {
        let mut calc = Calculator::new();
        calc.mode = CalcMode::Scientific;
        let root = build_ui(&calc);
        // Scientific mode should have more children than standard.
        let standard_calc = Calculator::new();
        let standard_root = build_ui(&standard_calc);
        assert!(root.children.len() > standard_root.children.len());
    }

    #[test]
    fn test_ui_builds_with_history() {
        let mut calc = Calculator::new();
        calc.show_history = true;
        calc.history.push_front(HistoryEntry {
            expression: String::from("2 + 2"),
            result: String::from("4"),
        });
        let root = build_ui(&calc);
        assert!(!root.children.is_empty());
    }

    // ----------------------------------------------------------------
    // Keyboard handling tests
    // ----------------------------------------------------------------

    #[test]
    fn test_key_digit() {
        let mut calc = Calculator::new();
        let key = KeyEvent {
            key: Key::Num5,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: Some('5'),
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.expression, "5");
    }

    #[test]
    fn test_key_enter() {
        let mut calc = Calculator::new();
        calc.input_digit('7');
        let key = KeyEvent {
            key: Key::Enter,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert!(handle_key(&mut calc, &key));
        assert!(calc.showing_result);
    }

    #[test]
    fn test_key_escape() {
        let mut calc = Calculator::new();
        calc.input_digit('9');
        let key = KeyEvent {
            key: Key::Escape,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.display, "0");
    }

    #[test]
    fn test_key_backspace() {
        let mut calc = Calculator::new();
        calc.input_digit('4');
        calc.input_digit('2');
        let key = KeyEvent {
            key: Key::Backspace,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: None,
        };
        assert!(handle_key(&mut calc, &key));
        assert_eq!(calc.expression, "4");
    }

    #[test]
    fn test_key_release_ignored() {
        let mut calc = Calculator::new();
        let key = KeyEvent {
            key: Key::Num5,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: Some('5'),
        };
        assert!(!handle_key(&mut calc, &key));
        assert_eq!(calc.expression, ""); // Not modified.
    }

    // ----------------------------------------------------------------
    // Edge cases
    // ----------------------------------------------------------------

    #[test]
    fn test_empty_expression() {
        let result = evaluate("", AngleUnit::Degrees);
        assert_eq!(result, Err("Empty expression"));
    }

    #[test]
    fn test_whitespace_only() {
        let result = evaluate("   ", AngleUnit::Degrees);
        assert_eq!(result, Err("Empty expression"));
    }

    #[test]
    fn test_mismatched_parens() {
        let result = evaluate("(2 + 3", AngleUnit::Degrees);
        assert_eq!(result, Err("Missing ')'"));
    }

    #[test]
    fn test_extra_close_paren() {
        let result = evaluate("2 + 3)", AngleUnit::Degrees);
        assert_eq!(result, Err("Unexpected token"));
    }

    #[test]
    fn test_consecutive_operators() {
        // "2 + * 3" is invalid because * after + is not a valid unary.
        let result = evaluate("2 + * 3", AngleUnit::Degrees);
        assert_eq!(result, Err("Unexpected token"));
    }

    #[test]
    fn test_just_a_number() {
        let result = evaluate("42", AngleUnit::Degrees);
        assert_eq!(result, Ok(42.0));
    }

    #[test]
    fn test_complex_expression() {
        // (2 + 3) * 4 - 10 / 2 = 20 - 5 = 15
        let result = evaluate("(2 + 3) * 4 - 10 / 2", AngleUnit::Degrees);
        assert_eq!(result, Ok(15.0));
    }

    #[test]
    fn test_scientific_notation_expression() {
        // sin(30) in degrees = 0.5
        let result = evaluate("sin(30)", AngleUnit::Degrees);
        assert!((result.expect("should succeed") - 0.5).abs() < 1e-10);
    }

    #[test]
    fn test_handle_button_digit() {
        let mut calc = Calculator::new();
        handle_button(&mut calc, "5");
        assert_eq!(calc.expression, "5");
    }

    #[test]
    fn test_handle_button_equals() {
        let mut calc = Calculator::new();
        calc.expression = String::from("2 + 3");
        handle_button(&mut calc, "=");
        assert_eq!(calc.display, "5");
    }

    #[test]
    fn test_handle_button_clear() {
        let mut calc = Calculator::new();
        calc.expression = String::from("123");
        handle_button(&mut calc, "C");
        assert_eq!(calc.display, "0");
    }

    #[test]
    fn test_handle_button_function() {
        let mut calc = Calculator::new();
        handle_button(&mut calc, "sin");
        assert_eq!(calc.expression, "sin(");
    }

    #[test]
    fn test_handle_button_mode_toggle() {
        let mut calc = Calculator::new();
        assert_eq!(calc.mode, CalcMode::Standard);
        handle_button(&mut calc, "Standard");
        assert_eq!(calc.mode, CalcMode::Scientific);
    }

    #[test]
    fn test_percent() {
        let mut calc = Calculator::new();
        calc.expression = String::from("200");
        calc.input_percent();
        assert_eq!(calc.display, "2");
    }
}
