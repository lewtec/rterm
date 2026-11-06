use eframe::egui;

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use rterm::attach::TabState;
use rterm::backend::Backend;
use rterm::hud::{FAIL_CLEAR_NANOSECONDS, PasteHud};
use rterm::paste::{self, Gesture, ImagePasteError};
use rterm::place::Place;
use rterm::store::{AppStore, PlaceEditorState};
use uuid::Uuid;

use crate::clip;
use crate::keys::{self, Action, Input, NamedKey};
use crate::session::{Pump, Session};
use crate::sleep_watch::SleepWatch;

const ROW_HEIGHT: f32 = 28.0;
const TRAFFIC_WIDTH: f32 = 78.0;

struct PasteRun {
    place_id: Uuid,
    generation: u32,
    token: u64,
    hud: Option<PasteHud>,
    fail_until: Option<Instant>,
    rx: Option<Receiver<PasteMsg>>,
}

enum PasteMsg {
    Progress {
        token: u64,
        sent: usize,
        total: usize,
    },
    Done {
        token: u64,
        result: Result<String, ImagePasteError>,
    },
}

pub struct RtermApp {
    store: AppStore,
    sessions: HashMap<Uuid, Session>,
    sleep: SleepWatch,
    fill: bool,
    windowed_size: Option<egui::Vec2>,
    notch: Option<rterm::notch::FillSplit>,
    mac_fill: Option<crate::fill::Backup>,
    last_cell: Option<(u16, u16)>,
    wheel_remainder: f32,
    ime_terminal: bool,
    drag_from: Option<Uuid>,
    tab_rects: Vec<(Uuid, egui::Rect)>,
    paste: PasteRun,
    paste_token: u64,
    lock: Option<std::fs::File>,
}

impl RtermApp {
    pub fn new(lock: Option<std::fs::File>) -> Self {
        Self {
            store: AppStore::system(),
            sessions: HashMap::new(),
            sleep: SleepWatch::start(),
            fill: false,
            windowed_size: None,
            notch: None,
            mac_fill: None,
            last_cell: None,
            wheel_remainder: 0.0,
            ime_terminal: false,
            drag_from: None,
            tab_rects: Vec::new(),
            paste: PasteRun {
                place_id: Uuid::nil(),
                generation: 0,
                token: 0,
                hud: None,
                fail_until: None,
                rx: None,
            },
            paste_token: 0,
            lock,
        }
    }

    fn pump(&mut self) {
        if self.sleep.poll() {
            self.store.drop_all_connections();
        }
        self.store.poll_fs();
        self.store.consume_pending_place();
        self.pump_paste();
        let mut notes = Vec::new();
        for (id, session) in self.sessions.iter_mut() {
            let generation = session.generation();
            for event in session.pump() {
                notes.push((*id, generation, event));
            }
        }
        for (id, generation, event) in notes {
            let current = self
                .store
                .attaches
                .get(&id)
                .map(|attach| attach.generation());
            if current != Some(generation) {
                continue;
            }
            match event {
                Pump::Output => self.store.note_tty(id),
                Pump::Trace(line) => self.store.ingest_trace(id, &line),
                Pump::Timeout => self.store.note_connect_timeout(id, Some(generation)),
                Pump::Exit(code) => self.store.handle_exit(id, code, Some(generation)),
            }
        }
        self.sync_sessions();
    }

    fn sync_sessions(&mut self) {
        let wanted: Vec<(Uuid, u32, Place)> = self
            .store
            .places
            .iter()
            .filter_map(|place| {
                let attach = self.store.attaches.get(&place.id)?;
                if !attach.shows_pane() {
                    return None;
                }
                Some((place.id, attach.generation(), place.clone()))
            })
            .collect();
        self.sessions.retain(|id, session| {
            wanted.iter().any(|(want_id, generation, _)| {
                want_id == id && *generation == session.generation()
            })
        });
        for (id, generation, place) in wanted {
            if self.sessions.get(&id).map(Session::generation) == Some(generation) {
                continue;
            }
            match Session::start(&place, generation) {
                Ok(session) => {
                    self.sessions.insert(id, session);
                }
                Err(_) => self.store.handle_exit(id, Some(1), Some(generation)),
            }
        }
    }

