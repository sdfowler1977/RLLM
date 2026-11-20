//! Anti-aliased drawing on a 32-bit pixel buffer. The desktop window paints its
//! panels, icons, diagrams and wallpaper through this file, so every platform
//! window that owns a pixel buffer can share one drawing path.

/// Colours are 0xAARRGGBB. Pixels are 0x00RRGGBB.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u32],
    pub width: i32,
    pub height: i32,
    pub clip: [i32; 4],
}

#[inline]
pub fn over(dst: u32, color: u32, alpha: u32) -> u32 {
    if alpha == 0 {
        return dst;
    }
    if alpha >= 255 {
        return color & 0x00FF_FFFF;
    }
    let inv = 255 - alpha;
    let rb = (color & 0x00FF_00FF) * alpha + (dst & 0x00FF_00FF) * inv + 0x0080_0080;
    let rb = ((rb + ((rb >> 8) & 0x00FF_00FF)) >> 8) & 0x00FF_00FF;
    let g = (color & 0x0000_FF00) * alpha + (dst & 0x0000_FF00) * inv + 0x0000_8000;
    let g = ((g + ((g >> 8) & 0x0000_FF00)) >> 8) & 0x0000_FF00;
    rb | g
}

/// One colour laid over another, both 0xAARRGGBB; the result is opaque.
pub fn flatten(color: u32, onto: u32) -> u32 {
    0xFF00_0000 | over(onto & 0x00FF_FFFF, color, color >> 24)
}

pub fn with_alpha(color: u32, alpha: u32) -> u32 {
    (color & 0x00FF_FFFF) | (alpha.min(255) << 24)
}

