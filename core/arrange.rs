//! Desktop layout. The core describes what is on screen; this file arranges it
//! for a resizable window and returns one list of things to draw, the areas
//! that react to the pointer, and the text boxes. It holds no window calls, so
//! any desktop window that can measure text and fill pixels can present it.
use super::raster::{color, flatten, with_alpha};
use crate::{boolean, get, number, text, Arr, Json, Null};

pub const BODY: usize = 0;
pub const SMALL: usize = 1;
pub const LABEL: usize = 2;
pub const HEAD: usize = 3;
pub const TITLE: usize = 4;
pub const DIALOG: usize = 5;
pub const VALUE: usize = 6;
pub const BUTTON: usize = 7;
pub const FONTS: usize = 8;
/// Pixel size at 96 dpi and weight of each font, in the order above.
pub const FACES: [(i32, i32); FONTS] = [(15, 400), (12, 400), (12, 700), (16, 700), (24, 700), (20, 700), (23, 700), (14, 600)];

pub const CENTER: u8 = 1;
pub const WRAP: u8 = 2;
pub const RIGHT: u8 = 4;
pub const SPACED: u8 = 8;

pub trait Ruler {
    /// Height of the text wrapped to a width.
    fn height(&mut self, font: usize, value: &str, width: i32) -> i32;
    /// Width of the text on one line.
    fn width(&mut self, font: usize, value: &str) -> i32;
    fn line(&self, font: usize) -> i32;
}

pub enum Item {
    Fill { area: [i32; 4], radius: i32, color: u32 },
    Edge { area: [i32; 4], radius: i32, color: u32 },
    Text { area: [i32; 4], font: usize, color: u32, value: String, mode: u8 },
    Icon { x: i32, y: i32, size: i32, name: String, color: u32 },
    Clip(Option<[i32; 4]>),
    Hot(usize),
    Line { from: (f32, f32), to: (f32, f32), width: f32, color: u32 },
    Arc { center: (f32, f32), radius: f32, start: f32, sweep: f32, width: f32, color: u32 },
    Disc { center: (f32, f32), radius: f32, color: u32 },
    Mark { x: f32, y: f32, size: f32, color: u32, align: u8, value: String },
    Busy { area: [i32; 4], color: u32 },
}

pub struct Hit {
    pub area: [i32; 4],
    pub action: String,
    pub radius: i32,
    pub clip: Option<[i32; 4]>,
    pub wash: bool,
}

#[derive(Clone)]
pub struct FieldBox {
    pub key: String,
    pub area: [i32; 4],
    pub value: String,
    pub hint: String,
    pub secret: bool,
    pub multiline: bool,
}

#[derive(Default)]
pub struct Scene {
    pub items: Vec<Item>,
    pub hits: Vec<Hit>,
    pub fields: Vec<FieldBox>,
    pub draft: Option<[i32; 4]>,
    pub view: Option<[i32; 4]>,
    pub content: i32,
    pub scroll: i32,
    pub modal_span: i32,
    pub page_size: usize,
    pub primary: String,
}

pub struct View<'a> {
    pub ui: &'a Json,
    pub profile_form: bool,
    pub width: i32,
    pub height: i32,
    pub dpi: u32,
    pub scroll: i32,
    pub stick: bool,
    pub modal_scroll: i32,
    pub page_at: usize,
    pub draft_lines: i32,
    pub working: bool,
    pub menu: bool,
    pub note: &'a str,
}

#[derive(Clone, Copy)]
pub struct Pal {
    pub ink: u32,
    pub surface: u32,
    pub card: u32,
    pub muted: u32,
    pub accent: u32,
    pub glass: u32,
    pub border: u32,
    pub veil: u32,
    pub violet: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    pub field: u32,
    pub raised: u32,
    pub dim: u32,
    pub dark: bool,
}

impl Pal {
    pub fn of(ui: &Json) -> Pal {
        let palette = get(ui, "palette");
        let read = |key: &str, fallback: u32| match color(&text(palette, key)) {
            0 => fallback,
            value => value,
        };
        let ink = read("ink", 0xFFF7FAFF);
        let surface = read("surface", 0xFF050910);
        let card = read("card", 0xB31A3242);
        let muted = read("muted", 0xFFA2BCC5);
        let glass = read("glass", 0xBF0F1724);
        let light = |value: u32| ((value >> 16) & 255) * 3 + ((value >> 8) & 255) * 6 + (value & 255);
        let raised = flatten(glass, surface);
        Pal {
            ink,
            surface,
            card,
            muted,
            accent: read("accent", 0xFF4EDFEA),
            glass,
            border: read("border", 0x557AC7D8),
            veil: read("veil", 0x66050910),
            violet: read("violet", 0xFFA78BFA),
            success: read("success", 0xFF34D399),
            warning: read("warning", 0xFFFBBF24),
            error: read("error", 0xFFFB7185),
            field: flatten(card, raised),
            raised,
            dim: flatten(with_alpha(muted, 0x8C), raised),
            dark: light(surface) < light(ink),
        }
    }

    pub fn named(&self, name: &str) -> u32 {
        match name {
            "ink" => self.ink,
            "surface" => self.surface,
            "card" => self.card,
            "accent" => self.accent,
            "glass" => self.glass,
            "border" => self.border,
            "violet" => self.violet,
            "success" => self.success,
            "warning" => self.warning,
            "error" => self.error,
            _ => self.muted,
        }
    }
}

pub fn rows(value: &Json, key: &str) -> Vec<Json> {
    match get(value, key) {
        Arr(values) => values.clone(),
        _ => Vec::new(),
    }
}