    fn pump_paste(&mut self) {
        if let Some(until) = self.paste.fail_until
            && Instant::now() >= until
        {
            self.paste.hud = None;
            self.paste.fail_until = None;
        }
        let Some(rx) = self.paste.rx.as_ref() else {
            return;
        };
        while let Ok(msg) = rx.try_recv() {
            match msg {
                PasteMsg::Progress { token, sent, total } if token == self.paste.token => {
                    let local = self
                        .store
                        .places
                        .iter()
                        .find(|place| place.id == self.paste.place_id)
                        .is_some_and(Place::is_local);
                    self.paste.hud = Some(PasteHud::working(local, sent, total));
                }
                PasteMsg::Done { token, result } if token == self.paste.token => {
                    self.paste.rx = None;
                    match result {
                        Ok(path) => {
                            self.paste.hud = None;
                            self.write_place(self.paste.place_id, path.as_bytes());
                        }
                        Err(_) => self.fail_paste(),
                    }
                    break;
                }
                _ => {}
            }
        }
    }

    fn fail_paste(&mut self) {
        clip::beep();
        self.paste.hud = Some(PasteHud::failure());
        self.paste.fail_until = Some(Instant::now() + Duration::from_nanos(FAIL_CLEAR_NANOSECONDS));
    }

    fn write_selected(&mut self, bytes: &[u8]) {
        if let Some(id) = self.store.selected_id {
            self.write_place(id, bytes);
        }
    }

    fn write_place(&mut self, id: Uuid, bytes: &[u8]) {
        if let Some(session) = self.sessions.get_mut(&id) {
            session.reveal_live();
            session.write(bytes);
        }
    }

    fn do_paste(&mut self, gesture: Gesture) {
        let Some(place) = self.store.selected_place().cloned() else {
            return;
        };
        let Some(generation) = self
            .store
            .attaches
            .get(&place.id)
            .map(|attach| attach.generation())
        else {
            return;
        };
        let snap = clip::snapshot();
        match keys::paste_effect(snap.has_image, snap.has_text, place.is_local(), gesture) {
            keys::PasteEffect::Image => {
                if snap.png.len() > paste::MAX_BYTES {
                    self.fail_paste();
                    return;
                }
                self.start_image(place, generation, snap.png);
            }
            keys::PasteEffect::Text => self.write_place(place.id, snap.text.as_bytes()),
            keys::PasteEffect::Byte(byte) => self.write_place(place.id, &[byte]),
            keys::PasteEffect::None => {}
        }
    }

    fn start_image(&mut self, place: Place, generation: u32, png: Vec<u8>) {
        self.paste_token = self.paste_token.wrapping_add(1);
        let token = self.paste_token;
        let local = place.is_local();
        self.paste.place_id = place.id;
        self.paste.generation = generation;
        self.paste.token = token;
        self.paste.fail_until = None;
        self.paste.hud = Some(PasteHud::working(local, 0, png.len()));
        let (tx, rx) = mpsc::channel();
        self.paste.rx = Some(rx);
        let id = rterm::uuidv7::generate();
        std::thread::spawn(move || {
            let progress = tx.clone();
            let result = paste::deliver(
                &png,
                &place,
                id,
                &mut |sent, total| {
                    let _ = progress.send(PasteMsg::Progress { token, sent, total });
                },
                None,
            );
            let _ = tx.send(PasteMsg::Done { token, result });
        });
    }

    fn apply(&mut self, ctx: &egui::Context, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Write(bytes) => self.write_selected(&bytes),
                Action::AddPlace => self.store.begin_add(),
                Action::Reconnect => self.store.reconnect_selected(),
                Action::SelectIndex(index) => self.store.select_place_at(index),
                Action::ToggleFill => self.toggle_fill(ctx),
                Action::Paste(gesture) => self.do_paste(gesture),
                Action::ExitFill => {
                    if self.fill {
                        self.toggle_fill(ctx);
                    }
                }
                Action::CloseEditor => self.store.editor = None,
                Action::SaveEditor => {
                    if self.store.editor.as_ref().is_some_and(can_save) {
                        self.store.save_editor();
                    }
                }
            }
        }
    }

    fn toggle_fill(&mut self, ctx: &egui::Context) {
        if self.fill {
            self.fill = false;
            self.leave_fill(ctx);
        } else {
            self.windowed_size = Some(ctx.input(|input| input.screen_rect().size()));
            self.fill = true;
            self.enter_fill(ctx);
        }
    }

    fn enter_fill(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_os = "macos"))]
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        #[cfg(target_os = "macos")]
        {
            let _ = ctx;
        }
    }

    fn leave_fill(&mut self, ctx: &egui::Context) {
        #[cfg(not(target_os = "macos"))]
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
            if let Some(size) = self.windowed_size {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
        }
        #[cfg(target_os = "macos")]
        {
            let _ = ctx;
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let events = ctx.input(|input| input.events.clone());
        // A commit replaces text events from the same frame. See `frame_text`.
        let saw_commit = events.iter().any(|event| {
            matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(text)) if !text.is_empty())
        });
        let mut batch = Vec::new();
        for event in events {
            match event {
                egui::Event::Ime(egui::ImeEvent::Commit(text)) if saw_commit => {
                    batch.extend(keys::frame_text(&[text], &[]).into_iter().map(Input::Text));
                }
                egui::Event::Text(text) if !saw_commit => {
                    batch.extend(keys::frame_text(&[], &[text]).into_iter().map(Input::Text));
                }
                egui::Event::Paste(text) => batch.push(Input::Pasted(text)),
                egui::Event::Key { .. } if saw_commit => {}
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => {
                    if let Some(named) = named_key(key) {
                        batch.push(Input::Key {
                            key: named,
                            pressed,
                            repeat,
                            ctrl: modifiers.ctrl,
                            mac_cmd: modifiers.mac_cmd,
                            alt: modifiers.alt,
                            shift: modifiers.shift,
                        });
                    }
                }
                _ => {}
            }
        }
        let (kitty_flags, app_cursor) = self.terminal_mode();
        let actions = keys::interpret_with(
            &batch,
            self.fill,
            self.store.editor.is_some(),
            ctx.wants_keyboard_input(),
            kitty_flags,
            app_cursor,
        );
        self.apply(ctx, actions);
    }
}