fn rounded(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = (px - cx).abs() - (hx - r);
    let qy = (py - cy).abs() - (hy - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

impl<'a> Canvas<'a> {
    pub fn new(pixels: &'a mut [u32], width: i32, height: i32) -> Canvas<'a> {
        Canvas { pixels, width, height, clip: [0, 0, width, height] }
    }

    pub fn set_clip(&mut self, clip: Option<[i32; 4]>) {
        self.clip = match clip {
            Some(area) => [area[0].max(0), area[1].max(0), area[2].min(self.width), area[3].min(self.height)],
            None => [0, 0, self.width, self.height],
        };
    }

    #[inline]
    fn dot(&mut self, x: i32, y: i32, color: u32, alpha: u32) {
        if x < self.clip[0] || x >= self.clip[2] || y < self.clip[1] || y >= self.clip[3] {
            return;
        }
        let at = (y * self.width + x) as usize;
        self.pixels[at] = over(self.pixels[at], color, alpha);
    }

    fn span(&mut self, y: i32, from: i32, to: i32, color: u32, alpha: u32) {
        if y < self.clip[1] || y >= self.clip[3] {
            return;
        }
        let from = from.max(self.clip[0]);
        let to = to.min(self.clip[2]);
        if from >= to {
            return;
        }
        let start = (y * self.width + from) as usize;
        let end = (y * self.width + to) as usize;
        if alpha >= 255 {
            self.pixels[start..end].fill(color & 0x00FF_FFFF);
        } else {
            for pixel in &mut self.pixels[start..end] {
                *pixel = over(*pixel, color, alpha);
            }
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let alpha = color >> 24;
        for row in y.max(self.clip[1])..(y + h).min(self.clip[3]) {
            self.span(row, x, x + w, color, alpha);
        }
    }

    /// A filled rounded rectangle.
    pub fn round(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, color: u32) {
        let base = color >> 24;
        if w <= 0 || h <= 0 || base == 0 {
            return;
        }
        let r = radius.max(0.0).min(w.min(h) as f32 / 2.0);
        let edge = r.ceil() as i32;
        let (x1, y1) = (x + w, y + h);
        let middle = x + w / 2;
        for row in y.max(self.clip[1])..y1.min(self.clip[3]) {
            let upper = row < y + edge;
            let lower = row >= y1 - edge;
            if edge == 0 || (!upper && !lower) {
                self.span(row, x, x1, color, base);
                continue;
            }
            self.span(row, x + edge, x1 - edge, color, base);
            let dy = if upper { y as f32 + r - (row as f32 + 0.5) } else { row as f32 + 0.5 - (y1 as f32 - r) };
            for col in x..(x + edge).min(middle) {
                let dx = x as f32 + r - (col as f32 + 0.5);
                let cover = if dx <= 0.0 || dy <= 0.0 { 1.0 } else { (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0) };
                self.dot(col, row, color, (base as f32 * cover) as u32);
            }
            for col in (x1 - edge).max(middle)..x1 {
                let dx = col as f32 + 0.5 - (x1 as f32 - r);
                let cover = if dx <= 0.0 || dy <= 0.0 { 1.0 } else { (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0) };
                self.dot(col, row, color, (base as f32 * cover) as u32);
            }
        }
    }

    /// The outline of a rounded rectangle, drawn inside its bounds.
    pub fn ring(&mut self, x: i32, y: i32, w: i32, h: i32, radius: f32, width: f32, color: u32) {
        let base = color >> 24;
        if w <= 0 || h <= 0 || base == 0 || width <= 0.0 {
            return;
        }
        let r = radius.max(0.0).min(w.min(h) as f32 / 2.0);
        let (hx, hy) = (w as f32 / 2.0, h as f32 / 2.0);
        let (cx, cy) = (x as f32 + hx, y as f32 + hy);
        let band = (r + width).ceil() as i32 + 1;
        let strip = width.ceil() as i32 + 1;
        let (x1, y1) = (x + w, y + h);
        for row in y.max(self.clip[1])..y1.min(self.clip[3]) {
            let whole = row < y + band || row >= y1 - band;
            let ranges = if whole { [(x, x1), (0, 0)] } else { [(x, (x + strip).min(x1)), ((x1 - strip).max(x + strip), x1)] };
            for (from, to) in ranges {
                for col in from.max(self.clip[0])..to.min(self.clip[2]) {
                    let d = rounded(col as f32 + 0.5, row as f32 + 0.5, cx, cy, hx, hy, r);
                    let cover = (0.5 - d).clamp(0.0, 1.0) - (0.5 - d - width).clamp(0.0, 1.0);
                    if cover > 0.0 {
                        self.dot(col, row, color, (base as f32 * cover) as u32);
                    }
                }
            }
        }
    }

    /// Round-capped strokes. The segments are covered as one shape, so joins
    /// are not blended twice.
    pub fn strokes(&mut self, lines: &[[f32; 4]], width: f32, color: u32) {
        let base = color >> 24;
        if lines.is_empty() || base == 0 {
            return;
        }
        let half = width / 2.0;
        let reach = half + 1.0;
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for line in lines {
            bounds[0] = bounds[0].min(line[0].min(line[2]));
            bounds[1] = bounds[1].min(line[1].min(line[3]));
            bounds[2] = bounds[2].max(line[0].max(line[2]));
            bounds[3] = bounds[3].max(line[1].max(line[3]));
        }
        let left = ((bounds[0] - reach).floor() as i32).max(self.clip[0]);
        let top = ((bounds[1] - reach).floor() as i32).max(self.clip[1]);
        let right = ((bounds[2] + reach).ceil() as i32).min(self.clip[2]);
        let bottom = ((bounds[3] + reach).ceil() as i32).min(self.clip[3]);
        for row in top..bottom {
            let py = row as f32 + 0.5;
            for col in left..right {
                let px = col as f32 + 0.5;
                let mut best = f32::MAX;
                for line in lines {
                    let (vx, vy) = (line[2] - line[0], line[3] - line[1]);
                    let (wx, wy) = (px - line[0], py - line[1]);
                    let span = vx * vx + vy * vy;
                    let t = if span <= 0.0 { 0.0 } else { ((wx * vx + wy * vy) / span).clamp(0.0, 1.0) };
                    let (dx, dy) = (wx - vx * t, wy - vy * t);
                    best = best.min(dx * dx + dy * dy);
                }
                let cover = (half + 0.5 - best.sqrt()).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.dot(col, row, color, (base as f32 * cover) as u32);
                }
            }
        }
    }

    pub fn disc(&mut self, cx: f32, cy: f32, radius: f32, color: u32) {
        let base = color >> 24;
        let reach = radius + 1.0;
        for row in ((cy - reach).floor() as i32).max(self.clip[1])..((cy + reach).ceil() as i32).min(self.clip[3]) {
            for col in ((cx - reach).floor() as i32).max(self.clip[0])..((cx + reach).ceil() as i32).min(self.clip[2]) {
                let (dx, dy) = (col as f32 + 0.5 - cx, row as f32 + 0.5 - cy);
                let cover = (radius + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.dot(col, row, color, (base as f32 * cover) as u32);
                }
            }
        }
    }

    /// A round-capped arc. Angles are degrees, clockwise from three o'clock,
    /// the convention the monitor geometry uses.
    pub fn arc(&mut self, cx: f32, cy: f32, radius: f32, start: f32, sweep: f32, width: f32, color: u32) {
        let base = color >> 24;
        if base == 0 || radius <= 0.0 {
            return;
        }
        let half = width / 2.0;
        let reach = radius + half + 1.0;
        let (from, extent) = if sweep < 0.0 { (start + sweep, -sweep) } else { (start, sweep) };
        let extent = extent.min(360.0);
        let ends = [from.to_radians(), (from + extent).to_radians()];
        let caps = [(cx + radius * ends[0].cos(), cy + radius * ends[0].sin()), (cx + radius * ends[1].cos(), cy + radius * ends[1].sin())];
        for row in ((cy - reach).floor() as i32).max(self.clip[1])..((cy + reach).ceil() as i32).min(self.clip[3]) {
            for col in ((cx - reach).floor() as i32).max(self.clip[0])..((cx + reach).ceil() as i32).min(self.clip[2]) {
                let (px, py) = (col as f32 + 0.5, row as f32 + 0.5);
                let (dx, dy) = (px - cx, py - cy);
                let length = (dx * dx + dy * dy).sqrt();
                if (length - radius).abs() > half + 1.0 {
                    continue;
                }
                let turn = (dy.atan2(dx).to_degrees() - from).rem_euclid(360.0);
                let distance = if turn <= extent {
                    (length - radius).abs()
                } else {
                    caps.iter().map(|cap| ((px - cap.0).powi(2) + (py - cap.1).powi(2)).sqrt()).fold(f32::MAX, f32::min)
                };
                let cover = (half + 0.5 - distance).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.dot(col, row, color, (base as f32 * cover) as u32);
                }
            }
        }
    }

    /// A filled shape made of closed outlines, non-zero winding.
    pub fn polygon(&mut self, outlines: &[Vec<(f32, f32)>], color: u32) {
        let base = color >> 24;
        if base == 0 {
            return;
        }
        let mut edges: Vec<[f32; 4]> = Vec::new();
        let mut bounds = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for outline in outlines {
            for (index, point) in outline.iter().enumerate() {
                let next = outline[(index + 1) % outline.len()];
                if point.1 != next.1 {
                    edges.push([point.0, point.1, next.0, next.1]);
                }
                bounds[0] = bounds[0].min(point.0);
                bounds[1] = bounds[1].min(point.1);
                bounds[2] = bounds[2].max(point.0);
                bounds[3] = bounds[3].max(point.1);
            }
        }
        if edges.is_empty() {
            return;
        }
        for row in (bounds[1].floor() as i32).max(self.clip[1])..(bounds[3].ceil() as i32).min(self.clip[3]) {
            for col in (bounds[0].floor() as i32).max(self.clip[0])..(bounds[2].ceil() as i32).min(self.clip[2]) {
                let mut inside = 0u32;
                for sample in 0..16 {
                    let px = col as f32 + (sample % 4) as f32 * 0.25 + 0.125;
                    let py = row as f32 + (sample / 4) as f32 * 0.25 + 0.125;
                    let mut winding = 0i32;
                    for edge in &edges {
                        let rising = edge[1] <= py && edge[3] > py;
                        let falling = edge[3] <= py && edge[1] > py;
                        if !rising && !falling {
                            continue;
                        }
                        let at = edge[0] + (py - edge[1]) / (edge[3] - edge[1]) * (edge[2] - edge[0]);
                        if at > px {
                            winding += if rising { 1 } else { -1 };
                        }
                    }
                    if winding != 0 {
                        inside += 1;
                    }
                }
                if inside > 0 {
                    self.dot(col, row, color, base * inside / 16);
                }
            }
        }
    }

    /// Draw a vector drawable into a square. A tint replaces the drawable's own
    /// colours, the way the Android layer tints its toolbar icons.
    pub fn glyph(&mut self, glyph: &Glyph, x: f32, y: f32, size: f32, tint: Option<u32>) {
        let scale = size / glyph.viewport.max(1.0);
        for part in &glyph.parts {
            if part.fill >> 24 != 0 {
                let outlines: Vec<Vec<(f32, f32)>> = part
                    .lines
                    .iter()
                    .map(|line| line.iter().map(|point| (x + point.0 * scale, y + point.1 * scale)).collect())
                    .collect();
                self.polygon(&outlines, tint.unwrap_or(part.fill));
            }
            if part.stroke > 0.0 && (part.stroke_color >> 24 != 0 || tint.is_some()) {
                let mut segments = Vec::new();
                for line in &part.lines {
                    for pair in line.windows(2) {
                        segments.push([x + pair[0].0 * scale, y + pair[0].1 * scale, x + pair[1].0 * scale, y + pair[1].1 * scale]);
                    }
                }
                self.strokes(&segments, part.stroke * scale, tint.unwrap_or(part.stroke_color));
            }
        }
    }
}

pub struct Part {
    pub lines: Vec<Vec<(f32, f32)>>,
    pub stroke: f32,
    pub stroke_color: u32,
    pub fill: u32,
}

pub struct Glyph {
    pub viewport: f32,
    pub parts: Vec<Part>,
}

fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("android:{name}=\"");
    let start = element.find(&marker)? + marker.len();
    let end = element[start..].find('"')? + start;
    Some(&element[start..end])
}

pub fn color(raw: &str) -> u32 {
    let hex = raw.trim().trim_start_matches('#');
    if !raw.trim().starts_with('#') {
        return 0;
    }
    match (hex.len(), u32::from_str_radix(hex, 16)) {
        (8, Ok(value)) => value,
        (6, Ok(value)) => 0xFF00_0000 | value,
        _ => 0,
    }
}

/// Read an Android vector drawable: the same file the Android build packages.
pub fn glyph(document: &str) -> Glyph {
    let viewport = attribute(document, "viewportWidth").and_then(|value| value.parse().ok()).unwrap_or(24.0);
    let mut parts = Vec::new();
    for element in document.split("<path").skip(1) {
        let element = element.split("/>").next().unwrap_or(element);
        let Some(data) = attribute(element, "pathData") else { continue };
        parts.push(Part {
            lines: outline(data),
            stroke: attribute(element, "strokeWidth").and_then(|value| value.parse().ok()).unwrap_or(0.0),
            stroke_color: attribute(element, "strokeColor").map(color).unwrap_or(0),
            fill: attribute(element, "fillColor").map(color).unwrap_or(0),
        });
    }
    Glyph { viewport, parts }
}

fn arc_points(from: (f32, f32), radii: (f32, f32), rotation: f32, large: bool, sweep: bool, to: (f32, f32), out: &mut Vec<(f32, f32)>) {
    let (mut rx, mut ry) = (radii.0.abs(), radii.1.abs());
    if rx == 0.0 || ry == 0.0 || from == to {
        out.push(to);
        return;
    }
    let (sin, cos) = rotation.to_radians().sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let stretch = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if stretch > 1.0 {
        rx *= stretch.sqrt();
        ry *= stretch.sqrt();
    }
    let below = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let above = rx * rx * ry * ry - below;
    let mut factor = if below == 0.0 { 0.0 } else { (above / below).max(0.0).sqrt() };
    if large == sweep {
        factor = -factor;
    }
    let (cxp, cyp) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
    let (cx, cy) = (cos * cxp - sin * cyp + (from.0 + to.0) / 2.0, sin * cxp + cos * cyp + (from.1 + to.1) / 2.0);
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let length = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let turn = if length == 0.0 { 0.0 } else { ((ux * vx + uy * vy) / length).clamp(-1.0, 1.0).acos() };
        if ux * vy - uy * vx < 0.0 { -turn } else { turn }
    };
    let (ux, uy) = ((x1 - cxp) / rx, (y1 - cyp) / ry);
    let theta = angle(1.0, 0.0, ux, uy);
    let mut delta = angle(ux, uy, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && delta > 0.0 {
        delta -= std::f32::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f32::consts::TAU;
    }
    let steps = ((delta.abs() / (std::f32::consts::PI / 18.0)).ceil() as usize).max(1);
    for step in 1..=steps {
        let (s, c) = (theta + delta * step as f32 / steps as f32).sin_cos();
        out.push((cx + rx * c * cos - ry * s * sin, cy + rx * c * sin + ry * s * cos));
    }
    if let Some(last) = out.last_mut() {
        *last = to;
    }
}

/// Flatten path data (M L H V C Q A Z, absolute or relative) into polylines.
pub fn outline(data: &str) -> Vec<Vec<(f32, f32)>> {
    let mut commands: Vec<(char, Vec<f32>)> = Vec::new();
    let mut word = String::new();
    fn close(word: &mut String, commands: &mut Vec<(char, Vec<f32>)>) {
        if word.is_empty() {
            return;
        }
        if let (Ok(value), Some(last)) = (word.parse::<f32>(), commands.last_mut()) {
            last.1.push(value);
        }
        word.clear();
    }
    for ch in data.chars() {
        if ch.is_ascii_alphabetic() && ch != 'e' && ch != 'E' {
            close(&mut word, &mut commands);
            commands.push((ch, Vec::new()));
        } else if ch == ',' || ch.is_whitespace() {
            close(&mut word, &mut commands);
        } else if ch == '-' && !word.is_empty() && !word.ends_with('e') && !word.ends_with('E') {
            close(&mut word, &mut commands);
            word.push(ch);
        } else {
            word.push(ch);
        }
    }
    close(&mut word, &mut commands);
    let mut lines: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut at = (0.0f32, 0.0f32);
    let mut start = at;
    for (command, values) in commands {
        let relative = command.is_ascii_lowercase();
        let origin = |at: (f32, f32)| if relative { at } else { (0.0, 0.0) };
        match command.to_ascii_uppercase() {
            'M' => {
                for (index, pair) in values.chunks_exact(2).enumerate() {
                    let base = origin(at);
                    at = (base.0 + pair[0], base.1 + pair[1]);
                    if index == 0 {
                        start = at;
                        lines.push(vec![at]);
                    } else if let Some(line) = lines.last_mut() {
                        line.push(at);
                    }
                }
            }
            'L' => {
                for pair in values.chunks_exact(2) {
                    let base = origin(at);
                    at = (base.0 + pair[0], base.1 + pair[1]);
                    if let Some(line) = lines.last_mut() {
                        line.push(at);
                    }
                }
            }
            'H' | 'V' => {
                for value in values {
                    let base = origin(at);
                    at = if command.to_ascii_uppercase() == 'H' { (base.0 + value, at.1) } else { (at.0, base.1 + value) };
                    if let Some(line) = lines.last_mut() {
                        line.push(at);
                    }
                }
            }
            'C' => {
                for group in values.chunks_exact(6) {
                    let base = origin(at);
                    let (p1, p2, p3) = ((base.0 + group[0], base.1 + group[1]), (base.0 + group[2], base.1 + group[3]), (base.0 + group[4], base.1 + group[5]));
                    if let Some(line) = lines.last_mut() {
                        for step in 1..=20 {
                            let t = step as f32 / 20.0;
                            let u = 1.0 - t;
                            line.push((
                                u * u * u * at.0 + 3.0 * u * u * t * p1.0 + 3.0 * u * t * t * p2.0 + t * t * t * p3.0,
                                u * u * u * at.1 + 3.0 * u * u * t * p1.1 + 3.0 * u * t * t * p2.1 + t * t * t * p3.1,
                            ));
                        }
                    }
                    at = p3;
                }
            }
            'Q' => {
                for group in values.chunks_exact(4) {
                    let base = origin(at);
                    let (p1, p2) = ((base.0 + group[0], base.1 + group[1]), (base.0 + group[2], base.1 + group[3]));
                    if let Some(line) = lines.last_mut() {
                        for step in 1..=12 {
                            let t = step as f32 / 12.0;
                            let u = 1.0 - t;
                            line.push((u * u * at.0 + 2.0 * u * t * p1.0 + t * t * p2.0, u * u * at.1 + 2.0 * u * t * p1.1 + t * t * p2.1));
                        }
                    }
                    at = p2;
                }
            }
            'A' => {
                for group in values.chunks_exact(7) {
                    let base = origin(at);
                    let to = (base.0 + group[5], base.1 + group[6]);
                    if let Some(line) = lines.last_mut() {
                        arc_points(at, (group[0], group[1]), group[2], group[3] != 0.0, group[4] != 0.0, to, line);
                    }
                    at = to;
                }
            }
            'Z' => {
                if let Some(line) = lines.last_mut() {
                    line.push(start);
                }
                at = start;
            }
            _ => {}
        }
    }
    lines
}

/// A decoded picture and the centre-crop scaling the wallpaper needs.
pub struct Picture {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

fn blend(a: u32, b: u32, weight: u32) -> u32 {
    let rb = (((a & 0x00FF_00FF) * (256 - weight) + (b & 0x00FF_00FF) * weight) >> 8) & 0x00FF_00FF;
    let g = (((a & 0x0000_FF00) * (256 - weight) + (b & 0x0000_FF00) * weight) >> 8) & 0x0000_FF00;
    rb | g
}

impl Picture {
    pub fn half(&self) -> Picture {
        let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut pixels = Vec::with_capacity(width * height);
        for row in 0..height {
            let upper = (row * 2).min(self.height - 1) * self.width;
            let lower = (row * 2 + 1).min(self.height - 1) * self.width;
            for col in 0..width {
                let (left, right) = ((col * 2).min(self.width - 1), (col * 2 + 1).min(self.width - 1));
                let top = blend(self.pixels[upper + left], self.pixels[upper + right], 128);
                let bottom = blend(self.pixels[lower + left], self.pixels[lower + right], 128);
                pixels.push(blend(top, bottom, 128));
            }
        }
        Picture { width, height, pixels }
    }

    /// Fill the target, keeping the picture's proportions and cropping what
    /// does not fit: the same rule as the Android wallpaper view.
    pub fn cover(&self, target: &mut [u32], width: usize, height: usize) {
        if width == 0 || height == 0 || self.width == 0 || self.height == 0 || target.len() < width * height {
            return;
        }
        let scale = (width as f32 / self.width as f32).max(height as f32 / self.height as f32);
        let left = (self.width as f32 - width as f32 / scale) / 2.0;
        let top = (self.height as f32 - height as f32 / scale) / 2.0;
        let columns: Vec<(usize, usize, u32)> = (0..width)
            .map(|col| {
                let at = ((col as f32 + 0.5) / scale + left - 0.5).clamp(0.0, (self.width - 1) as f32);
                let first = at.floor() as usize;
                (first, (first + 1).min(self.width - 1), ((at - first as f32) * 256.0) as u32)
            })
            .collect();
        for row in 0..height {
            let at = ((row as f32 + 0.5) / scale + top - 0.5).clamp(0.0, (self.height - 1) as f32);
            let first = at.floor() as usize;
            let second = (first + 1).min(self.height - 1);
            let weight = ((at - first as f32) * 256.0) as u32;
            let (upper, lower) = (first * self.width, second * self.width);
            let line = &mut target[row * width..(row + 1) * width];
            for (pixel, (a, b, mix)) in line.iter_mut().zip(&columns) {
                let above = blend(self.pixels[upper + a], self.pixels[upper + b], *mix);
                let below = blend(self.pixels[lower + a], self.pixels[lower + b], *mix);
                *pixel = blend(above, below, weight);
            }
        }
    }
}
