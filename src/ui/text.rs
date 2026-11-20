// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! Text on a surface: measured in OxiGuitar's own font (so it lands
//! exactly where OxiGuitar draws it), wrapped to a width, and the plugin's
//! own text box: a caret, a selection, typing, pasting and copying.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use oxiguitar_plugin::display::{Anchor, Font, Item, Paint, Point};
use oxiguitar_plugin::{Host, Key, KeyName};

/// Where each character of a text starts, and where it ends, at a size.
pub trait Measure {
    /// One more edge than `text` has characters, the first 0.
    fn edges(&self, text: &str, size: f32) -> Arc<Vec<f32>>;
}

/// Measured texts by their text and size (hundredths of a pixel).
type Cache = HashMap<(String, u32), Arc<Vec<f32>>>;

/// The host's measure, remembered: the same text at the same size is
/// measured once.
#[derive(Debug)]
pub struct Measured {
    host: Host,
    cache: Mutex<Cache>,
}

impl Measured {
    /// Measures with `host`.
    pub fn new(host: Host) -> Measured {
        Measured {
            host,
            cache: Mutex::default(),
        }
    }
}

impl Measure for Measured {
    fn edges(&self, text: &str, size: f32) -> Arc<Vec<f32>> {
        let key = (text.to_owned(), (size * 100.0) as u32);
        if let Some(e) = self
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&key)
        {
            return e.clone();
        }
        let edges = Arc::new(match self.host.measure_text(text, size, Font::Sans) {
            Ok(m) if m.edges.len() == text.chars().count() + 1 => m.edges,
            // No fonts to measure with (a program without a window): an
            // estimate, so nothing breaks.
            _ => Fixed.edges(text, size).to_vec(),
        });
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() > 4000 {
            cache.clear();
        }
        cache.insert(key, edges.clone());
        edges
    }
}

/// Every character half the size wide: for tests, and where nothing can
/// measure.
#[derive(Debug, Default)]
pub struct Fixed;

impl Measure for Fixed {
    fn edges(&self, text: &str, size: f32) -> Arc<Vec<f32>> {
        Arc::new(
            (0..=text.chars().count())
                .map(|i| i as f32 * size / 2.0)
                .collect(),
        )
    }
}

/// A line of wrapped text: characters `start..end` of its paragraph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    /// The paragraph's index.
    pub paragraph: usize,
    /// Its first character in the paragraph.
    pub start: usize,
    /// One past its last.
    pub end: usize,
}

/// `text` wrapped to `width` at `size`: its paragraphs (split at line
/// breaks), each broken after a space where it can, inside a word where a
/// word alone is wider.
pub fn wrap(measure: &dyn Measure, text: &str, size: f32, width: f32) -> (Vec<String>, Vec<Line>) {
    let paragraphs: Vec<String> = text.split('\n').map(str::to_owned).collect();
    let mut lines = Vec::new();
    for (p, para) in paragraphs.iter().enumerate() {
        let chars: Vec<char> = para.chars().collect();
        let edges = measure.edges(para, size);
        let mut start = 0;
        if chars.is_empty() {
            lines.push(Line {
                paragraph: p,
                start: 0,
                end: 0,
            });
            continue;
        }
        while start < chars.len() {
            let mut end = start;
            let mut space = None;
            while end < chars.len() && edges[end + 1] - edges[start] <= width.max(size) {
                if chars[end] == ' ' {
                    space = Some(end + 1);
                }
                end += 1;
            }
            if end < chars.len() {
                match space {
                    Some(s) if s > start => end = s,
                    _ if end == start => end = start + 1,
                    _ => {}
                }
            }
            lines.push(Line {
                paragraph: p,
                start,
                end,
            });
            start = end;
        }
    }
    (paragraphs, lines)
}

/// The text of a line.
pub fn line_text(paragraphs: &[String], line: &Line) -> String {
    paragraphs[line.paragraph]
        .chars()
        .skip(line.start)
        .take(line.end - line.start)
        .collect()
}

/// A box on a surface, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Its left.
    pub x: f32,
    /// Its top.
    pub y: f32,
    /// Its width.
    pub w: f32,
    /// Its height.
    pub h: f32,
}

impl Rect {
    /// Whether `p` is inside.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.x + self.w && p.y >= self.y && p.y <= self.y + self.h
    }
}

/// A filled rectangle.
pub fn rect(r: Rect, paint: Paint) -> Item {
    Item::Rect {
        at: Point::new(r.x, r.y),
        width: r.w.max(0.0),
        height: r.h.max(0.0),
        paint,
    }
}

