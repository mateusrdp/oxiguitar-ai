// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Mateus Rocha de Paula and the OxiGuitar contributors

//! The tab `chat`, in the window's right-hand area: one surface the plugin
//! draws itself: New Chat at the top, the conversation,
//! scrolled with the wheel and following the newest, the box the user
//! types in, and Send (Stop while the model works).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use oxiguitar_plugin::display::{Item, Paint, Point};
use oxiguitar_plugin::{
    Cursor, Error, Host, KeyName, PointerKind, Row, Surface, Tab, TabContent, TabEvent, Widget,
};

use crate::agent::{self, Entry, Shared};
use crate::settings::Settings;
use crate::tools::Editor;
use crate::ui::text::{self, Measure, Outcome, Rect, TextBox};

/// The surface's id.
const SURFACE: &str = "chat";
const PAD: f32 = 8.0;
const BUTTONS: f32 = 28.0;
/// The height of the row with New Chat.
const HEADER: f32 = 30.0;
const BODY: f32 = 13.0;
const SMALL: f32 = 11.0;

/// Something on the surface the pointer can use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    NewChat,
    Input,
    Send,
    Stop,
    Copy(usize),
    Allow,
    Deny,
}

/// What the surface remembers between events.
#[derive(Debug)]
struct State {
    size: (f32, f32),
    input: TextBox,
    focused: bool,
    /// How far up from the newest the conversation is scrolled, in pixels.
    scroll: f32,
    /// How far it can be.
    most: f32,
    hits: Vec<(Rect, Action)>,
    hover: Option<Action>,
    pressed: Option<Action>,
    input_rect: Rect,
}

/// The tab.
pub struct ChatTab {
    host: Host,
    shared: Arc<Shared>,
    measure: Arc<dyn Measure + Send + Sync>,
    editor: Arc<dyn Editor + Send + Sync>,
    state: Mutex<State>,
}

impl std::fmt::Debug for ChatTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatTab").finish_non_exhaustive()
    }
}

