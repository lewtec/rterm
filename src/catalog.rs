use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::codec::{self, CatalogDocument, CatalogError};
use crate::paths;
use crate::place::{self, FILE_EXTENSION};

struct IoState {
    ignore_until: Mutex<Option<Instant>>,
    events: Mutex<Vec<Instant>>,
}

pub struct CatalogIo {
    pub directory: PathBuf,
    legacy: PathBuf,
    state: Arc<IoState>,
    watcher: Mutex<Option<RecommendedWatcher>>,
}

impl CatalogIo {
    pub fn system() -> Self {
        Self::with_parts(paths::places_directory(), paths::legacy_places_file())
    }

    /// `file` is the legacy toml. Place files live in a sibling `places/` directory.
    pub fn from_legacy_file(file: PathBuf) -> Self {
        let directory = file
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("places");
        Self::with_parts(directory, file)
    }

    pub fn with_parts(directory: PathBuf, legacy: PathBuf) -> Self {
        Self {
            directory,
            legacy,
            state: Arc::new(IoState {
                ignore_until: Mutex::new(None),
                events: Mutex::new(Vec::new()),
            }),
            watcher: Mutex::new(None),
        }
    }

    /// Parent of the catalog directory, not Application Support.
    /// Cold-start URLs and the running app share this path.
    pub fn pending_place_file(&self) -> PathBuf {
        self.directory
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("pending-place")
    }

