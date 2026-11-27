use crate::tree::{Kind, Node, NodeId, Tree};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Stay on the disk the scan started on (skip other mounted disks).
    pub same_filesystem: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self { same_filesystem: true }
    }
}

/// Live counters the UI can read while a scan runs.
#[derive(Debug, Default)]
pub struct Progress {
    files: AtomicU64,
    bytes: AtomicU64,
    cancel: AtomicBool,
    current: Mutex<PathBuf>,
}

impl Progress {
    pub fn files(&self) -> u64 {
        self.files.load(Ordering::Relaxed)
    }
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
    /// A folder being read right now, for the "peeking into…" line.
    pub fn current(&self) -> PathBuf {
        self.current.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct ScanHandle {
    pub progress: Arc<Progress>,
    thread: JoinHandle<Tree>,
}

impl ScanHandle {
    pub fn is_finished(&self) -> bool {
        self.thread.is_finished()
    }

    /// Waits for the scan. A cancelled scan still returns the part it finished.
    pub fn join(self) -> Tree {
        self.thread.join().expect("scan thread panicked")
    }
}

/// Starts scanning `root` on a background thread and returns right away.
pub fn scan(root: impl Into<PathBuf>, options: ScanOptions) -> ScanHandle {
    scan_many(vec![root.into()], options)
}

/// Scans several places into one tree. With more than one root, the tree gets
/// a made-up top node ("This computer") holding one child per root.
pub fn scan_many(roots: Vec<PathBuf>, options: ScanOptions) -> ScanHandle {
    let progress = Arc::new(Progress::default());
    let thread = {
        let progress = progress.clone();
        std::thread::Builder::new()
            .name("computie-scan".into())
            .spawn(move || {
                if roots.len() == 1 {
                    let root = roots.into_iter().next().unwrap();
                    let found = scan_root(&root, &options, &progress);
                    build(found, root)
                } else {
                    let mut top = Found {
                        name: "This computer".into(),
                        size: 0,
                        kind: Kind::Dir,
                        files: 0,
                        errors: 0,
                        children: roots.iter().map(|r| scan_root(r, &options, &progress)).collect(),
                    };
                    for child in &top.children {
                        top.size += child.size;
                        top.files += child.files;
                        top.errors += child.errors;
                    }
                    top.children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
                    // Child names are full paths, and pushing a full path onto
                    // an empty one gives that path back, so Tree::path works.
                    build(top, PathBuf::new())
                }
            })
            .expect("could not start scan thread")
    };
    ScanHandle { progress, thread }
}

/// The places "Scan everything" covers on this computer.
pub fn default_roots() -> Vec<PathBuf> {
    platform::default_roots()
}

/// What one folder looked like, before it is flattened into the [`Tree`].
struct Found {
    name: String,
    size: u64,
    kind: Kind,
    files: u64,
    errors: u32,
    children: Vec<Found>,
}

struct Ctx<'a> {
    progress: &'a Progress,
    /// Hard-linked files already counted, so each is counted once.
    seen_links: Mutex<HashSet<(u64, u64)>>,
    root_dev: Option<u64>,
}

fn scan_root(root: &Path, options: &ScanOptions, progress: &Progress) -> Found {
    let root_dev = if options.same_filesystem {
        fs::metadata(root).ok().and_then(|m| platform::device(&m))
    } else {
        None
    };
    let ctx = Ctx { progress, seen_links: Mutex::new(HashSet::new()), root_dev };
    let name = root.to_string_lossy().into_owned();
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => scan_dir(root, name, &meta, &ctx),
        Ok(meta) => leaf(name, &meta, &ctx),
        Err(_) => unreadable(name),
    }
}

fn build(found: Found, root_path: PathBuf) -> Tree {
    let mut nodes = Vec::new();
    flatten(found, None, &mut nodes);
    Tree { nodes, root_path }
}

fn scan_dir(path: &Path, name: String, meta: &Metadata, ctx: &Ctx) -> Found {
    let own = platform::disk_size(path, meta);
    let mut dir = Found { name, size: own, kind: Kind::Dir, files: 0, errors: 0, children: Vec::new() };
    if ctx.progress.is_cancelled() {
        return dir;
    }
    if let Ok(mut current) = ctx.progress.current.try_lock() {
        *current = path.to_path_buf();
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => {
            dir.kind = Kind::Unreadable;
            dir.errors = 1;
            return dir;
        }
    };

    let mut subdirs = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            dir.errors += 1;
            continue;
        };
        let child_name = entry.file_name().to_string_lossy().into_owned();
        // symlink_metadata never follows links, so a link to a huge folder
        // counts only as the tiny link itself.
        match fs::symlink_metadata(entry.path()) {
            Ok(m) if m.is_dir() && !platform::is_link(&m) => {
                let other_disk = ctx.root_dev.is_some() && platform::device(&m) != ctx.root_dev;
                if other_disk {
                    continue;
                }
                subdirs.push((entry.path(), child_name, m));
            }
            Ok(m) => dir.children.push(leaf(child_name, &m, ctx)),
            Err(_) => dir.children.push(unreadable(child_name)),
        }
    }

    // Each subfolder becomes its own rayon task. Rayon's thread pool steals
    // work between threads, so one deep folder cannot starve the others.
    let scanned: Vec<Found> = subdirs
        .into_par_iter()
        .map(|(p, n, m)| scan_dir(&p, n, &m, ctx))
        .collect();
    dir.children.extend(scanned);

    for child in &dir.children {
        dir.size += child.size;
        dir.files += child.files;
        dir.errors += child.errors;
    }
    dir.children.sort_unstable_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
    dir
}

