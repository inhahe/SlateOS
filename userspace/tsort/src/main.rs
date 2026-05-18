//! tsort — topological sort for OurOS
//!
//! Reads pairs of strings from input and produces a topological
//! ordering. Reports cycles to stderr.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, BufRead};
use std::process;

/// Graph represented as adjacency list with in-degree tracking
struct Graph {
    /// Node name → index
    name_to_idx: BTreeMap<String, usize>,
    /// Index → node name
    idx_to_name: Vec<String>,
    /// Adjacency list: edges[from] = set of to indices
    edges: Vec<BTreeSet<usize>>,
    /// In-degree for each node
    in_degree: Vec<usize>,
}

impl Graph {
    fn new() -> Self {
        Self {
            name_to_idx: BTreeMap::new(),
            idx_to_name: Vec::new(),
            edges: Vec::new(),
            in_degree: Vec::new(),
        }
    }

    /// Get or create a node index for the given name
    fn get_or_insert(&mut self, name: &str) -> usize {
        if let Some(&idx) = self.name_to_idx.get(name) {
            return idx;
        }
        let idx = self.idx_to_name.len();
        self.name_to_idx.insert(name.to_string(), idx);
        self.idx_to_name.push(name.to_string());
        self.edges.push(BTreeSet::new());
        self.in_degree.push(0);
        idx
    }

    /// Add a directed edge from → to (a must come before b)
    fn add_edge(&mut self, from: usize, to: usize) {
        if from != to && self.edges[from].insert(to) {
            self.in_degree[to] += 1;
        }
    }

    /// Kahn's algorithm for topological sort
    /// Returns (sorted_names, has_cycle)
    fn topological_sort(&self) -> (Vec<String>, bool) {
        let n = self.idx_to_name.len();
        let mut in_degree = self.in_degree.clone();
        let mut result = Vec::with_capacity(n);

        // Start with all nodes that have no incoming edges
        let mut queue: VecDeque<usize> = VecDeque::new();
        for i in 0..n {
            if in_degree[i] == 0 {
                queue.push_back(i);
            }
        }

        while let Some(node) = queue.pop_front() {
            result.push(self.idx_to_name[node].clone());

            for &neighbor in &self.edges[node] {
                in_degree[neighbor] -= 1;
                if in_degree[neighbor] == 0 {
                    queue.push_back(neighbor);
                }
            }
        }

        let has_cycle = result.len() != n;
        if has_cycle {
            // Report which nodes are in cycles
            for i in 0..n {
                if in_degree[i] > 0 {
                    eprintln!("tsort: {}: input contains a loop", self.idx_to_name[i]);
                }
            }
            // Still output what we can — add remaining nodes in order
            let in_result: BTreeSet<String> = result.iter().cloned().collect();
            for i in 0..n {
                if !in_result.contains(&self.idx_to_name[i]) {
                    result.push(self.idx_to_name[i].clone());
                }
            }
        }

        (result, has_cycle)
    }
}

fn print_help() {
    println!("Usage: tsort [FILE]");
    println!("Topological sort of a directed acyclic graph.");
    println!();
    println!("Read pairs of strings from FILE (or standard input) indicating");
    println!("that the first string must come before the second. Print the");
    println!("result in a topologically sorted order.");
    println!();
    println!("Options:");
    println!("  --help     display this help and exit");
    println!("  --version  output version information and exit");
}

fn read_tokens(input: &str) -> Vec<String> {
    input.split_whitespace().map(|s| s.to_string()).collect()
}