/// A rectangle's outline, `width` thick.
pub fn frame(r: Rect, width: f32, paint: Paint) -> Vec<Item> {
    let line = |a: Point, b: Point| Item::Line {
        from: a,
        to: b,
        width,
        paint,
    };
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    vec![
        line(Point::new(x0, y0), Point::new(x1, y0)),
        line(Point::new(x1, y0), Point::new(x1, y1)),
        line(Point::new(x1, y1), Point::new(x0, y1)),
        line(Point::new(x0, y1), Point::new(x0, y0)),
    ]
}

/// Text on its baseline.
pub fn text(s: &str, x: f32, baseline: f32, size: f32, paint: Paint) -> Item {
    Item::Text {
        text: s.to_owned(),
        at: Point::new(x, baseline),
        size,
        font: Font::Sans,
        anchor: Anchor::Start,
        paint,
    }
}

/// What a key or text did to a text box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing the owner needs to know.
    Nothing,
    /// The text changed.
    Changed,
    /// Enter was pressed in a box that sends.
    Submit,
    /// The user copied (or cut) this text.
    Copy(String),
}

/// The plugin's own text box: one line or several, its text shown or
/// hidden (a key), with a caret and a selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBox {
    /// The text.
    pub text: String,
    /// The caret, in characters.
    caret: usize,
    /// Where the selection began, when there is one.
    anchor: Option<usize>,
    /// Shown as dots.
    pub masked: bool,
    /// Several lines; Enter sends unless Shift is held.
    pub multiline: bool,
    /// Enter sends ([`Outcome::Submit`]); otherwise it breaks the line in
    /// a box of several.
    pub sends: bool,
    /// The first line shown, when there are more than fit.
    first: usize,
}

/// The text box's size and padding.
pub const SIZE: f32 = 13.0;
const PAD: f32 = 6.0;

/// A line's height at [`SIZE`].
pub fn line_height() -> f32 {
    SIZE * 1.45
}

impl TextBox {
    /// A box holding `text`, the caret at its end.
    pub fn new(text: &str, multiline: bool, sends: bool, masked: bool) -> TextBox {
        TextBox {
            text: text.to_owned(),
            caret: text.chars().count(),
            anchor: None,
            masked,
            multiline,
            sends,
            first: 0,
        }
    }

    /// Replaces the text, the caret at its end.
    pub fn set(&mut self, text: &str) {
        *self = TextBox::new(text, self.multiline, self.sends, self.masked);
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn shown(&self) -> String {
        if self.masked {
            "•".repeat(self.len())
        } else {
            self.text.clone()
        }
    }

    fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    fn byte(&self, char_index: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_index)
            .map_or(self.text.len(), |(b, _)| b)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let (ba, bb) = (self.byte(a), self.byte(b));
        self.text.replace_range(ba..bb, "");
        self.caret = a;
        self.anchor = None;
        true
    }

    /// Text typed, entered through an input method or pasted.
    pub fn typed(&mut self, s: &str) -> Outcome {
        let s = s.replace("\r\n", "\n").replace('\r', "\n");
        let s: String = if self.multiline {
            s
        } else {
            s.replace('\n', " ")
        };
        let s: String = s
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect();
        if s.is_empty() {
            return Outcome::Nothing;
        }
        self.delete_selection();
        let at = self.byte(self.caret);
        self.text.insert_str(at, &s);
        self.caret += s.chars().count();
        Outcome::Changed
    }

