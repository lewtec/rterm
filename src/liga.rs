//! Programming ligatures for the terminal face.
//!
//! egui draws one character at a time, so a font's ligatures never form on
//! their own. When the chosen face has them, a run of plain ASCII is shaped
//! and the face's own glyphs are drawn. A variable font whose default master
//! is lighter than Regular is drawn at its Regular instance instead.

use ab_glyph::{Font, GlyphId, VariableFont};

pub struct TerminalFont {
    bytes: Vec<u8>,
    font: ab_glyph::FontVec,
    /// `wght` coordinate when the file's default master is below Regular.
    weight: Option<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphPlan {
    pub start: usize,
    pub end: usize,
    pub glyph_id: u16,
    /// Shift from the pen, in font units. Positive Y is up.
    pub x_offset: i32,
    pub y_offset: i32,
}

pub struct Coverage {
    pub width: usize,
    pub height: usize,
    pub alpha: Vec<u8>,
    /// Top-left of the bitmap relative to the baseline pen.
    pub left: f32,
    pub top: f32,
}

impl TerminalFont {
    pub fn load(bytes: Vec<u8>) -> Option<Self> {
        let weight = regular_weight(&bytes);
        let mut font = ab_glyph::FontVec::try_from_vec_and_index(bytes.clone(), 0).ok()?;
        if let Some(weight) = weight {
            let _ = font.set_variation(b"wght", weight);
        }
        rustybuzz::Face::from_slice(&bytes, 0)?;
        Some(Self {
            bytes,
            font,
            weight,
        })
    }

    #[cfg(test)]
    fn weight(&self) -> Option<f32> {
        self.weight
    }

    pub fn glyph_id(&self, ch: char) -> u16 {
        self.font.glyph_id(ch).0
    }

    /// Shaped glyphs when the run uses a ligature, otherwise one glyph per char.
    pub fn glyphs(&self, text: &str) -> Vec<GlyphPlan> {
        self.plan(text).unwrap_or_else(|| self.plain(text))
    }

    fn plain(&self, text: &str) -> Vec<GlyphPlan> {
        text.char_indices()
            .map(|(start, ch)| GlyphPlan {
                start,
                end: start + ch.len_utf8(),
                glyph_id: self.glyph_id(ch),
                x_offset: 0,
                y_offset: 0,
            })
            .collect()
    }

    pub fn plan(&self, text: &str) -> Option<Vec<GlyphPlan>> {
        if text.is_empty() || !may_ligate(text) {
            return None;
        }
        let face = self.shaped_face()?;
        let mut buffer = rustybuzz::UnicodeBuffer::new();
        buffer.push_str(text);
        let shaped = rustybuzz::shape(&face, &[], buffer);
        let infos = shaped.glyph_infos();
        let positions = shaped.glyph_positions();
        if infos.len() != positions.len() || infos.is_empty() {
            return None;
        }
        let mut plan = Vec::with_capacity(infos.len());
        for (index, info) in infos.iter().enumerate() {
            let start = info.cluster as usize;
            let end = infos
                .get(index + 1)
                .map(|next| next.cluster as usize)
                .unwrap_or(text.len());
            if start >= end || end > text.len() {
                return None;
            }
            let position = &positions[index];
            plan.push(GlyphPlan {
                start,
                end,
                glyph_id: u16::try_from(info.glyph_id).unwrap_or(0),
                x_offset: position.x_offset,
                y_offset: position.y_offset,
            });
        }
        let substituted = plan.iter().any(|glyph| {
            glyph.end - glyph.start != 1
                || text[glyph.start..glyph.end]
                    .chars()
                    .next()
                    .is_some_and(|ch| self.font.glyph_id(ch).0 != glyph.glyph_id)
        });
        substituted.then_some(plan)
    }

    fn shaped_face(&self) -> Option<rustybuzz::Face<'_>> {
        let mut face = rustybuzz::Face::from_slice(&self.bytes, 0)?;
        apply_weight(&mut face, self.weight);
        Some(face)
    }

    pub fn unit_scale(&self, px: f32) -> f32 {
        px / self.font.units_per_em().unwrap_or(1000.0)
    }

    /// Pixel scale egui uses for this point size: the em box times the font's
    /// height, not a 96 DPI point conversion. `pt_to_px_scale` draws the glyphs
    /// about 1.6× too big for the cell.
    pub fn raster_scale(&self, size_points: f32, pixels_per_point: f32) -> f32 {
        let upem = self.font.units_per_em().unwrap_or(1000.0);
        let height = self.font.height_unscaled();
        (size_points * pixels_per_point.max(1.0) * height / upem)
            .round()
            .max(1.0)
    }

    pub fn raster(&self, glyph_id: u16, px_scale: f32) -> Option<Coverage> {
        let outlined = self
            .font
            .outline_glyph(GlyphId(glyph_id).with_scale(px_scale))?;
        let bounds = outlined.px_bounds();
        let width = bounds.width().ceil() as usize;
        let height = bounds.height().ceil() as usize;
        if width == 0 || height == 0 || width > 512 || height > 256 {
            return None;
        }
        let mut alpha = vec![0u8; width * height];
        outlined.draw(|x, y, coverage| {
            let index = y as usize * width + x as usize;
            if let Some(slot) = alpha.get_mut(index) {
                *slot = (coverage * 255.0).round() as u8;
            }
        });
        Some(Coverage {
            width,
            height,
            alpha,
            left: bounds.min.x,
            top: bounds.min.y,
        })
    }
}

