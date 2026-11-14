//! Generates the macOS icon set from the single master in packaging/icon.png.
//!
//! The artwork is one file. The per-size PNGs that `AppIcon.appiconset` lists
//! and that the app embeds are derived from it here, so the repository carries
//! no generated images.

use std::path::Path;

use image::imageops::FilterType;

/// Every image in `Assets.xcassets/AppIcon.appiconset/Contents.json`.
const ICONS: &[(&str, u32)] = &[
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

const MASTER: &str = "packaging/icon.png";
const ICONSET: &str = "Assets.xcassets/AppIcon.appiconset";

fn main() {
    println!("cargo:rerun-if-changed={MASTER}");

    let master = image::open(MASTER)
        .unwrap_or_else(|err| panic!("read {MASTER}: {err}"))
        .into_rgba8();
    let iconset = Path::new(ICONSET);
    std::fs::create_dir_all(iconset).unwrap_or_else(|err| panic!("create {ICONSET}: {err}"));

    for (name, size) in ICONS {
        let icon = if *size == master.width() {
            master.clone()
        } else {
            image::imageops::resize(&master, *size, *size, FilterType::Lanczos3)
        };
        let path = iconset.join(name);
        icon.save(&path)
            .unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
    }
}
