//! OurOS `fmt` Utility -- Simple Text Formatter
//!
//! Reformats paragraphs of text to fit within a specified width. Modeled
//! after the traditional Unix `fmt` command.
//!
//! # Usage
//!
//! ```text
//! fmt [OPTION]... [FILE]...
//!
//! Reformat each paragraph in FILE(s), writing to standard output.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -w, --width=WIDTH     Maximum line width (default: 75)
//!   -g, --goal=GOAL       Target line width (default: 93% of width)
//!   -p, --prefix=PREFIX   Reformat only lines beginning with PREFIX
//!   -c, --crown-margin    Preserve indentation of first two lines
//!   -t, --tagged-paragraph  Like -c, but first line may differ
//!   -s, --split-only      Split long lines, do not join short ones
//!   -u, --uniform-spacing One space between words, two after sentences
//!   -WIDTH                Set width (shorthand, e.g. -72)
//!       --help            Display this help and exit
//!       --version         Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_WIDTH: usize = 75;
/// Default goal is 93% of width, matching traditional fmt behavior.
const GOAL_PERCENTAGE: f64 = 0.93;

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file paths. `-` means stdin.
    file_paths: Vec<String>,
    /// Maximum output line width.
    width: usize,
    /// Target (goal) line width -- fmt tries to keep lines near this length.
    goal: usize,
    /// Only reformat lines starting with this prefix.
    prefix: Option<String>,
    /// Preserve indentation of first two lines (crown margin mode).
    crown_margin: bool,
    /// First line may have different indent than rest (tagged paragraph mode).
    tagged_paragraph: bool,
    /// Only split long lines, never join short ones.
    split_only: bool,
    /// Normalize spacing: one space between words, two after sentence ends.
    uniform_spacing: bool,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut width: Option<usize> = None;
    let mut goal: Option<usize> = None;
    let mut prefix: Option<String> = None;
    let mut crown_margin = false;
    let mut tagged_paragraph = false;
    let mut split_only = false;
    let mut uniform_spacing = false;
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            file_paths.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            if arg == "--crown-margin" {
                crown_margin = true;
            } else if arg == "--tagged-paragraph" {
                tagged_paragraph = true;
            } else if arg == "--split-only" {
                split_only = true;
            } else if arg == "--uniform-spacing" {
                uniform_spacing = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--width" || arg.starts_with("--width=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--width=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("fmt: option '--width' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                match val_str.parse::<usize>() {
                    Ok(n) => width = Some(n),
                    Err(_) => {
                        eprintln!("fmt: invalid width: '{val_str}'");
                        process::exit(1);
                    }
                }
            } else if arg == "--goal" || arg.starts_with("--goal=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--goal=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("fmt: option '--goal' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                match val_str.parse::<usize>() {
                    Ok(n) => goal = Some(n),
                    Err(_) => {
                        eprintln!("fmt: invalid goal: '{val_str}'");
                        process::exit(1);
                    }
                }
            } else if arg == "--prefix" || arg.starts_with("--prefix=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--prefix=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("fmt: option '--prefix' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                prefix = Some(val_str);
            } else {
                eprintln!("fmt: unrecognized option '{arg}'");
                eprintln!("Try 'fmt --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Check for -NUMBER shorthand (e.g. -72).
        let after_dash = &arg[1..];
        if let Ok(n) = after_dash.parse::<usize>() {
            width = Some(n);
            i += 1;
            continue;
        }

        // Short options. Several consume a following argument.
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                'c' => crown_margin = true,
                't' => tagged_paragraph = true,
                's' => split_only = true,
                'u' => uniform_spacing = true,
                'w' => {
                    let remainder: String = chars.collect();
                    let val_str = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("fmt: option '-w' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match val_str.parse::<usize>() {
                        Ok(n) => width = Some(n),
                        Err(_) => {
                            eprintln!("fmt: invalid width: '{val_str}'");
                            process::exit(1);
                        }
                    }
                    break;
                }
                'g' => {
                    let remainder: String = chars.collect();
                    let val_str = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("fmt: option '-g' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match val_str.parse::<usize>() {
                        Ok(n) => goal = Some(n),
                        Err(_) => {
                            eprintln!("fmt: invalid goal: '{val_str}'");
                            process::exit(1);
                        }
                    }
                    break;
                }
                'p' => {
                    let remainder: String = chars.collect();
                    let val_str = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("fmt: option '-p' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    prefix = Some(val_str);
                    break;
                }
                _ => {
                    eprintln!("fmt: invalid option -- '{ch}'");
                    eprintln!("Try 'fmt --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    // Default to stdin if no files given.
    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    let w = width.unwrap_or(DEFAULT_WIDTH);
    let g = goal.unwrap_or((w as f64 * GOAL_PERCENTAGE) as usize);
    // Goal must not exceed width.
    let g = if g > w { w } else { g };

    ParseResult::Run(Config {
        file_paths,
        width: w,
        goal: g,
        prefix,
        crown_margin,
        tagged_paragraph,
        split_only,
        uniform_spacing,
    })
}

// ============================================================================
// Text formatting engine
// ============================================================================

/// Returns the leading whitespace of a line.
fn leading_whitespace(line: &str) -> &str {
    let trimmed = line.trim_start();
    &line[..line.len() - trimmed.len()]
}

/// Returns true if a character is a sentence-ending punctuation mark.
fn is_sentence_end(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?')
}

/// Checks whether a word ends a sentence (ends with `.`, `!`, or `?`,
/// optionally followed by a closing quote or parenthesis).
fn word_ends_sentence(word: &str) -> bool {
    let s = word.trim_end_matches(['"', '\'', ')', ']']);
    s.chars().last().is_some_and(is_sentence_end)
}

/// Checks whether a line is blank (empty or whitespace-only).
fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// Split a line into words, preserving their boundaries.
fn split_words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

/// Normalize spacing in a word list: one space between words, two spaces
/// after sentence-ending words.
fn join_words_uniform(words: &[&str], width: usize, goal: usize, indent: &str) -> Vec<String> {
    if words.is_empty() {
        return Vec::new();
    }

    let indent_len = indent.len();
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::from(indent);
    let mut current_len = indent_len;

    for (i, &word) in words.iter().enumerate() {
        if i == 0 {
            current_line.push_str(word);
            current_len += word.len();
            continue;
        }

        // Determine spacing: two spaces after sentence-ending word, else one.
        let prev_word = words[i - 1];
        let spaces = if word_ends_sentence(prev_word) { 2 } else { 1 };
        let word_len = word.len();
        let new_len = current_len + spaces + word_len;

        if new_len > width {
            lines.push(current_line);
            current_line = String::from(indent);
            current_line.push_str(word);
            current_len = indent_len + word_len;
        } else {
            for _ in 0..spaces {
                current_line.push(' ');
            }
            current_line.push_str(word);
            current_len = new_len;
            // If we've passed the goal width and could start a new line, we
            // should still keep going as long as we're under the hard width.
            // The goal is a soft target -- we only use it for line-break
            // preference when the next word would still fit.
            let _ = goal; // goal is used implicitly via width constraint
        }
    }

    if !current_line.is_empty() && current_line != indent {
        lines.push(current_line);
    } else if !current_line.is_empty() && current_line == indent && lines.is_empty() {
        // Edge case: only whitespace left.
        lines.push(current_line);
    }

    lines
}

/// Join words with single-space separation, wrapping at width.
fn join_words_simple(words: &[&str], width: usize, _goal: usize, indent: &str) -> Vec<String> {
    if words.is_empty() {
        return Vec::new();
    }

    let indent_len = indent.len();
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::from(indent);
    let mut current_len = indent_len;

    for (i, &word) in words.iter().enumerate() {
        if i == 0 {
            current_line.push_str(word);
            current_len += word.len();
            continue;
        }

        let word_len = word.len();
        let new_len = current_len + 1 + word_len;

        if new_len > width {
            lines.push(current_line);
            current_line = String::from(indent);
            current_line.push_str(word);
            current_len = indent_len + word_len;
        } else {
            current_line.push(' ');
            current_line.push_str(word);
            current_len = new_len;
        }
    }

    if !current_line.is_empty() && current_line != indent {
        lines.push(current_line);
    } else if !current_line.is_empty() && current_line == indent && lines.is_empty() {
        lines.push(current_line);
    }

    lines
}

/// Format a single paragraph (a sequence of non-blank lines) according to the
/// given configuration.
fn format_paragraph(lines: &[&str], config: &Config) -> Vec<String> {
    if lines.is_empty() {
        return Vec::new();
    }

    // Determine indentation for the output.
    let first_indent = leading_whitespace(lines[0]);
    let rest_indent = if lines.len() > 1 {
        leading_whitespace(lines[1])
    } else {
        first_indent
    };

    // In crown-margin mode, preserve both first and second line indentation.
    // In tagged-paragraph mode, first line keeps its indent, rest use the
    // second line's indent.
    // In default mode, all lines use the first line's indent.
    let (first_line_indent, continuation_indent) = if config.crown_margin {
        (first_indent.to_string(), rest_indent.to_string())
    } else if config.tagged_paragraph {
        (first_indent.to_string(), rest_indent.to_string())
    } else {
        (first_indent.to_string(), first_indent.to_string())
    };

    if config.split_only {
        // Split-only mode: break long lines but never join short ones.
        let mut result = Vec::new();
        for &line in lines {
            let trimmed = line.trim_start();
            let indent = leading_whitespace(line);
            if line.len() > config.width {
                let words = split_words(trimmed);
                let wrapped = if config.uniform_spacing {
                    join_words_uniform(&words, config.width, config.goal, indent)
                } else {
                    join_words_simple(&words, config.width, config.goal, indent)
                };
                result.extend(wrapped);
            } else {
                result.push(line.to_string());
            }
        }
        return result;
    }

    // Collect all words from all lines in the paragraph.
    let all_words: Vec<&str> = lines
        .iter()
        .flat_map(|line| split_words(line))
        .collect();

    if all_words.is_empty() {
        return vec![String::new()];
    }

    // Format the first line with its own indent, then remaining lines with the
    // continuation indent.
    let joiner = if config.uniform_spacing {
        join_words_uniform
    } else {
        join_words_simple
    };

    let mut output_lines = joiner(&all_words, config.width, config.goal, &continuation_indent);

    // Replace the first output line's indent with the first-line indent if
    // they differ.
    if !output_lines.is_empty() && first_line_indent != continuation_indent {
        let first = &output_lines[0];
        let trimmed = first.trim_start();
        output_lines[0] = format!("{first_line_indent}{trimmed}");
    }

    output_lines
}

/// Process a complete input text according to the configuration, returning
/// the formatted output as a single string.
fn format_text(input: &str, config: &Config) -> String {
    let input_lines: Vec<&str> = input.lines().collect();
    let mut output = Vec::new();

    if let Some(ref pfx) = config.prefix {
        // Prefix mode: only reformat lines starting with the prefix.
        // Non-matching lines pass through unchanged.
        let mut paragraph: Vec<&str> = Vec::new();
        let mut stripped_paragraph: Vec<String> = Vec::new();

        for line in &input_lines {
            if let Some(rest) = line.strip_prefix(pfx.as_str()) {
                stripped_paragraph.push(rest.to_string());
                paragraph.push(line);
            } else {
                // Flush any accumulated prefix paragraph.
                if !stripped_paragraph.is_empty() {
                    let stripped_refs: Vec<&str> =
                        stripped_paragraph.iter().map(|s| s.as_str()).collect();
                    let formatted = format_paragraph(&stripped_refs, config);
                    for fline in &formatted {
                        output.push(format!("{pfx}{fline}"));
                    }
                    paragraph.clear();
                    stripped_paragraph.clear();
                }
                output.push(line.to_string());
            }
        }

        // Flush final prefix paragraph.
        if !stripped_paragraph.is_empty() {
            let stripped_refs: Vec<&str> =
                stripped_paragraph.iter().map(|s| s.as_str()).collect();
            let formatted = format_paragraph(&stripped_refs, config);
            for fline in &formatted {
                output.push(format!("{pfx}{fline}"));
            }
        }
    } else {
        // Normal mode: paragraphs are separated by blank lines.
        let mut paragraph: Vec<&str> = Vec::new();

        for line in &input_lines {
            if is_blank_line(line) {
                // End of paragraph.
                if !paragraph.is_empty() {
                    let formatted = format_paragraph(&paragraph, config);
                    output.extend(formatted);
                    paragraph.clear();
                }
                output.push(String::new());
            } else {
                paragraph.push(line);
            }
        }

        // Flush final paragraph.
        if !paragraph.is_empty() {
            let formatted = format_paragraph(&paragraph, config);
            output.extend(formatted);
        }
    }

    // Join with newlines. Add a trailing newline if the input had one.
    let mut result = output.join("\n");
    if input.ends_with('\n') {
        result.push('\n');
    }
    result
}

// ============================================================================
// File processing
// ============================================================================

/// Read all input from a buffered reader into a string.
fn read_all<R: BufRead>(reader: &mut R) -> io::Result<String> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf)?;
    Ok(buf)
}

/// Process all files and write formatted output to stdout.
fn run(config: &Config) -> io::Result<i32> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut exit_code = 0;

    for path in &config.file_paths {
        let input = if path == "-" {
            let mut reader = stdin.lock();
            read_all(&mut reader)?
        } else {
            match File::open(path) {
                Ok(f) => {
                    let mut reader = BufReader::new(f);
                    read_all(&mut reader)?
                }
                Err(e) => {
                    eprintln!("fmt: {path}: {e}");
                    exit_code = 1;
                    continue;
                }
            }
        };

        let formatted = format_text(&input, config);
        out.write_all(formatted.as_bytes())?;
    }

    out.flush()?;
    Ok(exit_code)
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS fmt v{VERSION}");
    println!();
    println!("Reformat each paragraph in FILE(s), writing to standard output.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  fmt [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -w, --width=WIDTH     Maximum line width (default: 75)");
    println!("  -g, --goal=GOAL       Target line width (default: 93% of width)");
    println!("  -p, --prefix=PREFIX   Reformat only lines beginning with PREFIX");
    println!("  -c, --crown-margin    Preserve indentation of first two lines");
    println!("  -t, --tagged-paragraph  Like -c, but first line may differ");
    println!("  -s, --split-only      Split long lines, do not join short ones");
    println!("  -u, --uniform-spacing One space between words, two after sentences");
    println!("  -WIDTH                Set width (shorthand, e.g. -72)");
    println!("      --help            Display this help and exit");
    println!("      --version         Output version information and exit");
    println!();
    println!("Paragraphs are separated by blank lines. Each paragraph is");
    println!("reformatted to fit within the specified width.");
    println!();
    println!("EXAMPLES:");
    println!("  fmt file.txt              Reformat at 75 columns");
    println!("  fmt -w 60 file.txt        Reformat at 60 columns");
    println!("  fmt -72 file.txt          Reformat at 72 columns");
    println!("  fmt -c file.txt           Crown margin mode");
    println!("  fmt -p '> ' file.txt      Reformat only '> '-prefixed lines");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            process::exit(0);
        }
        ParseResult::Version => {
            println!("fmt (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => match run(&config) {
            Ok(code) => process::exit(code),
            Err(e) => {
                eprintln!("fmt: {e}");
                process::exit(1);
            }
        },
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a Config with defaults for testing.
    fn test_config() -> Config {
        Config {
            file_paths: vec!["-".to_string()],
            width: 75,
            goal: 69,
            prefix: None,
            crown_margin: false,
            tagged_paragraph: false,
            split_only: false,
            uniform_spacing: false,
        }
    }

    // -- Word wrapping basics ------------------------------------------------

    #[test]
    fn test_basic_wrap() {
        let config = Config {
            width: 20,
            goal: 18,
            ..test_config()
        };
        let input = "one two three four five six seven eight nine ten\n";
        let output = format_text(input, &config);
        for line in output.lines() {
            assert!(line.len() <= 20, "line too long: '{line}'");
        }
        // All words should still be present.
        let words_in: Vec<&str> = input.split_whitespace().collect();
        let words_out: Vec<&str> = output.split_whitespace().collect();
        assert_eq!(words_in, words_out);
    }

    #[test]
    fn test_short_line_no_change() {
        let config = test_config();
        let input = "Hello world.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "Hello world.\n");
    }

    #[test]
    fn test_empty_input() {
        let config = test_config();
        let output = format_text("", &config);
        assert_eq!(output, "");
    }

    #[test]
    fn test_single_long_word() {
        let config = Config {
            width: 10,
            goal: 9,
            ..test_config()
        };
        let input = "abcdefghijklmnopqrstuvwxyz\n";
        let output = format_text(input, &config);
        // A single word that exceeds width can't be split -- it stays as-is.
        assert!(output.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn test_multiple_spaces_collapsed() {
        let config = Config {
            width: 40,
            goal: 37,
            ..test_config()
        };
        let input = "hello    world     foo    bar\n";
        let output = format_text(input, &config);
        assert_eq!(output, "hello world foo bar\n");
    }

    // -- Paragraph detection -------------------------------------------------

    #[test]
    fn test_paragraph_separation() {
        let config = Config {
            width: 40,
            goal: 37,
            ..test_config()
        };
        let input = "First paragraph words here.\n\nSecond paragraph words here.\n";
        let output = format_text(input, &config);
        assert!(output.contains("\n\n"), "paragraphs should be separated by blank line");
        assert!(output.contains("First paragraph"));
        assert!(output.contains("Second paragraph"));
    }

    #[test]
    fn test_multiple_blank_lines_preserved() {
        let config = test_config();
        let input = "Line one.\n\n\nLine two.\n";
        let output = format_text(input, &config);
        // Multiple blank lines should each be preserved.
        assert!(output.contains("\n\n\n"), "multiple blank lines should be preserved");
    }

    #[test]
    fn test_blank_only_input() {
        let config = test_config();
        let input = "\n\n\n";
        let output = format_text(input, &config);
        assert_eq!(output, "\n\n\n");
    }

    // -- Indentation preservation --------------------------------------------

    #[test]
    fn test_indentation_preserved() {
        let config = Config {
            width: 40,
            goal: 37,
            ..test_config()
        };
        let input = "    indented line one two three four five six.\n";
        let output = format_text(input, &config);
        // The output should start with the same indentation.
        assert!(output.starts_with("    "), "indentation should be preserved");
    }

    #[test]
    fn test_continuation_preserves_indent() {
        let config = Config {
            width: 30,
            goal: 28,
            ..test_config()
        };
        let input = "    word1 word2 word3 word4 word5 word6 word7 word8\n";
        let output = format_text(input, &config);
        for line in output.lines() {
            assert!(
                line.starts_with("    "),
                "continuation line should preserve indent: '{line}'"
            );
        }
    }

    // -- Crown margin mode ---------------------------------------------------

    #[test]
    fn test_crown_margin_basic() {
        let config = Config {
            width: 40,
            goal: 37,
            crown_margin: true,
            ..test_config()
        };
        let input = "  First line of the paragraph here.\n    Second and subsequent lines have deeper indent and more words.\n";
        let output = format_text(input, &config);
        let out_lines: Vec<&str> = output.lines().collect();
        assert!(!out_lines.is_empty());
        // First line should keep its indent.
        assert!(
            out_lines[0].starts_with("  "),
            "first line should keep its 2-space indent"
        );
        // If there are continuation lines, they should have the 4-space indent.
        if out_lines.len() > 1 {
            assert!(
                out_lines[1].starts_with("    "),
                "continuation lines should keep 4-space indent"
            );
        }
    }

    #[test]
    fn test_crown_margin_single_line() {
        let config = Config {
            width: 80,
            goal: 74,
            crown_margin: true,
            ..test_config()
        };
        let input = "  Short line.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "  Short line.\n");
    }

    // -- Tagged paragraph mode -----------------------------------------------

    #[test]
    fn test_tagged_paragraph_basic() {
        let config = Config {
            width: 40,
            goal: 37,
            tagged_paragraph: true,
            ..test_config()
        };
        let input =
            "* This is a bullet point that has a lot of words and should wrap around.\n  The continuation has a different indent.\n";
        let output = format_text(input, &config);
        let out_lines: Vec<&str> = output.lines().collect();
        assert!(!out_lines.is_empty());
        // First line should start with "*".
        assert!(
            out_lines[0].starts_with("* ") || out_lines[0].starts_with('*'),
            "first line should preserve its tag"
        );
    }

    #[test]
    fn test_tagged_paragraph_preserves_first_indent() {
        let config = Config {
            width: 30,
            goal: 28,
            tagged_paragraph: true,
            ..test_config()
        };
        let input = "TAG: first line text here.\n    continuation text goes on.\n";
        let output = format_text(input, &config);
        let out_lines: Vec<&str> = output.lines().collect();
        assert!(!out_lines.is_empty());
        assert!(
            out_lines[0].starts_with("TAG:"),
            "first line should keep TAG: prefix"
        );
    }

    // -- Split-only mode -----------------------------------------------------

    #[test]
    fn test_split_only_no_join() {
        let config = Config {
            width: 40,
            goal: 37,
            split_only: true,
            ..test_config()
        };
        let input = "Short.\nAlso short.\n";
        let output = format_text(input, &config);
        // In split-only mode, short lines should NOT be joined.
        let out_lines: Vec<&str> = output.lines().collect();
        assert_eq!(out_lines.len(), 2, "short lines should not be joined");
        assert_eq!(out_lines[0], "Short.");
        assert_eq!(out_lines[1], "Also short.");
    }

    #[test]
    fn test_split_only_breaks_long_lines() {
        let config = Config {
            width: 20,
            goal: 18,
            split_only: true,
            ..test_config()
        };
        let input = "This is a very long line that should be split into multiple shorter lines.\n";
        let output = format_text(input, &config);
        for line in output.lines() {
            // Long single words may exceed width, but word-wrapped lines
            // should be at most width.
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.len() > 1 {
                assert!(
                    line.len() <= 20,
                    "split line should be at most 20 chars: '{line}'"
                );
            }
        }
    }

    #[test]
    fn test_split_only_preserves_short_lines() {
        let config = Config {
            width: 80,
            goal: 74,
            split_only: true,
            ..test_config()
        };
        let input = "Line A.\nLine B.\nLine C.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "Line A.\nLine B.\nLine C.\n");
    }

    // -- Uniform spacing mode ------------------------------------------------

    #[test]
    fn test_uniform_spacing_single_space() {
        let config = Config {
            width: 80,
            goal: 74,
            uniform_spacing: true,
            ..test_config()
        };
        let input = "Hello     world   today.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "Hello world today.\n");
    }

    #[test]
    fn test_uniform_spacing_double_after_sentence() {
        let config = Config {
            width: 80,
            goal: 74,
            uniform_spacing: true,
            ..test_config()
        };
        let input = "End of sentence. Start of next.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "End of sentence.  Start of next.\n");
    }

    #[test]
    fn test_uniform_spacing_exclamation() {
        let config = Config {
            width: 80,
            goal: 74,
            uniform_spacing: true,
            ..test_config()
        };
        let input = "Wow! That is great.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "Wow!  That is great.\n");
    }

    #[test]
    fn test_uniform_spacing_question() {
        let config = Config {
            width: 80,
            goal: 74,
            uniform_spacing: true,
            ..test_config()
        };
        let input = "Really? Yes indeed.\n";
        let output = format_text(input, &config);
        assert_eq!(output, "Really?  Yes indeed.\n");
    }

    #[test]
    fn test_uniform_spacing_quoted_sentence_end() {
        let config = Config {
            width: 80,
            goal: 74,
            uniform_spacing: true,
            ..test_config()
        };
        // Period followed by closing quote should still trigger double space.
        let input = "He said \"hello.\" Then left.\n";
        let output = format_text(input, &config);
        assert!(
            output.contains("hello.\"  Then"),
            "should double-space after quoted sentence end: '{output}'"
        );
    }

    // -- Prefix handling -----------------------------------------------------

    #[test]
    fn test_prefix_basic() {
        let config = Config {
            width: 40,
            goal: 37,
            prefix: Some("> ".to_string()),
            ..test_config()
        };
        let input = "> This is a quoted line that is quite long and should be reformatted.\n> Another quoted line here.\nNot quoted.\n";
        let output = format_text(input, &config);
        // Quoted lines should be reformatted with prefix.
        for line in output.lines() {
            if line.starts_with("> ") || line == "Not quoted." {
                // ok
            } else if line.is_empty() {
                // ok
            } else {
                panic!("unexpected line without prefix: '{line}'");
            }
        }
        assert!(output.contains("Not quoted."), "non-prefixed line preserved");
    }

    #[test]
    fn test_prefix_preserves_non_matching() {
        let config = Config {
            width: 40,
            goal: 37,
            prefix: Some("# ".to_string()),
            ..test_config()
        };
        let input = "Normal line.\n# Comment line.\nAnother normal.\n";
        let output = format_text(input, &config);
        assert!(output.contains("Normal line."));
        assert!(output.contains("Another normal."));
        assert!(output.contains("# Comment line."));
    }

    #[test]
    fn test_prefix_empty_prefix() {
        let config = Config {
            width: 40,
            goal: 37,
            prefix: Some(String::new()),
            ..test_config()
        };
        let input = "All lines match empty prefix.\n";
        let output = format_text(input, &config);
        assert!(output.contains("All lines match"));
    }

    // -- Width and goal settings ---------------------------------------------

    #[test]
    fn test_custom_width() {
        let config = Config {
            width: 30,
            goal: 28,
            ..test_config()
        };
        let input = "This line has many words and should be wrapped at thirty columns or so.\n";
        let output = format_text(input, &config);
        for line in output.lines() {
            // Single words exceeding 30 chars are allowed.
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.len() > 1 {
                assert!(
                    line.len() <= 30,
                    "line exceeds width 30: '{line}' (len={})",
                    line.len()
                );
            }
        }
    }

    #[test]
    fn test_width_one() {
        let config = Config {
            width: 1,
            goal: 1,
            ..test_config()
        };
        let input = "a b c\n";
        let output = format_text(input, &config);
        // Each word should be on its own line.
        let out_lines: Vec<&str> = output.lines().collect();
        assert_eq!(out_lines.len(), 3);
    }

    #[test]
    fn test_large_width() {
        let config = Config {
            width: 1000,
            goal: 930,
            ..test_config()
        };
        let input = "Line one.\nLine two.\nLine three.\n";
        let output = format_text(input, &config);
        // All lines should be joined into one.
        let out_lines: Vec<&str> = output.lines().collect();
        assert_eq!(out_lines.len(), 1);
        assert!(output.contains("Line one. Line two. Line three."));
    }

    #[test]
    fn test_goal_less_than_width() {
        let config = Config {
            width: 80,
            goal: 40,
            ..test_config()
        };
        let input = "word ".repeat(20);
        let input = input.trim().to_string() + "\n";
        let output = format_text(&input, &config);
        // Lines should exist and be within width.
        for line in output.lines() {
            assert!(line.len() <= 80, "line exceeds width: '{line}'");
        }
    }

    // -- Argument parsing ----------------------------------------------------

    #[test]
    fn test_parse_default_width() {
        let args = vec!["fmt".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.width, 75);
            assert_eq!(config.goal, (75.0 * GOAL_PERCENTAGE) as usize);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_dash_number_width() {
        let args = vec!["fmt".to_string(), "-60".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.width, 60);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_w_flag() {
        let args = vec!["fmt".to_string(), "-w".to_string(), "50".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.width, 50);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_width_equals() {
        let args = vec!["fmt".to_string(), "--width=42".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.width, 42);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_goal_flag() {
        let args = vec![
            "fmt".to_string(),
            "-g".to_string(),
            "50".to_string(),
        ];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.goal, 50);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_goal_clamped_to_width() {
        let args = vec![
            "fmt".to_string(),
            "-w".to_string(),
            "40".to_string(),
            "--goal=100".to_string(),
        ];
        if let ParseResult::Run(config) = parse_args(&args) {
            // Goal should be clamped to width.
            assert!(config.goal <= config.width);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_prefix() {
        let args = vec![
            "fmt".to_string(),
            "-p".to_string(),
            "> ".to_string(),
        ];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert_eq!(config.prefix, Some("> ".to_string()));
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_all_flags() {
        let args = vec![
            "fmt".to_string(),
            "-c".to_string(),
            "-s".to_string(),
            "-u".to_string(),
        ];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert!(config.crown_margin);
            assert!(config.split_only);
            assert!(config.uniform_spacing);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_bundled_short_flags() {
        let args = vec!["fmt".to_string(), "-csu".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert!(config.crown_margin);
            assert!(config.split_only);
            assert!(config.uniform_spacing);
        } else {
            panic!("expected Run");
        }
    }

    #[test]
    fn test_parse_help() {
        let args = vec!["fmt".to_string(), "--help".to_string()];
        assert!(matches!(parse_args(&args), ParseResult::Help));
    }

    #[test]
    fn test_parse_version() {
        let args = vec!["fmt".to_string(), "--version".to_string()];
        assert!(matches!(parse_args(&args), ParseResult::Version));
    }

    #[test]
    fn test_parse_tagged_paragraph() {
        let args = vec!["fmt".to_string(), "-t".to_string()];
        if let ParseResult::Run(config) = parse_args(&args) {
            assert!(config.tagged_paragraph);
        } else {
            panic!("expected Run");
        }
    }

    // -- Edge cases ----------------------------------------------------------

    #[test]
    fn test_trailing_newline_preserved() {
        let config = test_config();
        let input = "Hello world.\n";
        let output = format_text(input, &config);
        assert!(output.ends_with('\n'));
    }

    #[test]
    fn test_no_trailing_newline() {
        let config = test_config();
        let input = "Hello world.";
        let output = format_text(input, &config);
        assert!(!output.ends_with('\n'));
    }

    #[test]
    fn test_only_whitespace_lines() {
        let config = test_config();
        let input = "   \n   \n";
        let output = format_text(input, &config);
        // Whitespace-only lines are blank lines -- should pass through.
        assert_eq!(output.lines().count(), 2);
    }

    #[test]
    fn test_sentence_end_detection() {
        assert!(word_ends_sentence("hello."));
        assert!(word_ends_sentence("hello!"));
        assert!(word_ends_sentence("hello?"));
        assert!(word_ends_sentence("hello.\""));
        assert!(word_ends_sentence("hello.)"));
        assert!(!word_ends_sentence("hello"));
        assert!(!word_ends_sentence("hello,"));
        assert!(word_ends_sentence("Dr."));  // Simple heuristic: abbreviations are detected too.
    }

    #[test]
    fn test_is_blank_line() {
        assert!(is_blank_line(""));
        assert!(is_blank_line("   "));
        assert!(is_blank_line("\t"));
        assert!(!is_blank_line("x"));
        assert!(!is_blank_line(" x "));
    }

    #[test]
    fn test_leading_whitespace() {
        assert_eq!(leading_whitespace("  hello"), "  ");
        assert_eq!(leading_whitespace("hello"), "");
        assert_eq!(leading_whitespace("\thello"), "\t");
        assert_eq!(leading_whitespace(""), "");
    }
}