impl eframe::App for RtermApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.notch = crate::fill::sync(self.fill, &mut self.mac_fill, frame);
        let terminal_ime = self.store.editor.is_none();
        if self.ime_terminal != terminal_ime {
            self.ime_terminal = terminal_ime;
            ctx.send_viewport_cmd(egui::ViewportCommand::IMEPurpose(if terminal_ime {
                egui::IMEPurpose::Terminal
            } else {
                egui::IMEPurpose::Normal
            }));
        }
        self.pump();
        self.handle_input(ctx);
        self.tab_rects.clear();
        egui::CentralPanel::default().show(ctx, |ui| {
            self.chrome(ui);
            if let Some(message) = self.store.catalog_error.clone() {
                self.banner(ui, &message);
            }
            self.pane(ui);
        });
        self.editor_window(ctx);
        self.sync_sessions();
        if self.paste.rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if !self.sessions.is_empty() || self.paste.hud.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        let _ = &self.lock;
    }
}

impl RtermApp {
    fn chrome(&mut self, ui: &mut egui::Ui) {
        let row_h = self
            .notch
            .map(|split| split.row_height as f32)
            .unwrap_or(ROW_HEIGHT);
        let bar = ui.available_rect_before_wrap();
        let chrome_rect = egui::Rect::from_min_size(bar.min, egui::vec2(bar.width(), row_h));
        ui.painter()
            .rect_filled(chrome_rect, 0.0, egui::Color32::from_rgb(36, 36, 38));
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let places = self.store.places.clone();
                if let Some(split) = self.notch {
                    if self.fill {
                        self.traffic(ui);
                    }
                    self.place_tabs(ui, &places, split.left_cap as f32, "tabs");
                    if split.notch_width > 0.5 {
                        ui.allocate_exact_size(
                            egui::vec2(split.notch_width as f32, row_h),
                            egui::Sense::hover(),
                        );
                    }
                    let rest = ui.available_width().max(0.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(rest, row_h),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| self.place_buttons(ui),
                    );
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.place_buttons(ui);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if self.fill {
                                self.traffic(ui);
                            }
                            let width = ui.available_width().max(0.0);
                            self.place_tabs(ui, &places, width, "tabs");
                        });
                    });
                }
            },
        );
        if ui.input(|input| input.pointer.any_released())
            && let Some(from) = self.drag_from.take()
            && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
            && let Some(onto) = self
                .tab_rects
                .iter()
                .find(|(_, rect)| rect.contains(pos))
                .map(|(id, _)| *id)
        {
            self.store.move_place(from, onto);
        }
        ui.separator();
    }

    fn place_buttons(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        if ui.button("+").on_hover_text("Add Place").clicked() {
            self.store.begin_add();
        }
        ui.menu_button("Place", |ui| self.place_menu(ui));
    }

    fn place_tabs(&mut self, ui: &mut egui::Ui, places: &[Place], cap: f32, salt: &str) {
        let width = cap.max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.available_height()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt(salt)
                    .scroll_bar_visibility(
                        egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                    )
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for place in places {
                                self.tab(ui, place);
                            }
                        });
                    });
            },
        );
    }

    fn terminal_mode(&self) -> (u16, bool) {
        let Some(id) = self.store.selected_id else {
            return (0, false);
        };
        let Some(session) = self.sessions.get(&id) else {
            return (0, false);
        };
        (session.kitty_flags(), session.screen().application_cursor())
    }

    fn traffic(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            egui::vec2(TRAFFIC_WIDTH, ROW_HEIGHT),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let ctx = ui.ctx().clone();
                if dot(ui, egui::Color32::from_rgb(255, 94, 87)).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if dot(ui, egui::Color32::from_rgb(255, 189, 46)).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                if dot(ui, egui::Color32::from_rgb(41, 199, 64)).clicked() {
                    self.toggle_fill(&ctx);
                }
            },
        );
    }

    fn place_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Add Place…").clicked() {
            self.store.begin_add();
            ui.close_menu();
        }
        if ui
            .add_enabled(
                self.store.selected_id.is_some(),
                egui::Button::new("Reconnect"),
            )
            .clicked()
        {
            self.store.reconnect_selected();
            ui.close_menu();
        }
        ui.separator();
        for index in 0..10 {
            let title = self
                .store
                .places
                .get(index)
                .map(|place| place.display_label())
                .unwrap_or_else(|| format!("Place {}", index + 1));
            if ui
                .add_enabled(self.store.places.len() > index, egui::Button::new(title))
                .clicked()
            {
                self.store.select_place_at(index);
                ui.close_menu();
            }
        }
        ui.separator();
        if ui.button("Toggle Debug Overlay").clicked() {
            self.store.debug_overlay = !self.store.debug_overlay;
            ui.close_menu();
        }
        if ui.button("Reveal Catalog").clicked() {
            self.store.reveal_catalog();
            ui.close_menu();
        }
    }

    fn tab(&mut self, ui: &mut egui::Ui, place: &Place) {
        let selected = self.store.selected_id == Some(place.id);
        let state = self.store.tab_state(place.id);
        let fill = if selected {
            egui::Color32::from_rgba_unmultiplied(80, 140, 220, 70)
        } else {
            egui::Color32::TRANSPARENT
        };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(tab_width(place) as f32, 22.0),
            egui::Sense::click_and_drag(),
        );
        self.tab_rects.push((place.id, rect));
        ui.painter().rect_filled(rect, 10.0, fill);
        if selected {
            ui.painter().rect_stroke(
                rect,
                10.0,
                egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 140, 220)),
                egui::StrokeKind::Inside,
            );
        }
        let dot_center = rect.left_center() + egui::vec2(12.0, 0.0);
        paint_status(ui, dot_center, &state);
        ui.painter().text(
            rect.left_center() + egui::vec2(20.0, 0.0),
            egui::Align2::LEFT_CENTER,
            place.display_label(),
            egui::FontId::proportional(13.0),
            egui::Color32::WHITE,
        );
        response.clone().on_hover_text(place.display_label());
        if response.clicked() {
            self.store.select(place.id);
        }
        if response.drag_started() {
            self.drag_from = Some(place.id);
        }
        let place = place.clone();
        response.context_menu(|ui| {
            if ui.button("Reconnect").clicked() {
                self.store.reconnect(place.id);
                ui.close_menu();
            }
            if ui.button("Edit").clicked() {
                self.store.begin_edit(&place);
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                self.store.delete(place.id);
                ui.close_menu();
            }
        });
    }

    fn banner(&mut self, ui: &mut egui::Ui, message: &str) {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(255, 180, 60), message);
            if ui.button("Reveal").clicked() {
                self.store.reveal_catalog();
            }
            if ui.button("Reload").clicked() {
                self.store.reload_from_disk();
            }
        });
    }

    fn pane(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_rgb(18, 18, 18));
        let response = ui.allocate_rect(rect, egui::Sense::click());
        if self.store.places.is_empty() {
            center_text(ui, rect, "No places", "Add a place to attach later.");
            return;
        }
        let Some(place) = self.store.selected_place().cloned() else {
            return;
        };
        let phase = self
            .store
            .attaches
            .get(&place.id)
            .map(|attach| attach.phase().clone());
        let show = self
            .store
            .attaches
            .get(&place.id)
            .is_some_and(|attach| attach.shows_pane());
        if show {
            let font = egui::FontId::monospace(14.0);
            let galley = ui.ctx().fonts(|fonts| {
                fonts.layout_no_wrap("M".to_owned(), font.clone(), egui::Color32::WHITE)
            });
            let char_w = galley.size().x.max(1.0);
            let char_h = galley.size().y.max(1.0);
            if let Some(session) = self.sessions.get_mut(&place.id) {
                session.fit(rect.width(), rect.height(), char_w, char_h);
                paint_screen(ui, rect, session.screen(), &font, char_w, char_h);
                if self.store.editor.is_none() {
                    mark_ime(ui, ime_cursor(rect, session.screen(), char_w, char_h));
                }
                self.pointer_pane(ui, &response, rect, place.id, char_w, char_h);
            }
            if let Some(progress) = self.store.connect_progress(place.id)
                && progress.is_visible()
            {
                paint_connect(ui, rect, progress.headline(), progress.crumbs());
            }
            if let Some(hud) = self.paste.hud.clone()
                && self.paste.place_id == place.id
            {
                paint_hud(ui, rect, &hud);
            }
        } else {
            match phase {
                Some(rterm::attach::Phase::Failed(message)) => {
                    if failure_overlay(ui, rect, &message).clicked() {
                        self.store.reconnect(place.id);
                    }
                }
                _ => center_text(ui, rect, "Idle", "Click this tab or Reconnect to attach."),
            }
        }
        if self.store.debug_overlay && failure_overlay(ui, rect, "debug overlay").clicked() {
            self.store.reconnect(place.id);
        }
    }

    fn pointer_pane(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        place_id: Uuid,
        char_w: f32,
        char_h: f32,
    ) {
        let pos = ui
            .input(|input| input.pointer.hover_pos().or(input.pointer.interact_pos()))
            .filter(|pos| rect.contains(*pos));
        let Some(pos) = pos else {
            self.wheel_remainder = 0.0;
            return;
        };
        let col = ((pos.x - rect.min.x) / char_w).floor().max(0.0) as u16;
        let row = ((pos.y - rect.min.y) / char_h).floor().max(0.0) as u16;
        let pressed = ui.input(|input| input.pointer.primary_pressed());
        let released = ui.input(|input| input.pointer.primary_released());
        let down = ui.input(|input| input.pointer.primary_down());
        let moved = self.last_cell != Some((col, row));
        let Some(session) = self.sessions.get_mut(&place_id) else {
            return;
        };
        wheel_pane(
            &mut self.wheel_remainder,
            ui,
            session,
            col,
            row,
            char_h,
            rect.height(),
        );
        let mode = session.screen().mouse_protocol_mode();
        let encoding = session.screen().mouse_protocol_encoding();
        if mode != vt100::MouseProtocolMode::None {
            let click = if pressed {
                Some(rterm::click::Click::Press)
            } else if released {
                Some(rterm::click::Click::Release)
            } else if down && moved {
                Some(rterm::click::Click::Drag)
            } else if moved {
                Some(rterm::click::Click::Move)
            } else {
                None
            };
            if let Some(click) = click
                && let Some(bytes) = rterm::click::report(mode, encoding, click, 0, col, row)
            {
                session.write(&bytes);
            }
            self.last_cell = Some((col, row));
            return;
        }
        self.last_cell = Some((col, row));
        if response.clicked()
            && let Some(url) = rterm::click::link_at(session.screen(), row, col)
        {
            let _ = rterm::paths::open_directory(std::path::Path::new(&url));
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        } else if rterm::click::link_at(session.screen(), row, col).is_some() {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        }
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.store.editor.clone() else {
            return;
        };
        let title = if editor.origin_id.is_none() {
            "Add Place"
        } else {
            "Edit Place"
        };
        let mut open = true;
        let mut dismiss = false;
        let directory = self.store.places_directory().display().to_string();
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label("User");
                ui.text_edit_singleline(&mut editor.user);
                ui.label("Host");
                ui.text_edit_singleline(&mut editor.host);
                egui::ComboBox::from_label("Backend")
                    .selected_text(editor.backend.as_str())
                    .show_ui(ui, |ui| {
                        for backend in Backend::ALL {
                            ui.selectable_value(&mut editor.backend, backend, backend.as_str());
                        }
                    });
                if editor.backend != Backend::Herdr {
                    ui.label("Session");
                    ui.text_edit_singleline(&mut editor.session);
                }
                ui.label("Label");
                ui.text_edit_singleline(&mut editor.label);
                ui.label(directory);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        dismiss = true;
                    }
                    if ui
                        .add_enabled(can_save(&editor), egui::Button::new("Save"))
                        .clicked()
                    {
                        self.store.editor = Some(editor.clone());
                        self.store.save_editor();
                    }
                });
            });
        if dismiss {
            open = false;
        }
        if self.store.editor.is_some() {
            if open {
                self.store.editor = Some(editor);
            } else {
                self.store.editor = None;
            }
        }
    }
}