fn real(value: &Json, key: &str) -> f32 {
    match get(value, key) {
        crate::Num(raw) | crate::Str(raw) => raw.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Style {
    Tonal,
    Primary,
    Ghost,
    Quiet,
}

struct Pen<'a> {
    items: Vec<Item>,
    hits: Vec<Hit>,
    fields: Vec<FieldBox>,
    ruler: &'a mut dyn Ruler,
    pal: Pal,
    dpi: u32,
    clip: Option<[i32; 4]>,
    dry: bool,
}

impl Pen<'_> {
    fn d(&self, dp: i32) -> i32 {
        dp * self.dpi as i32 / 96
    }

    fn fill(&mut self, area: [i32; 4], radius: i32, color: u32) {
        if !self.dry && color >> 24 != 0 {
            let radius = self.d(radius);
            self.items.push(Item::Fill { area, radius, color });
        }
    }

    fn edge(&mut self, area: [i32; 4], radius: i32, color: u32) {
        if !self.dry {
            let radius = self.d(radius);
            self.items.push(Item::Edge { area, radius, color });
        }
    }

    fn panel(&mut self, area: [i32; 4], radius: i32, color: u32) {
        self.fill(area, radius, color);
        let border = self.pal.border;
        self.edge(area, radius, border);
    }

    fn text(&mut self, area: [i32; 4], font: usize, color: u32, value: &str, mode: u8) {
        if !self.dry && !value.is_empty() && area[2] > 0 && area[3] > 0 {
            self.items.push(Item::Text { area, font, color, value: value.to_string(), mode });
        }
    }

    fn icon(&mut self, x: i32, y: i32, size: i32, name: &str, color: u32) {
        if !self.dry {
            self.items.push(Item::Icon { x, y, size, name: name.to_string(), color });
        }
    }

    fn clip(&mut self, area: Option<[i32; 4]>) {
        self.clip = area;
        if !self.dry {
            self.items.push(Item::Clip(area));
        }
    }

    fn visible(&self, area: [i32; 4]) -> bool {
        match self.clip {
            Some(clip) => area[1] < clip[3] && area[1] + area[3] > clip[1],
            None => true,
        }
    }

    fn hit(&mut self, area: [i32; 4], action: &str, radius: i32, wash: bool) {
        if self.dry || !self.visible(area) {
            return;
        }
        let radius = self.d(radius);
        self.hits.push(Hit { area, action: action.to_string(), radius, clip: self.clip, wash });
        if wash {
            self.items.push(Item::Hot(self.hits.len() - 1));
        }
    }

    fn button(&mut self, area: [i32; 4], label: &str, action: &str, style: Style, enabled: bool) {
        self.control(area, label, action, style, enabled, 10);
    }

    fn control(&mut self, area: [i32; 4], label: &str, action: &str, style: Style, enabled: bool, radius: i32) {
        let pal = self.pal;
        let (fill, ink) = match style {
            Style::Primary => (pal.accent, pal.surface),
            Style::Tonal => (pal.card, pal.accent),
            Style::Ghost => (0, pal.accent),
            Style::Quiet => (0, pal.muted),
        };
        self.fill(area, radius, fill);
        if style == Style::Tonal {
            self.edge(area, radius, pal.border);
        }
        if enabled {
            self.hit(area, action, radius, true);
        }
        let pad = self.d(10);
        self.text([area[0] + pad, area[1], area[2] - pad * 2, area[3]], BUTTON, if enabled { ink } else { pal.dim }, label, CENTER);
    }

    fn glyph_button(&mut self, x: i32, y: i32, side: i32, name: &str, action: &str, color: u32) {
        let icon = self.d(22);
        self.hit([x, y, side, side], action, 22, true);
        self.icon(x + (side - icon) / 2, y + (side - icon) / 2, icon, name, color);
    }

    fn fit(&mut self, font: usize, value: &str, pad: i32, least: i32, most: i32) -> i32 {
        (self.ruler.width(font, value) + pad).clamp(least, most.max(least))
    }
}

fn caption(label: &str, fallback: &str) -> String {
    if label.is_empty() { fallback.to_string() } else { label.to_string() }
}

fn heading(ui: &Json) -> String {
    let page = text(ui, "page");
    if page == "chat" {
        return caption(&text(ui, "connection_label"), "RLLM");
    }
    let mut letters = page.chars();
    match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => "RLLM".into(),
    }
}

/// The geometry the monitor computed, scaled to a width. Returns the height.
fn diagram(pen: &mut Pen, visual: &Json, x: i32, y: i32, width: i32) -> i32 {
    let span = real(visual, "width").max(1.0);
    let scale = width as f32 / span;
    let height = (real(visual, "height") * scale).round() as i32;
    if pen.dry || !pen.visible([x, y, width, height]) {
        return height;
    }
    let pal = pen.pal;
    for shape in rows(visual, "shapes").iter().take(2048) {
        let tone = pal.named(&text(shape, "color"));
        let at = |key: &str, origin: i32| origin as f32 + real(shape, key) * scale;
        let (x1, y1, x2, y2) = (at("x", x), at("y", y), at("x2", x), at("y2", y));
        let stroke = if get(shape, "width") == &Null { 2.0 } else { real(shape, "width") };
        let weight = (stroke * scale).max(1.0);
        match text(shape, "kind").as_str() {
            "line" => pen.items.push(Item::Line { from: (x1, y1), to: (x2, y2), width: weight, color: tone }),
            "rect" => {
                let area = [x1.round() as i32, y1.round() as i32, (x2 - x1).round().max(1.0) as i32, (y2 - y1).round().max(1.0) as i32];
                pen.items.push(Item::Fill { area, radius: (10.0 * scale).round() as i32, color: tone });
                let event = text(shape, "event");
                if !event.is_empty() {
                    pen.hit(area, &event, 0, true);
                }
            }
            "circle" => pen.items.push(Item::Disc { center: (x1, y1), radius: real(shape, "radius") * scale, color: tone }),
            "arc" => pen.items.push(Item::Arc {
                center: ((x1 + x2) / 2.0, (y1 + y2) / 2.0),
                radius: (x2 - x1) / 2.0,
                start: real(shape, "start"),
                sweep: real(shape, "sweep"),
                width: weight,
                color: tone,
            }),
            "text" => {
                let size = if get(shape, "size") == &Null { 30.0 } else { real(shape, "size") };
                let align = match text(shape, "align").as_str() {
                    "center" => CENTER,
                    "right" => RIGHT,
                    _ => 0,
                };
                pen.items.push(Item::Mark { x: x1, y: y1, size: (size * scale).max(9.0), color: tone, align, value: text(shape, "value") });
            }
            _ => {}
        }
    }
    height
}

/// One monitor card. Returns its height; draws nothing while measuring.
fn monitor_card(pen: &mut Pen, card: &Json, x: i32, y: i32, width: i32, height: i32) -> i32 {
    let pal = pen.pal;
    let pad = pen.d(14);
    let inner = width - pad * 2;
    if height > 0 {
        pen.panel([x, y, width, height], 12, pal.glass);
    }
    let mut cursor = y + pad;
    let line = pen.ruler.line(LABEL);
    pen.text([x + pad, cursor, inner, line], LABEL, pal.ink, &text(card, "title"), SPACED);
    cursor += line + pen.d(4);
    if get(card, "value") != &Null {
        let tall = pen.ruler.line(VALUE);
        pen.text([x + pad, cursor + pen.d(2), inner, tall], VALUE, pal.accent, &text(card, "value"), 0);
        cursor += tall + pen.d(6);
    }
    let visual = get(card, "visual");
    let note = text(card, "caption");
    if text(card, "kind") == "gauge" {
        let dial = if visual == &Null { 0 } else { pen.d(124).min(inner / 2) };
        let words = inner - dial - pen.d(8);
        let tall = pen.ruler.height(SMALL, &note, words);
        pen.text([x + pad, cursor + pen.d(4), words, tall], SMALL, pal.muted, &note, WRAP);
        let drawn = if dial > 0 { diagram(pen, visual, x + width - pad - dial, cursor, dial) } else { 0 };
        cursor += (tall + pen.d(4)).max(drawn);
        return cursor + pad - y;
    }
    if visual != &Null {
        cursor += diagram(pen, visual, x + pad, cursor, inner) + pen.d(4);
    }
    for entry in rows(card, "legend") {
        let tall = pen.ruler.line(SMALL) + pen.d(2);
        let tone = pal.named(&text(&entry, "color"));
        pen.text([x + pad, cursor, inner, tall], SMALL, tone, &format!("●  {}", text(&entry, "label")), 0);
        cursor += tall;
    }
    let tall = pen.ruler.height(SMALL, &note, inner);
    pen.text([x + pad, cursor + pen.d(4), inner, tall], SMALL, pal.muted, &note, WRAP);
    cursor + pen.d(4) + tall + pad - y
}