    /// A key on the box, which has the keyboard; `width` is its inner
    /// width, for moving up and down its lines.
    pub fn key(&mut self, key: Key, measure: &dyn Measure, width: f32) -> Outcome {
        let extend = |b: &mut TextBox| {
            if key.shift {
                b.anchor.get_or_insert(b.caret);
            } else {
                b.anchor = None;
            }
        };
        match key.name {
            KeyName::Enter if self.multiline && (key.shift || !self.sends) => self.typed("\n"),
            KeyName::Enter if self.sends => Outcome::Submit,
            KeyName::Enter => Outcome::Nothing,
            KeyName::Backspace => {
                if !self.delete_selection() && self.caret > 0 {
                    let to = self.caret;
                    let from = if key.ctrl {
                        self.word_start(to)
                    } else {
                        to - 1
                    };
                    let (a, b) = (self.byte(from), self.byte(to));
                    self.text.replace_range(a..b, "");
                    self.caret = from;
                }
                Outcome::Changed
            }
            KeyName::Delete => {
                if !self.delete_selection() && self.caret < self.len() {
                    let (a, b) = (self.byte(self.caret), self.byte(self.caret + 1));
                    self.text.replace_range(a..b, "");
                }
                Outcome::Changed
            }
            KeyName::Left => {
                if !key.shift
                    && let Some((a, _)) = self.selection()
                {
                    self.caret = a;
                    self.anchor = None;
                } else {
                    extend(self);
                    self.caret = if key.ctrl {
                        self.word_start(self.caret)
                    } else {
                        self.caret.saturating_sub(1)
                    };
                }
                Outcome::Nothing
            }
            KeyName::Right => {
                if !key.shift
                    && let Some((_, b)) = self.selection()
                {
                    self.caret = b;
                    self.anchor = None;
                } else {
                    extend(self);
                    self.caret = (self.caret + 1).min(self.len());
                }
                Outcome::Nothing
            }
            KeyName::Home | KeyName::End | KeyName::Up | KeyName::Down => {
                extend(self);
                let (paragraphs, lines) = wrap(measure, &self.shown(), SIZE, width);
                let (li, col_x) = self.caret_line(&paragraphs, &lines, measure);
                let target = match key.name {
                    KeyName::Home => Some((li, 0.0)),
                    KeyName::End => Some((li, f32::INFINITY)),
                    KeyName::Up => li.checked_sub(1).map(|l| (l, col_x)),
                    _ => (li + 1 < lines.len()).then_some((li + 1, col_x)),
                };
                if let Some((l, x)) = target {
                    self.caret = self.index_at(&paragraphs, &lines, l, x, measure);
                } else if key.name == KeyName::Up {
                    self.caret = 0;
                } else {
                    self.caret = self.len();
                }
                Outcome::Nothing
            }
            KeyName::Char('a') if key.ctrl => {
                self.anchor = Some(0);
                self.caret = self.len();
                Outcome::Nothing
            }
            KeyName::Char('c') | KeyName::Char('x') if key.ctrl => {
                let Some((a, b)) = self.selection() else {
                    return Outcome::Nothing;
                };
                if self.masked {
                    return Outcome::Nothing;
                }
                let copied: String = self.text.chars().skip(a).take(b - a).collect();
                if key.name == KeyName::Char('x') {
                    self.delete_selection();
                }
                Outcome::Copy(copied)
            }
            _ => Outcome::Nothing,
        }
    }