fn can_save(editor: &PlaceEditorState) -> bool {
    let user = editor.user.trim();
    let host = editor.host.trim();
    if !Place::is_loopback_host(host) && user.is_empty() {
        return false;
    }
    if editor.backend != Backend::Herdr && editor.session.trim().is_empty() {
        return false;
    }
    true
}

fn tab_width(place: &Place) -> f64 {
    keys::estimated_tab_width(&place.display_label())
}

fn named_key(key: egui::Key) -> Option<NamedKey> {
    Some(match key {
        egui::Key::Enter => NamedKey::Enter,
        egui::Key::Backspace => NamedKey::Backspace,
        egui::Key::Tab => NamedKey::Tab,
        egui::Key::Escape => NamedKey::Escape,
        egui::Key::ArrowUp => NamedKey::Up,
        egui::Key::ArrowDown => NamedKey::Down,
        egui::Key::ArrowLeft => NamedKey::Left,
        egui::Key::ArrowRight => NamedKey::Right,
        egui::Key::Home => NamedKey::Home,
        egui::Key::End => NamedKey::End,
        egui::Key::PageUp => NamedKey::PageUp,
        egui::Key::PageDown => NamedKey::PageDown,
        egui::Key::Delete => NamedKey::Delete,
        egui::Key::Insert => NamedKey::Insert,
        egui::Key::A => NamedKey::Letter(b'a'),
        egui::Key::B => NamedKey::Letter(b'b'),
        egui::Key::C => NamedKey::Letter(b'c'),
        egui::Key::D => NamedKey::Letter(b'd'),
        egui::Key::E => NamedKey::Letter(b'e'),
        egui::Key::F => NamedKey::Letter(b'f'),
        egui::Key::G => NamedKey::Letter(b'g'),
        egui::Key::H => NamedKey::Letter(b'h'),
        egui::Key::I => NamedKey::Letter(b'i'),
        egui::Key::J => NamedKey::Letter(b'j'),
        egui::Key::K => NamedKey::Letter(b'k'),
        egui::Key::L => NamedKey::Letter(b'l'),
        egui::Key::M => NamedKey::Letter(b'm'),
        egui::Key::N => NamedKey::Letter(b'n'),
        egui::Key::O => NamedKey::Letter(b'o'),
        egui::Key::P => NamedKey::Letter(b'p'),
        egui::Key::Q => NamedKey::Letter(b'q'),
        egui::Key::R => NamedKey::Letter(b'r'),
        egui::Key::S => NamedKey::Letter(b's'),
        egui::Key::T => NamedKey::Letter(b't'),
        egui::Key::U => NamedKey::Letter(b'u'),
        egui::Key::V => NamedKey::Letter(b'v'),
        egui::Key::W => NamedKey::Letter(b'w'),
        egui::Key::X => NamedKey::Letter(b'x'),
        egui::Key::Y => NamedKey::Letter(b'y'),
        egui::Key::Z => NamedKey::Letter(b'z'),
        egui::Key::Num0 => NamedKey::Digit(0),
        egui::Key::Num1 => NamedKey::Digit(1),
        egui::Key::Num2 => NamedKey::Digit(2),
        egui::Key::Num3 => NamedKey::Digit(3),
        egui::Key::Num4 => NamedKey::Digit(4),
        egui::Key::Num5 => NamedKey::Digit(5),
        egui::Key::Num6 => NamedKey::Digit(6),
        egui::Key::Num7 => NamedKey::Digit(7),
        egui::Key::Num8 => NamedKey::Digit(8),
        egui::Key::Num9 => NamedKey::Digit(9),
        _ => return None,
    })
}

