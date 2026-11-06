use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn login_name() -> String {
    for key in ["USER", "LOGNAME"] {
        if let Ok(name) = std::env::var(key)
            && !name.is_empty()
        {
            return name;
        }
    }
    "user".into()
}

pub fn places_directory() -> PathBuf {
    home_dir().join("Documents").join("rterm")
}

pub fn caches_directory() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir().join("Library").join("Caches").join("rterm")
    } else {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".cache"));
        root.join("rterm")
    }
}

pub fn app_support_directory() -> PathBuf {
    if cfg!(target_os = "macos") {
        home_dir()
            .join("Library")
            .join("Application Support")
            .join("rterm")
    } else {
        let root = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local").join("share"));
        root.join("rterm")
    }
}

pub fn legacy_places_file() -> PathBuf {
    app_support_directory().join("places.toml")
}

pub fn verbose_log_path(place_id: &str, generation: u32) -> PathBuf {
    std::env::temp_dir().join(format!("rterm-ssh-{place_id}-{generation}.log"))
}

pub fn open_directory(path: &Path) -> std::io::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(path).spawn()?;
    Ok(())
}