/// Cards in equal columns; every row is as tall as its tallest card.
fn card_grid(pen: &mut Pen, cards: &[Json], x: i32, mut y: i32, width: i32, columns: usize) -> i32 {
    let gap = pen.d(10);
    let columns = columns.max(1);
    let each = (width - gap * (columns as i32 - 1)) / columns as i32;
    for group in cards.chunks(columns) {
        let was = pen.dry;
        pen.dry = true;
        let tall = group.iter().map(|card| monitor_card(pen, card, x, y, each, 0)).max().unwrap_or(0);
        pen.dry = was;
        for (index, card) in group.iter().enumerate() {
            monitor_card(pen, card, x + index as i32 * (each + gap), y, each, tall);
        }
        y += tall + gap;
    }
    y
}

fn monitor(pen: &mut Pen, model: &Json, area: [i32; 4], scroll: i32) -> i32 {
    let pal = pen.pal;
    let [x, top, width, _] = area;
    let mut y = top + pen.d(6) - scroll;
    let line = pen.ruler.line(SMALL);
    pen.text([x, y, width, line + pen.d(6)], SMALL, pal.muted, &text(model, "subtitle"), 0);
    y += line + pen.d(12);
    let tall = pen.d(38);
    let label = caption(&text(model, "range_label"), "Average over…");
    let wide = pen.fit(BUTTON, &label, pen.d(32), pen.d(120), width / 2);
    pen.button([x, y, wide, tall], &label, "monitor:range", Style::Tonal, true);
    let chosen = match number(model, "window") {
        0 => 10,
        value => value,
    };
    let mut left = x + wide + pen.d(12);
    for value in [5i64, 10, 25, 50] {
        let side = pen.d(52);
        pen.button([left, y, side, tall], &value.to_string(), &format!("monitor:window:{value}"), if value == chosen { Style::Primary } else { Style::Tonal }, true);
        left += side + pen.d(6);
    }
    y += tall + pen.d(12);
    let live = get(model, "live");
    if live != &Null {
        y = card_grid(pen, &[live.clone()], x, y, width, 1);
    }
    let gauges = rows(model, "gauges");
    let columns = if width >= pen.d(1040) { 4 } else if width >= pen.d(560) { 2 } else { 1 };
    y = card_grid(pen, &gauges, x, y, width, columns);
    let cards = rows(model, "cards");
    y = card_grid(pen, &cards, x, y, width, if width >= pen.d(900) { 2 } else { 1 });
    let mut left = x;
    for action in rows(model, "actions") {
        let label = text(&action, "label");
        let wide = pen.fit(BUTTON, &label, pen.d(32), pen.d(120), width);
        if left + wide > x + width {
            left = x;
            y += tall + pen.d(8);
        }
        pen.button([left, y, wide, tall], &label, &text(&action, "event"), Style::Tonal, true);
        left += wide + pen.d(8);
    }
    y + tall + pen.d(12) + scroll - top
}

fn thumb(pen: &mut Pen, view: [i32; 4], total: i32, scroll: i32) {
    if total <= view[3] || view[3] <= 0 {
        return;
    }
    let pal = pen.pal;
    let tall = (view[3] as i64 * view[3] as i64 / total as i64).max(pen.d(28) as i64) as i32;
    let travel = (view[3] - tall).max(0);
    let offset = (scroll as i64 * travel as i64 / (total - view[3]).max(1) as i64) as i32;
    pen.fill([view[0] + view[2] - pen.d(4), view[1] + offset, pen.d(4), tall], 2, with_alpha(pal.muted, 0x70));
}

/// More actions than this in one dialog are laid out side by side.
const MANY: usize = 6;

