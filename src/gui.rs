use eframe::egui;
use eframe::egui::emath::GuiRounding;

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use rterm::attach::TabState;
use rterm::backend::Backend;
use rterm::hud::{FAIL_CLEAR_NANOSECONDS, PasteHud};
use rterm::paste::{self, Gesture, ImagePasteError};
use rterm::place::Place;
use rterm::probe::{self, Choice, ChooseError, Discovered};
use rterm::route::{self, Route, RouteKind};
use rterm::store::{AppStore, PlaceEditorState};
use uuid::Uuid;

use crate::clip;
use crate::keys::{self, Action, Input, NamedKey};
use crate::session::{Pump, Session};
use crate::sleep_watch::SleepWatch;

const ROW_HEIGHT: f32 = 28.0;
const TRAFFIC_WIDTH: f32 = 78.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    Appearance,
    Places,
}

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

struct PendingRoute {
    generation: u32,
    rx: Receiver<Result<Choice, ChooseError>>,
}

struct WizardRun {
    key: Option<Uuid>,
    rx: Receiver<WizardMsg>,
    results: Vec<Discovered>,
    done: bool,
}

enum WizardMsg {
    Found(Discovered),
    Done,
}

pub struct RtermApp {
    store: AppStore,
    sessions: HashMap<Uuid, Session>,
    pending_routes: HashMap<Uuid, PendingRoute>,
    active_routes: HashMap<Uuid, Route>,
    forced_routes: HashMap<Uuid, Route>,
    wizard: Option<WizardRun>,
    fingerprint_check: Option<Receiver<Result<Choice, ChooseError>>>,
    fingerprint_error: Option<String>,
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
    tab_scroll: f32,
    appearance: crate::appearance::Appearance,
    faces: Vec<crate::appearance::Face>,
    applied_font: Option<String>,
    terminal_font: Option<crate::liga::TerminalFont>,
    liga_cache: std::collections::HashMap<(u16, u32), LigaGlyph>,
    settings_open: bool,
    settings_tab: SettingsTab,
    settings_drag: Option<Uuid>,
    paste: PasteRun,
    paste_token: u64,
    lock: Option<std::fs::File>,
}

impl RtermApp {
    pub fn new(lock: Option<std::fs::File>) -> Self {
        Self {
            store: AppStore::system(),
            sessions: HashMap::new(),
            pending_routes: HashMap::new(),
            active_routes: HashMap::new(),
            forced_routes: HashMap::new(),
            wizard: None,
            fingerprint_check: None,
            fingerprint_error: None,
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
            tab_scroll: 0.0,
            appearance: crate::appearance::load(&crate::appearance::path()),
            faces: crate::appearance::faces(),
            applied_font: None,
            terminal_font: None,
            liga_cache: std::collections::HashMap::new(),
            settings_open: false,
            settings_tab: SettingsTab::Appearance,
            settings_drag: None,
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
        self.pending_routes.retain(|id, pending| {
            wanted
                .iter()
                .any(|(want_id, generation, _)| want_id == id && *generation == pending.generation)
        });
        self.active_routes
            .retain(|id, _| wanted.iter().any(|(want_id, _, _)| want_id == id));
        self.forced_routes
            .retain(|id, _| wanted.iter().any(|(want_id, _, _)| want_id == id));
        for (id, generation, place) in wanted {
            if self.sessions.get(&id).map(Session::generation) == Some(generation) {
                continue;
            }
            if place.is_local() {
                self.start_session(&place, None, generation);
                continue;
            }
            let received = self
                .pending_routes
                .get(&id)
                .map(|pending| pending.rx.try_recv());
            match received {
                None => {
                    let probe_place = place.clone();
                    let forced = self.forced_routes.get(&id).cloned();
                    let headline = if forced.is_some() {
                        "Trying the chosen route…"
                    } else if route::last_route(&place).is_some() {
                        "Trying the last route…"
                    } else {
                        "Finding a route…"
                    };
                    let (tx, rx) = mpsc::channel();
                    std::thread::spawn(move || {
                        let result = match forced {
                            Some(route) => probe::choose_route(&probe_place, &route),
                            None => probe::choose_preferred(&probe_place),
                        };
                        let _ = tx.send(result);
                    });
                    self.pending_routes
                        .insert(id, PendingRoute { generation, rx });
                    self.store.note_progress(id, headline);
                }
                Some(Err(mpsc::TryRecvError::Empty)) => {}
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.pending_routes.remove(&id);
                    self.forced_routes.remove(&id);
                    self.store
                        .fail_attach(id, "route discovery died".into(), Some(generation));
                }
                Some(Ok(Err(error))) => {
                    self.pending_routes.remove(&id);
                    self.forced_routes.remove(&id);
                    self.store
                        .fail_attach(id, error.to_string(), Some(generation));
                }
                Some(Ok(Ok(choice))) => {
                    self.pending_routes.remove(&id);
                    self.forced_routes.remove(&id);
                    if let Some(learned) = choice.learned.as_ref() {
                        self.store.learn_fingerprint(id, learned);
                    }
                    if let Err(err) = probe::install_known_hosts(&place, &choice) {
                        self.store.fail_attach(
                            id,
                            format!("could not store host key: {err}"),
                            Some(generation),
                        );
                        continue;
                    }
                    self.store.remember_route(id, &choice.route);
                    self.active_routes.insert(id, choice.route.clone());
                    self.start_session(&place, Some(&choice.route), generation);
                }
            }
        }
    }