impl ChatTab {
    /// The tab, measuring with `measure` and changing the score through
    /// `editor`.
    pub fn new(
        host: Host,
        shared: Arc<Shared>,
        measure: Arc<dyn Measure + Send + Sync>,
        editor: Arc<dyn Editor + Send + Sync>,
    ) -> ChatTab {
        ChatTab {
            host,
            shared,
            measure,
            editor,
            state: Mutex::new(State {
                size: (300.0, 400.0),
                input: TextBox::new("", true, true, false),
                focused: false,
                scroll: 0.0,
                most: 0.0,
                hits: Vec::new(),
                hover: None,
                pressed: None,
                input_rect: Rect::default(),
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn redraw(&self) -> Arc<dyn Fn() + Send + Sync> {
        let host = self.host.clone();
        Arc::new(move || host.redraw(crate::CHAT))
    }

    /// The user sends what the box holds.
    fn submit(&self, st: &mut State) {
        let question = st.input.text.trim().to_owned();
        if question.is_empty() || self.shared.lock().running {
            return;
        }
        st.input.set("");
        st.scroll = 0.0;
        let settings = Settings::load(&self.host);
        if !settings.may_send() {
            self.shared.lock().waiting = Some((settings.server(), question));
            return;
        }
        agent::ask(
            &self.shared,
            settings,
            self.editor.clone(),
            question,
            self.redraw(),
        );
    }

    /// The user allows the server they were asked about, and the question
    /// waiting for it goes.
    fn allow(&self) {
        let Some((server, question)) = self.shared.lock().waiting.take() else {
            return;
        };
        let mut settings = Settings::load(&self.host);
        if !settings.allowed.contains(&server) {
            settings.allowed.push(server);
        }
        if let Err(e) = self.host.set_settings(&settings.to_map()) {
            self.shared.lock().entries.push(Entry::Error(format!(
                "the permission couldn't be saved: {}",
                e.message
            )));
            return;
        }
        agent::ask(
            &self.shared,
            settings,
            self.editor.clone(),
            question,
            self.redraw(),
        );
    }

    fn act(&self, action: Action, st: &mut State) {
        match action {
            Action::NewChat => {
                self.shared.clear();
                st.scroll = 0.0;
            }
            Action::Input => {}
            Action::Send => self.submit(st),
            Action::Stop => self.shared.stop(),
            Action::Copy(i) => {
                let copied = match self.shared.lock().entries.get(i) {
                    Some(Entry::Assistant(s)) => Some(visible(s)),
                    _ => None,
                };
                if let Some(t) = copied {
                    let _ = self.host.copy(&t);
                }
            }
            Action::Allow => self.allow(),
            Action::Deny => {
                self.shared.lock().waiting = None;
            }
        }
    }
}

/// What the model said, without the thinking some models write out
/// (`<think>…</think>`).
pub fn visible(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        match rest.find("<think>") {
            Some(i) => {
                out.push_str(&rest[..i]);
                match rest[i..].find("</think>") {
                    Some(j) => rest = &rest[i + j + "</think>".len()..],
                    None => break,
                }
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    out.trim().to_owned()
}

/// A block of the conversation laid out: its items, from y 0, and height.
struct Block {
    items: Vec<Item>,
    height: f32,
    hits: Vec<(Rect, Action)>,
}

/// Where and how a paragraph is drawn.
#[derive(Clone, Copy)]
struct Pen {
    x: f32,
    y: f32,
    size: f32,
    width: f32,
    paint: Paint,
}

/// Draws `s` wrapped with `pen`; how tall it is.
fn paragraph(measure: &dyn Measure, s: &str, pen: Pen, out: &mut Vec<Item>) -> f32 {
    let (paragraphs, lines) = text::wrap(measure, s, pen.size, pen.width);
    let lh = pen.size * 1.4;
    for (i, l) in lines.iter().enumerate() {
        out.push(text::text(
            text::line_text(&paragraphs, l).trim_end(),
            pen.x,
            pen.y + pen.size + i as f32 * lh,
            pen.size,
            pen.paint,
        ));
    }
    lines.len() as f32 * lh
}

fn pen(x: f32, y: f32, size: f32, width: f32, paint: Paint) -> Pen {
    Pen {
        x,
        y,
        size,
        width,
        paint,
    }
}

fn button(label: &str, r: Rect, hover: bool, primary: bool, out: &mut Vec<Item>) {
    out.push(text::rect(
        r,
        if primary || hover {
            Paint::Accent
        } else {
            Paint::Muted
        },
    ));
    out.push(Item::Text {
        text: label.to_owned(),
        at: Point::new(r.x + r.w / 2.0, r.y + r.h / 2.0 + BODY * 0.35),
        size: BODY,
        font: oxiguitar_plugin::display::Font::Sans,
        anchor: oxiguitar_plugin::display::Anchor::Middle,
        paint: Paint::Page,
    });
}

fn block(
    measure: &dyn Measure,
    entry: &Entry,
    index: usize,
    width: f32,
    hover: Option<Action>,
) -> Block {
    let mut items = Vec::new();
    let mut hits = Vec::new();
    let inner = width - 2.0 * PAD;
    let h = match entry {
        Entry::User(s) => {
            items.push(text::text("You", PAD, SMALL, SMALL, Paint::Accent));
            SMALL
                + 4.0
                + paragraph(
                    measure,
                    s,
                    pen(PAD, SMALL + 4.0, BODY, inner, Paint::Ink),
                    &mut items,
                )
        }
        Entry::Assistant(s) => {
            let shown = visible(s);
            items.push(text::text("AI", PAD, SMALL, SMALL, Paint::Muted));
            if !shown.is_empty() {
                let copy = Rect {
                    x: width - PAD - 40.0,
                    y: 0.0,
                    w: 40.0,
                    h: SMALL + 3.0,
                };
                let lit = hover == Some(Action::Copy(index));
                items.push(text::text(
                    "Copy",
                    copy.x + 8.0,
                    SMALL,
                    SMALL,
                    if lit { Paint::Accent } else { Paint::Muted },
                ));
                hits.push((copy, Action::Copy(index)));
            }
            let body = if shown.is_empty() { "…" } else { &shown };
            SMALL
                + 4.0
                + paragraph(
                    measure,
                    body,
                    pen(PAD, SMALL + 4.0, BODY, inner, Paint::Ink),
                    &mut items,
                )
        }
        Entry::Tool(s) => paragraph(
            measure,
            &format!("· {s}"),
            pen(PAD, 0.0, SMALL, inner, Paint::Muted),
            &mut items,
        ),
        Entry::Notice(s) => paragraph(
            measure,
            s,
            pen(PAD, 0.0, SMALL, inner, Paint::Muted),
            &mut items,
        ),
        Entry::Error(s) => {
            let mut inside = Vec::new();
            let h = paragraph(
                measure,
                s,
                pen(PAD + 4.0, 3.0, BODY, inner - 8.0, Paint::Ink),
                &mut inside,
            ) + 6.0;
            items.push(text::rect(
                Rect {
                    x: PAD,
                    y: 0.0,
                    w: inner,
                    h,
                },
                Paint::Warning,
            ));
            items.extend(inside);
            h
        }
    };
    Block {
        items,
        height: h,
        hits,
    }
}

fn shifted(item: Item, dy: f32) -> Item {
    let mv = |p: Point| Point::new(p.x, p.y + dy);
    match item {
        Item::Line {
            from,
            to,
            width,
            paint,
        } => Item::Line {
            from: mv(from),
            to: mv(to),
            width,
            paint,
        },
        Item::Rect {
            at,
            width,
            height,
            paint,
        } => Item::Rect {
            at: mv(at),
            width,
            height,
            paint,
        },
        Item::Text {
            text,
            at,
            size,
            font,
            anchor,
            paint,
        } => Item::Text {
            text,
            at: mv(at),
            size,
            font,
            anchor,
            paint,
        },
        other => other,
    }
}

impl Tab for ChatTab {
    fn content(&self) -> TabContent {
        let measure = &*self.measure;
        let mut st = self.state();
        let (w, h) = st.size;
        let (entries, running, status, waiting) = {
            let c = self.shared.lock();
            (
                c.entries.clone(),
                c.running,
                c.status.clone(),
                c.waiting.clone(),
            )
        };
        let mut items = vec![text::rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
            Paint::Page,
        )];
        let mut hits = Vec::new();
        // The bottom: the box, then the buttons' row.
        let inner = TextBox::inner(w - 2.0 * PAD);
        let box_h = st.input.height(measure, inner, 6);
        let buttons_y = h - PAD - BUTTONS;
        let input = Rect {
            x: PAD,
            y: buttons_y - PAD - box_h,
            w: w - 2.0 * PAD,
            h: box_h,
        };
        let top = input.y - PAD;
        // The conversation, laid out from its top, shown from its bottom.
        let mut blocks: Vec<Block> = entries
            .iter()
            .enumerate()
            .map(|(i, e)| block(measure, e, i, w, st.hover))
            .collect();
        if let Some((server, _)) = &waiting {
            let mut b = Vec::new();
            let text_h = paragraph(
                measure,
                &format!(
                    "{server} isn't on this computer. Sending this question sends the conversation and what the model reads of the score there. Allow it?"
                ),
                pen(PAD, 0.0, BODY, w - 2.0 * PAD, Paint::Ink),
                &mut b,
            );
            let allow = Rect {
                x: PAD,
                y: text_h + 4.0,
                w: 70.0,
                h: 24.0,
            };
            let deny = Rect {
                x: PAD + 78.0,
                y: text_h + 4.0,
                w: 70.0,
                h: 24.0,
            };
            button(
                "Allow",
                allow,
                st.hover == Some(Action::Allow),
                true,
                &mut b,
            );
            button(
                "Cancel",
                deny,
                st.hover == Some(Action::Deny),
                false,
                &mut b,
            );
            blocks.push(Block {
                items: b,
                height: text_h + 32.0,
                hits: vec![(allow, Action::Allow), (deny, Action::Deny)],
            });
        }
        if running && !status.is_empty() {
            let mut b = Vec::new();
            let hgt = paragraph(
                measure,
                &status,
                pen(PAD, 0.0, SMALL, w - 2.0 * PAD, Paint::Muted),
                &mut b,
            );
            blocks.push(Block {
                items: b,
                height: hgt,
                hits: Vec::new(),
            });
        }
        if blocks.is_empty() {
            let s = Settings::load(&self.host);
            let model = if s.model.trim().is_empty() {
                "no model chosen yet".to_owned()
            } else {
                format!("model {}", s.model.trim())
            };
            let hint = format!(
                "Ask about the score, or ask for changes: \"write a bass line for bars 1 to 8\", \"fit every bar to its time signature\".\n\nServer: {} ({model}). Plugins → AI Settings… chooses another.",
                s.server()
            );
            let mut b = Vec::new();
            let hgt = paragraph(
                measure,
                &hint,
                pen(PAD, 0.0, BODY, w - 2.0 * PAD, Paint::Muted),
                &mut b,
            );
            blocks.push(Block {
                items: b,
                height: hgt,
                hits: Vec::new(),
            });
        }
        let gap = 10.0;
        let room = top - HEADER;
        let total: f32 = blocks.iter().map(|b| b.height + gap).sum::<f32>() + PAD;
        st.most = (total - room).max(0.0);
        st.scroll = st.scroll.clamp(0.0, st.most);
        // Short conversations start at the top, long ones show their end.
        let mut y = HEADER
            + PAD
            + if total < room {
                0.0
            } else {
                room - total + st.scroll
            };
        for b in blocks {
            if y + b.height > 0.0 && y < top + 50.0 {
                items.extend(b.items.into_iter().map(|i| shifted(i, y)));
                for (r, a) in b.hits {
                    let r = Rect { y: r.y + y, ..r };
                    if r.y + r.h <= top && r.y >= HEADER {
                        hits.push((r, a));
                    }
                }
            }
            y += b.height + gap;
        }
        // The top row covers what the conversation drew above its room.
        items.push(text::rect(
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h: HEADER,
            },
            Paint::Page,
        ));
        let label = "New Chat";
        let action = Action::NewChat;
        let width = self
            .measure
            .edges(label, BODY)
            .last()
            .copied()
            .unwrap_or(60.0)
            + 16.0;
        let r = Rect {
            x: PAD,
            y: 4.0,
            w: width,
            h: HEADER - 8.0,
        };
        let lit = st.hover == Some(action);
        items.extend(text::frame(
            r,
            1.0,
            if lit { Paint::Accent } else { Paint::Muted },
        ));
        items.push(text::text(
            label,
            r.x + 8.0,
            r.y + r.h / 2.0 + BODY * 0.35,
            BODY,
            if lit { Paint::Accent } else { Paint::Ink },
        ));
        hits.push((r, action));
        items.push(Item::Line {
            from: Point::new(0.0, HEADER - 0.5),
            to: Point::new(w, HEADER - 0.5),
            width: 1.0,
            paint: Paint::Muted,
        });
        // The bottom panel covers what the conversation drew past its room.
        items.push(text::rect(
            Rect {
                x: 0.0,
                y: top,
                w,
                h: h - top,
            },
            Paint::Page,
        ));
        if st.most > 0.0 && st.scroll > 0.0 {
            items.push(text::text(
                "More below: scroll down",
                PAD,
                top - 2.0,
                SMALL,
                Paint::Accent,
            ));
        }
        let focused = st.focused;
        st.input
            .draw(input, focused, "Ask the AI…", measure, &mut items);
        st.input_rect = input;
        hits.push((input, Action::Input));
        let send = Rect {
            x: w - PAD - 80.0,
            y: buttons_y,
            w: 80.0,
            h: BUTTONS,
        };
        if running {
            button(
                "Stop",
                send,
                st.hover == Some(Action::Stop),
                false,
                &mut items,
            );
            hits.push((send, Action::Stop));
        } else {
            let ready = !st.input.text.trim().is_empty();
            button(
                "Send",
                send,
                st.hover == Some(Action::Send),
                ready,
                &mut items,
            );
            hits.push((send, Action::Send));
        }
        let cursor = match st.hover {
            Some(Action::Input) => Cursor::Text,
            Some(_) => Cursor::Pointer,
            None => Cursor::Default,
        };
        st.hits = hits;
        TabContent {
            rows: vec![Row {
                widgets: vec![Widget::Surface(Surface {
                    id: SURFACE.into(),
                    height: 0.0,
                    items,
                    keys: true,
                    cursor,
                })],
            }],
        }
    }

    fn on(&self, widget: &str, event: TabEvent) -> Result<(), Error> {
        if widget != SURFACE {
            return Ok(());
        }
        let measure = &*self.measure;
        let mut st = self.state();
        let hit = |st: &State, at: Point| {
            st.hits
                .iter()
                .find(|(r, _)| r.contains(at))
                .map(|(_, a)| *a)
        };
        match event {
            TabEvent::Resized { width, height } => st.size = (width.max(80.0), height.max(80.0)),
            TabEvent::Focus(on) => st.focused = on,
            TabEvent::Wheel { dy, .. } => {
                st.scroll = (st.scroll - dy).clamp(0.0, st.most);
            }
            TabEvent::Pointer(p) => match p.kind {
                PointerKind::Down => {
                    let action = hit(&st, p.at);
                    st.pressed = action;
                    if action == Some(Action::Input) {
                        let r = st.input_rect;
                        st.input.click(p.at, r, measure, false);
                    }
                }
                PointerKind::Drag => {
                    if st.pressed == Some(Action::Input) {
                        let r = st.input_rect;
                        st.input.drag(p.at, r, measure);
                    }
                }
                PointerKind::Up => {
                    let action = hit(&st, p.at);
                    if let Some(a) =
                        action.filter(|a| Some(*a) == st.pressed && *a != Action::Input)
                    {
                        self.act(a, &mut st);
                    }
                    st.pressed = None;
                }
                PointerKind::Move => st.hover = hit(&st, p.at),
                PointerKind::Leave => st.hover = None,
            },
            TabEvent::Typed(s) => {
                st.input.typed(&s);
            }
            TabEvent::Key(k) => match k.name {
                KeyName::PageUp => st.scroll = (st.scroll + 200.0).min(st.most),
                KeyName::PageDown => st.scroll = (st.scroll - 200.0).max(0.0),
                KeyName::Escape if self.shared.lock().running => self.shared.stop(),
                _ => {
                    let inner = TextBox::inner(st.input_rect.w);
                    match st.input.key(k, measure, inner) {
                        Outcome::Submit => self.submit(&mut st),
                        Outcome::Copy(t) => {
                            let _ = self.host.copy(&t);
                        }
                        Outcome::Changed | Outcome::Nothing => {}
                    }
                }
            },
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_is_left_out_of_what_shows() {
        assert_eq!(visible("<think>hmm</think>\nThe answer."), "The answer.");
        assert_eq!(visible("A <think>x</think>B"), "A B");
        assert_eq!(visible("<think>still thinking"), "");
        assert_eq!(visible("plain"), "plain");
    }
}
