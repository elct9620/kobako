//! The directories of an Overlay opened as Filesystems of their own. Each
//! is kept by the directory above it and its name there, as overlayfs
//! keeps its dentries, so a directory moved through the Overlay takes
//! every one opened beneath it along. A node holds no Layer's directory
//! open itself, so the descriptors an Overlay holds stay bounded by the
//! directories handed out.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use super::super::path::{components, name, parent};

/// One directory of the view, and where it stands now.
pub(super) struct Node {
    state: Mutex<State>,
}

struct State {
    /// The directory above, `None` for the root.
    parent: Option<Arc<Node>>,
    name: String,
    /// Whether the directory was removed or replaced through the Overlay,
    /// so nothing reaches it any more.
    removed: bool,
}

impl Node {
    fn new(parent: Option<Arc<Node>>, name: &str) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                parent,
                name: name.to_string(),
                removed: false,
            }),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where the directory stands beneath the Overlay's root.
    pub(super) fn path(&self) -> String {
        let mut names = Vec::new();
        let mut at = {
            let state = self.state();
            names.push(state.name.clone());
            state.parent.clone()
        };
        while let Some(node) = at {
            let state = node.state();
            names.push(state.name.clone());
            at = state.parent.clone();
        }
        names.reverse();
        names.retain(|name| !name.is_empty());
        names.join("/")
    }

    pub(super) fn removed(&self) -> bool {
        self.state().removed
    }
}

/// Every directory of one Overlay that is open, by the directory above it
/// and its name there.
pub(super) struct Nodes {
    root: Arc<Node>,
    open: Mutex<Open>,
}

/// The open directories, and how many entries the table may reach before
/// those no longer open are swept out: twice what the last sweep left, so
/// each opening pays a constant share of the sweeps.
#[derive(Default)]
struct Open {
    entries: HashMap<(usize, String), Weak<Node>>,
    sweep_at: usize,
}

impl Open {
    fn sweep_when_grown(&mut self) {
        if self.entries.len() < self.sweep_at {
            return;
        }
        self.entries.retain(|_, node| node.strong_count() > 0);
        self.sweep_at = (self.entries.len() * 2).max(16);
    }

    fn get(&self, key: &(usize, String)) -> Option<Arc<Node>> {
        self.entries.get(key).and_then(Weak::upgrade)
    }
}

impl Default for Nodes {
    fn default() -> Self {
        Self {
            root: Node::new(None, ""),
            open: Mutex::default(),
        }
    }
}

impl Nodes {
    fn open(&self) -> MutexGuard<'_, Open> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn root(&self) -> Arc<Node> {
        self.root.clone()
    }

    /// The directory at `path` beneath the root, opened unless it is open
    /// already, with each directory above it.
    pub(super) fn at(&self, path: &str) -> Arc<Node> {
        let mut open = self.open();
        open.sweep_when_grown();
        let mut node = self.root.clone();
        for name in components(path) {
            let key = (Arc::as_ptr(&node) as usize, name.to_string());
            node = match open.get(&key) {
                Some(child) => child,
                None => {
                    let child = Node::new(Some(node), name);
                    open.entries.insert(key, Arc::downgrade(&child));
                    child
                }
            };
        }
        node
    }

    /// The open directory at `path`, if one is.
    fn find(&self, path: &str) -> Option<Arc<Node>> {
        let open = self.open();
        let mut node = self.root.clone();
        for name in components(path) {
            let key = (Arc::as_ptr(&node) as usize, name.to_string());
            node = open.get(&key)?;
        }
        Some(node)
    }

    /// The directory at `path` was removed, or replaced by a move.
    pub(super) fn removed(&self, path: &str) {
        let Some(node) = self.find(path) else {
            return;
        };
        let parent = node.state().parent.clone();
        if let Some(parent) = parent {
            self.open()
                .entries
                .remove(&(Arc::as_ptr(&parent) as usize, name(path).to_string()));
        }
        node.state().removed = true;
    }

    /// The entry at `from` moved to `to`, taking the directory opened
    /// there, and every one beneath it, along.
    pub(super) fn moved(&self, from: &str, to: &str) {
        let Some(node) = self.find(from) else {
            return;
        };
        let target = self.at(parent(to));
        let mut open = self.open();
        let mut state = node.state();
        if let Some(previous) = &state.parent {
            open.entries
                .remove(&(Arc::as_ptr(previous) as usize, state.name.clone()));
        }
        open.entries.insert(
            (Arc::as_ptr(&target) as usize, name(to).to_string()),
            Arc::downgrade(&node),
        );
        state.parent = Some(target);
        state.name = name(to).to_string();
    }
}
