use std::path::{Path, PathBuf};

/// Index of a node inside [`Tree`]. The root is always `0`.
pub type NodeId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Symlink,
    /// Something we could not read (permission denied, vanished mid-scan).
    Unreadable,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    /// Space taken on disk, in bytes, including everything underneath.
    pub size: u64,
    pub kind: Kind,
    pub parent: Option<NodeId>,
    /// Sorted biggest first.
    pub children: Vec<NodeId>,
    /// Files underneath (a file counts itself).
    pub files: u64,
    /// Entries underneath that could not be read.
    pub errors: u32,
}

/// The result of a scan: every file and folder, stored flat in one `Vec`.
#[derive(Debug)]
pub struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) root_path: PathBuf,
}

impl Tree {
    pub const ROOT: NodeId = 0;

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Children of `id`, biggest first.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = (NodeId, &Node)> + '_ {
        self.node(id).children.iter().map(move |&c| (c, self.node(c)))
    }

    /// Full path on disk for a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = id;
        while let Some(parent) = self.node(cur).parent {
            parts.push(self.node(cur).name.as_str());
            cur = parent;
        }
        let mut path = self.root_path.clone();
        for part in parts.into_iter().rev() {
            path.push(part);
        }
        path
    }

    /// Takes a node out of the tree after it was trashed, and shrinks every
    /// folder above it by the same amount.
    pub fn remove(&mut self, id: NodeId) {
        let Some(parent) = self.node(id).parent else { return };
        let (size, files, errors) = {
            let n = self.node(id);
            (n.size, n.files, n.errors)
        };
        self.nodes[parent as usize].children.retain(|&c| c != id);
        let mut cur = Some(parent);
        while let Some(p) = cur {
            let n = &mut self.nodes[p as usize];
            n.size = n.size.saturating_sub(size);
            n.files = n.files.saturating_sub(files);
            n.errors = n.errors.saturating_sub(errors);
            cur = n.parent;
        }
    }
}
