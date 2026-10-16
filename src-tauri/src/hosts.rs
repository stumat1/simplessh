//! Persists the saved home-lab host list to a JSON file. Secrets are never
//! stored here — only a reference (host/port/user); the password itself
//! lives in the secret store.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::HostEntry;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersistShape {
    #[serde(default)]
    hosts: Vec<HostEntry>,
}

pub struct HostStore {
    file_path: PathBuf,
    data: PersistShape,
}

impl HostStore {
    pub fn new(file_path: PathBuf) -> Self {
        // Unreadable store — start empty rather than crash (a corrupt file
        // is kept aside; the warning is logged by store::load).
        let (data, _warning) = crate::store::load(&file_path);
        Self { file_path, data }
    }

    /// Write the store to disk. Errors are returned (not swallowed) so
    /// explicit user actions (save/delete) can report them.
    fn persist(&self, data: &PersistShape) -> Result<(), String> {
        crate::store::save(&self.file_path, data)
    }

    pub fn list(&self) -> Vec<HostEntry> {
        self.data.hosts.clone()
    }

    /// Create or update a host entry (upsert by id). A new entry (empty id)
    /// for a user@host:port that is already saved updates that entry instead
    /// of adding a duplicate. Returns the stored entry.
    pub fn save(&mut self, input: HostEntry) -> Result<HostEntry, String> {
        let id = if !input.id.is_empty() {
            input.id.clone()
        } else if let Some(existing) = self.data.hosts.iter().find(|h| h.same_target(&input)) {
            existing.id.clone()
        } else {
            Uuid::new_v4().to_string()
        };
        let entry = HostEntry { id, ..input };
        let mut candidate = self.data.clone();
        match candidate.hosts.iter_mut().find(|h| h.id == entry.id) {
            Some(existing) => *existing = entry.clone(),
            None => candidate.hosts.push(entry.clone()),
        }
        self.persist(&candidate)?;
        self.data = candidate;
        Ok(entry)
    }

    /// Remove a host entry; returns the removed entry, or None if no entry
    /// had that id.
    pub fn delete(&mut self, id: &str) -> Result<Option<HostEntry>, String> {
        let mut candidate = self.data.clone();
        let Some(index) = candidate.hosts.iter().position(|h| h.id == id) else {
            return Ok(None);
        };
        let removed = candidate.hosts.remove(index);
        self.persist(&candidate)?;
        self.data = candidate;
        Ok(Some(removed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, HostStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = HostStore::new(dir.path().join("hosts.json"));
        (dir, store)
    }

    #[test]
    fn save_assigns_id() {
        let (_dir, mut store) = store();
        let saved = store
            .save(HostEntry {
                id: String::new(),
                name: "Test".into(),
                host: "h".into(),
                port: 22,
                username: "u".into(),
                save_password: false,
            })
            .unwrap();
        assert!(!saved.id.is_empty());
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn save_updates_existing_by_id() {
        let (_dir, mut store) = store();
        let saved = store
            .save(HostEntry {
                id: String::new(),
                name: "Test".into(),
                host: "h".into(),
                port: 22,
                username: "u".into(),
                save_password: false,
            })
            .unwrap();
        store
            .save(HostEntry {
                name: "Renamed".into(),
                ..saved.clone()
            })
            .unwrap();
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Renamed");
    }

    #[test]
    fn delete_removes_entry() {
        let (_dir, mut store) = store();
        let saved = store
            .save(HostEntry {
                id: String::new(),
                name: "Test".into(),
                host: "h".into(),
                port: 22,
                username: "u".into(),
                save_password: false,
            })
            .unwrap();
        let removed = store.delete(&saved.id).unwrap();
        assert_eq!(removed.map(|h| h.id), Some(saved.id.clone()));
        assert!(store.list().is_empty());
        assert!(store.delete(&saved.id).unwrap().is_none());
    }

    #[test]
    fn saving_same_target_again_updates_instead_of_duplicating() {
        let (_dir, mut store) = store();
        let first = store
            .save(HostEntry {
                id: String::new(),
                name: "NAS".into(),
                host: "h".into(),
                port: 22,
                username: "u".into(),
                save_password: false,
            })
            .unwrap();
        let second = store
            .save(HostEntry {
                id: String::new(),
                name: "NAS (renamed)".into(),
                host: "h".into(),
                port: 22,
                username: "u".into(),
                save_password: true,
            })
            .unwrap();
        assert_eq!(first.id, second.id);
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "NAS (renamed)");
        assert!(list[0].save_password);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts.json");
        {
            let mut store = HostStore::new(path.clone());
            store
                .save(HostEntry {
                    id: String::new(),
                    name: "Test".into(),
                    host: "h".into(),
                    port: 22,
                    username: "u".into(),
                    save_password: true,
                })
                .unwrap();
        }
        let reloaded = HostStore::new(path);
        assert_eq!(reloaded.list().len(), 1);
        assert!(reloaded.list()[0].save_password);
    }
}