fn run(args: &[String]) -> i32 {
    let mut file: Option<String> = None;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            "--version" => {
                println!("tsort (OurOS coreutils) 0.1.0");
                return 0;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                eprintln!("tsort: unknown option '{}'", arg);
                return 1;
            }
            _ => {
                if file.is_some() {
                    eprintln!("tsort: extra operand '{}'", arg);
                    return 1;
                }
                file = Some(arg.clone());
            }
        }
    }

    // Read all tokens from input
    let tokens = if let Some(ref path) = file {
        if path == "-" {
            read_stdin_tokens()
        } else {
            match fs::read_to_string(path) {
                Ok(content) => read_tokens(&content),
                Err(e) => {
                    eprintln!("tsort: {}: {}", path, e);
                    return 1;
                }
            }
        }
    } else {
        read_stdin_tokens()
    };

    if tokens.len() % 2 != 0 {
        eprintln!("tsort: input contains an odd number of tokens");
        return 1;
    }

    // Build graph from pairs
    let mut graph = Graph::new();

    // First pass: register all nodes
    for token in &tokens {
        graph.get_or_insert(token);
    }

    // Second pass: add edges
    let mut i = 0;
    while i + 1 < tokens.len() {
        let from = graph.get_or_insert(&tokens[i]);
        let to = graph.get_or_insert(&tokens[i + 1]);
        if from != to {
            graph.add_edge(from, to);
        }
        i += 2;
    }

    // Run topological sort
    let (sorted, has_cycle) = graph.topological_sort();

    // Output
    for name in &sorted {
        println!("{}", name);
    }

    if has_cycle { 1 } else { 0 }
}