    fn start_session(&mut self, place: &Place, route: Option<&Route>, generation: u32) {
        match Session::start(place, route, generation) {
            Ok(session) => {
                self.sessions.insert(place.id, session);
            }
            Err(err) => self.store.fail_attach(
                place.id,
                format!("could not start attach: {err}"),
                Some(generation),
            ),
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
        let route = self.active_routes.get(&place.id).cloned();
        std::thread::spawn(move || {
            let progress = tx.clone();
            let result = paste::deliver(
                &png,
                &place,
                route.as_ref(),
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
                Action::AddPlace => self.start_add(),
                Action::Reconnect => self.store.reconnect_selected(),
                Action::SelectIndex(index) => self.store.select_place_at(index),
                Action::ToggleFill => self.toggle_fill(ctx),
                Action::Paste(gesture) => self.do_paste(gesture),
                Action::ExitFill => {
                    if self.fill {
                        self.toggle_fill(ctx);
                    }
                }
                Action::CloseEditor => {
                    self.store.editor = None;
                    self.settings_open = false;
                }
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
        let sheet_open = self.store.editor.is_some() || self.settings_open;
        let actions = keys::interpret_with(
            &batch,
            self.fill,
            sheet_open,
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
        self.ensure_font(ctx);
        let sheet_open = self.store.editor.is_some() || self.settings_open;
        let terminal_ime = !sheet_open;
        if self.ime_terminal != terminal_ime {
            self.ime_terminal = terminal_ime;
            ctx.send_viewport_cmd(egui::ViewportCommand::IMEPurpose(if terminal_ime {
                egui::IMEPurpose::Terminal
            } else {
                egui::IMEPurpose::Normal
            }));
        }
        self.pump();
        crate::app_icon::install();
        crate::mac_paste::install();
        crate::mac_paste::set_editor_open(sheet_open);
        if crate::mac_paste::take_command_v() {
            self.do_paste(rterm::paste::Gesture::CommandV);
        }
        self.handle_input(ctx);
        self.tab_rects.clear();
        let panel = ctx.style().visuals.panel_fill;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(panel))
            .show(ctx, |ui| {
                self.chrome(ui);
                if let Some(message) = self.store.catalog_error.clone() {
                    self.banner(ui, &message);
                }
                self.pane(ui);
            });
        self.editor_window(ctx);
        self.settings_window(ctx);
        self.ensure_font(ctx);
        self.sync_sessions();
        if self.paste.rx.is_some()
            || self.fingerprint_check.is_some()
            || !self.pending_routes.is_empty()
            || self.wizard.as_ref().is_some_and(|run| !run.done)
        {
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
            .rect_filled(chrome_rect, 0.0, ui.visuals().panel_fill);
        let mut add_rect = egui::Rect::NOTHING;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let places = self.store.places.clone();
                if let Some(split) = self.notch {
                    if self.fill {
                        self.traffic(ui);
                    }
                    self.place_tabs(ui, &places, split.left_cap as f32);
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
                        |ui| {
                            add_rect = self.add_button(ui);
                            self.settings_button(ui);
                        },
                    );
                } else {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        add_rect = self.add_button(ui);
                        self.settings_button(ui);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if self.fill {
                                self.traffic(ui);
                            } else if cfg!(target_os = "macos") {
                                ui.add_space(TRAFFIC_WIDTH);
                            }
                            let width = ui.available_width().max(0.0);
                            self.place_tabs(ui, &places, width);
                        });
                    });
                }
            },
        );
        ui.painter().hline(
            chrome_rect.x_range(),
            chrome_rect.bottom() - 0.5,
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        if ui.input(|input| input.pointer.primary_pressed())
            && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
            && chrome_rect.contains(pos)
            && !add_rect.contains(pos)
            && !self.tab_rects.iter().any(|(_, rect)| rect.contains(pos))
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
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
    }

    fn add_button(&mut self, ui: &mut egui::Ui) -> egui::Rect {
        ui.add_space(8.0);
        let response = plus_button(ui, "Add Place");
        if response.clicked() {
            self.start_add();
        }
        let rect = response.rect;
        response.context_menu(|ui| self.place_menu(ui));
        rect
    }

    fn place_tabs(&mut self, ui: &mut egui::Ui, places: &[Place], cap: f32) {
        let width = cap.max(0.0);
        let height = 22.0;
        let (strip, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let widths: Vec<f32> = places.iter().map(|place| tab_width(place) as f32).collect();
        let gaps = TAB_GAP * places.len().saturating_sub(1) as f32;
        let content = widths.iter().sum::<f32>() + gaps;
        let max = (content - strip.width()).max(0.0);
        let mut wheel = egui::Vec2::ZERO;
        if ui.rect_contains_pointer(strip) {
            wheel = ui.input(|input| input.smooth_scroll_delta);
            if wheel != egui::Vec2::ZERO {
                // The strip owns the wheel while hovered, so no other scroll
                // area reacts to the same event.
                ui.input_mut(|input| input.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        // Trackpad horizontal scroll wins; a plain mouse wheel scrolls the tabs.
        let delta = if wheel.x != 0.0 { wheel.x } else { wheel.y };
        self.tab_scroll = (self.tab_scroll - delta).clamp(0.0, max);
        let painter = ui.painter().with_clip_rect(strip);
        let mut x = strip.left() - self.tab_scroll;
        for (place, width) in places.iter().zip(widths) {
            let rect =
                egui::Rect::from_min_size(egui::pos2(x, strip.top()), egui::vec2(width, height));
            x += width + TAB_GAP;
            let visible = rect.intersect(strip);
            if visible.width() <= 0.5 {
                continue;
            }
            self.tab(ui, &painter, place, rect, visible);
        }
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
            self.start_add();
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

    fn tab(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        place: &Place,
        rect: egui::Rect,
        visible: egui::Rect,
    ) {
        let selected = self.store.selected_id == Some(place.id);
        let state = self.store.tab_state(place.id);
        let response = ui.interact(
            visible,
            ui.id().with(("tab", place.id)),
            pointer_sense(true),
        );
        let fill = if selected {
            egui::Color32::from_rgba_unmultiplied(80, 140, 220, 70)
        } else if response.hovered() {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
        } else {
            egui::Color32::TRANSPARENT
        };
        self.tab_rects.push((place.id, visible));
        painter.rect_filled(rect, 10.0, fill);
        if selected {
            painter.rect_stroke(
                rect,
                10.0,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(80, 140, 220)),
                egui::StrokeKind::Inside,
            );
        }
        let dot_center = rect.left_center() + egui::vec2(12.0, 0.0);
        paint_status(painter, dot_center, &state);
        let text = egui::Rect::from_min_max(
            rect.left_center() + egui::vec2(20.0, -10.0),
            rect.right_center() + egui::vec2(-4.0, 10.0),
        )
        .intersect(visible);
        let painter = painter.with_clip_rect(text);
        painter.text(
            rect.left_center() + egui::vec2(20.0, 0.0),
            egui::Align2::LEFT_CENTER,
            place.display_label(),
            egui::FontId::proportional(13.0),
            ui.visuals().text_color(),
        );
        let hover = match place
            .fingerprint
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            Some(fingerprint) => {
                format!(
                    "{}\n{}",
                    place.display_label(),
                    short_fingerprint(fingerprint)
                )
            }
            None => place.display_label(),
        };
        response.clone().on_hover_text(hover);
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
            let candidates = route::candidates(&place);
            if !candidates.is_empty() {
                ui.menu_button("Reconnect as", |ui| {
                    let in_use = self.in_use_route(&place);
                    for route in candidates {
                        let current = in_use
                            .as_ref()
                            .is_some_and(|current| route::same_route(current, &route));
                        let label = format!("{} ({})", route.display(), route.kind.label());
                        if ui.radio(current, label).clicked() {
                            self.forced_routes.insert(place.id, route.clone());
                            self.store.reconnect(place.id);
                            ui.close_menu();
                        }
                    }
                });
            }
            if ui.button("Edit").clicked() {
                self.settings_open = false;
                self.wizard = None;
                self.store.begin_edit(&place);
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                self.store.delete(place.id);
                ui.close_menu();
            }
        });
    }

    /// The endpoint this place is attached through, or the one the next attach
    /// would try: the session's real route wins over the saved last route.
    fn in_use_route(&self, place: &Place) -> Option<Route> {
        self.active_routes
            .get(&place.id)
            .cloned()
            .or_else(|| route::current(place))
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
        let response = ui.allocate_rect(rect, pointer_sense(false));
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
            let font = self.terminal_font();
            let galley = ui.ctx().fonts(|fonts| {
                fonts.layout_no_wrap("M".to_owned(), font.clone(), egui::Color32::WHITE)
            });
            let char_w = galley.size().x.max(1.0);
            let char_h = galley.size().y.max(1.0);
            if let Some(session) = self.sessions.get_mut(&place.id) {
                session.fit(rect.width(), rect.height(), char_w, char_h);
                paint_screen(
                    ui,
                    rect,
                    session.screen(),
                    &font,
                    char_w,
                    char_h,
                    self.terminal_font.as_ref(),
                    &mut self.liga_cache,
                );
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
                    if place.is_local() {
                        if failure_overlay(ui, rect, &message).clicked() {
                            self.store.reconnect(place.id);
                        }
                    } else {
                        self.route_picker(ui, rect, &place, &message);
                    }
                }
                _ => center_text(ui, rect, "Idle", "Click this tab or Reconnect to attach."),
            }
        }
        if self.store.debug_overlay && failure_overlay(ui, rect, "debug overlay").clicked() {
            self.store.reconnect(place.id);
        }
    }

    /// When the last route fails, the retry is choosing another path. The list is
    /// the same one the attach walks, so what you pick is what gets probed.
    fn route_picker(&mut self, ui: &mut egui::Ui, rect: egui::Rect, place: &Place, message: &str) {
        let routes = route::candidates(place);
        let in_use = self.in_use_route(place);
        ui.painter()
            .rect_filled(rect, 0.0, egui::Color32::from_black_alpha(190));
        let width = (rect.width() - 64.0).clamp(260.0, 560.0);
        let height = (rect.height() - 48.0).clamp(160.0, 420.0);
        let area = egui::Rect::from_center_size(rect.center(), egui::vec2(width, height));
        let mut chosen: Option<Route> = None;
        let mut retry = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            ui.label(
                egui::RichText::new(format!("Could not reach {}", place.display_label()))
                    .size(17.0)
                    .strong(),
            );
            ui.add(egui::Label::new(egui::RichText::new(message).small().weak()).wrap());
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Choose another route to retry").strong());
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(height - 130.0)
                .show(ui, |ui| {
                    for route in &routes {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(route.kind.label()).small().weak());
                            let current = in_use
                                .as_ref()
                                .is_some_and(|current| route::same_route(current, route));
                            ui.monospace(format!(
                                "{} {}",
                                if current { "✓" } else { " " },
                                route.display()
                            ));
                            if route.last {
                                ui.label(
                                    egui::RichText::new("last used")
                                        .small()
                                        .color(egui::Color32::from_rgb(110, 200, 120)),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Try").clicked() {
                                        chosen = Some(route.clone());
                                    }
                                },
                            );
                        });
                    }
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Try the last route again").clicked() {
                    retry = true;
                }
            });
        });
        if let Some(route) = chosen {
            self.forced_routes.insert(place.id, route);
            self.store.reconnect(place.id);
        } else if retry {
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
        // A sheet or menu above the pane owns the pointer. Raw input still says
        // "pressed" over the pane, so without this check a drag on Settings
        // becomes a click in the terminal behind it.
        if let Some(over) = ui.ctx().layer_id_at(pos)
            && over != ui.layer_id()
        {
            self.wheel_remainder = 0.0;
            self.last_cell = None;
            return;
        }
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

    fn start_add(&mut self) {
        self.settings_open = false;
        self.wizard = None;
        self.fingerprint_check = None;
        self.fingerprint_error = None;
        self.store.begin_add();
    }

    fn start_fingerprint_check(&mut self) {
        let Some(editor) = self.split_editor_endpoint() else {
            return;
        };
        let Some(place) = editor_draft(&editor) else {
            self.fingerprint_error = Some("Enter a host to check.".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(probe::choose(&place));
        });
        self.fingerprint_check = Some(rx);
        self.fingerprint_error = None;
    }

    fn poll_fingerprint(&mut self) {
        let Some(rx) = self.fingerprint_check.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.fingerprint_check = None;
                self.fingerprint_error = Some("host key check died".into());
            }
            Ok(Err(error)) => {
                self.fingerprint_check = None;
                self.fingerprint_error = Some(error.to_string());
            }
            Ok(Ok(choice)) => {
                self.fingerprint_check = None;
                let Some(editor) = self.store.editor.clone() else {
                    return;
                };
                let Some(fingerprint) = choice.learned.clone().filter(|value| !value.is_empty())
                else {
                    self.fingerprint_error = Some("no host key".into());
                    return;
                };
                let id = self.store.add_endpoint(
                    editor.user.trim(),
                    editor.host.trim(),
                    fingerprint,
                    choice.route.clone(),
                );
                self.store.editor = None;
                // The attach keys known_hosts by place id, so install after the
                // node is known: the route may have folded into an existing place.
                if let Some(place) = self
                    .store
                    .places
                    .iter()
                    .find(|place| place.id == id)
                    .cloned()
                    && let Err(err) = probe::install_known_hosts(&place, &choice)
                {
                    self.store
                        .fail_attach(id, format!("could not store host key: {err}"), None);
                }
            }
        }
    }

    fn start_wizard(&mut self) {
        let Some(editor) = self.split_editor_endpoint() else {
            return;
        };
        self.start_wizard_for(&editor);
    }

    /// `user@endpoint` is split as soon as it is used, so the form shows the
    /// login and the address apart and nothing downstream sees the doubled value.
    fn split_editor_endpoint(&mut self) -> Option<PlaceEditorState> {
        let mut editor = self.store.editor.clone()?;
        let (user, host) = Place::split_endpoint(&editor.host);
        if let Some(user) = user {
            editor.user = user;
            editor.host = host;
            self.store.editor = Some(editor.clone());
        }
        Some(editor)
    }

    fn start_wizard_for(&mut self, editor: &PlaceEditorState) {
        let Some(place) = editor_draft(editor) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            probe::discover_with(&place, &probe::SshProber, &mut |found| {
                let _ = tx.send(WizardMsg::Found(found));
            });
            let _ = tx.send(WizardMsg::Done);
        });
        self.wizard = Some(WizardRun {
            key: editor.origin_id,
            rx,
            results: Vec::new(),
            done: false,
        });
    }

    fn poll_wizard(&mut self) {
        let key = self
            .store
            .editor
            .as_ref()
            .and_then(|editor| editor.origin_id);
        if self.wizard.as_ref().is_none_or(|run| run.key != key) {
            self.wizard = None;
            return;
        }
        let Some(run) = self.wizard.as_mut() else {
            return;
        };
        let mut found = Vec::new();
        let mut done = false;
        while let Ok(message) = run.rx.try_recv() {
            match message {
                WizardMsg::Found(item) => found.push(item),
                WizardMsg::Done => done = true,
            }
        }
        run.done |= done;
        for item in found {
            run.results.push(item.clone());
            if let Discovered::Verified {
                route, fingerprint, ..
            } = &item
                && let Some(editor) = self.store.editor.as_mut()
            {
                if editor.fingerprint.trim().is_empty() {
                    editor.fingerprint = fingerprint.clone();
                }
                if editor.routes.iter().any(|route| route.last) {
                    route::push_unique(&mut editor.routes, route.clone());
                } else {
                    route::remember(&mut editor.routes, route.clone());
                }
            }
        }
        if run.results.len() > 12 {
            run.results.drain(..run.results.len() - 12);
        }
        let done = run.done;
        if done && let Some(editor) = self.store.editor.as_mut() {
            editor.routes = editor
                .routes
                .iter()
                .filter(|route| !route.host.trim().is_empty())
                .cloned()
                .collect();
        }
    }

    fn settings_button(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), pointer_sense(false));
        let color = if self.settings_open {
            ACCENT
        } else {
            ui.visuals().text_color()
        };
        crate::icons::paint_wrench(ui.painter(), rect.shrink(4.0), color);
        if response.clicked() {
            self.settings_open = !self.settings_open;
            if self.settings_open {
                self.store.editor = None;
            } else {
                self.settings_drag = None;
            }
        }
        response.on_hover_text("Settings");
    }

    fn terminal_font(&self) -> egui::FontId {
        egui::FontId::monospace(self.appearance.size)
    }

    fn ensure_font(&mut self, ctx: &egui::Context) {
        if self.applied_font.as_deref() == Some(self.appearance.font.as_str()) {
            return;
        }
        crate::appearance::apply(ctx, &self.appearance.font, &self.faces);
        self.liga_cache.clear();
        self.terminal_font = self
            .faces
            .iter()
            .find(|face| face.name == self.appearance.font)
            .and_then(|face| std::fs::read(&face.path).ok())
            .and_then(crate::liga::TerminalFont::load);
        self.applied_font = Some(self.appearance.font.clone());
    }

    fn store_appearance(&mut self) {
        self.appearance.size = crate::appearance::clamp(self.appearance.size);
        if self.appearance.font.trim().is_empty() {
            self.appearance.font = crate::appearance::BUILT_IN.into();
        }
        let _ = crate::appearance::save(&crate::appearance::path(), &self.appearance);
    }

    fn editor_window(&mut self, ctx: &egui::Context) {
        self.poll_wizard();
        self.poll_fingerprint();
        let adding = self
            .store
            .editor
            .as_ref()
            .is_some_and(|editor| editor.origin_id.is_none());
        if adding {
            self.add_window(ctx);
        } else {
            self.edit_window(ctx);
        }
    }

    fn edit_window(&mut self, ctx: &egui::Context) {
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
        let mut saved = false;
        let mut discover = false;
        let discovering = self.wizard.as_ref().is_some_and(|run| !run.done);
        let found = self
            .wizard
            .as_ref()
            .map(|run| run.results.clone())
            .unwrap_or_default();
        let remote = !Place::is_loopback_host(&editor.host) && !editor.host.trim().is_empty();
        sheet(ctx, "place-editor", &mut open, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(title).size(18.0));
            ui.label(egui::RichText::new("Where this tab connects.").weak());
            ui.add_space(8.0);
            identity_fields(ui, &mut editor);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(backend_hint(editor.backend))
                    .small()
                    .weak(),
            );
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Routes").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if discovering { "Finding…" } else { "Find routes" };
                    if ui
                        .add_enabled(
                            remote && editor_draft(&editor).is_some() && !discovering,
                            egui::Button::new(label),
                        )
                        .on_hover_text(
                            "Probe DNS, mDNS and Tailscale. Each route must answer with this node's host key.",
                        )
                        .clicked()
                    {
                        discover = true;
                    }
                    if discovering {
                        ui.spinner();
                    }
                });
            });
            ui.label(
                egui::RichText::new(
                    "The last one that worked is tried first. Empty means discover direct, mDNS and Tailscale.",
                )
                .small()
                .weak(),
            );
            if remote {
                routes_list(ui, &mut editor.routes);
                discovery_list(ui, &found);
            } else {
                ui.label(
                    egui::RichText::new("This machine has no remote routes.")
                        .small()
                        .weak(),
                );
            }
            ui.add_space(6.0);
            ui.add(
                egui::Label::new(egui::RichText::new(directory).small().weak()).selectable(true),
            );
            ui.add_space(8.0);
            if let Some(issue) = save_issue(&editor) {
                ui.label(egui::RichText::new(issue).color(egui::Color32::from_rgb(255, 180, 60)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(can_save(&editor), egui::Button::new("Save").fill(ACCENT))
                    .clicked()
                {
                    saved = true;
                }
                if ui.button("Cancel").clicked() {
                    dismiss = true;
                }
            });
        });
        if dismiss {
            open = false;
        }
        if saved {
            self.wizard = None;
            self.store.editor = Some(editor);
            self.store.save_editor();
            return;
        }
        if self.store.editor.is_some() {
            if open {
                self.store.editor = Some(editor);
            } else {
                self.wizard = None;
                self.store.editor = None;
                return;
            }
        }
        if discover {
            self.start_wizard();
        }
    }

    fn add_window(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.store.editor.clone() else {
            return;
        };
        let mut open = true;
        let mut dismiss = false;
        let mut submit = false;
        let checking = self.fingerprint_check.is_some();
        sheet(ctx, "place-editor", &mut open, |ui| {
            ui.set_min_width(380.0);
            ui.label(egui::RichText::new("Add Place").size(18.0));
            ui.label(
                egui::RichText::new(
                    "Who logs in, and one way to reach it. The SSH host key identifies the node; this address is just the first endpoint. Leave the user empty to use your login.",
                )
                .weak(),
            );
            ui.add_space(8.0);
            ui.add_enabled_ui(!checking, |ui| {
                egui::Grid::new("add-fields")
                    .num_columns(2)
                    .spacing(egui::vec2(12.0, 8.0))
                    .show(ui, |ui| {
                        let hint = user_hint(&editor);
                        place_field(ui, "User", &mut editor.user, hint);
                        place_field(
                            ui,
                            "Endpoint",
                            &mut editor.host,
                            "hostname, IP, SSH alias or user@host; empty is this machine",
                        );
                    });
            });
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(if is_remote(&editor) {
                    "If this fingerprint is already in the catalog, the address becomes another endpoint on that node instead of a new tab."
                } else {
                    "This machine needs no host key or endpoint."
                })
                .small()
                .weak(),
            );
            if let Some(error) = self.fingerprint_error.as_deref() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(error).color(egui::Color32::from_rgb(255, 180, 60)));
            }
            if let Some(issue) = save_issue(&editor) {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(issue).color(egui::Color32::from_rgb(255, 180, 60)));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if checking { "Checking…" } else { "Add" };
                if ui
                    .add_enabled(
                        !checking && save_issue(&editor).is_none(),
                        egui::Button::new(label).fill(ACCENT),
                    )
                    .clicked()
                {
                    submit = true;
                }
                if checking {
                    ui.spinner();
                }
                if ui.button("Cancel").clicked() {
                    dismiss = true;
                }
            });
        });
        if dismiss {
            open = false;
        }
        if submit {
            let remote = is_remote(&editor);
            let user = editor.user.clone();
            if remote {
                self.store.editor = Some(editor);
                self.start_fingerprint_check();
            } else {
                self.store.editor = None;
                self.store.add_local_place(user);
            }
            return;
        }
        if open {
            self.store.editor = Some(editor);
        } else {
            self.fingerprint_check = None;
            self.fingerprint_error = None;
            self.store.editor = None;
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = true;
        let mut done = false;
        let mut tab = self.settings_tab;
        let size = self.appearance.size;
        let current = self.appearance.font.clone();
        let mut next_size = None;
        let mut next_font = None;
        let mut listed = None;
        let places = self.store.places.clone();
        let selected_id = self.store.selected_id;
        let pointer_released = ctx.input(|input| input.pointer.any_released());
        let pointer_pos = ctx.input(|input| input.pointer.interact_pos());
        let mut drag = self.settings_drag;
        let mut row_rects: Vec<(Uuid, egui::Rect)> = Vec::new();
        let mut reorder = None;
        let mut select_place = None;
        let mut remove_place = None;
        let mut add_place = false;
        sheet(ctx, "settings", &mut open, |ui| {
            ui.set_min_width(420.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Settings").size(18.0));
                ui.add_space(12.0);
                ui.selectable_value(&mut tab, SettingsTab::Appearance, "Appearance");
                ui.selectable_value(&mut tab, SettingsTab::Places, "Places");
            });
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);
            match tab {
                SettingsTab::Appearance => {
                    ui.label("Font");
                    let shown = if current == crate::appearance::BUILT_IN {
                        crate::appearance::BUILT_IN
                    } else {
                        current.as_str()
                    };
                    egui::ComboBox::from_id_salt("terminal-font")
                        .selected_text(shown)
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            let faces = crate::appearance::faces();
                            if ui
                                .selectable_label(
                                    current == crate::appearance::BUILT_IN,
                                    "Built-in",
                                )
                                .clicked()
                            {
                                next_font = Some(crate::appearance::BUILT_IN.to_string());
                            }
                            for face in &faces {
                                if ui
                                    .selectable_label(current == face.name, face.name.as_str())
                                    .clicked()
                                {
                                    next_font = Some(face.name.clone());
                                }
                            }
                            listed = Some(faces);
                        });
                    ui.add_space(8.0);
                    ui.label("Size");
                    ui.horizontal(|ui| {
                        if ui.button("−").on_hover_text("Smaller").clicked() {
                            next_size = Some(size - 1.0);
                        }
                        ui.monospace(format!("{size:.0}"));
                        if ui.button("+").on_hover_text("Larger").clicked() {
                            next_size = Some(size + 1.0);
                        }
                    });
                    ui.add_space(8.0);
                    font_preview(self, ui, size);
                }
                SettingsTab::Places => {
                    ui.horizontal(|ui| {
                        ui.label("Tab order");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if plus_button(ui, "Add place").clicked() {
                                add_place = true;
                            }
                        });
                    });
                    ui.label(
                        egui::RichText::new(
                            "Drag a place to reorder it. The title bar follows this order.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.add_space(8.0);
                    egui::ScrollArea::vertical()
                        .id_salt("settings-places")
                        .max_height(280.0)
                        .drag_to_scroll(false)
                        .show(ui, |ui| {
                            if places.is_empty() {
                                ui.label(egui::RichText::new("No places yet.").weak());
                            }
                            for (index, place) in places.iter().enumerate() {
                                let (body, delete) = place_row(
                                    ui,
                                    index,
                                    place,
                                    selected_id == Some(place.id),
                                    drag == Some(place.id),
                                );
                                row_rects.push((place.id, body.rect));
                                if delete.clicked() {
                                    remove_place = Some(place.id);
                                } else if body.clicked() {
                                    select_place = Some(place.id);
                                } else if body.drag_started() {
                                    drag = Some(place.id);
                                }
                            }
                        });
                }
            }
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").clicked() {
                    done = true;
                }
            });
        });
        self.settings_tab = tab;
        if let Some(id) = drag
            && pointer_released
            && let Some(pos) = pointer_pos
            && let Some((onto, _)) = row_rects.iter().find(|(_, rect)| rect.contains(pos))
        {
            reorder = Some((id, *onto));
        }
        self.settings_drag = if pointer_released { None } else { drag };
        if let Some((id, onto)) = reorder {
            self.store.move_place(id, onto);
        }
        if let Some(id) = select_place {
            self.store.select(id);
        }
        if let Some(id) = remove_place {
            self.store.delete(id);
        }
        if add_place {
            self.start_add();
            open = false;
        }
        if let Some(size) = next_size {
            self.appearance.size = size;
            self.store_appearance();
        }
        if let Some(font) = next_font {
            self.appearance.font = font;
            self.store_appearance();
        }
        if let Some(faces) = listed {
            self.faces = faces;
        }
        if done {
            open = false;
        }
        if !open {
            self.settings_open = false;
            self.settings_drag = None;
        }
    }
}

