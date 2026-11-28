use std::path::{Component, Path};

/// Decides whether computie must refuse to trash `path`.
///
/// This is the last check before something leaves the user's disk. The
/// policy leans cautious: when in doubt, refuse.
///
/// Protected:
/// - anything that is not a plain absolute path (relative, or with `..`)
/// - the top of a disk, and the mount folder of an external disk
/// - the home folder, every folder above it, and its standard folders
///   themselves (Documents, Desktop, ...). Things *inside* those are fine.
/// - operating system folders and everything inside them, except a few
///   places that only hold throwaway data (caches, temp files)
///
/// `home` is the user's home folder when we know it.
pub fn is_protected(path: &Path, home: Option<&Path>) -> bool {
    let Some(parts) = parts(path) else { return true };
    if parts.is_empty() {
        return true; // the top of a disk
    }

    if let Some(home_parts) = home.and_then(parts_of_home) {
        // Home itself and everything above it ("/home", "/Users").
        if home_parts.starts_with(&parts) {
            return true;
        }
        // The standard folders directly inside home, but not their contents.
        if parts.len() == home_parts.len() + 1 && parts.starts_with(&home_parts) {
            let name = parts.last().unwrap().as_str();
            if HOME_FOLDERS.iter().any(|f| same(f, name)) {
                return true;
            }
        }
    }

    // Throwaway places inside system folders are allowed, but only their
    // contents, never the folder itself.
    for allowed in rules::ALLOWED_INSIDE {
        if parts.len() > allowed.len() && starts_with(&parts, allowed) {
            return false;
        }
    }
    for tree in rules::WHOLE_TREE {
        if starts_with(&parts, tree) {
            return true;
        }
    }
    for folder in rules::FOLDER_ONLY {
        if parts.len() == folder.len() && starts_with(&parts, folder) {
            return true;
        }
    }
    // Where external disks get mounted: protect the disk's own folder.
    for (mounts, depth) in rules::MOUNT_PARENTS {
        if starts_with(&parts, mounts) && parts.len() <= mounts.len() + depth {
            return true;
        }
    }
    false
}

/// Standard folders inside a home folder. Trashing one of these whole is
/// almost never what someone means.
const HOME_FOLDERS: [&str; 12] = [
    "Desktop", "Documents", "Downloads", "Pictures", "Music", "Movies", "Videos", "Library",
    "Applications", "Public", "AppData", "OneDrive",
];

#[cfg(not(windows))]
mod rules {
    /// These folders and everything in them.
    pub const WHOLE_TREE: &[&[&str]] = &[
        &["System"], &["bin"], &["sbin"], &["usr"], &["etc"], &["lib"], &["lib32"], &["lib64"],
        &["boot"], &["dev"], &["proc"], &["sys"], &["run"], &["var"], &["private"], &["Library"],
        &["cores"], &["snap"], &["nix"],
    ];
    /// Only the folder itself; what is inside may go.
    pub const FOLDER_ONLY: &[&[&str]] = &[
        &["Applications"], &["opt"], &["home"], &["Users"], &["root"], &["srv"], &["tmp"],
        &["Volumes"], &["mnt"], &["media"],
    ];
    /// Exceptions to WHOLE_TREE: contents of these may go.
    pub const ALLOWED_INSIDE: &[&[&str]] = &[
        &["usr", "local"],
        &["var", "tmp"],
        &["var", "cache"],
        &["private", "tmp"],
        &["private", "var", "tmp"],
        &["private", "var", "folders"],
        &["Library", "Caches"],
    ];
    /// (folder, how many levels below it are mount points)
    pub const MOUNT_PARENTS: &[(&[&str], usize)] =
        &[(&["Volumes"], 1), (&["mnt"], 1), (&["media"], 2), (&["run", "media"], 2)];
}

#[cfg(windows)]
mod rules {
    pub const WHOLE_TREE: &[&[&str]] = &[
        &["Windows"], &["Program Files"], &["Program Files (x86)"], &["ProgramData"],
        &["$Recycle.Bin"], &["System Volume Information"], &["Recovery"], &["Boot"],
    ];
    pub const FOLDER_ONLY: &[&[&str]] = &[&["Users"]];
    pub const ALLOWED_INSIDE: &[&[&str]] = &[&["Windows", "Temp"]];
    pub const MOUNT_PARENTS: &[(&[&str], usize)] = &[];
}