/// The dialog body. Called once to measure and once to draw.
fn dialog_body(pen: &mut Pen, modal: &Json, buttons: &[Json], x: i32, top: i32, width: i32) -> i32 {
    let pal = pen.pal;
    let mut y = top;
    let small = pen.ruler.line(SMALL);
    let body = pen.ruler.line(BODY);
    for field in rows(modal, "fields") {
        let kind = text(&field, "kind");
        let key = text(&field, "key");
        let label = text(&field, "label");
        let value = text(&field, "value");
        if kind == "check" {
            let tall = pen.ruler.height(BODY, &label, width - pen.d(40)).max(body);
            let row = [x - pen.d(6), y, width + pen.d(12), tall + pen.d(18)];
            pen.hit(row, &format!("check:{key}"), 8, true);
            let side = pen.d(20);
            let mark = [x, y + (row[3] - side) / 2, side, side];
            if value == "true" {
                pen.fill(mark, 5, pal.accent);
                pen.icon(mark[0] + pen.d(3), mark[1] + pen.d(3), side - pen.d(6), "confirmation", pal.surface);
            } else {
                pen.edge(mark, 5, pal.muted);
            }
            pen.text([x + pen.d(32), y + pen.d(9), width - pen.d(40), tall], BODY, pal.ink, &label, WRAP);
            y += row[3] + pen.d(4);
            continue;
        }
        if !label.is_empty() {
            pen.text([x, y, width, small + pen.d(2)], SMALL, pal.muted, &label, 0);
            y += small + pen.d(6);
        }
        if kind == "choice" {
            let (mut left, tall, gap) = (x, pen.d(36), pen.d(8));
            for option in rows(&field, "options") {
                let name = text(&option, "label");
                let wide = pen.fit(BUTTON, &name, pen.d(28), pen.d(56), width);
                if left + wide > x + width && left > x {
                    left = x;
                    y += tall + gap;
                }
                let style = if text(&option, "value") == value { Style::Primary } else { Style::Tonal };
                pen.button([left, y, wide, tall], &name, &format!("choice:{key}:{}", text(&option, "value")), style, true);
                left += wide + gap;
            }
            y += tall + pen.d(14);
        } else if kind == "readonly" {
            let shown = if value.is_empty() { "—".to_string() } else { value };
            let tall = pen.ruler.height(BODY, &shown, width);
            pen.text([x, y, width, tall], BODY, pal.ink, &shown, WRAP);
            y += tall + pen.d(14);
        } else {
            let multiline = kind == "multiline";
            let tall = if multiline { body * 5 + pen.d(16) } else { pen.d(40) };
            let area = [x, y, width, tall];
            pen.fill(area, 8, pal.field);
            pen.edge(area, 8, pal.border);
            let inset = pen.d(10);
            let inner = if multiline { [x + inset, y + pen.d(8), width - inset * 2, tall - pen.d(16)] } else { [x + inset, y + (tall - body) / 2, width - inset * 2, body] };
            let whole = match pen.clip {
                Some(clip) => area[1] >= clip[1] && area[1] + area[3] <= clip[3],
                None => true,
            };
            if !pen.dry && whole {
                let hint = if key == "endpoint" { "Local IP → http://; explicit https:// is kept".to_string() } else { String::new() };
                pen.fields.push(FieldBox { key, area: inner, value, hint, secret: kind == "secret", multiline });
            }
            y += tall + pen.d(14);
        }
    }
    let list = text(modal, "presentation") == "list";
    let pills = text(modal, "presentation") == "pills";
    if buttons.is_empty() && y == top {
        // A list with nothing in it says so instead of showing an empty frame.
        pen.text([x, y, width, body], BODY, pal.muted, "Nothing here yet.", 0);
        return body + pen.d(8);
    }
    // A long run of actions sits in two columns on a wide window, so the whole
    // menu is in view at once instead of behind a scroll.
    let paired = !list && buttons.len() > MANY && width >= pen.d(520);
    if paired {
        let (gap, tall) = (pen.d(10), pen.d(44));
        let radius = if pills { 22 } else { 10 };
        let half = (width - gap) / 2;
        for (index, button) in buttons.iter().enumerate() {
            let enabled = get(button, "enabled") == &Null || boolean(button, "enabled");
            let (column, row) = ((index % 2) as i32, (index / 2) as i32);
            let area = [x + column * (half + gap), y + row * (tall + gap), half, tall];
            pen.control(area, &text(button, "label"), &text(button, "event"), Style::Tonal, enabled, radius);
        }
        let lines = ((buttons.len() + 1) / 2) as i32;
        return y + lines * (tall + gap) - top;
    }
    for (index, button) in buttons.iter().enumerate() {
        let enabled = get(button, "enabled") == &Null || boolean(button, "enabled");
        let label = text(button, "label");
        let event = text(button, "event");
        if list {
            // A long entry wraps onto more lines rather than being cut short.
            let words = pen.ruler.height(BODY, &label, width - pen.d(8)).max(body);
            let tall = (words + pen.d(22)).max(pen.d(46));
            if index > 0 {
                pen.fill([x, y, width, 1], 0, pal.border);
            }
            let row = [x - pen.d(8), y + 1, width + pen.d(16), tall - 1];
            if enabled {
                pen.hit(row, &event, 8, true);
            }
            pen.text([x + pen.d(4), y + (tall - words) / 2, width - pen.d(8), words], BODY, if enabled { pal.ink } else { pal.dim }, &label, WRAP);
            y += tall;
        } else {
            let tall = pen.d(42);
            let radius = if pills { 21 } else { 10 };
            pen.control([x, y, width, tall], &label, &event, Style::Tonal, enabled, radius);
            y += tall + pen.d(8);
        }
    }
    y - top
}

fn dialog(pen: &mut Pen, view: &View, scene: &mut Scene) {
    let pal = pen.pal;
    let modal = get(view.ui, "modal");
    let (w, h) = (view.width, view.height);
    pen.fill([0, 0, w, h], 0, with_alpha(pal.surface, if pal.dark { 0xA6 } else { 0x80 }));
    pen.hit([0, 0, w, h], "close", 0, false);
    let buttons: Vec<Json> = rows(modal, "buttons").into_iter().filter(|button| !(view.profile_form && text(button, "event") == "profile_save")).collect();
    let fields = rows(modal, "fields");
    let entry = fields.iter().any(|field| !matches!(text(field, "kind").as_str(), "readonly"));
    let list = text(modal, "presentation") == "list";
    let wide = (w - pen.d(48)).min(pen.d(if list { 460 } else if buttons.len() > MANY { 760 } else if entry { 640 } else { 520 })).max(pen.d(240));
    let pad = pen.d(24);
    let inner = wide - pad * 2;
    let (head, foot) = (pen.d(68), pen.d(64));
    pen.dry = true;
    let total = dialog_body(pen, modal, &buttons, 0, 0, inner) + pen.d(4);
    pen.dry = false;
    let room = (h - pen.d(48) - head - foot).max(pen.d(80));
    let body = total.min(room).max(pen.d(24));
    let tall = head + body + foot;
    let (left, top) = ((w - wide) / 2, (h - tall) / 2);
    let frame = [left, top, wide, tall];
    pen.fill(frame, 20, pal.raised);
    pen.edge(frame, 20, pal.border);
    pen.hit(frame, "", 20, false);
    pen.text([left + pad, top, inner, head], DIALOG, pal.ink, &text(modal, "title"), 0);
    let span = (total - body).max(0);
    let scroll = view.modal_scroll.clamp(0, span);
    scene.modal_span = span;
    let window = [left, top + head, left + wide, top + head + body];
    pen.clip(Some(window));
    dialog_body(pen, modal, &buttons, left + pad, top + head - scroll, inner);
    pen.clip(None);
    if span > 0 {
        pen.fill([left, top + head - 1, wide, 1], 0, pal.border);
        pen.fill([left, top + head + body, wide, 1], 0, pal.border);
        thumb(pen, [left, top + head, wide - pen.d(6), body], total, scroll);
    }
    let (high, base) = (pen.d(40), top + tall - pen.d(52));
    let mut right = left + wide - pad;
    if view.profile_form {
        let size = pen.d(104);
        pen.button([right - size, base, size, high], "Save", "profile_save", Style::Primary, true);
        right -= size + pen.d(8);
        scene.primary = "profile_save".into();
    }
    let close = caption(&text(modal, "close_label"), "Close");
    let size = pen.fit(BUTTON, &close, pen.d(36), pen.d(96), inner / 2);
    pen.button([right - size, base, size, high], &close, "close", Style::Ghost, true);
}