const ACCENT: egui::Color32 = egui::Color32::from_rgb(80, 140, 220);

fn sheet(ctx: &egui::Context, id: &str, open: &mut bool, add: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .frame(egui::Frame::window(&ctx.style()).inner_margin(16))
        .open(open)
        .show(ctx, add);
}

fn place_field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(260.0)
            .id(egui::Id::new(label)),
    );
    ui.end_row();
}

fn user_hint(editor: &PlaceEditorState) -> &'static str {
    if Place::is_loopback_host(&editor.host) {
        "defaults to you"
    } else {
        "empty uses your login"
    }
}

fn is_remote(editor: &PlaceEditorState) -> bool {
    let host = editor.host.trim();
    !host.is_empty() && !Place::is_loopback_host(host)
}

fn backend_name(backend: Backend) -> &'static str {
    match backend {
        Backend::Herdr => "Herdr",
        Backend::Tmux => "tmux",
        Backend::Screen => "screen",
    }
}

fn backend_hint(backend: Backend) -> &'static str {
    match backend {
        Backend::Herdr => "Herdr attaches directly. It does not use a session name.",
        Backend::Tmux => "Attaches to a tmux session on that machine.",
        Backend::Screen => "Attaches to a screen session on that machine.",
    }
}

fn identity_fields(ui: &mut egui::Ui, editor: &mut PlaceEditorState) {
    egui::Grid::new("place-fields")
        .num_columns(2)
        .spacing(egui::vec2(12.0, 8.0))
        .show(ui, |ui| {
            let hint = user_hint(editor);
            place_field(ui, "User", &mut editor.user, hint);
            place_field(
                ui,
                "Endpoint",
                &mut editor.host,
                "hostname, IP, SSH alias or user@host; empty is this machine",
            );
            ui.label("Backend");
            ui.horizontal(|ui| {
                for backend in Backend::ALL {
                    ui.selectable_value(&mut editor.backend, backend, backend_name(backend));
                }
            });
            ui.end_row();
            if editor.backend != Backend::Herdr {
                place_field(ui, "Session", &mut editor.session, "session name");
            }
            place_field(ui, "Label", &mut editor.label, "optional, shown on the tab");
            ui.label("Fingerprint");
            ui.add(
                egui::TextEdit::singleline(&mut editor.fingerprint)
                    .hint_text("SHA256:… learned on first connect")
                    .desired_width(320.0)
                    .font(egui::TextStyle::Monospace)
                    .id(egui::Id::new("place-fingerprint")),
            )
            .on_hover_text("SSH host key pinned to this node. Leave empty to learn.");
            ui.end_row();
        });
}