fn dot(ui: &mut egui::Ui, color: egui::Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    ui.painter().circle_filled(rect.center(), 6.0, color);
    response
}

fn paint_status(ui: &egui::Ui, center: egui::Pos2, state: &TabState) {
    let color = match state {
        TabState::Idle => egui::Color32::GRAY,
        TabState::Attaching => egui::Color32::GRAY,
        TabState::Live => egui::Color32::from_rgb(40, 200, 80),
        TabState::Failed(_) => egui::Color32::from_rgb(220, 60, 60),
    };
    if matches!(state, TabState::Attaching) {
        let t = ui.input(|input| input.time);
        ui.painter()
            .circle_stroke(center, 4.0, egui::Stroke::new(1.5, color));
        let angle = t as f32 * 6.0;
        let tip = center + egui::vec2(angle.cos(), angle.sin()) * 4.0;
        ui.painter().circle_filled(tip, 1.5, color);
    } else {
        ui.painter().circle_filled(center, 3.5, color);
    }
}

fn center_text(ui: &egui::Ui, rect: egui::Rect, title: &str, body: &str) {
    let painter = ui.painter();
    painter.text(
        rect.center() + egui::vec2(0.0, -12.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(22.0),
        egui::Color32::WHITE,
    );
    painter.text(
        rect.center() + egui::vec2(0.0, 14.0),
        egui::Align2::CENTER_CENTER,
        body,
        egui::FontId::proportional(14.0),
        egui::Color32::LIGHT_GRAY,
    );
}

fn failure_overlay(ui: &mut egui::Ui, rect: egui::Rect, message: &str) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(message), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(180));
    ui.painter().text(
        rect.center() + egui::vec2(0.0, -16.0),
        egui::Align2::CENTER_CENTER,
        message,
        egui::FontId::proportional(18.0),
        egui::Color32::WHITE,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        "Click to attach again",
        egui::FontId::proportional(13.0),
        egui::Color32::LIGHT_GRAY,
    );
    response
}

