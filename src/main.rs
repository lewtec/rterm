mod app_icon;
mod clip;
mod fill;
mod gui;
mod keys;
mod mac_paste;
mod osc52;
mod session;
mod sleep_watch;

use std::fs::{File, OpenOptions};
use std::process::ExitCode;

use eframe::egui;

use rterm::catalog::{self, CatalogIo};
use rterm::link;
use rterm::paths;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{}", help_text());
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", rterm::version());
        return ExitCode::SUCCESS;
    }
    if let Some(key) = pending_key(&args) {
        let catalog = CatalogIo::system();
        catalog::write_pending(&key, &catalog.pending_place_file());
    }
    let lock = match instance_lock() {
        Lock::Held(file) => Some(file),
        Lock::Busy => return ExitCode::SUCCESS,
        Lock::Failed(err) => {
            eprintln!("rterm: instance lock: {err}");
            None
        }
    };
    let icon = crate::app_icon::icon_data(include_bytes!(
        "../Assets.xcassets/AppIcon.appiconset/icon_512x512.png"
    ))
    .expect("app icon png");
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("rterm")
            .with_icon(std::sync::Arc::new(icon))
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([720.0, 420.0])
            // The tab row sits in the title bar, beside the traffic lights.
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false),
        persist_window: true,
        persistence_path: Some(paths::caches_directory()),
        ..Default::default()
    };
    if let Err(err) = eframe::run_native(
        "rterm",
        native,
        Box::new(move |_cc| Ok(Box::new(gui::RtermApp::new(lock)))),
    ) {
        eprintln!("rterm: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn help_text() -> String {
    format!(
        "\
rterm {version}

Usage: rterm [rterm://place/<key> | file.rterm-place]

  --help, -h       Show this help
  --version, -V    Show the version
",
        version = rterm::version()
    )
}

fn pending_key(args: &[String]) -> Option<String> {
    let mut last = None;
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        if let Some(key) = link::switch_key_from(arg) {
            last = Some(key);
        }
    }
    last
}

enum Lock {
    Held(File),
    Busy,
    Failed(std::io::Error),
}

fn instance_lock() -> Lock {
    let dir = paths::caches_directory();
    if let Err(err) = std::fs::create_dir_all(&dir) {
        return Lock::Failed(err);
    }
    let path = dir.join("instance.lock");
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
    {
        Ok(file) => file,
        Err(err) => return Lock::Failed(err),
    };
    match file.try_lock() {
        Ok(()) => Lock::Held(file),
        Err(std::fs::TryLockError::WouldBlock) => Lock::Busy,
        Err(std::fs::TryLockError::Error(err)) => Lock::Failed(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_argument_is_the_pending_key() {
        let args = vec!["rterm://place/ada@localhost:herdr".into()];
        assert_eq!(pending_key(&args).as_deref(), Some("ada@localhost:herdr"));
    }

    #[test]
    fn flags_are_not_place_files() {
        assert_eq!(pending_key(&["--help".into()]), None);
    }

    #[test]
    fn place_file_argument_decodes() {
        let dir = std::env::temp_dir().join(format!("rterm-arg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("one.rterm-place");
        std::fs::write(&path, "ada@localhost:herdr\n").unwrap();
        assert_eq!(
            pending_key(&[path.to_string_lossy().into_owned()]).as_deref(),
            Some("ada@localhost:herdr")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