fn routes_list(ui: &mut egui::Ui, routes: &mut Vec<Route>) {
    let mut remove = None;
    for (index, route) in routes.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt(("route-kind", index))
                .selected_text(route.kind.label())
                .width(92.0)
                .show_ui(ui, |ui| {
                    for kind in RouteKind::ALL {
                        ui.selectable_value(&mut route.kind, kind, kind.label());
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut route.host)
                    .hint_text("host, IP or MagicDNS name")
                    .desired_width(240.0)
                    .font(egui::TextStyle::Monospace)
                    .id(egui::Id::new(("route-host", index))),
            );
            if ui.button("×").on_hover_text("Remove route").clicked() {
                remove = Some(index);
            }
            if route.last {
                ui.label(
                    egui::RichText::new("last used")
                        .small()
                        .color(egui::Color32::from_rgb(110, 200, 120)),
                );
            }
        });
    }
    if let Some(index) = remove {
        routes.remove(index);
    }
    if ui.button("+ Add route").clicked() {
        routes.push(Route::direct(""));
    }
}

fn discovery_list(ui: &mut egui::Ui, found: &[Discovered]) {
    for item in found {
        let (mark, color, detail) = discovery_line(item);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(mark).color(color));
            ui.monospace(item.route().display());
            ui.label(egui::RichText::new(detail).small().weak());
        });
    }
}