fn read_stdin_tokens() -> Vec<String> {
    let stdin = io::stdin();
    let mut tokens = Vec::new();
    for line in stdin.lock().lines() {
        match line {
            Ok(l) => {
                for tok in l.split_whitespace() {
                    tokens.push(tok.to_string());
                }
            }
            Err(e) => {
                eprintln!("tsort: read error: {}", e);
                break;
            }
        }
    }
    tokens
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let exit_code = run(&args);
    process::exit(exit_code);
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sort_from_pairs(pairs: &[(&str, &str)]) -> (Vec<String>, bool) {
        let mut graph = Graph::new();
        for (a, b) in pairs {
            let from = graph.get_or_insert(a);
            let to = graph.get_or_insert(b);
            if from != to {
                graph.add_edge(from, to);
            }
        }
        graph.topological_sort()
    }

    fn position_of(sorted: &[String], name: &str) -> Option<usize> {
        sorted.iter().position(|s| s == name)
    }

    #[test]
    fn test_simple_chain() {
        let (sorted, cycle) = sort_from_pairs(&[("a", "b"), ("b", "c")]);
        assert!(!cycle);
        assert!(position_of(&sorted, "a").unwrap() < position_of(&sorted, "b").unwrap());
        assert!(position_of(&sorted, "b").unwrap() < position_of(&sorted, "c").unwrap());
    }

    #[test]
    fn test_diamond() {
        let (sorted, cycle) = sort_from_pairs(&[
            ("a", "b"),
            ("a", "c"),
            ("b", "d"),
            ("c", "d"),
        ]);
        assert!(!cycle);
        let pa = position_of(&sorted, "a").unwrap();
        let pb = position_of(&sorted, "b").unwrap();
        let pc = position_of(&sorted, "c").unwrap();
        let pd = position_of(&sorted, "d").unwrap();
        assert!(pa < pb);
        assert!(pa < pc);
        assert!(pb < pd);
        assert!(pc < pd);
    }

    #[test]
    fn test_single_pair() {
        let (sorted, cycle) = sort_from_pairs(&[("x", "y")]);
        assert!(!cycle);
        assert_eq!(sorted, vec!["x", "y"]);
    }

    #[test]
    fn test_self_loop() {
        // Self-loop pairs (same node twice) should not create an edge
        let (sorted, cycle) = sort_from_pairs(&[("a", "a")]);
        assert!(!cycle);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], "a");
    }

    #[test]
    fn test_cycle_detected() {
        let (_, cycle) = sort_from_pairs(&[("a", "b"), ("b", "c"), ("c", "a")]);
        assert!(cycle);
    }

    #[test]
    fn test_independent_nodes() {
        let (sorted, cycle) = sort_from_pairs(&[("a", "a"), ("b", "b"), ("c", "c")]);
        assert!(!cycle);
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn test_multiple_roots() {
        let (sorted, cycle) = sort_from_pairs(&[
            ("a", "c"),
            ("b", "c"),
        ]);
        assert!(!cycle);
        let pa = position_of(&sorted, "a").unwrap();
        let pb = position_of(&sorted, "b").unwrap();
        let pc = position_of(&sorted, "c").unwrap();
        assert!(pa < pc);
        assert!(pb < pc);
    }

    #[test]
    fn test_long_chain() {
        let (sorted, cycle) = sort_from_pairs(&[
            ("1", "2"),
            ("2", "3"),
            ("3", "4"),
            ("4", "5"),
        ]);
        assert!(!cycle);
        for i in 0..4 {
            assert!(
                position_of(&sorted, &(i + 1).to_string()).unwrap()
                    < position_of(&sorted, &(i + 2).to_string()).unwrap()
            );
        }
    }

    #[test]
    fn test_duplicate_edges() {
        // Duplicate edges should be handled gracefully
        let (sorted, cycle) = sort_from_pairs(&[
            ("a", "b"),
            ("a", "b"),
            ("b", "c"),
            ("b", "c"),
        ]);
        assert!(!cycle);
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn test_graph_node_count() {
        let mut graph = Graph::new();
        graph.get_or_insert("a");
        graph.get_or_insert("b");
        graph.get_or_insert("a"); // duplicate
        assert_eq!(graph.idx_to_name.len(), 2);
    }

    #[test]
    fn test_graph_edge_count() {
        let mut graph = Graph::new();
        let a = graph.get_or_insert("a");
        let b = graph.get_or_insert("b");
        graph.add_edge(a, b);
        graph.add_edge(a, b); // duplicate
        assert_eq!(graph.edges[a].len(), 1);
    }

    #[test]
    fn test_in_degree_tracking() {
        let mut graph = Graph::new();
        let a = graph.get_or_insert("a");
        let b = graph.get_or_insert("b");
        let c = graph.get_or_insert("c");
        graph.add_edge(a, c);
        graph.add_edge(b, c);
        assert_eq!(graph.in_degree[a], 0);
        assert_eq!(graph.in_degree[b], 0);
        assert_eq!(graph.in_degree[c], 2);
    }

    #[test]
    fn test_read_tokens() {
        let tokens = read_tokens("a b c d");
        assert_eq!(tokens, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_read_tokens_multiline() {
        let tokens = read_tokens("a b\nc d\ne f");
        assert_eq!(tokens, vec!["a", "b", "c", "d", "e", "f"]);
    }

    #[test]
    fn test_read_tokens_extra_whitespace() {
        let tokens = read_tokens("  a   b  \n  c   d  ");
        assert_eq!(tokens, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn test_read_tokens_empty() {
        let tokens = read_tokens("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_complex_dag() {
        // A complex DAG: build system dependency order
        let (sorted, cycle) = sort_from_pairs(&[
            ("libcore", "liballoc"),
            ("liballoc", "libstd"),
            ("libstd", "myapp"),
            ("libcore", "libstd"),
            ("libcore", "myapp"),
        ]);
        assert!(!cycle);
        let core = position_of(&sorted, "libcore").unwrap();
        let alloc = position_of(&sorted, "liballoc").unwrap();
        let std = position_of(&sorted, "libstd").unwrap();
        let app = position_of(&sorted, "myapp").unwrap();
        assert!(core < alloc);
        assert!(alloc < std);
        assert!(std < app);
    }

    #[test]
    fn test_two_component_graph() {
        // Two disconnected components
        let (sorted, cycle) = sort_from_pairs(&[
            ("a", "b"),
            ("c", "d"),
        ]);
        assert!(!cycle);
        assert_eq!(sorted.len(), 4);
        assert!(position_of(&sorted, "a").unwrap() < position_of(&sorted, "b").unwrap());
        assert!(position_of(&sorted, "c").unwrap() < position_of(&sorted, "d").unwrap());
    }

    #[test]
    fn test_wide_graph() {
        // Many nodes depending on one
        let (sorted, cycle) = sort_from_pairs(&[
            ("root", "a"),
            ("root", "b"),
            ("root", "c"),
            ("root", "d"),
            ("root", "e"),
        ]);
        assert!(!cycle);
        let root = position_of(&sorted, "root").unwrap();
        assert_eq!(root, 0);
    }

    #[test]
    fn test_reverse_input_order() {
        // Input in reverse order should still produce valid sort
        let (sorted, cycle) = sort_from_pairs(&[
            ("c", "d"),
            ("b", "c"),
            ("a", "b"),
        ]);
        assert!(!cycle);
        assert!(position_of(&sorted, "a").unwrap() < position_of(&sorted, "b").unwrap());
        assert!(position_of(&sorted, "b").unwrap() < position_of(&sorted, "c").unwrap());
        assert!(position_of(&sorted, "c").unwrap() < position_of(&sorted, "d").unwrap());
    }
}