    pub fn prepare(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)
    }

    pub fn file_exists(&self) -> bool {
        !self.place_files().is_empty() || self.legacy.is_file()
    }

    pub fn load(&self) -> Result<CatalogDocument, LoadError> {
        self.prepare()?;
        let files = self.place_files();
        if files.is_empty() && self.legacy.is_file() {
            let text = std::fs::read_to_string(&self.legacy)?;
            let document = codec::decode(&text)?;
            self.save(&document)?;
            let _ = std::fs::remove_file(&self.legacy);
            return Ok(document);
        }
        let mut places = Vec::new();
        for file in files {
            let text = std::fs::read_to_string(&file)?;
            places.push(codec::decode_place_file(&text)?);
        }
        Ok(CatalogDocument {
            version: codec::CURRENT_VERSION,
            places,
        })
    }

    pub fn save(&self, document: &CatalogDocument) -> Result<(), LoadError> {
        self.prepare()?;
        if let Ok(mut until) = self.state.ignore_until.lock() {
            *until = Some(Instant::now() + Duration::from_millis(400));
        }
        let mut written = HashSet::new();
        for (index, place) in document.places.iter().enumerate() {
            let name = place::file_name(index, place);
            let path = self.directory.join(&name);
            atomic_write(&path, &codec::encode_place_file(place))?;
            written.insert(name);
        }
        for file in self.place_files() {
            let name = file
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !written.contains(&name) {
                let _ = std::fs::remove_file(file);
            }
        }
        Ok(())
    }

    pub fn start_watching(&self) -> Result<(), String> {
        self.prepare().map_err(|err| err.to_string())?;
        let state = Arc::clone(&self.state);
        let mut watcher =
            notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
                if result.is_ok()
                    && let Ok(mut events) = state.events.lock()
                {
                    events.push(Instant::now());
                }
            })
            .map_err(|err| err.to_string())?;
        watcher
            .watch(&self.directory, RecursiveMode::NonRecursive)
            .map_err(|err| err.to_string())?;
        *self.watcher.lock().unwrap_or_else(|err| err.into_inner()) = Some(watcher);
        Ok(())
    }

    /// True when a change landed, the own-write window passed, and the
    /// 150ms debounce since the last event has elapsed.
    pub fn poll_changes(&self) -> bool {
        let Ok(mut events) = self.state.events.lock() else {
            return false;
        };
        if events.is_empty() {
            return false;
        }
        if let Ok(until) = self.state.ignore_until.lock()
            && let Some(until) = *until
            && Instant::now() < until
        {
            events.clear();
            return false;
        }
        let newest = events.iter().copied().max().unwrap_or_else(Instant::now);
        if newest.elapsed() < Duration::from_millis(150) {
            return false;
        }
        events.clear();
        true
    }

    fn place_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.directory) else {
            return files;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(FILE_EXTENSION) {
                files.push(path);
            }
        }
        files.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
        files
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Catalog(CatalogError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Catalog(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<CatalogError> for LoadError {
    fn from(err: CatalogError) -> Self {
        Self::Catalog(err)
    }
}

pub fn write_pending(key: &str, to: &Path) {
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(to, key);
}

pub fn take_pending(from: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(from).ok()?;
    let _ = std::fs::remove_file(from);
    let key = raw.trim().to_string();
    if key.is_empty() { None } else { Some(key) }
}

fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::place::Place;
    use uuid::Uuid;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rterm-cat-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn migrates_legacy_toml_into_place_files() {
        let tmp = Tmp::new();
        let toml = tmp.0.join("places.toml");
        let place = Place {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            user: "ada".into(),
            host: "riverwood".into(),
            backend: Backend::Herdr,
            session: None,
            label: None,
        };
        std::fs::write(
            &toml,
            codec::encode(&CatalogDocument {
                version: 1,
                places: vec![place.clone()],
            }),
        )
        .unwrap();
        let io = CatalogIo::from_legacy_file(toml.clone());
        let loaded = io.load().unwrap();
        assert_eq!(loaded.places, vec![place.clone()]);
        assert!(!toml.exists());
        let places_dir = tmp.0.join("places");
        assert!(places_dir.join(place::file_name(0, &place)).is_file());
    }

    #[test]
    fn save_preserves_tab_order() {
        let tmp = Tmp::new();
        let directory = tmp.0.join("places");
        let places = vec![
            Place::new("ada", "remote.example", Backend::Herdr, None),
            Place::new("ada", "riverwood", Backend::Tmux, Some("foo".into())),
        ];
        let io = CatalogIo::with_parts(directory, tmp.0.join("missing.toml"));
        io.save(&CatalogDocument {
            version: 1,
            places: places.clone(),
        })
        .unwrap();
        let loaded: Vec<_> = io
            .load()
            .unwrap()
            .places
            .iter()
            .map(Place::switch_key)
            .collect();
        let expect: Vec<_> = places.iter().map(Place::switch_key).collect();
        assert_eq!(loaded, expect);
    }

    #[test]
    fn save_renumbers_prefixes_after_reorder() {
        let tmp = Tmp::new();
        let directory = tmp.0.join("places");
        let first = Place::new("ada", "remote.example", Backend::Herdr, None);
        let second = Place::new("ada", "riverwood", Backend::Tmux, Some("foo".into()));
        let third = Place::new("ada", "riften", Backend::Herdr, None);
        let io = CatalogIo::with_parts(directory.clone(), tmp.0.join("missing.toml"));
        io.save(&CatalogDocument {
            version: 1,
            places: vec![first.clone(), second.clone(), third.clone()],
        })
        .unwrap();
        io.save(&CatalogDocument {
            version: 1,
            places: vec![second.clone(), third.clone(), first.clone()],
        })
        .unwrap();
        assert_eq!(
            place_file_names(&directory),
            vec![
                place::file_name(0, &second),
                place::file_name(1, &third),
                place::file_name(2, &first),
            ]
        );
        let loaded: Vec<_> = io
            .load()
            .unwrap()
            .places
            .iter()
            .map(Place::switch_key)
            .collect();
        assert_eq!(
            loaded,
            vec![second.switch_key(), third.switch_key(), first.switch_key()]
        );
    }

    #[test]
    fn take_pending_reads_and_removes_the_file() {
        let tmp = Tmp::new();
        let url = tmp.0.join("pending-place");
        write_pending("ada@remote.example:herdr", &url);
        assert_eq!(take_pending(&url).as_deref(), Some("ada@remote.example:herdr"));
        assert!(take_pending(&url).is_none());
    }

    fn place_file_names(directory: &Path) -> Vec<String> {
        let mut names = Vec::new();
        let Ok(entries) = std::fs::read_dir(directory) else {
            return names;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(FILE_EXTENSION) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
    }
}
