//! Headless front end for the scanner: `computie-cli <path> [--top N]`.

use computie_core::{format_size, scan, ScanOptions, Tree};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut top = 15usize;
    while let Some(arg) = args.next() {
        if arg == "--top" {
            top = args.next().and_then(|n| n.parse().ok()).unwrap_or(top);
        } else {
            path = Some(arg);
        }
    }
    let Some(path) = path else {
        eprintln!("usage: computie-cli <path> [--top N]");
        std::process::exit(2);
    };

    let started = Instant::now();
    let handle = scan(&path, ScanOptions::default());
    while !handle.is_finished() {
        eprint!(
            "\r{} files, {} so far…",
            handle.progress.files(),
            format_size(handle.progress.bytes())
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let tree = handle.join();
    let root = tree.node(Tree::ROOT);
    eprintln!("\r{:60}", "");

    println!(
        "{}  {}  ({} files, {} unreadable, {:.1}s)",
        format_size(root.size),
        tree.root_path().display(),
        root.files,
        root.errors,
        started.elapsed().as_secs_f32()
    );
    for (_, child) in tree.children(Tree::ROOT).take(top) {
        let share = if root.size > 0 { 100.0 * child.size as f64 / root.size as f64 } else { 0.0 };
        println!("{:>9}  {:5.1}%  {}", format_size(child.size), share, child.name);
    }
    // Raw bytes on the last line, so scripts can compare against `du`.
    println!("bytes={}", root.size);
}
