use std::path::Path;

/// Decides whether computie must refuse to trash `path`.
///
/// This is the last line of defence before something leaves the user's disk,
/// so the policy here is the most important product decision in the core.
///
/// `home` is the user's home folder when we know it.
///
/// TODO(you): write the real policy. Until then every path is protected, so
/// the app cannot trash anything (safe, but useless).
///
/// Things to decide:
/// - The root of a disk (`/`, `C:\`) and `home` itself: surely protected.
/// - Top-level system folders: `/System`, `/usr`, `/bin`, `/etc`, `/Library`,
///   `C:\Windows`, `C:\Program Files`. Protect only the folder, or everything
///   inside it too? (`/usr/local` and `/Library/Caches` are fair game for many
///   people.)
/// - Direct children of home such as `Documents` or `Desktop`: allow trashing
///   the whole folder, or only things inside it?
/// - Compare paths component by component (`Path::components`,
///   `Path::starts_with`), not as strings: "/usr" must not match "/usrdata".
pub fn is_protected(path: &Path, home: Option<&Path>) -> bool {
    let _ = (path, home);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_home_are_protected() {
        let home = Path::new("/home/sam");
        assert!(is_protected(Path::new("/"), Some(home)));
        assert!(is_protected(home, Some(home)));
    }

    #[test]
    #[ignore = "enable once is_protected has a real policy"]
    fn ordinary_folders_are_not_protected() {
        let home = Path::new("/home/sam");
        assert!(!is_protected(Path::new("/home/sam/Downloads/old.zip"), Some(home)));
        assert!(!is_protected(Path::new("/home/sam/.cache/thumbnails"), Some(home)));
    }

    #[test]
    #[ignore = "enable once is_protected has a real policy"]
    fn similar_names_do_not_match() {
        assert!(!is_protected(Path::new("/usrdata/stuff"), None));
    }
}
