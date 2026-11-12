//! A table of objects held elsewhere, by key, as Linux's `dentry_hashtable`
//! and `inode_hashtable` find the dentries and inodes in use. It holds each
//! weakly, so one nothing else holds drops out, where Linux's stays until the
//! shrinker prunes it.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Weak};

/// Each object by its key while something else holds it. Entries no longer
/// held are swept out once the table reaches twice what the last sweep
/// left, so each insertion pays a constant share of the sweeps.
pub(crate) struct Hashtable<K, V> {
    entries: HashMap<K, Weak<V>>,
    sweep_at: usize,
}

impl<K, V> Default for Hashtable<K, V> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            sweep_at: 0,
        }
    }
}

impl<K: Hash + Eq, V> Hashtable<K, V> {
    /// The object at `key`, while something holds it.
    pub(crate) fn get(&self, key: &K) -> Option<Arc<V>> {
        self.entries.get(key).and_then(Weak::upgrade)
    }

    pub(crate) fn insert(&mut self, key: K, value: &Arc<V>) {
        if self.entries.len() >= self.sweep_at {
            self.entries.retain(|_, held| held.strong_count() > 0);
            self.sweep_at = (self.entries.len() * 2).max(16);
        }
        self.entries.insert(key, Arc::downgrade(value));
    }

    /// Take the object at `key` out of the table, answering it while
    /// something holds it.
    pub(crate) fn remove(&mut self, key: &K) -> Option<Arc<V>> {
        self.entries.remove(key).and_then(|held| held.upgrade())
    }
}