    fn word_start(&self, from: usize) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let mut i = from;
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && chars[i - 1] != ' ' && chars[i - 1] != '\n' {
            i -= 1;
        }
        i
    }

    /// The line the caret is on and its x in that line.
    fn caret_line(
        &self,
        paragraphs: &[String],
        lines: &[Line],
        measure: &dyn Measure,
    ) -> (usize, f32) {
        let mut before = 0;
        for (i, l) in lines.iter().enumerate() {
            let start = before + l.start;
            let end = before + l.end;
            let last_of_paragraph = lines.get(i + 1).is_none_or(|n| n.paragraph != l.paragraph);
            if self.caret >= start && (self.caret < end || (self.caret == end && last_of_paragraph))
            {
                let edges = measure.edges(&paragraphs[l.paragraph], SIZE);
                let x = edges[self.caret - before] - edges[l.start];
                return (i, x);
            }
            if last_of_paragraph {
                before += paragraphs[l.paragraph].chars().count() + 1;
            }
        }
        (lines.len().saturating_sub(1), 0.0)
    }

    /// The character index nearest `x` on line `li`.
    fn index_at(
        &self,
        paragraphs: &[String],
        lines: &[Line],
        li: usize,
        x: f32,
        measure: &dyn Measure,
    ) -> usize {
        let before: usize = paragraphs[..lines[li].paragraph]
            .iter()
            .map(|p| p.chars().count() + 1)
            .sum();
        let l = lines[li];
        let edges = measure.edges(&paragraphs[l.paragraph], SIZE);
        let base = edges[l.start];
        let last_of_paragraph = lines.get(li + 1).is_none_or(|n| n.paragraph != l.paragraph);
        let end = if last_of_paragraph {
            l.end
        } else {
            l.end.saturating_sub(1).max(l.start)
        };
        let mut best = l.start;
        for i in l.start..=end {
            let mid = if i < end {
                (edges[i] + edges[i + 1]) / 2.0 - base
            } else {
                f32::INFINITY
            };
            best = i;
            if x <= mid {
                break;
            }
        }
        before + best
    }

    /// How tall the box is for its text at `width` (inner), showing at
    /// most `most` lines.
    pub fn height(&self, measure: &dyn Measure, width: f32, most: usize) -> f32 {
        let lines = if self.multiline {
            wrap(measure, &self.shown(), SIZE, width)
                .1
                .len()
                .clamp(1, most)
        } else {
            1
        };
        lines as f32 * line_height() + 2.0 * PAD
    }

    /// The inner width of a box `outer` wide.
    pub fn inner(outer: f32) -> f32 {
        (outer - 2.0 * PAD).max(1.0)
    }

    /// A click at `at` in the box drawn at `r`: the caret goes there; with
    /// Shift held, the selection grows to it.
    pub fn click(&mut self, at: Point, r: Rect, measure: &dyn Measure, extend: bool) {
        let (paragraphs, lines) = self.layout(measure, r.w);
        if lines.is_empty() {
            return;
        }
        let row = ((at.y - r.y - PAD) / line_height()).floor().max(0.0) as usize + self.first;
        let li = row.min(lines.len() - 1);
        if extend {
            self.anchor.get_or_insert(self.caret);
        } else {
            self.anchor = None;
        }
        self.caret = self.index_at(&paragraphs, &lines, li, at.x - r.x - PAD, measure);
    }

    /// A drag to `at`: the selection runs from where it began.
    pub fn drag(&mut self, at: Point, r: Rect, measure: &dyn Measure) {
        let anchor = *self.anchor.get_or_insert(self.caret);
        self.click(at, r, measure, true);
        self.anchor = Some(anchor);
    }

    fn layout(&self, measure: &dyn Measure, outer: f32) -> (Vec<String>, Vec<Line>) {
        let inner = TextBox::inner(outer);
        if self.multiline {
            wrap(measure, &self.shown(), SIZE, inner)
        } else {
            let shown = self.shown();
            let n = shown.chars().count();
            (
                vec![shown],
                vec![Line {
                    paragraph: 0,
                    start: 0,
                    end: n,
                }],
            )
        }
    }

    /// Draws the box at `r` (its height from [`TextBox::height`]), with the
    /// caret when `focused`, and `placeholder` while it is empty.
    pub fn draw(
        &mut self,
        r: Rect,
        focused: bool,
        placeholder: &str,
        measure: &dyn Measure,
        out: &mut Vec<Item>,
    ) {
        out.push(rect(r, Paint::Page));
        out.extend(frame(
            r,
            if focused { 1.5 } else { 1.0 },
            if focused { Paint::Accent } else { Paint::Muted },
        ));
        let (paragraphs, lines) = self.layout(measure, r.w);
        let visible = (((r.h - 2.0 * PAD) / line_height()).floor() as usize).max(1);
        let (caret_line, caret_x) = self.caret_line(&paragraphs, &lines, measure);
        // Keep the caret's line in view.
        if caret_line < self.first {
            self.first = caret_line;
        } else if caret_line >= self.first + visible {
            self.first = caret_line + 1 - visible;
        }
        let inner_w = TextBox::inner(r.w);
        // One line: scrolled sideways so the caret shows.
        let shift = if self.multiline {
            0.0
        } else {
            (caret_x - inner_w + 2.0).max(0.0)
        };
        if self.text.is_empty() && !placeholder.is_empty() && !focused {
            out.push(text(
                placeholder,
                r.x + PAD,
                r.y + PAD + SIZE,
                SIZE,
                Paint::Muted,
            ));
        }
        let selection = self.selection();
        let mut before = 0;
        for (i, l) in lines.iter().enumerate() {
            let last_of_paragraph = lines.get(i + 1).is_none_or(|n| n.paragraph != l.paragraph);
            if i >= self.first && i < self.first + visible {
                let y = r.y + PAD + (i - self.first) as f32 * line_height();
                let edges = measure.edges(&paragraphs[l.paragraph], SIZE);
                let base = edges[l.start];
                if let Some((a, b)) = selection {
                    let (la, lb) = (before + l.start, before + l.end);
                    let (sa, sb) = (a.max(la), b.min(lb));
                    if sa < sb {
                        let x0 = edges[sa - before] - base - shift;
                        let x1 = edges[sb - before] - base - shift;
                        out.push(rect(
                            Rect {
                                x: r.x + PAD + x0.max(0.0),
                                y,
                                w: (x1 - x0.max(0.0)).min(inner_w),
                                h: line_height(),
                            },
                            Paint::Muted,
                        ));
                    }
                }
                let shown = line_text(&paragraphs, l);
                // Only what fits: characters past the box's right edge are
                // cut (with no clip, they would spill out).
                let fits: String = shown
                    .chars()
                    .enumerate()
                    .filter(|(k, _)| {
                        let x0 = edges[l.start + k] - base - shift;
                        let x1 = edges[l.start + k + 1] - base - shift;
                        x0 >= -0.5 && x1 <= inner_w + 0.5
                    })
                    .map(|(_, c)| c)
                    .collect();
                let first_x = shown
                    .chars()
                    .enumerate()
                    .find(|(k, _)| edges[l.start + k] - base - shift >= -0.5)
                    .map_or(0.0, |(k, _)| edges[l.start + k] - base - shift);
                out.push(text(&fits, r.x + PAD + first_x, y + SIZE, SIZE, Paint::Ink));
                if focused && i == caret_line {
                    let x = r.x + PAD + (caret_x - shift).clamp(0.0, inner_w);
                    out.push(Item::Line {
                        from: Point::new(x, y + 2.0),
                        to: Point::new(x, y + line_height() - 2.0),
                        width: 1.5,
                        paint: Paint::Accent,
                    });
                }
            }
            if last_of_paragraph {
                before += paragraphs[l.paragraph].chars().count() + 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: KeyName) -> Key {
        Key {
            name,
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[test]
    fn wrapping_breaks_after_spaces_and_inside_long_words() {
        // Six pixels a character at 12.
        let (p, lines) = wrap(&Fixed, "hello world foo\nabcdefghij", 12.0, 36.0);
        let texts: Vec<String> = lines.iter().map(|l| line_text(&p, l)).collect();
        assert_eq!(texts, ["hello ", "world ", "foo", "abcdef", "ghij"]);
        let (_, empty) = wrap(&Fixed, "", 12.0, 36.0);
        assert_eq!(empty.len(), 1);
    }

    #[test]
    fn typing_moving_selecting_and_copying() {
        let mut b = TextBox::new("", true, true, false);
        assert_eq!(b.typed("héllo"), Outcome::Changed);
        assert_eq!(
            b.key(key(KeyName::Backspace), &Fixed, 100.0),
            Outcome::Changed
        );
        assert_eq!(b.text, "héll");
        b.key(key(KeyName::Home), &Fixed, 100.0);
        b.typed("Oh ");
        assert_eq!(b.text, "Oh héll");
        b.key(
            Key {
                ctrl: true,
                ..key(KeyName::Char('a'))
            },
            &Fixed,
            100.0,
        );
        assert_eq!(
            b.key(
                Key {
                    ctrl: true,
                    ..key(KeyName::Char('c'))
                },
                &Fixed,
                100.0
            ),
            Outcome::Copy("Oh héll".into())
        );
        b.typed("new");
        assert_eq!(b.text, "new");
        assert_eq!(
            b.key(
                Key {
                    shift: true,
                    ..key(KeyName::Enter)
                },
                &Fixed,
                100.0
            ),
            Outcome::Changed
        );
        b.typed("line");
        assert_eq!(b.text, "new\nline");
        assert_eq!(b.key(key(KeyName::Enter), &Fixed, 100.0), Outcome::Submit);
        b.key(key(KeyName::Up), &Fixed, 100.0);
        b.typed("!");
        assert_eq!(b.text, "new!\nline");
        b.key(
            Key {
                ctrl: true,
                ..key(KeyName::Backspace)
            },
            &Fixed,
            100.0,
        );
        assert_eq!(b.text, "\nline");
    }

    #[test]
    fn a_key_box_is_one_hidden_line_that_never_copies() {
        let mut b = TextBox::new("", false, false, true);
        b.typed("sk-a\nb");
        assert_eq!(b.text, "sk-a b");
        b.key(
            Key {
                ctrl: true,
                ..key(KeyName::Char('a'))
            },
            &Fixed,
            100.0,
        );
        assert_eq!(
            b.key(
                Key {
                    ctrl: true,
                    ..key(KeyName::Char('c'))
                },
                &Fixed,
                100.0
            ),
            Outcome::Nothing
        );
        let mut items = Vec::new();
        b.draw(
            Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 30.0,
            },
            true,
            "",
            &Fixed,
            &mut items,
        );
        let shown: Vec<String> = items
            .iter()
            .filter_map(|i| match i {
                Item::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(shown, ["••••••"]);
    }

    #[test]
    fn a_click_puts_the_caret_by_the_nearest_character() {
        let mut b = TextBox::new("abcdef", false, true, false);
        let r = Rect {
            x: 10.0,
            y: 0.0,
            w: 200.0,
            h: 30.0,
        };
        // Characters are 6.5 wide at 13; the padding is 6: x 10 + 6 + 15
        // is past the second character's middle.
        b.click(Point::new(31.0, 10.0), r, &Fixed, false);
        b.typed("X");
        assert_eq!(b.text, "abXcdef");
    }
}
