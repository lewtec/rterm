//! Terminal face and size, saved beside the place catalog.

use std::path::{Path, PathBuf};

use rterm::paths;

pub const BUILT_IN: &str = "Built-in";
pub const DEFAULT_SIZE: f32 = 14.0;
const MIN_SIZE: f32 = 11.0;
const MAX_SIZE: f32 = 22.0;
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Appearance {
    pub size: f32,
    pub font: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            size: DEFAULT_SIZE,
            font: BUILT_IN.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub name: String,
    pub path: PathBuf,
}

pub fn path() -> PathBuf {
    paths::app_support_directory().join("appearance")
}

pub fn clamp(size: f32) -> f32 {
    size.clamp(MIN_SIZE, MAX_SIZE)
}

pub fn load(path: &Path) -> Appearance {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Appearance::default();
    };
    parse(&text)
}

pub fn parse(text: &str) -> Appearance {
    let mut appearance = Appearance::default();
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("font_size=") {
            if let Ok(size) = value.trim().parse::<f32>() {
                appearance.size = clamp(size);
            }
        } else if let Some(value) = line.strip_prefix("font=") {
            let name = value.trim();
            if !name.is_empty() {
                appearance.font = name.to_string();
            }
        }
    }
    appearance
}

pub fn save(path: &Path, appearance: &Appearance) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let size = clamp(appearance.size) as i32;
    let font = appearance.font.trim();
    let font = if font.is_empty() { BUILT_IN } else { font };
    std::fs::write(path, format!("font_size={size}\nfont={font}\n"))
}

pub fn faces() -> Vec<Face> {
    let mut files = Vec::new();
    for dir in font_dirs() {
        collect_fonts(&dir, 4, &mut files);
    }
    fontconfig_files(&mut files);
    files.sort();
    files.dedup();
    let stamp = file_stamp(&files);
    if let Some(cached) = cached_faces(stamp) {
        return cached;
    }
    let mut best: Vec<(String, u8, PathBuf)> = Vec::new();
    for path in files {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !looks_mono(stem) {
            continue;
        }
        let name = family_name(stem);
        let rank = weight_rank(stem);
        if let Some(existing) = best.iter_mut().find(|(have, _, _)| have == &name) {
            if rank < existing.1 {
                existing.1 = rank;
                existing.2 = path;
            }
        } else if font_bytes_ok(&path) {
            best.push((name, rank, path));
        }
    }
    best.sort_by(|a, b| a.0.cmp(&b.0));
    let faces: Vec<Face> = best
        .into_iter()
        .map(|(name, _, path)| Face { name, path })
        .collect();
    store_faces(stamp, &faces);
    faces
}

struct FaceCache {
    stamp: u64,
    faces: Vec<Face>,
}

fn face_cache() -> &'static std::sync::Mutex<Option<FaceCache>> {
    static CACHE: std::sync::Mutex<Option<FaceCache>> = std::sync::Mutex::new(None);
    &CACHE
}

fn cached_faces(stamp: u64) -> Option<Vec<Face>> {
    face_cache()
        .lock()
        .ok()?
        .as_ref()
        .and_then(|cached| (cached.stamp == stamp).then(|| cached.faces.clone()))
}

fn store_faces(stamp: u64, faces: &[Face]) {
    if let Ok(mut cache) = face_cache().lock() {
        *cache = Some(FaceCache {
            stamp,
            faces: faces.to_vec(),
        });
    }
}

fn file_stamp(files: &[PathBuf]) -> u64 {
    let mut stamp = files.len() as u64;
    for path in files {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        stamp = stamp.wrapping_mul(31).wrapping_add(meta.len());
        if let Ok(modified) = meta.modified()
            && let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH)
        {
            stamp = stamp.wrapping_add(since.as_nanos() as u64);
        }
        for byte in path.to_string_lossy().bytes() {
            stamp = stamp.wrapping_mul(33).wrapping_add(u64::from(byte));
        }
    }
    stamp
}

fn fontconfig_files(into: &mut Vec<PathBuf>) {
    let Ok(output) = std::process::Command::new("fc-list")
        .args([":mono", "file"])
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = line.split(':').next().unwrap_or("").trim();
        if is_font_file(Path::new(path)) {
            into.push(PathBuf::from(path));
        }
    }
}

