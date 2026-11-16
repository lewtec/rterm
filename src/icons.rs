//! Small vector icons from Lucide (ISC).
//!
//! The path data is copied from <https://lucide.dev>. egui has no SVG path
//! loader, so this module flattens the handful of commands the icons use and
//! paints the result with egui shapes.

use eframe::egui;

/// Lucide `wrench`.
const WRENCH: &str = "M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z";

/// Lucide `arrow-up`.
const ARROW_UP: &str = "M5 12 12 5 19 12 M12 19 V5";

/// Lucide `arrow-down`.
const ARROW_DOWN: &str = "M12 5 V19 M19 12 12 19 5 12";

pub fn paint_wrench(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    paint_icon(painter, rect, color, WRENCH);
}

pub fn paint_arrow_up(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    paint_icon(painter, rect, color, ARROW_UP);
}

pub fn paint_arrow_down(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    paint_icon(painter, rect, color, ARROW_DOWN);
}

fn paint_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32, path: &str) {
    let scale = rect.width().min(rect.height()) / 24.0;
    let origin = rect.center() - egui::vec2(12.0 * scale, 12.0 * scale);
    // Lucide's default stroke width is 2 on a 24 px viewBox.
    let stroke = egui::Stroke::new((2.0 * scale).max(1.0), color);
    for subpath in parse_path(path) {
        let points: Vec<egui::Pos2> = subpath
            .points
            .iter()
            .map(|(x, y)| origin + egui::vec2(x * scale, y * scale))
            .collect();
        if points.len() < 2 {
            continue;
        }
        // Round caps and joins like the source SVG.
        for point in &points {
            painter.circle_filled(*point, stroke.width / 2.0, color);
        }
        if subpath.closed {
            painter.add(egui::Shape::closed_line(points, stroke));
        } else {
            painter.add(egui::Shape::line(points, stroke));
        }
    }
}

struct Subpath {
    points: Vec<(f32, f32)>,
    closed: bool,
}

#[derive(Clone, Copy)]
enum Token {
    Command(char),
    Number(f32),
}

fn tokenize(path: &str) -> Vec<Token> {
    let bytes = path.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() || byte == b',' {
            index += 1;
        } else if byte.is_ascii_alphabetic() {
            tokens.push(Token::Command(byte as char));
            index += 1;
        } else {
            let start = index;
            if byte == b'+' || byte == b'-' {
                index += 1;
            }
            let mut dot = false;
            while index < bytes.len() {
                if bytes[index].is_ascii_digit() {
                    index += 1;
                } else if bytes[index] == b'.' && !dot {
                    dot = true;
                    index += 1;
                } else {
                    break;
                }
            }
            if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
                index += 1;
                if index < bytes.len() && (bytes[index] == b'+' || bytes[index] == b'-') {
                    index += 1;
                }
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
            }
            if let Ok(value) = path[start..index].parse::<f32>() {
                tokens.push(Token::Number(value));
            }
        }
    }
    tokens
}

fn parse_path(path: &str) -> Vec<Subpath> {
    let tokens = tokenize(path);
    let mut index = 0;
    let mut current = (0.0, 0.0);
    let mut start = (0.0, 0.0);
    let mut closed = false;
    let mut points = Vec::new();
    let mut subpaths = Vec::new();

    while index < tokens.len() {
        let Token::Command(command) = tokens[index] else {
            index += 1;
            continue;
        };
        index += 1;
        let relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            'M' => {
                flush(&mut subpaths, &mut points, &mut closed);
                let Some((x, y)) = read_pair(&tokens, &mut index) else {
                    break;
                };
                current = offset(current, x, y, relative);
                start = current;
                points.push(current);
                while let Some((x, y)) = read_pair(&tokens, &mut index) {
                    current = offset(current, x, y, relative);
                    points.push(current);
                }
            }
            'L' => {
                while let Some((x, y)) = read_pair(&tokens, &mut index) {
                    current = offset(current, x, y, relative);
                    points.push(current);
                }
            }
            'H' => {
                while let Some(x) = read_number(&tokens, &mut index) {
                    current.0 = if relative { current.0 + x } else { x };
                    points.push(current);
                }
            }
            'V' => {
                while let Some(y) = read_number(&tokens, &mut index) {
                    current.1 = if relative { current.1 + y } else { y };
                    points.push(current);
                }
            }
            'C' => {
                while let Some([x1, y1, x2, y2, x, y]) = read_numbers::<6>(&tokens, &mut index) {
                    let control1 = offset(current, x1, y1, relative);
                    let control2 = offset(current, x2, y2, relative);
                    let end = offset(current, x, y, relative);
                    points.extend(cubic_points(current, control1, control2, end));
                    current = end;
                }
            }
            'A' => {
                while let Some([rx, ry, rotation, large, sweep, x, y]) =
                    read_numbers::<7>(&tokens, &mut index)
                {
                    let end = offset(current, x, y, relative);
                    points.extend(arc_points(
                        current,
                        rx,
                        ry,
                        rotation,
                        large != 0.0,
                        sweep != 0.0,
                        end,
                    ));
                    current = end;
                }
            }
            'Z' => {
                closed = true;
                current = start;
                flush(&mut subpaths, &mut points, &mut closed);
                points.push(start);
            }
            _ => {}
        }
    }
    flush(&mut subpaths, &mut points, &mut closed);
    subpaths
}