fn paint_connect(ui: &egui::Ui, rect: egui::Rect, headline: &str, crumbs: &[String]) {
    let painter = ui.painter();
    painter.text(
        rect.center() + egui::vec2(0.0, -28.0),
        egui::Align2::CENTER_CENTER,
        headline,
        egui::FontId::proportional(18.0),
        egui::Color32::WHITE,
    );
    for (index, crumb) in crumbs.iter().enumerate() {
        painter.text(
            rect.center() + egui::vec2(0.0, index as f32 * 16.0),
            egui::Align2::CENTER_CENTER,
            crumb,
            egui::FontId::monospace(12.0),
            egui::Color32::from_white_alpha(180),
        );
    }
}

fn paint_hud(ui: &egui::Ui, rect: egui::Rect, hud: &PasteHud) {
    let text = hud.accessibility_text();
    let anchor = rect.right_bottom() + egui::vec2(-12.0, -12.0);
    ui.painter().text(
        anchor,
        egui::Align2::RIGHT_BOTTOM,
        text,
        egui::FontId::proportional(13.0),
        egui::Color32::WHITE,
    );
}

fn paint_screen(
    ui: &egui::Ui,
    rect: egui::Rect,
    screen: &vt100::Screen,
    font: &egui::FontId,
    char_w: f32,
    char_h: f32,
) {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();
    let painter = ui.painter();
    for row in 0..rows {
        let links = rterm::click::links_in_row(screen, row);
        let mut col = 0u16;
        while col < cols {
            let Some(cell) = screen.cell(row, col) else {
                col += 1;
                continue;
            };
            if cell.is_wide_continuation() {
                col += 1;
                continue;
            }
            let span = if cell.is_wide() { 2 } else { 1 };
            let origin = rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h);
            let cell_rect =
                egui::Rect::from_min_size(origin, egui::vec2(char_w * span as f32, char_h));
            let mut fg = color_of(cell.fgcolor(), egui::Color32::from_rgb(220, 220, 220));
            let mut bg = color_of(cell.bgcolor(), egui::Color32::TRANSPARENT);
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if bg != egui::Color32::TRANSPARENT {
                painter.rect_filled(cell_rect, 0.0, bg);
            }
            let text = cell.contents();
            if !text.is_empty() && text != " " {
                painter.text(origin, egui::Align2::LEFT_TOP, text, font.clone(), fg);
            }
            if links
                .iter()
                .any(|(start, end, _)| col >= *start && col <= *end)
            {
                painter.hline(
                    cell_rect.x_range(),
                    cell_rect.bottom() - 1.0,
                    egui::Stroke::new(1.0, fg),
                );
            }
            col += span;
        }
    }
    if screen.scrollback() == 0 {
        let (cursor_row, cursor_col) = screen.cursor_position();
        let cursor = rect.min + egui::vec2(cursor_col as f32 * char_w, cursor_row as f32 * char_h);
        painter.rect_filled(
            egui::Rect::from_min_size(cursor, egui::vec2(char_w, char_h)),
            0.0,
            egui::Color32::from_white_alpha(50),
        );
    }
}