fn chat(pen: &mut Pen, view: &View, scene: &mut Scene, panel: [i32; 4], column: [i32; 2]) {
    let pal = pen.pal;
    let ui = view.ui;
    let (cx, cw) = (column[0], column[1]);
    let busy = boolean(ui, "busy") || view.working;
    let line = pen.ruler.line(BODY);
    let lines = view.draft_lines.clamp(1, 6);
    let tall = (lines * line + pen.d(26)).max(pen.d(56));
    let composer = [cx, panel[1] + panel[3] - pen.d(16) - tall, cw, tall];
    pen.fill(composer, 14, pal.field);
    pen.edge(composer, 14, pal.border);
    let side = pen.d(44);
    pen.glyph_button(cx + pen.d(6), composer[1] + tall - side - pen.d(6), side, "addition", "menu:open", pal.accent);
    let send = [cx + cw - side - pen.d(6), composer[1] + tall - side - pen.d(6), side, side];
    pen.fill(send, 12, pal.accent);
    if !boolean(ui, "capture_active") {
        pen.hit(send, if busy { "cancel" } else { "send" }, 12, true);
    }
    let mark = pen.d(22);
    pen.icon(send[0] + (side - mark) / 2, send[1] + (side - mark) / 2, mark, if busy { "cancellation" } else { "arrow_up" }, pal.surface);
    scene.draft = Some([cx + side + pen.d(14), composer[1] + (tall - lines * line) / 2, cw - side * 2 - pen.d(30), lines * line]);
    let mut floor = composer[1] - pen.d(4);

    for (index, item) in rows(ui, "draft_attachments").iter().enumerate().rev() {
        let high = pen.d(36);
        floor -= high;
        let name = format!("{} · {}×{}", text(item, "name"), number(item, "width"), number(item, "height"));
        pen.text([cx + pen.d(12), floor, cw - pen.d(120), high], SMALL, pal.ink, &name, 0);
        pen.button([cx + cw - pen.d(96), floor + pen.d(2), pen.d(92), high - pen.d(4)], "Remove", &format!("attachment_remove#index={index}"), Style::Ghost, !busy);
    }
    let voice = get(ui, "voice_status");
    if voice != &Null {
        let high = pen.d(40);
        floor -= high;
        let mut right = cx + cw;
        pen.button([right - pen.d(104), floor + pen.d(2), pen.d(104), high - pen.d(4)], "Stop voice", "voice_stop", Style::Tonal, true);
        right -= pen.d(112);
        if text(voice, "phase") == "paused" {
            pen.button([right - pen.d(160), floor + pen.d(2), pen.d(160), high - pen.d(4)], "Resume microphone", "voice_listen", Style::Tonal, boolean(voice, "resume_available"));
            right -= pen.d(168);
        }
        pen.text([cx + pen.d(12), floor, right - cx - pen.d(12), high], BODY, pal.ink, &format!("Voice: {}", text(voice, "label")), 0);
    }

    let high = pen.d(40);
    floor -= high;
    let model = if text(ui, "connection_label").is_empty() { "Choose LLM  ▾".to_string() } else { format!("{}  ▾", text(ui, "connection_label")) };
    let agent = format!("{}  ▾", caption(&text(ui, "agent_name"), "General"));
    let mode = format!("{}  ▾", caption(&text(ui, "permission_label"), "Ask"));
    let rate = caption(&text(get(ui, "local_rate"), "label"), "— tok/s");
    let rate_wide = pen.fit(BUTTON, &rate, pen.d(24), pen.d(72), cw / 3);
    let mut left = cx + pen.d(4);
    let limit = cx + cw - rate_wide - pen.d(12);
    for (label, action, enabled) in [(model, "chat_models", true), (agent, "chat_agents", !busy), (mode, "permission_modes", !busy)] {
        let wide = pen.fit(BUTTON, &label, pen.d(24), pen.d(72), (limit - left).max(pen.d(72)));
        if left + wide > limit {
            break;
        }
        pen.button([left, floor + pen.d(4), wide, high - pen.d(8)], &label, action, Style::Ghost, enabled);
        left += wide + pen.d(6);
    }
    pen.button([cx + cw - rate_wide - pen.d(4), floor + pen.d(4), rate_wide, high - pen.d(8)], &rate, "monitor:all", Style::Quiet, true);

    let status = text(ui, "visible_status");
    if !status.is_empty() {
        let high = pen.ruler.height(SMALL, &status, cw - pen.d(24)).min(pen.ruler.line(SMALL) * 2) + pen.d(12);
        floor -= high;
        pen.hit([cx, floor, cw, high], "status", 8, true);
        pen.text([cx + pen.d(12), floor + pen.d(6), cw - pen.d(24), high - pen.d(12)], SMALL, pal.muted, &status, WRAP);
    }

    let mut top = panel[1] + pen.d(12);
    if panel[3] >= pen.d(420) {
        pen.text([panel[0] + pen.d(22), top, panel[2] - pen.d(44), pen.d(28)], LABEL, pal.ink, "CONVERSATION", SPACED);
        top += pen.d(34);
    }
    let window = [cx, top, cw, (floor - top - pen.d(4)).max(pen.d(40))];
    scene.view = Some(window);
    let messages = rows(ui, "messages");
    if messages.is_empty() {
        let connected = boolean(ui, "connected_profile");
        let help = if connected { text(ui, "connection_label") } else { "Connect any LLM with its URL, model name and optional API key. Your conversations and tools stay on this computer.".to_string() };
        let wide = cw.min(pen.d(580));
        let head = pen.ruler.line(TITLE);
        let words = pen.ruler.height(BODY, &help, wide);
        let block = head + pen.d(10) + words + if connected { 0 } else { pen.d(24) + pen.d(44) };
        let mut y = window[1] + ((window[3] - block) / 2).max(0);
        pen.clip(Some([window[0], window[1], window[0] + window[2], window[1] + window[3]]));
        pen.text([cx, y, cw, head], TITLE, pal.ink, "Start a conversation", CENTER);
        y += head + pen.d(10);
        pen.text([cx + (cw - wide) / 2, y, wide, words], BODY, pal.muted, &help, WRAP | CENTER);
        y += words + pen.d(24);
        if !connected {
            let (each, gap) = (pen.d(190).min((cw - pen.d(12)) / 2), pen.d(12));
            let start = cx + (cw - each * 2 - gap) / 2;
            pen.button([start, y, each, pen.d(44)], "Connect to LLM", "profile_new", Style::Primary, true);
            pen.button([start + each + gap, y, each, pen.d(44)], "Workspace", "workspace", Style::Tonal, true);
        }
        pen.clip(None);
        return;
    }
    let pad = pen.d(14);
    let wide = cw - pen.d(10);
    let inner = wide - pad * 2;
    let role = pen.ruler.line(LABEL);
    let small = pen.ruler.line(SMALL);
    let gap = pen.d(10);
    let mut blocks = Vec::with_capacity(messages.len());
    let mut total = pen.d(6);
    for message in &messages {
        let content = text(message, "content");
        let shown = if content.is_empty() { "…".to_string() } else { content };
        let body = pen.ruler.height(BODY, &shown, inner);
        let extra = rows(message, "attachments").len() as i32 * (small + pen.d(2));
        let block = pen.d(10) + role + pen.d(4) + body + extra + pen.d(12);
        blocks.push((block, body, shown));
        total += block + gap;
    }
    let files = rows(ui, "service_artifacts");
    total += files.len() as i32 * (pen.d(44) + gap);
    let span = (total - window[3]).max(0);
    let scroll = if view.stick { span } else { view.scroll.clamp(0, span) };
    scene.content = span;
    scene.scroll = scroll;
    pen.clip(Some([window[0], window[1], window[0] + window[2], window[1] + window[3]]));
    let mut y = window[1] + pen.d(6) - scroll;
    for (index, message) in messages.iter().enumerate() {
        let (block, body, shown) = &blocks[index];
        let area = [cx, y, wide, *block];
        if pen.visible(area) {
            let user = text(message, "role") == "user";
            if user {
                pen.panel(area, 12, pal.card);
            }
            if !boolean(message, "streaming") {
                pen.hit(area, &format!("branch:{index}"), 12, true);
            }
            let speaker = caption(&text(message, "speaker"), if user { "You" } else { "Assistant" });
            pen.text([cx + pad, y + pen.d(10), inner, role], LABEL, if user { pal.muted } else { pal.accent }, &speaker, 0);
            pen.text([cx + pad, y + pen.d(14) + role, inner, *body], BODY, pal.ink, shown, WRAP);
            let mut below = y + pen.d(14) + role + body;
            for image in rows(message, "attachments") {
                let name = format!("Image: {} · {}×{}", text(&image, "name"), number(&image, "width"), number(&image, "height"));
                pen.text([cx + pad, below, inner, small + pen.d(2)], SMALL, pal.muted, &name, 0);
                below += small + pen.d(2);
            }
        }
        y += block + gap;
    }
    for (index, file) in files.iter().enumerate() {
        let name = format!("File: {} · {} · {} bytes", text(file, "name"), text(file, "kind"), number(file, "bytes"));
        pen.button([cx, y, wide, pen.d(44)], &name, &format!("media_artifact:{index}"), Style::Tonal, !busy);
        y += pen.d(44) + gap;
    }
    pen.clip(None);
    thumb(pen, window, total, scroll);
    if scroll < span - pen.d(4) {
        let area = [cx + wide - pen.d(104), window[1] + window[3] - pen.d(48), pen.d(96), pen.d(38)];
        pen.fill(area, 10, pal.raised);
        pen.button(area, "Latest  ↓", "latest", Style::Tonal, true);
    }
}