fn offset(from: (f32, f32), x: f32, y: f32, relative: bool) -> (f32, f32) {
    if relative {
        (from.0 + x, from.1 + y)
    } else {
        (x, y)
    }
}

fn flush(subpaths: &mut Vec<Subpath>, points: &mut Vec<(f32, f32)>, closed: &mut bool) {
    if points.len() >= 2 {
        subpaths.push(Subpath {
            points: std::mem::take(points),
            closed: *closed,
        });
    } else {
        points.clear();
    }
    *closed = false;
}

fn read_number(tokens: &[Token], index: &mut usize) -> Option<f32> {
    let Some(Token::Number(value)) = tokens.get(*index) else {
        return None;
    };
    *index += 1;
    Some(*value)
}

fn read_pair(tokens: &[Token], index: &mut usize) -> Option<(f32, f32)> {
    let x = read_number(tokens, index)?;
    let y = read_number(tokens, index)?;
    Some((x, y))
}

fn read_numbers<const N: usize>(tokens: &[Token], index: &mut usize) -> Option<[f32; N]> {
    let start = *index;
    let mut values = [0.0; N];
    for value in &mut values {
        let Some(Token::Number(number)) = tokens.get(*index) else {
            *index = start;
            return None;
        };
        *value = *number;
        *index += 1;
    }
    Some(values)
}

fn cubic_points(
    start: (f32, f32),
    control1: (f32, f32),
    control2: (f32, f32),
    end: (f32, f32),
) -> Vec<(f32, f32)> {
    const STEPS: usize = 12;
    (1..=STEPS)
        .map(|step| {
            let t = step as f32 / STEPS as f32;
            let mt = 1.0 - t;
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let d = t * t * t;
            (
                a * start.0 + b * control1.0 + c * control2.0 + d * end.0,
                a * start.1 + b * control1.1 + c * control2.1 + d * end.1,
            )
        })
        .collect()
}

fn arc_points(
    from: (f32, f32),
    rx: f32,
    ry: f32,
    rotation: f32,
    large_arc: bool,
    sweep: bool,
    to: (f32, f32),
) -> Vec<(f32, f32)> {
    let mut rx = rx.abs();
    let mut ry = ry.abs();
    if rx == 0.0 || ry == 0.0 {
        return vec![to];
    }
    let phi = rotation.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;
    let radius = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if radius > 1.0 {
        let scale = radius.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let numerator = (rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1).max(0.0);
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let factor = if denominator == 0.0 {
        0.0
    } else {
        sign * (numerator / denominator).sqrt()
    };
    let cx1 = factor * (rx * y1 / ry);
    let cy1 = factor * (-ry * x1 / rx);
    let center = (
        cos_phi * cx1 - sin_phi * cy1 + (from.0 + to.0) / 2.0,
        sin_phi * cx1 + cos_phi * cy1 + (from.1 + to.1) / 2.0,
    );
    let ux = (x1 - cx1) / rx;
    let uy = (y1 - cy1) / ry;
    let vx = (-x1 - cx1) / rx;
    let vy = (-y1 - cy1) / ry;
    let start_angle = vector_angle(1.0, 0.0, ux, uy);
    let mut delta = vector_angle(ux, uy, vx, vy);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let steps = ((delta.abs() / (std::f32::consts::PI / 16.0)).ceil() as usize).max(1);
    (1..=steps)
        .map(|step| {
            let angle = start_angle + delta * (step as f32 / steps as f32);
            let (sin_angle, cos_angle) = angle.sin_cos();
            (
                center.0 + rx * cos_angle * cos_phi - ry * sin_angle * sin_phi,
                center.1 + rx * cos_angle * sin_phi + ry * sin_angle * cos_phi,
            )
        })
        .collect()
}

fn vector_angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let length = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    if length == 0.0 {
        return 0.0;
    }
    let mut angle = (dot / length).clamp(-1.0, 1.0).acos();
    if ux * vy - uy * vx < 0.0 {
        angle = -angle;
    }
    angle
}

#[cfg(test)]
mod tests {
    use super::{ARROW_DOWN, ARROW_UP, WRENCH, parse_path, tokenize};

    #[test]
    fn wrench_is_one_closed_subpath() {
        let subpaths = parse_path(WRENCH);
        assert_eq!(subpaths.len(), 1);
        assert!(subpaths[0].closed);
        assert!(subpaths[0].points.len() > 20);
        for (x, y) in &subpaths[0].points {
            assert!((-1.0..=25.0).contains(x));
            assert!((-1.0..=25.0).contains(y));
        }
    }

    #[test]
    fn arrows_are_open_subpaths() {
        for path in [ARROW_UP, ARROW_DOWN] {
            let subpaths = parse_path(path);
            assert_eq!(subpaths.len(), 2);
            assert!(subpaths.iter().all(|subpath| !subpath.closed));
        }
    }

    #[test]
    fn adjacent_decimals_do_not_merge() {
        let tokens = tokenize("c.438.12.54.662.219.984");
        assert_eq!(tokens.len(), 7);
    }
}