pub fn apply(ctx: &eframe::egui::Context, font: &str, faces: &[Face]) {
    let mut fonts = eframe::egui::FontDefinitions::default();
    if font != BUILT_IN
        && let Some(face) = faces.iter().find(|face| face.name == font)
        && let Some(bytes) = read_font(&face.path)
    {
        fonts.font_data.insert(
            "terminal".into(),
            std::sync::Arc::new(eframe::egui::FontData::from_owned(bytes)),
        );
        if let Some(mono) = fonts.families.get_mut(&eframe::egui::FontFamily::Monospace) {
            mono.insert(0, "terminal".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn font_dirs() -> Vec<PathBuf> {
    let home = paths::home_dir();
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend([
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/System/Library/Fonts/Supplemental"),
            PathBuf::from("/Library/Fonts"),
            home.join("Library/Fonts"),
        ]);
    } else if cfg!(target_os = "windows") {
        dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
    } else {
        dirs.extend([
            PathBuf::from("/usr/share/fonts"),
            home.join(".local/share/fonts"),
            home.join(".fonts"),
        ]);
    }
    dirs
}

fn collect_fonts(dir: &Path, depth: u8, into: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, depth - 1, into);
        } else if is_font_file(&path) {
            into.push(path);
        }
    }
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("ttf" | "otf" | "ttc" | "TTF" | "OTF" | "TTC")
    )
}

fn looks_mono(stem: &str) -> bool {
    let name = stem.to_ascii_lowercase().replace(['-', '_', ' '], "");
    [
        "mono",
        "menlo",
        "monaco",
        "consolas",
        "hack",
        "jetbrains",
        "cascadia",
        "iosevka",
        "inconsolata",
        "firacode",
        "sourcecode",
        "courier",
        "commitmono",
        "geistmono",
        "sfnsmono",
        "sfmono",
        "sarasa",
        "envycode",
        "anonymouspro",
        "terminus",
        "intelone",
        "monaspace",
    ]
    .iter()
    .any(|key| name.contains(key))
}

fn family_name(stem: &str) -> String {
    let stem = stem.split('.').next().unwrap_or(stem);
    let mut words = Vec::new();
    for part in stem.split(['-', '_', ' ']) {
        if part.is_empty() || is_weight(part) || part.eq_ignore_ascii_case("vf") {
            continue;
        }
        let chars: Vec<char> = part.chars().collect();
        let mut word = String::new();
        for (index, ch) in chars.iter().enumerate() {
            if index > 0 && ch.is_uppercase() && chars[index - 1].is_lowercase() && !word.is_empty()
            {
                words.push(std::mem::take(&mut word));
            }
            word.push(*ch);
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    if words.is_empty() {
        stem.to_string()
    } else {
        words.join(" ")
    }
}

fn is_weight(part: &str) -> bool {
    matches!(
        part.to_ascii_lowercase().as_str(),
        "regular"
            | "roman"
            | "book"
            | "bold"
            | "italic"
            | "light"
            | "medium"
            | "semibold"
            | "bolditalic"
            | "thin"
            | "black"
            | "heavy"
            | "extralight"
            | "extrabold"
            | "retina"
            | "nl"
    )
}

fn weight_rank(stem: &str) -> u8 {
    let name = stem.to_ascii_lowercase();
    if name.contains("vf") || name.contains("variable") {
        return 4;
    }
    if name.contains("italic") || name.contains("oblique") || name.contains("bold") {
        3
    } else if name.contains("light") || name.contains("thin") || name.contains("medium") {
        2
    } else if name.contains("regular") || name.contains("roman") || name.contains("book") {
        0
    } else {
        1
    }
}

fn font_bytes_ok(path: &Path) -> bool {
    read_font(path).is_some()
}

fn read_font(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
        return None;
    }
    ab_glyph::FontRef::try_from_slice_and_index(&bytes, 0).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{Appearance, BUILT_IN, DEFAULT_SIZE, family_name, looks_mono, parse};

    #[test]
    fn missing_file_uses_the_built_in_face() {
        let appearance = parse("");
        assert_eq!(appearance.size, DEFAULT_SIZE);
        assert_eq!(appearance.font, BUILT_IN);
        assert_eq!(parse("font_size=nope\n").size, DEFAULT_SIZE);
    }

    #[test]
    fn size_is_clamped_and_the_face_is_kept() {
        assert_eq!(parse("font_size=4\n").size, 11.0);
        assert_eq!(parse("font_size=40\n").size, 22.0);
        let appearance = parse("font_size=16\nfont=JetBrains Mono\n");
        assert_eq!(
            appearance,
            Appearance {
                size: 16.0,
                font: "JetBrains Mono".into(),
            }
        );
    }

    #[test]
    fn family_names_drop_the_weight() {
        assert!(looks_mono("JetBrainsMono-Regular"));
        assert_eq!(family_name("JetBrainsMono-Regular"), "Jet Brains Mono");
        assert_eq!(family_name("Fira Code Regular"), "Fira Code");
        assert_eq!(family_name("Menlo"), "Menlo");
        assert!(!looks_mono("Helvetica"));
    }
}
