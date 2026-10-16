//! JSON-file persistence shared by the host and known-hosts stores: writes are
//! atomic, and a corrupt file is set aside rather than silently overwritten.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Load a JSON store. A missing file is an empty store. A file that can't be
/// read or parsed also yields an empty store, plus a warning describing why;
/// a corrupt file is first moved aside so the next save can't destroy it.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> (T, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (T::default(), None),
        Err(e) => {
            let warning = format!("{} could not be read: {e}", path.display());
            log::warn!("{warning}");
            return (T::default(), Some(warning));
        }
    };
    match serde_json::from_str(&text) {
        Ok(data) => (data, None),
        Err(e) => {
            let backup = backup_path(path);
            let warning = match std::fs::rename(path, &backup) {
                Ok(()) => format!(
                    "{} was corrupt ({e}); it was moved to {}",
                    path.display(),
                    backup.display()
                ),
                Err(move_err) => format!(
                    "{} was corrupt ({e}) and could not be moved aside: {move_err}",
                    path.display()
                ),
            };
            log::warn!("{warning}");
            (T::default(), Some(warning))
        }
    }
}

/// Write a JSON store atomically: write a sibling temp file, flush it to disk,
/// then rename it over the original. A crash mid-write leaves the previous
/// version intact instead of a truncated file.
pub fn save<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    let tmp = sibling(path, "tmp");
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

fn backup_path(path: &Path) -> PathBuf {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    sibling(path, &format!("corrupt-{secs}"))
}

/// `dir/name.json` -> `dir/name.json.<suffix>`
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    type Data = HashMap<String, u32>;

    #[test]
    fn missing_file_is_empty_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (data, warning) = load::<Data>(&dir.path().join("store.json"));
        assert!(data.is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("store.json");
        save(&path, &Data::from([("a".into(), 1)])).unwrap();
        save(&path, &Data::from([("b".into(), 2)])).unwrap();
        let (data, warning) = load::<Data>(&path);
        assert_eq!(data, Data::from([("b".into(), 2)]));
        assert!(warning.is_none());
        assert!(!sibling(&path, "tmp").exists());
    }

    #[test]
    fn corrupt_file_is_moved_aside_with_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        std::fs::write(&path, "{ not json").unwrap();

        let (data, warning) = load::<Data>(&path);
        assert!(data.is_empty());
        assert!(warning.is_some());
        assert!(!path.exists());

        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.starts_with("store.json.corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1);
        let backup = std::fs::read_to_string(dir.path().join(&backups[0])).unwrap();
        assert_eq!(backup, "{ not json");
    }
}