fn editor_draft(editor: &PlaceEditorState) -> Option<Place> {
    let host = editor.host.trim();
    let user = editor.user.trim();
    if host.is_empty() || Place::is_loopback_host(host) {
        return None;
    }
    let backend = editor.backend;
    // Discovery only needs the node, so a missing session must not block it.
    let session = editor.session.trim();
    let session = if session.is_empty() {
        None
    } else {
        Some(session.to_string())
    };
    let fingerprint = editor.fingerprint.trim();
    Some(Place {
        id: Uuid::new_v4(),
        user: user.to_string(),
        host: host.to_string(),
        backend,
        session,
        label: None,
        routes: editor.routes.clone(),
        fingerprint: if fingerprint.is_empty() {
            None
        } else {
            Some(fingerprint.to_string())
        },
    })
}

fn discovery_line(item: &Discovered) -> (&'static str, egui::Color32, String) {
    match item {
        Discovered::Verified {
            fingerprint,
            learned,
            ..
        } => (
            "✓",
            egui::Color32::from_rgb(110, 200, 120),
            if *learned {
                format!("learned {}", short_fingerprint(fingerprint))
            } else {
                format!("matches pin · {}", short_fingerprint(fingerprint))
            },
        ),
        Discovered::HostKeyMismatch { found, .. } => (
            "✗",
            egui::Color32::from_rgb(255, 94, 87),
            format!("different host key ({})", short_fingerprint(found)),
        ),
        Discovered::Unreachable { reason, .. } => ("…", egui::Color32::GRAY, reason.clone()),
    }
}