fn ime_cursor(rect: egui::Rect, screen: &vt100::Screen, char_w: f32, char_h: f32) -> egui::Rect {
    let (row, col) = screen.cursor_position();
    let origin = if screen.scrollback() == 0 {
        rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h)
    } else {
        egui::pos2(rect.min.x, (rect.max.y - char_h).max(rect.min.y))
    };
    egui::Rect::from_min_size(origin, egui::vec2(char_w.max(1.0), char_h.max(1.0)))
}

fn mark_ime(ui: &egui::Ui, cursor: egui::Rect) {
    let to_global = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or_default();
    ui.ctx().output_mut(|output| {
        output.ime = Some(egui::output::IMEOutput {
            rect: to_global * cursor,
            cursor_rect: to_global * cursor,
        });
    });
}

fn wheel_pane(
    remainder: &mut f32,
    ui: &egui::Ui,
    session: &mut Session,
    col: u16,
    row: u16,
    char_h: f32,
    pane_h: f32,
) {
    let wheels: Vec<(egui::MouseWheelUnit, f32, bool)> = ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } => Some((*unit, delta.y, modifiers.shift)),
                _ => None,
            })
            .collect()
    });
    if wheels.is_empty() {
        return;
    }
    let page_rows = if char_h > 0.0 {
        (pane_h / char_h).max(1.0)
    } else {
        1.0
    };
    for (unit, delta_y, shift) in wheels {
        let lines =
            rterm::click::take_wheel_lines(remainder, wheel_unit(unit), delta_y, char_h, page_rows);
        let mouse = session.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None;
        let alternate = session.screen().alternate_screen();
        let Some(action) = rterm::click::wheel_action(mouse, alternate, shift, lines) else {
            continue;
        };
        match action {
            rterm::click::WheelAction::Report { button, times } => {
                let mode = session.screen().mouse_protocol_mode();
                let encoding = session.screen().mouse_protocol_encoding();
                for _ in 0..times {
                    if let Some(bytes) = rterm::click::report(
                        mode,
                        encoding,
                        rterm::click::Click::Press,
                        button,
                        col,
                        row,
                    ) {
                        session.write(&bytes);
                    }
                }
            }
            rterm::click::WheelAction::Cursor { up, times } => {
                let key = if up { NamedKey::Up } else { NamedKey::Down };
                let app_cursor = session.screen().application_cursor();
                if let Some(bytes) = keys::special_bytes(key, false, app_cursor) {
                    for _ in 0..times {
                        session.write(&bytes);
                    }
                }
            }
            rterm::click::WheelAction::Scroll(rows) => session.scroll_by(rows),
        }
    }
}

