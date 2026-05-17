//! tsort — topological sort.
//!
//! Usage: tsort [FILE]
//!   Read pairs of strings from FILE (or stdin if omitted) representing
//!   directed edges in a graph, and output the nodes in topological order.
//!   Detects and reports cycles.
//!
//! Input format: whitespace-separated pairs, one edge per pair.
//!   a b     means "a must come before b"
//!   c c     means "c exists" (self-edge, just adds the node)
//!
//! Exit codes:
//!   0  success
//!   1  cycle detected (partial output is still produced)

use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let reader: Box<dyn Read> = if args.is_empty() || args[0] == "-" {
        Box::new(io::stdin())
    } else {
        match File::open(&args[0]) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("tsort: {}: {e}", args[0]);
                process::exit(1);
            }
        }
    };

    // Parse all tokens from input.
    let buf = BufReader::new(reader);
    let mut tokens: Vec<String> = Vec::new();
    for line in buf.lines() {
        match line {
            Ok(l) => {
                for tok in l.split_whitespace() {
                    tokens.push(tok.to_string());
                }
            }
            Err(e) => {
                eprintln!("tsort: {e}");
                process::exit(1);
            }
        }
    }

    if tokens.len() % 2 != 0 {
        eprintln!("tsort: odd number of tokens");
        process::exit(1);
    }

    // Build adjacency list and in-degree map.
    // Assign each unique string an index for efficient processing.
    let mut name_to_id: HashMap<String, usize> = HashMap::new();
    let mut id_to_name: Vec<String> = Vec::new();

    let get_id = |name: &str, map: &mut HashMap<String, usize>, names: &mut Vec<String>| -> usize {
        if let Some(&id) = map.get(name) {
            id
        } else {
            let id = names.len();
            map.insert(name.to_string(), id);
            names.push(name.to_string());
            id
        }
    };

    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let a = get_id(&tokens[i], &mut name_to_id, &mut id_to_name);
        let b = get_id(&tokens[i + 1], &mut name_to_id, &mut id_to_name);
        if a != b {
            edges.push((a, b));
        }
        i += 2;
    }

    let n = id_to_name.len();
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];

    for &(from, to) in &edges {
        adj[from].push(to);
        in_degree[to] += 1;
    }

    // Kahn's algorithm for topological sort.
    let mut queue: VecDeque<usize> = VecDeque::new();
    for node in 0..n {
        if in_degree[node] == 0 {
            queue.push_back(node);
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut count = 0;
    let mut has_cycle = false;

    while let Some(node) = queue.pop_front() {
        let _ = writeln!(out, "{}", id_to_name[node]);
        count += 1;
        for &neighbor in &adj[node] {
            in_degree[neighbor] -= 1;
            if in_degree[neighbor] == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    if count < n {
        has_cycle = true;
        // Report cycle and output remaining nodes in some order.
        eprintln!("tsort: input contains a cycle");

        // Output the nodes that were not emitted (they are in cycles).
        // Use DFS to find and report one cycle.
        let mut visited = vec![false; n];
        // Mark already-output nodes as visited.
        // Re-run Kahn's to figure out which were output: nodes with in_degree
        // that reached 0. Since we modified in_degree, we need to check count.
        // Simpler: nodes already printed are "count" nodes from the start.
        // But we don't track which ones. Instead, output remaining by checking
        // which still have in_degree > 0 (they weren't emitted).
        for node in 0..n {
            // in_degree was decremented as nodes were processed.
            // Nodes still with in_degree > 0 are in cycles.
            if in_degree[node] > 0 && !visited[node] {
                let _ = writeln!(out, "{}", id_to_name[node]);
                visited[node] = true;
            }
        }
    }

    if has_cycle {
        process::exit(1);
    }
}