fn short_fingerprint(fingerprint: &str) -> String {
    let keep = fingerprint.chars().take(24).collect::<String>();
    if fingerprint.chars().count() > 24 {
        format!("{keep}…")
    } else {
        keep
    }
}

fn details_issue(editor: &PlaceEditorState) -> Option<&'static str> {
    match editor.backend {
        Backend::Herdr => None,
        Backend::Tmux if editor.session.trim().is_empty() => Some("tmux needs a session name."),
        Backend::Screen if editor.session.trim().is_empty() => Some("screen needs a session name."),
        Backend::Tmux | Backend::Screen => None,
    }
}

fn save_issue(editor: &PlaceEditorState) -> Option<&'static str> {
    details_issue(editor)
}

fn can_save(editor: &PlaceEditorState) -> bool {
    save_issue(editor).is_none()
}

/// Clicks and drags stay on the pointer. Tab and Enter belong to the terminal.
fn pointer_sense(drag: bool) -> egui::Sense {
    if drag {
        egui::Sense::CLICK | egui::Sense::DRAG
    } else {
        egui::Sense::CLICK
    }
}

const TAB_GAP: f32 = 4.0;

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
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), pointer_sense(false));
    ui.painter().circle_filled(rect.center(), 6.0, color);
    response
}

const SETTINGS_ROW_HEIGHT: f32 = 26.0;

fn place_row(
    ui: &mut egui::Ui,
    index: usize,
    place: &Place,
    selected: bool,
    dragging: bool,
) -> (egui::Response, egui::Response) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SETTINGS_ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let body = ui.interact(
        rect,
        ui.id().with(("place-row", place.id)),
        pointer_sense(true),
    );
    let delete_rect = egui::Rect::from_center_size(
        rect.right_center() - egui::vec2(14.0, 0.0),
        egui::vec2(20.0, 20.0),
    );
    let delete = ui.interact(
        delete_rect,
        ui.id().with(("place-delete", place.id)),
        egui::Sense::click(),
    );
    let fill = if dragging {
        egui::Color32::from_rgba_unmultiplied(80, 140, 220, 90)
    } else if selected {
        egui::Color32::from_rgba_unmultiplied(80, 140, 220, 70)
    } else if body.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 8.0, fill);
    }
    if delete.hovered() {
        ui.painter()
            .rect_filled(delete_rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    paint_grip(
        &painter,
        rect.left_center() + egui::vec2(12.0, 0.0),
        ui.visuals().weak_text_color(),
    );
    painter.text(
        rect.left_center() + egui::vec2(30.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{:02}", index + 1),
        egui::FontId::monospace(11.0),
        ui.visuals().weak_text_color(),
    );
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    painter.text(
        rect.left_center() + egui::vec2(58.0, 0.0),
        egui::Align2::LEFT_CENTER,
        place.display_label(),
        egui::FontId::proportional(13.0),
        color,
    );
    let delete_color = if delete.hovered() {
        egui::Color32::from_rgb(255, 94, 87)
    } else {
        ui.visuals().weak_text_color()
    };
    crate::icons::paint_x(&painter, delete_rect.shrink(5.0), delete_color);
    (
        body.on_hover_cursor(egui::CursorIcon::Grab),
        delete
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Remove place"),
    )
}

fn plus_button(ui: &mut egui::Ui, hover: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), pointer_sense(false));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, ui.visuals().widgets.hovered.bg_fill);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "+",
        egui::FontId::proportional(18.0),
        ui.visuals().text_color(),
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover)
}