fn history(pen: &mut Pen, view: &View, scene: &mut Scene, area: [i32; 4]) {
    let pal = pen.pal;
    let ui = view.ui;
    let busy = boolean(ui, "busy") || view.working;
    let [cx, top, cw, tall] = area;
    let entries = rows(ui, "rows");
    let (mut left, mut y, high) = (cx, top, pen.d(40));
    for entry in entries.iter().filter(|entry| text(entry, "kind") != "conversation") {
        let label = text(entry, "label");
        let wide = pen.fit(BUTTON, &label, pen.d(36), pen.d(120), cw);
        if left + wide > cx + cw && left > cx {
            left = cx;
            y += high + pen.d(8);
        }
        let event = text(entry, "event");
        pen.button([left, y, wide, high], &label, &event, if event == "new" { Style::Primary } else { Style::Tonal }, !busy);
        left += wide + pen.d(8);
    }
    y += high + pen.d(14);
    let window = [cx, y, cw, (top + tall - y).max(pen.d(40))];
    scene.view = Some(window);
    let cards: Vec<&Json> = entries.iter().filter(|entry| text(entry, "kind") == "conversation").collect();
    if cards.is_empty() {
        pen.text([cx, y + pen.d(8), cw, pen.d(48)], BODY, pal.muted, "No conversations. Start a new conversation or restore one from Trash.", WRAP);
        return;
    }
    let (block, gap) = (pen.d(92), pen.d(10));
    let total = cards.len() as i32 * (block + gap);
    let span = (total - window[3]).max(0);
    let scroll = view.scroll.clamp(0, span);
    scene.content = span;
    scene.scroll = scroll;
    pen.clip(Some([window[0], window[1], window[0] + window[2], window[1] + window[3]]));
    let wide = cw - pen.d(10);
    let mut at = y - scroll;
    for entry in cards {
        let card = [cx, at, wide, block];
        if pen.visible(card) {
            pen.fill(card, 12, pal.card);
            pen.edge(card, 12, if boolean(entry, "active") { with_alpha(pal.accent, 0xB0) } else { pal.border });
            if !busy {
                pen.hit(card, &text(entry, "event"), 12, true);
            }
            let words = wide - pen.d(32) - pen.d(150);
            pen.text([cx + pen.d(16), at + pen.d(10), words, pen.ruler.line(HEAD)], HEAD, pal.ink, &text(entry, "title"), 0);
            pen.text([cx + pen.d(16), at + pen.d(36), words, pen.ruler.line(BODY)], BODY, pal.muted, &text(entry, "preview"), 0);
            pen.text([cx + pen.d(16), at + pen.d(62), words, pen.ruler.line(SMALL)], SMALL, pal.muted, &text(entry, "detail"), 0);
            if !busy {
                let side = pen.d(40);
                pen.glyph_button(cx + wide - pen.d(146), at + (block - side) / 2, side, "actions", &text(entry, "options_event"), pal.accent);
                pen.button([cx + wide - pen.d(96), at + (block - pen.d(36)) / 2, pen.d(80), pen.d(36)], "Delete", &text(entry, "delete_event"), Style::Tonal, true);
            }
        }
        at += block + gap;
    }
    pen.clip(None);
    thumb(pen, window, total, scroll);
}