fn leaf(name: String, meta: &Metadata, ctx: &Ctx) -> Found {
    let kind = if platform::is_link(meta) { Kind::Symlink } else { Kind::File };
    let mut size = platform::disk_size(Path::new(""), meta);
    if let Some(key) = platform::hard_link_key(meta) {
        // Second and later names for the same data take no extra space.
        if !ctx.seen_links.lock().unwrap().insert(key) {
            size = 0;
        }
    }
    ctx.progress.files.fetch_add(1, Ordering::Relaxed);
    ctx.progress.bytes.fetch_add(size, Ordering::Relaxed);
    Found { name, size, kind, files: 1, errors: 0, children: Vec::new() }
}

fn unreadable(name: String) -> Found {
    Found { name, size: 0, kind: Kind::Unreadable, files: 0, errors: 1, children: Vec::new() }
}

fn flatten(found: Found, parent: Option<NodeId>, nodes: &mut Vec<Node>) -> NodeId {
    let id = nodes.len() as NodeId;
    nodes.push(Node {
        name: found.name,
        size: found.size,
        kind: found.kind,
        parent,
        children: Vec::with_capacity(found.children.len()),
        files: found.files,
        errors: found.errors,
    });
    for child in found.children {
        let child_id = flatten(child, Some(id), nodes);
        nodes[id as usize].children.push(child_id);
    }
    id
}

#[cfg(unix)]
mod platform {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    /// Space really used on disk. `blocks` is always in 512-byte units,
    /// whatever the disk's own block size is.
    pub fn disk_size(_path: &Path, meta: &Metadata) -> u64 {
        meta.blocks() * 512
    }
    pub fn device(meta: &Metadata) -> Option<u64> {
        Some(meta.dev())
    }
    pub fn is_link(meta: &Metadata) -> bool {
        meta.file_type().is_symlink()
    }
    pub fn hard_link_key(meta: &Metadata) -> Option<(u64, u64)> {
        (meta.nlink() > 1 && meta.is_file()).then(|| (meta.dev(), meta.ino()))
    }

    #[cfg(target_os = "macos")]
    pub fn default_roots() -> Vec<std::path::PathBuf> {
        use std::path::PathBuf;
        // Modern macOS keeps user data on a separate "Data" volume that only
        // looks like part of "/". Scanning "/" alone would stop at that
        // boundary, so both are listed. NOT YET TESTED ON A REAL MAC.
        let mut roots = vec![PathBuf::from("/")];
        let data = PathBuf::from("/System/Volumes/Data");
        if data.is_dir() {
            roots.push(data);
        }
        if let Ok(volumes) = std::fs::read_dir("/Volumes") {
            for v in volumes.flatten() {
                let is_real_dir = v.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_real_dir {
                    roots.push(v.path());
                }
            }
        }
        roots
    }

    #[cfg(not(target_os = "macos"))]
    pub fn default_roots() -> Vec<std::path::PathBuf> {
        use std::path::PathBuf;
        // Real disks only: /proc/mounts also lists dozens of virtual ones.
        const REAL: [&str; 12] = [
            "ext2", "ext3", "ext4", "btrfs", "xfs", "zfs", "f2fs", "ntfs", "ntfs3", "vfat",
            "exfat", "fuseblk",
        ];
        let mut roots = vec![PathBuf::from("/")];
        let mut seen_devices = std::collections::HashSet::new();
        if let Ok(m) = std::fs::metadata("/") {
            seen_devices.insert(m.dev());
        }
        if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
            for line in mounts.lines() {
                let mut parts = line.split_whitespace();
                let (Some(_dev), Some(dir), Some(fs)) = (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                if !REAL.contains(&fs) || dir.starts_with("/boot") {
                    continue;
                }
                // /proc/mounts writes a space in a folder name as \040.
                let dir = PathBuf::from(dir.replace("\\040", " "));
                if let Ok(m) = std::fs::metadata(&dir) {
                    if m.is_dir() && seen_devices.insert(m.dev()) {
                        roots.push(dir);
                    }
                }
            }
        }
        roots
    }
}