fn paint_grip(painter: &egui::Painter, center: egui::Pos2, color: egui::Color32) {
    for row in -1..=1 {
        for column in 0..=1 {
            painter.circle_filled(
                center + egui::vec2(column as f32 * 3.0, row as f32 * 4.0),
                1.0,
                color,
            );
        }
    }
}

fn paint_status(painter: &egui::Painter, center: egui::Pos2, state: &TabState) {
    let color = match state {
        TabState::Idle => egui::Color32::GRAY,
        TabState::Attaching => egui::Color32::GRAY,
        TabState::Live => egui::Color32::from_rgb(40, 200, 80),
        TabState::Failed(_) => egui::Color32::from_rgb(220, 60, 60),
    };
    if matches!(state, TabState::Attaching) {
        let t = painter.ctx().input(|input| input.time);
        painter.circle_stroke(center, 4.0, egui::Stroke::new(1.5_f32, color));
        let angle = t as f32 * 6.0;
        let tip = center + egui::vec2(angle.cos(), angle.sin()) * 4.0;
        painter.circle_filled(tip, 1.5, color);
    } else {
        painter.circle_filled(center, 3.5, color);
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
    let response = ui.interact(rect, ui.id().with(message), pointer_sense(false));
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

#[derive(Clone)]
struct LigaGlyph {
    texture: egui::TextureHandle,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[allow(clippy::too_many_arguments)]
fn paint_screen(
    ui: &egui::Ui,
    rect: egui::Rect,
    screen: &vt100::Screen,
    font: &egui::FontId,
    char_w: f32,
    char_h: f32,
    terminal_font: Option<&crate::liga::TerminalFont>,
    liga_cache: &mut std::collections::HashMap<(u16, u32), LigaGlyph>,
) {
    let (rows, cols) = screen.size();
    let rows = rows.get();
    let cols = cols.get();
    let painter = ui.painter();
    let baseline = text_baseline(ui, font);
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
            if let Some(face) = terminal_font {
                if cell_is_plain_ascii(cell) {
                    let end = ascii_run_end(screen, row, col, cols);
                    paint_ascii_run(
                        ui,
                        screen,
                        rect,
                        row,
                        col,
                        end,
                        font,
                        char_w,
                        char_h,
                        baseline,
                        &links,
                        Some(face),
                        liga_cache,
                    );
                    col = end;
                    continue;
                }
                let text = cell.contents();
                let ch = text.chars().next().filter(|_| text.chars().count() == 1);
                if let Some(ch) = ch
                    && !cell.is_wide()
                    && face.glyph_id(ch) != 0
                {
                    paint_cell_background(painter, screen, rect, row, col, char_w, char_h, &links);
                    let (fg, _, _) = cell_ink(cell);
                    let origin = rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h);
                    if !paint_face_glyph(
                        ui,
                        liga_cache,
                        face,
                        face.glyph_id(ch),
                        font.size,
                        origin.x,
                        origin.y + baseline,
                        0,
                        face.unit_scale(font.size),
                        ui.ctx().pixels_per_point(),
                        fg,
                    ) {
                        paint_cell(
                            painter, screen, rect, row, col, 1, font, char_w, char_h, &links,
                        );
                    }
                    col += 1;
                    continue;
                }
            }
            let span = if cell.is_wide() { 2 } else { 1 };
            paint_cell(
                painter, screen, rect, row, col, span, font, char_w, char_h, &links,
            );
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

fn font_preview(app: &mut RtermApp, ui: &mut egui::Ui, size: f32) {
    let sample = "x -> y => z != w";
    let Some(face) = app.terminal_font.as_ref() else {
        ui.label(egui::RichText::new(sample).font(egui::FontId::monospace(size)));
        return;
    };
    let font = egui::FontId::monospace(size);
    let em = ui
        .painter()
        .layout_no_wrap("M".to_owned(), font.clone(), egui::Color32::WHITE);
    let char_w = em.size().x.max(1.0);
    let char_h = em.size().y.max(1.0);
    let baseline = text_baseline(ui, &font);
    let width = sample.chars().count() as f32 * char_w;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, char_h), egui::Sense::hover());
    let scale = face.unit_scale(size);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let color = ui.visuals().text_color();
    for glyph in face.glyphs(sample) {
        if glyph.glyph_id == 0 {
            continue;
        }
        let _ = paint_face_glyph(
            ui,
            &mut app.liga_cache,
            face,
            glyph.glyph_id,
            size,
            rect.min.x + glyph.start as f32 * char_w + glyph.x_offset as f32 * scale,
            rect.min.y + baseline,
            glyph.y_offset,
            scale,
            pixels_per_point,
            color,
        );
    }
}

fn text_baseline(ui: &egui::Ui, font: &egui::FontId) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap("M".to_owned(), font.clone(), egui::Color32::WHITE);
    // pos.y is the baseline egui actually uses. font_ascent is the whole
    // family, including the emoji face, so it sits the letters too low.
    galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map(|glyph| glyph.pos.y)
        .unwrap_or(font.size * 0.8)
}

fn cell_is_plain_ascii(cell: &vt100::Cell) -> bool {
    if cell.is_wide() {
        return false;
    }
    let text = cell.contents();
    text.is_empty() || text == " " || (text.len() == 1 && text.as_bytes()[0].is_ascii())
}

fn cell_ink(cell: &vt100::Cell) -> (egui::Color32, egui::Color32, bool) {
    let mut fg = color_of(cell.fgcolor(), egui::Color32::from_rgb(220, 220, 220));
    let mut bg = color_of(cell.bgcolor(), egui::Color32::TRANSPARENT);
    if cell.inverse() {
        std::mem::swap(&mut fg, &mut bg);
    }
    (fg, bg, cell.underline())
}

fn ascii_run_end(screen: &vt100::Screen, row: u16, start: u16, cols: u16) -> u16 {
    let Some(first) = screen.cell(row, start) else {
        return start + 1;
    };
    let ink = cell_ink(first);
    let mut col = start + 1;
    while col < cols {
        let Some(cell) = screen.cell(row, col) else {
            break;
        };
        if cell.is_wide_continuation() || !cell_is_plain_ascii(cell) || cell_ink(cell) != ink {
            break;
        }
        col += 1;
    }
    col
}

#[allow(clippy::too_many_arguments)]
fn paint_cell(
    painter: &egui::Painter,
    screen: &vt100::Screen,
    rect: egui::Rect,
    row: u16,
    col: u16,
    span: u16,
    font: &egui::FontId,
    char_w: f32,
    char_h: f32,
    links: &[(u16, u16, String)],
) {
    let Some(cell) = screen.cell(row, col) else {
        return;
    };
    let origin = rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h);
    let cell_rect = egui::Rect::from_min_size(origin, egui::vec2(char_w * span as f32, char_h));
    let (fg, bg, _) = cell_ink(cell);
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
}

