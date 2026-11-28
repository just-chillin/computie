//! computie-core: disk scanning and the size tree. No UI code lives here.

mod actions;
mod protect;
mod scan;
mod tree;

pub use actions::{move_to_trash, reveal, ActionError};
pub use protect::is_protected;
pub use scan::{default_roots, scan, scan_many, Progress, ScanHandle, ScanOptions};
pub use tree::{Kind, Node, NodeId, Tree};

/// Formats a byte count the way the UI shows it: "512 B", "1.4 MB", "238 GB".
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_size;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_400_000), "1.4 MB");
        assert_eq!(format_size(238_000_000_000), "238 GB");
    }
}