fn wheel_unit(unit: egui::MouseWheelUnit) -> rterm::click::WheelUnit {
    match unit {
        egui::MouseWheelUnit::Point => rterm::click::WheelUnit::Point,
        egui::MouseWheelUnit::Line => rterm::click::WheelUnit::Line,
        egui::MouseWheelUnit::Page => rterm::click::WheelUnit::Page,
    }
}

fn color_of(color: vt100::Color, fallback: egui::Color32) -> egui::Color32 {
    match color {
        vt100::Color::Default => fallback,
        vt100::Color::Idx(index) => ansi(index),
        vt100::Color::Rgb(red, green, blue) => egui::Color32::from_rgb(red, green, blue),
    }
}

fn ansi(index: u8) -> egui::Color32 {
    const BASIC: [[u8; 3]; 16] = [
        [0, 0, 0],
        [205, 0, 0],
        [0, 205, 0],
        [205, 205, 0],
        [0, 0, 238],
        [205, 0, 205],
        [0, 205, 205],
        [229, 229, 229],
        [127, 127, 127],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [92, 92, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    if index < 16 {
        let item = BASIC[index as usize];
        return egui::Color32::from_rgb(item[0], item[1], item[2]);
    }
    if (16..232).contains(&index) {
        let value = index - 16;
        let step = |part: u8| {
            if part == 0 { 0 } else { 55 + 40 * part }
        };
        return egui::Color32::from_rgb(step(value / 36), step((value % 36) / 6), step(value % 6));
    }
    let gray = 8 + 10 * (index.saturating_sub(232));
    egui::Color32::from_rgb(gray, gray, gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_requires_a_remote_user_and_a_session() {
        let mut editor = PlaceEditorState {
            origin_id: None,
            user: String::new(),
            host: "remote.example".into(),
            backend: Backend::Tmux,
            session: String::new(),
            label: String::new(),
        };
        assert!(!can_save(&editor));
        editor.user = "ada".into();
        assert!(!can_save(&editor));
        editor.session = "dev".into();
        assert!(can_save(&editor));
        editor.host.clear();
        editor.user.clear();
        editor.backend = Backend::Herdr;
        assert!(can_save(&editor));
    }
}