#[allow(clippy::too_many_arguments)]
fn paint_ascii_run(
    ui: &egui::Ui,
    screen: &vt100::Screen,
    rect: egui::Rect,
    row: u16,
    start: u16,
    end: u16,
    font: &egui::FontId,
    char_w: f32,
    char_h: f32,
    baseline: f32,
    links: &[(u16, u16, String)],
    terminal_font: Option<&crate::liga::TerminalFont>,
    liga_cache: &mut std::collections::HashMap<(u16, u32), LigaGlyph>,
) {
    let painter = ui.painter();
    let mut text = String::new();
    for col in start..end {
        paint_cell_background(painter, screen, rect, row, col, char_w, char_h, links);
        let contents = screen
            .cell(row, col)
            .map(|cell| cell.contents())
            .unwrap_or("");
        text.push(if contents.is_empty() {
            ' '
        } else {
            contents.chars().next().unwrap_or(' ')
        });
    }
    let Some(face) = terminal_font else {
        paint_ascii_text(painter, screen, rect, row, start, end, font, char_w, char_h);
        return;
    };
    let (fg, _, _) = screen.cell(row, start).map(cell_ink).unwrap_or((
        egui::Color32::WHITE,
        egui::Color32::TRANSPARENT,
        false,
    ));
    let scale = face.unit_scale(font.size);
    let pixels_per_point = ui.ctx().pixels_per_point();
    for glyph in face.glyphs(&text) {
        let col = start + glyph.start as u16;
        if glyph.glyph_id == 0
            || !paint_face_glyph(
                ui,
                liga_cache,
                face,
                glyph.glyph_id,
                font.size,
                rect.min.x + col as f32 * char_w + glyph.x_offset as f32 * scale,
                rect.min.y + row as f32 * char_h + baseline,
                glyph.y_offset,
                scale,
                pixels_per_point,
                fg,
            )
        {
            paint_ascii_text(
                painter,
                screen,
                rect,
                row,
                col,
                start + glyph.end as u16,
                font,
                char_w,
                char_h,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_face_glyph(
    ui: &egui::Ui,
    cache: &mut std::collections::HashMap<(u16, u32), LigaGlyph>,
    face: &crate::liga::TerminalFont,
    glyph_id: u16,
    px: f32,
    pen_x: f32,
    baseline_y: f32,
    y_offset: i32,
    scale: f32,
    pixels_per_point: f32,
    color: egui::Color32,
) -> bool {
    let Some(drawn) = liga_glyph(ui.ctx(), cache, face, glyph_id, px, pixels_per_point) else {
        return false;
    };
    let pen_y = baseline_y - y_offset as f32 * scale;
    let glyph_rect = egui::emath::Rect::from_min_size(
        egui::pos2(pen_x + drawn.left, pen_y + drawn.top),
        egui::vec2(drawn.width, drawn.height),
    )
    .round_to_pixels(pixels_per_point.max(1.0));
    ui.painter().image(
        drawn.texture.id(),
        glyph_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
    true
}

#[allow(clippy::too_many_arguments)]
fn paint_ascii_text(
    painter: &egui::Painter,
    screen: &vt100::Screen,
    rect: egui::Rect,
    row: u16,
    start: u16,
    end: u16,
    font: &egui::FontId,
    char_w: f32,
    char_h: f32,
) {
    for col in start..end {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        let text = cell.contents();
        if text.is_empty() || text == " " {
            continue;
        }
        let origin = rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h);
        let (fg, _, _) = cell_ink(cell);
        painter.text(origin, egui::Align2::LEFT_TOP, text, font.clone(), fg);
        let _ = char_h;
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_cell_background(
    painter: &egui::Painter,
    screen: &vt100::Screen,
    rect: egui::Rect,
    row: u16,
    col: u16,
    char_w: f32,
    char_h: f32,
    links: &[(u16, u16, String)],
) {
    let Some(cell) = screen.cell(row, col) else {
        return;
    };
    let origin = rect.min + egui::vec2(col as f32 * char_w, row as f32 * char_h);
    let cell_rect = egui::Rect::from_min_size(origin, egui::vec2(char_w, char_h));
    let (fg, bg, _) = cell_ink(cell);
    if bg != egui::Color32::TRANSPARENT {
        painter.rect_filled(cell_rect, 0.0, bg);
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
}

fn liga_glyph(
    ctx: &egui::Context,
    cache: &mut std::collections::HashMap<(u16, u32), LigaGlyph>,
    font: &crate::liga::TerminalFont,
    glyph_id: u16,
    px: f32,
    pixels_per_point: f32,
) -> Option<LigaGlyph> {
    let raster_px = font.raster_scale(px, pixels_per_point);
    let key = (glyph_id, raster_px.to_bits());
    if cache.contains_key(&key) {
        return cache.get(&key).cloned();
    }
    let coverage = font.raster(glyph_id, raster_px)?;
    let pixels_per_point = pixels_per_point.max(1.0);
    // Same coverage curve as egui's font atlas, so these glyphs match the fallback faces.
    let pixels = coverage
        .alpha
        .iter()
        .map(|alpha| {
            let eased = (*alpha as f32 / 255.0).powf(0.55);
            let a = (eased * 255.0).round() as u8;
            egui::Color32::from_rgba_premultiplied(a, a, a, a)
        })
        .collect();
    let image = egui::ColorImage {
        size: [coverage.width, coverage.height],
        pixels,
    };
    let texture = ctx.load_texture(
        format!("liga-{glyph_id}-{}", px.to_bits()),
        image,
        egui::TextureOptions::LINEAR,
    );
    let drawn = LigaGlyph {
        texture,
        left: coverage.left / pixels_per_point,
        top: coverage.top / pixels_per_point,
        width: coverage.width as f32 / pixels_per_point,
        height: coverage.height as f32 / pixels_per_point,
    };
    cache.insert(key, drawn.clone());
    Some(drawn)
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
    fn editor_needs_a_session_but_the_login_is_optional() {
        let mut editor = PlaceEditorState {
            origin_id: None,
            user: String::new(),
            host: "remote.example".into(),
            backend: Backend::Tmux,
            session: String::new(),
            label: String::new(),
            routes: Vec::new(),
            fingerprint: String::new(),
        };
        assert_eq!(save_issue(&editor), Some("tmux needs a session name."));
        editor.session = "dev".into();
        assert!(can_save(&editor));
        editor.host.clear();
        editor.backend = Backend::Herdr;
        assert!(can_save(&editor));
    }

    #[test]
    fn endpoint_with_a_login_splits_into_user_and_host() {
        let mut editor = PlaceEditorState {
            origin_id: None,
            user: String::new(),
            host: "lucao.whiterun@cloud.shellhub.io".into(),
            backend: Backend::Herdr,
            session: String::new(),
            label: String::new(),
            routes: Vec::new(),
            fingerprint: String::new(),
        };
        let (user, host) = Place::split_endpoint(&editor.host);
        assert_eq!(user.as_deref(), Some("lucao.whiterun"));
        assert_eq!(host, "cloud.shellhub.io");
        editor.host = host;
        editor.user = user.unwrap_or_default();
        assert_eq!(
            rterm::driver::ssh_target(&editor_draft(&editor).unwrap()),
            "lucao.whiterun@cloud.shellhub.io"
        );
    }
}