fn apply_weight(face: &mut rustybuzz::Face<'_>, weight: Option<f32>) {
    if let Some(weight) = weight {
        face.set_variations(&[rustybuzz::Variation {
            tag: rustybuzz::ttf_parser::Tag::from_bytes(b"wght"),
            value: weight,
        }]);
    }
}

/// Weight coordinate of the Regular instance, when the file's default is lighter.
fn regular_weight(bytes: &[u8]) -> Option<f32> {
    let fvar = table(bytes, b"fvar")?;
    let axis_count = u16_at(fvar, 8)? as usize;
    let axis_size = u16_at(fvar, 10)? as usize;
    let instance_count = u16_at(fvar, 12)? as usize;
    let instance_size = u16_at(fvar, 14)? as usize;
    let axes_at = u16_at(fvar, 4)? as usize;
    if axis_count == 0 || axis_size < 20 {
        return None;
    }
    let mut wght = None;
    for index in 0..axis_count {
        let at = axes_at + index * axis_size;
        if fvar.get(at..at + 4)? != b"wght" {
            continue;
        }
        wght = Some((
            index,
            fixed(fvar, at + 4)?,
            fixed(fvar, at + 8)?,
            fixed(fvar, at + 12)?,
        ));
    }
    let (index, min, default, max) = wght?;
    if default >= 399.0 {
        return None;
    }
    let names = english_names(bytes);
    let instances_at = axes_at + axis_count * axis_size;
    for nth in 0..instance_count {
        let at = instances_at + nth * instance_size;
        let name_id = u16_at(fvar, at)?;
        let value = fixed(fvar, at + 4 + index * 4)?;
        if names
            .get(&name_id)
            .is_some_and(|name| is_regular_name(name))
        {
            return Some(value.clamp(min, max));
        }
    }
    (min..=max).contains(&400.0).then_some(400.0)
}

fn is_regular_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "regular" | "roman" | "book" | "normal" | "text"
    )
}

fn english_names(bytes: &[u8]) -> std::collections::HashMap<u16, String> {
    let mut names = std::collections::HashMap::new();
    let Some(table) = table(bytes, b"name") else {
        return names;
    };
    let Some(count) = u16_at(table, 2) else {
        return names;
    };
    let Some(string_at) = u16_at(table, 4) else {
        return names;
    };
    for index in 0..count as usize {
        let at = 6 + index * 12;
        let Some(platform) = u16_at(table, at) else {
            break;
        };
        let Some(encoding) = u16_at(table, at + 2) else {
            break;
        };
        let Some(language) = u16_at(table, at + 4) else {
            break;
        };
        if platform != 3 || encoding != 1 || language != 0x0409 {
            continue;
        }
        let Some(name_id) = u16_at(table, at + 6) else {
            break;
        };
        let Some(length) = u16_at(table, at + 8) else {
            break;
        };
        let Some(offset) = u16_at(table, at + 10) else {
            break;
        };
        let start = string_at as usize + offset as usize;
        let Some(raw) = table.get(start..start + length as usize) else {
            continue;
        };
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&units) {
            names.insert(name_id, text);
        }
    }
    names
}

fn table<'a>(bytes: &'a [u8], want: &[u8; 4]) -> Option<&'a [u8]> {
    let count = u16_at(bytes, 4)? as usize;
    let mut at = 12usize;
    for _ in 0..count {
        let tag = bytes.get(at..at + 4)?;
        let start = u32_at(bytes, at + 8)? as usize;
        let len = u32_at(bytes, at + 12)? as usize;
        if tag == want {
            return bytes.get(start..start + len);
        }
        at += 16;
    }
    None
}

fn fixed(data: &[u8], at: usize) -> Option<f32> {
    Some(i32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as f32 / 65536.0)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

pub fn may_ligate(text: &str) -> bool {
    text.bytes().any(|byte| {
        matches!(
            byte,
            b'=' | b'<'
                | b'>'
                | b'-'
                | b'+'
                | b'*'
                | b'/'
                | b'|'
                | b'&'
                | b':'
                | b'?'
                | b'!'
                | b'#'
                | b'.'
                | b'~'
                | b'%'
                | b'\\'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::TerminalFont;

    #[test]
    fn fira_code_collapses_an_arrow() {
        let path = "/nix/store/p6bc5cmay7rzrvfk0m27gh30alcqp4zw-fira-code-6.2/share/fonts/truetype/FiraCode-VF.ttf";
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let Some(font) = TerminalFont::load(bytes) else {
            return;
        };
        assert!(font.plan("->").is_some(), "arrow should become a ligature");
        assert!(font.plan("ab").is_none());
        assert_eq!(font.weight(), Some(400.0));
        use ab_glyph::Font as _;
        use ab_glyph::ScaleFont as _;
        let scale = font.raster_scale(14.0, 2.0);
        let coverage = font.raster(font.glyph_id('M'), scale).unwrap();
        let advance = font
            .font
            .as_scaled(scale)
            .h_advance(ab_glyph::GlyphId(font.glyph_id('M')));
        let right = coverage.left + coverage.width as f32;
        assert!(
            right <= advance + 1.5,
            "M ink ends at {right}, advance is {advance}"
        );
    }
}
