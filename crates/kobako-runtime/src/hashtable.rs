//! A table of objects held elsewhere, by key, as Linux's `hashtable.h`
//! keeps the dentries and inodes in use.

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

    /// The object at `key`, or the one `make` makes, kept there from now on.
    pub(crate) fn get_or_insert_with(&mut self, key: K, make: impl FnOnce() -> Arc<V>) -> Arc<V> {
        if let Some(held) = self.get(&key) {
            return held;
        }
        let made = make();
        self.insert(key, &made);
        made
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

    /// Keep only the entries whose key `keep` answers true for.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.entries.retain(|key, _| keep(key));
    }
}