fn listing(pen: &mut Pen, view: &View, scene: &mut Scene, area: [i32; 4]) {
    let ui = view.ui;
    let busy = boolean(ui, "busy") || view.working;
    let [cx, top, cw, tall] = area;
    let entries = rows(ui, "rows");
    let columns = (cw / pen.d(300)).clamp(1, 3) as usize;
    let gap = pen.d(10);
    let each = (cw - pen.d(10) - gap * (columns as i32 - 1)) / columns as i32;
    let pal = pen.pal;
    let sizes: Vec<i32> = entries.iter().map(|entry| (pen.ruler.height(BUTTON, &text(entry, "label"), each - pen.d(36)) + pen.d(24)).max(pen.d(56))).collect();
    let bands: Vec<i32> = sizes.chunks(columns).map(|group| group.iter().copied().max().unwrap_or(0)).collect();
    let total: i32 = bands.iter().map(|band| band + gap).sum();
    let window = [cx, top, cw, tall];
    scene.view = Some(window);
    let span = (total - tall).max(0);
    let scroll = view.scroll.clamp(0, span);
    scene.content = span;
    scene.scroll = scroll;
    pen.clip(Some([cx, top, cx + cw, top + tall]));
    let mut y = top - scroll;
    for (band, group) in bands.iter().zip(entries.chunks(columns)) {
        for (index, entry) in group.iter().enumerate() {
            let cell = [cx + index as i32 * (each + gap), y, each, *band];
            let event = text(entry, "event");
            let enabled = !busy || matches!(event.as_str(), "configuration" | "configuration_export" | "export_review");
            pen.panel(cell, 12, pal.card);
            if enabled {
                pen.hit(cell, &event, 12, true);
            }
            let label = text(entry, "label");
            let words = pen.ruler.height(BUTTON, &label, each - pen.d(36));
            pen.text([cell[0] + pen.d(18), y + (band - words) / 2, each - pen.d(36), words], BUTTON, if enabled { pal.accent } else { pal.dim }, &label, WRAP);
        }
        y += band + gap;
    }
    pen.clip(None);
    thumb(pen, window, total, scroll);
}

fn paged(pen: &mut Pen, view: &View, scene: &mut Scene, area: [i32; 4]) {
    let pal = pen.pal;
    let ui = view.ui;
    let busy = boolean(ui, "busy") || view.working;
    let [cx, top, cw, tall] = area;
    let entries = rows(ui, "rows");
    if entries.is_empty() {
        pen.text([cx, top, cw, tall.min(pen.d(160))], BODY, pal.muted, "No items on this page", CENTER);
        return;
    }
    let columns = if cw >= pen.d(820) { 2 } else { 1 };
    let (block, gap, foot) = (pen.d(68), pen.d(8), pen.d(52));
    let fit = (((tall - foot + gap) / (block + gap)).max(1) * columns) as usize;
    scene.page_size = fit;
    let last = (entries.len() - 1) / fit * fit;
    let start = (view.page_at / fit * fit).min(last);
    let each = (cw - gap * (columns - 1)) / columns;
    for (index, entry) in entries.iter().enumerate().skip(start).take(fit) {
        let slot = (index - start) as i32;
        let cell = [cx + (slot % columns) * (each + gap), top + (slot / columns) * (block + gap), each, block];
        let event = text(entry, "event");
        let enabled = !busy || matches!(event.as_str(), "configuration" | "configuration_export" | "export_review");
        pen.panel(cell, 12, pal.card);
        if enabled && !event.is_empty() {
            pen.hit(cell, &event, 12, true);
        }
        pen.clip(Some([cell[0], cell[1] + pen.d(8), cell[0] + cell[2], cell[1] + cell[3] - pen.d(8)]));
        pen.text([cell[0] + pen.d(16), cell[1] + pen.d(10), each - pen.d(32), block - pen.d(20)], BUTTON, if enabled { pal.ink } else { pal.dim }, &text(entry, "label"), WRAP);
        pen.clip(None);
    }
    let pages = entries.len().div_ceil(fit);
    if pages > 1 {
        let base = top + tall - pen.d(46);
        let side = pen.d(44);
        let middle = cx + cw / 2;
        if start > 0 {
            pen.glyph_button(middle - pen.d(80) - side, base, side, "arrow_left", "page:prev", pal.accent);
        }
        pen.text([middle - pen.d(80), base, pen.d(160), side], SMALL, pal.ink, &format!("{} / {}", start / fit + 1, pages), CENTER);
        if start + fit < entries.len() {
            pen.glyph_button(middle + pen.d(80), base, side, "arrow_right", "page:next", pal.accent);
        }
    }
}

