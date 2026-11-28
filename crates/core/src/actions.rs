use crate::protect::is_protected;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub enum ActionError {
    /// computie refuses to touch this path.
    Protected(PathBuf),
    Trash(trash::Error),
    Io(std::io::Error),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protected(p) => write!(f, "{} is protected and stays put", p.display()),
            Self::Trash(e) => write!(f, "could not move to the Trash: {e}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// Moves `path` to the system Trash / Recycle Bin. Nothing is ever deleted
/// outright, so the user can always put it back.
pub fn move_to_trash(path: &Path, home: Option<&Path>) -> Result<(), ActionError> {
    if is_protected(path, home) {
        return Err(ActionError::Protected(path.to_path_buf()));
    }
    trash::delete(path).map_err(ActionError::Trash)
}

/// Shows `path` in Finder, Explorer or the Linux file manager.
pub fn reveal(path: &Path) -> Result<(), ActionError> {
    let mut command = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg("-R").arg(path);
        c
    } else if cfg!(target_os = "windows") {
        let mut c = Command::new("explorer");
        c.arg(format!("/select,{}", path.display()));
        c
    } else {
        // xdg-open cannot select a file, so open the folder that holds it.
        let mut c = Command::new("xdg-open");
        c.arg(path.parent().unwrap_or(path));
        c
    };
    command.spawn().map(|_| ()).map_err(ActionError::Io)
}
