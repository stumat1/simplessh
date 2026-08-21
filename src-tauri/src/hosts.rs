//! Persists the saved home-lab host list to a JSON file. Secrets are never
//! stored here — only a reference (host/port/user); the password itself
//! lives in the secret store.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::HostEntry;

#[derive(Debug, Default, Serialize, Deserialize)]
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
        // Corrupt/unreadable store — start empty rather than crash.
        let data = std::fs::read_to_string(&file_path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { file_path, data }
    }

    /// Write the store to disk. Errors are returned (not swallowed) so
    /// explicit user actions (save/delete) can report them.
    fn persist(&self) -> Result<(), String> {
        if let Some(dir) = self.file_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&self.data).map_err(|e| e.to_string())?;
        std::fs::write(&self.file_path, json).map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<HostEntry> {
        self.data.hosts.clone()
    }

    /// Create or update a host entry (upsert by id). Returns the stored entry.
    pub fn save(&mut self, input: HostEntry) -> Result<HostEntry, String> {
        let entry = HostEntry {
            id: if input.id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                input.id.clone()
            },
            ..input
        };
        match self.data.hosts.iter_mut().find(|h| h.id == entry.id) {
            Some(existing) => *existing = entry.clone(),
            None => self.data.hosts.push(entry.clone()),
        }
        self.persist()?;
        Ok(entry)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.data.hosts.len();
        self.data.hosts.retain(|h| h.id != id);
        if self.data.hosts.len() != before {
            self.persist()?;
        }
        Ok(())
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
        store.delete(&saved.id).unwrap();
        assert!(store.list().is_empty());
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