#[cfg(windows)]
mod platform {
    use std::fs::Metadata;
    use std::os::windows::fs::MetadataExt;
    use std::path::Path;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    /// v1 uses the plain file length, rounded up to a 4 KB cluster.
    /// Compressed and sparse files will read too big until this calls
    /// GetCompressedFileSizeW.
    pub fn disk_size(_path: &Path, meta: &Metadata) -> u64 {
        if meta.is_dir() {
            0
        } else {
            meta.file_size().div_ceil(4096) * 4096
        }
    }
    /// Junctions and symlinks are skipped, which keeps the scan on one volume.
    pub fn device(_meta: &Metadata) -> Option<u64> {
        None
    }
    pub fn is_link(meta: &Metadata) -> bool {
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    /// Hard links are not de-duplicated on Windows yet.
    pub fn hard_link_key(_meta: &Metadata) -> Option<(u64, u64)> {
        None
    }

    /// Every drive letter that answers. NOT YET TESTED ON REAL WINDOWS;
    /// network drives should be filtered out here later.
    pub fn default_roots() -> Vec<std::path::PathBuf> {
        (b'C'..=b'Z')
            .map(|letter| std::path::PathBuf::from(format!("{}:\\", letter as char)))
            .filter(|p| p.is_dir())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scan_now(path: &Path) -> Tree {
        scan(path, ScanOptions::default()).join()
    }

    fn write(path: &Path, bytes: usize) {
        fs::write(path, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn sums_sizes_and_sorts_biggest_first() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("small")).unwrap();
        fs::create_dir(dir.path().join("big")).unwrap();
        write(&dir.path().join("small/a.bin"), 10_000);
        write(&dir.path().join("big/b.bin"), 500_000);
        write(&dir.path().join("big/c.bin"), 200_000);

        let tree = scan_now(dir.path());
        let root = tree.node(Tree::ROOT);
        assert_eq!(root.files, 3);
        assert!(root.size >= 710_000, "root was {}", root.size);

        let names: Vec<_> = tree.children(Tree::ROOT).map(|(_, n)| n.name.clone()).collect();
        assert_eq!(names, ["big", "small"]);

        let (big_id, big) = tree.children(Tree::ROOT).next().unwrap();
        assert!(big.size >= 700_000);
        assert_eq!(tree.path(big_id), dir.path().join("big"));
        let (first_id, first) = tree.children(big_id).next().unwrap();
        assert_eq!(first.name, "b.bin");
        assert_eq!(tree.path(first_id), dir.path().join("big/b.bin"));
    }

    #[cfg(unix)]
    #[test]
    fn counts_hard_links_once() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("one.bin"), 400_000);
        fs::hard_link(dir.path().join("one.bin"), dir.path().join("two.bin")).unwrap();

        let tree = scan_now(dir.path());
        let root = tree.node(Tree::ROOT);
        assert_eq!(root.files, 2);
        assert!(root.size < 600_000, "counted twice: {}", root.size);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("huge.bin"), 900_000);
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();

        let tree = scan_now(dir.path());
        assert!(tree.node(Tree::ROOT).size < 100_000);
        let (_, link) = tree.children(Tree::ROOT).next().unwrap();
        assert_eq!(link.kind, Kind::Symlink);
        assert!(link.children.is_empty());
    }

    #[test]
    fn cancel_stops_early_and_still_returns_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.bin"), 1000);
        let handle = scan(dir.path(), ScanOptions::default());
        handle.progress.cancel();
        let tree = handle.join();
        assert!(!tree.is_empty());
    }

    #[test]
    fn remove_shrinks_parents() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        write(&dir.path().join("sub/x.bin"), 300_000);
        write(&dir.path().join("keep.bin"), 50_000);

        let mut tree = scan_now(dir.path());
        let before = tree.node(Tree::ROOT).size;
        let (sub_id, sub_size) = tree
            .children(Tree::ROOT)
            .find(|(_, n)| n.name == "sub")
            .map(|(id, n)| (id, n.size))
            .unwrap();
        tree.remove(sub_id);
        assert_eq!(tree.node(Tree::ROOT).size, before - sub_size);
        assert_eq!(tree.node(Tree::ROOT).files, 1);
        assert_eq!(tree.children(Tree::ROOT).count(), 1);
    }

    #[test]
    fn several_roots_share_one_tree() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(&a.path().join("a.bin"), 300_000);
        write(&b.path().join("b.bin"), 100_000);
        let tree = scan_many(
            vec![a.path().to_path_buf(), b.path().to_path_buf()],
            ScanOptions::default(),
        )
        .join();
        let root = tree.node(Tree::ROOT);
        assert_eq!(root.name, "This computer");
        assert_eq!(root.files, 2);
        let (first_id, _) = tree.children(Tree::ROOT).next().unwrap();
        assert_eq!(tree.path(first_id), a.path());
        let (file_id, _) = tree.children(first_id).next().unwrap();
        assert_eq!(tree.path(file_id), a.path().join("a.bin"));
    }

    #[test]
    fn default_roots_is_never_empty() {
        assert!(!default_roots().is_empty());
    }

    #[test]
    fn missing_root_gives_an_unreadable_node() {
        let tree = scan_now(Path::new("/definitely/not/here/computie"));
        assert_eq!(tree.node(Tree::ROOT).kind, Kind::Unreadable);
    }
}