/// The path's folder names from the top down, or `None` if the path is not
/// a plain absolute path. An empty list means the top of a disk.
fn parts(path: &Path) -> Option<Vec<String>> {
    if !path.is_absolute() {
        return None;
    }
    let mut out = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(name) => out.push(name.to_string_lossy().into_owned()),
            // "." and ".." could point anywhere; refuse instead of guessing.
            Component::CurDir | Component::ParentDir => return None,
        }
    }
    Some(out)
}

fn parts_of_home(home: &Path) -> Option<Vec<String>> {
    parts(home).filter(|p| !p.is_empty())
}

/// Windows and (by default) macOS treat "documents" and "Documents" as the
/// same folder, so names are compared ignoring case there.
fn same(a: &str, b: &str) -> bool {
    if cfg!(target_os = "linux") {
        a == b
    } else {
        a.eq_ignore_ascii_case(b)
    }
}

fn starts_with(parts: &[String], prefix: &[&str]) -> bool {
    parts.len() >= prefix.len() && prefix.iter().zip(parts).all(|(a, b)| same(a, b))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn check(path: &str) -> bool {
        is_protected(Path::new(path), Some(Path::new("/home/sam")))
    }

    #[test]
    fn roots_and_home_are_protected() {
        assert!(check("/"));
        assert!(check("/home"));
        assert!(check("/home/sam"));
        assert!(check("/home/sam/"));
    }

    #[test]
    fn standard_home_folders_are_protected_but_not_their_contents() {
        assert!(check("/home/sam/Documents"));
        assert!(check("/home/sam/Desktop"));
        assert!(!check("/home/sam/Documents/old-taxes"));
        assert!(!check("/home/sam/Downloads/old.zip"));
    }

    #[test]
    fn ordinary_folders_are_not_protected() {
        assert!(!check("/home/sam/projects"));
        assert!(!check("/home/sam/.cache/thumbnails"));
        assert!(!check("/home/sam/Library/Caches/com.example"));
    }

    #[test]
    fn system_folders_are_protected_all_the_way_down() {
        assert!(check("/usr"));
        assert!(check("/usr/lib/x86_64-linux-gnu"));
        assert!(check("/etc/passwd"));
        assert!(check("/System/Library/Frameworks"));
        assert!(check("/var/lib/dpkg"));
        assert!(check("/private/etc/hosts"));
    }

    #[test]
    fn throwaway_places_inside_system_folders_are_allowed() {
        assert!(!check("/usr/local/share/old-tool"));
        assert!(!check("/var/cache/apt/archives"));
        assert!(!check("/var/tmp/build-123"));
        assert!(!check("/Library/Caches/com.example"));
        // ...but never the throwaway folder itself.
        assert!(check("/usr/local"));
        assert!(check("/var/cache"));
        assert!(check("/Library/Caches"));
    }

    #[test]
    fn some_folders_are_protected_only_themselves() {
        assert!(check("/Applications"));
        assert!(!check("/Applications/Old Game.app"));
        assert!(check("/opt"));
        assert!(!check("/opt/old-sdk"));
    }

    #[test]
    fn external_disk_folders_are_protected() {
        assert!(check("/Volumes/Backup"));
        assert!(!check("/Volumes/Backup/old-photos"));
        assert!(check("/media/sam/USB"));
        assert!(!check("/media/sam/USB/stuff"));
        assert!(check("/mnt/data"));
    }

    #[test]
    fn similar_names_do_not_match() {
        assert!(!check("/usrdata/stuff"));
        assert!(!check("/home/sam/usr"));
        assert!(!check("/data/etc/notes"));
    }

    #[test]
    fn odd_paths_are_refused() {
        assert!(check("relative/path"));
        assert!(check("/home/sam/Downloads/../../.."));
        assert!(check(""));
    }

    #[test]
    fn works_without_a_known_home() {
        assert!(is_protected(Path::new("/usr/bin"), None));
        assert!(!is_protected(Path::new("/data/old"), None));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn check(path: &str) -> bool {
        is_protected(Path::new(path), Some(Path::new(r"C:\Users\sam")))
    }

    #[test]
    fn windows_policy() {
        assert!(check(r"C:\"));
        assert!(check(r"C:\Users"));
        assert!(check(r"C:\Users\sam"));
        assert!(check(r"C:\Users\sam\Documents"));
        assert!(!check(r"C:\Users\sam\Documents\old"));
        assert!(check(r"C:\Windows\System32"));
        assert!(check(r"c:\windows\system32"));
        assert!(!check(r"C:\Windows\Temp\junk"));
        assert!(check(r"C:\Program Files\App"));
        assert!(!check(r"D:\Games\old"));
    }
}