pub fn compose(view: &View, ruler: &mut dyn Ruler) -> Scene {
    let ui = view.ui;
    let pal = Pal::of(ui);
    let mut scene = Scene::default();
    let mut pen = Pen { items: Vec::new(), hits: Vec::new(), fields: Vec::new(), ruler, pal, dpi: view.dpi, clip: None, dry: false };
    let (w, h) = (view.width.max(1), view.height.max(1));
    let m = pen.d(12);
    let page = text(ui, "page");
    let busy = boolean(ui, "busy") || view.working;
    let rail = if w >= pen.d(1000) { pen.d(228) } else { 0 };

    if rail > 0 {
        pen.panel([m, m, rail, h - m * 2], 18, pal.glass);
        pen.icon(m + pen.d(16), m + pen.d(15), pen.d(32), "icon", 0);
        pen.text([m + pen.d(60), m + pen.d(13), rail - pen.d(72), pen.d(36)], HEAD, pal.ink, "RLLM", 0);
        let mut y = m + pen.d(70);
        for tab in rows(ui, "tabs") {
            let event = text(&tab, "event");
            let active = event == page;
            let row = [m + pen.d(8), y, rail - pen.d(16), pen.d(44)];
            if active {
                pen.fill(row, 10, pal.card);
                pen.edge(row, 10, pal.border);
            }
            pen.hit(row, &event, 10, true);
            pen.icon(row[0] + pen.d(14), y + pen.d(12), pen.d(20), &caption(&text(&tab, "icon"), "conversation"), if active { pal.accent } else { pal.muted });
            pen.text([row[0] + pen.d(48), y, row[2] - pen.d(56), pen.d(44)], BUTTON, if active { pal.accent } else { pal.ink }, &text(&tab, "label"), 0);
            y += pen.d(48);
        }
        let detail: Vec<String> = text(ui, "detail").lines().filter(|line| !line.trim().is_empty()).map(|line| line.to_string()).collect();
        let detail = detail.join("\n");
        let tall = pen.ruler.height(SMALL, &detail, rail - pen.d(36));
        if y + tall + pen.d(24) < h - m {
            pen.fill([m + pen.d(16), h - m - tall - pen.d(30), rail - pen.d(32), 1], 0, pal.border);
            pen.text([m + pen.d(18), h - m - tall - pen.d(16), rail - pen.d(36), tall], SMALL, pal.muted, &detail, WRAP);
        }
    }

    let x0 = m + if rail > 0 { rail + pen.d(10) } else { 0 };
    let mw = (w - x0 - m).max(pen.d(160));
    let banner = [x0, m, mw, pen.d(60)];
    pen.panel(banner, 12, pal.glass);
    let side = pen.d(44);
    let mut left = x0 + pen.d(8);
    if rail == 0 {
        pen.glyph_button(left, m + pen.d(8), side, "actions", "navigation", pal.accent);
        left += side + pen.d(4);
    }
    if boolean(ui, "can_navigate_back") {
        pen.glyph_button(left, m + pen.d(8), side, "arrow_left", "back", pal.accent);
        left += side + pen.d(4);
    }
    let right = x0 + mw - pen.d(8);
    pen.glyph_button(right - side * 2 - pen.d(4), m + pen.d(8), side, "history", "history", pal.accent);
    if !busy {
        pen.glyph_button(right - side, m + pen.d(8), side, "message", "profile_edit", pal.accent);
    } else {
        let icon = pen.d(22);
        pen.icon(right - side + (side - icon) / 2, m + pen.d(8) + (side - icon) / 2, icon, "message", pal.dim);
    }
    let title = heading(ui);
    let room = (right - side * 2 - pen.d(16) - left - pen.d(8)).max(pen.d(60));
    let server = if page == "chat" { text(ui, "server") } else { String::new() };
    let reach = pen.ruler.width(HEAD, &title).max(pen.ruler.width(SMALL, &server)).min(room - pen.d(20));
    if page == "chat" {
        pen.hit([left, m + pen.d(6), reach + pen.d(24), pen.d(48)], "chat_models", 10, true);
    }
    if server.is_empty() {
        pen.text([left + pen.d(12), m, room - pen.d(20), pen.d(60)], HEAD, pal.ink, &title, 0);
    } else {
        pen.text([left + pen.d(12), m + pen.d(9), room - pen.d(20), pen.d(24)], HEAD, pal.ink, &title, 0);
        pen.text([left + pen.d(12), m + pen.d(33), room - pen.d(20), pen.d(18)], SMALL, pal.muted, &server, 0);
    }
    if busy {
        pen.items.push(Item::Busy { area: [x0 + pen.d(16), m + pen.d(60) - pen.d(3), mw - pen.d(32), pen.d(2)], color: pal.accent });
    }

    let top = m + pen.d(70);
    let panel = [x0, top, mw, (h - top - m).max(pen.d(120))];
    pen.panel(panel, 18, pal.glass);
    if page == "chat" {
        let cw = (mw - pen.d(40)).min(pen.d(960)).max(pen.d(200));
        chat(&mut pen, view, &mut scene, panel, [x0 + (mw - cw) / 2, cw]);
        if view.menu {
            if let Some(draft) = scene.draft {
                let choices = rows(ui, "composer_menu");
                let (each, wide) = (pen.d(46), pen.d(264));
                let tall = each * choices.len().max(1) as i32 + pen.d(12);
                let cx = x0 + (mw - cw) / 2;
                let card = [cx + pen.d(2), draft[1] - pen.d(22) - tall, wide, tall];
                pen.hit([0, 0, w, h], "menu:close", 0, false);
                pen.fill(card, 12, pal.raised);
                pen.edge(card, 12, pal.border);
                for (index, choice) in choices.iter().enumerate() {
                    let row = [card[0] + pen.d(6), card[1] + pen.d(6) + index as i32 * each, wide - pen.d(12), each];
                    pen.hit(row, &text(choice, "event"), 8, true);
                    pen.icon(row[0] + pen.d(12), row[1] + (each - pen.d(20)) / 2, pen.d(20), &text(choice, "icon"), pal.accent);
                    pen.text([row[0] + pen.d(46), row[1], row[2] - pen.d(54), each], BODY, pal.ink, &text(choice, "label"), 0);
                }
            }
        }
    } else {
        let status = text(ui, "visible_status");
        let foot = if !status.is_empty() || busy { pen.d(44) } else { 0 };
        let most = if page == "diagnostics" { 1240 } else { 960 };
        let cw = (mw - pen.d(40)).min(pen.d(most)).max(pen.d(200));
        let cx = x0 + (mw - cw) / 2;
        let area = [cx, panel[1] + pen.d(18), cw, panel[3] - pen.d(34) - foot];
        if page == "history" {
            history(&mut pen, view, &mut scene, area);
        } else if page == "diagnostics" {
            scene.view = Some(area);
            pen.clip(Some([area[0], area[1], area[0] + area[2], area[1] + area[3]]));
            let inset = pen.d(10);
            let total = monitor(&mut pen, get(ui, "monitor"), [area[0], area[1], area[2] - inset, area[3]], view.scroll);
            pen.clip(None);
            scene.content = (total - area[3]).max(0);
            scene.scroll = view.scroll.clamp(0, scene.content);
            thumb(&mut pen, area, total, scene.scroll);
        } else if text(get(ui, "rows_layout"), "kind") == "list" {
            listing(&mut pen, view, &mut scene, area);
        } else {
            paged(&mut pen, view, &mut scene, area);
        }
        if foot > 0 {
            let base = panel[1] + panel[3] - pen.d(14) - pen.d(36);
            let mut room = cw;
            if busy {
                pen.button([cx + cw - pen.d(104), base, pen.d(104), pen.d(36)], "Cancel", "cancel", Style::Tonal, true);
                room -= pen.d(116);
            }
            if !status.is_empty() {
                pen.hit([cx, base, room, pen.d(36)], "status", 8, true);
                pen.text([cx + pen.d(12), base, room - pen.d(24), pen.d(36)], SMALL, pal.muted, &status, 0);
            }
        }
    }

    if get(ui, "modal") != &Null {
        scene.draft = None;
        scene.view = None;
        let base = pen.hits.len();
        dialog(&mut pen, view, &mut scene);
        // Nothing under an open dialog reacts to the pointer.
        for hit in pen.hits.iter_mut().take(base) {
            hit.action.clear();
            hit.wash = false;
        }
    }
    if !view.note.is_empty() {
        let wide = pen.fit(SMALL, view.note, pen.d(36), pen.d(120), w - pen.d(48));
        let tall = pen.ruler.height(SMALL, view.note, wide - pen.d(32)) + pen.d(20);
        // Over the banner: clear of the conversation, and of the text boxes,
        // which sit on top of the drawing.
        let area = [(w - wide) / 2, pen.d(20), wide, tall];
        pen.fill(area, 10, pal.raised);
        pen.edge(area, 10, pal.border);
        pen.text([area[0] + pen.d(16), area[1] + pen.d(10), wide - pen.d(32), tall - pen.d(20)], SMALL, pal.ink, view.note, WRAP | CENTER);
    }
    scene.items = pen.items;
    scene.hits = pen.hits;
    scene.fields = pen.fields;
    scene
}
