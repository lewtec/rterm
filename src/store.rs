use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::attach::{MAX_CONNECT_ATTEMPTS, Phase, PlaceAttach, TabState};
use crate::backend::Backend;
use crate::catalog::{self, CatalogIo};
use crate::codec::{self, CatalogDocument};
use crate::paths::{self, login_name};
use crate::place::Place;
use crate::progress::ConnectProgress;
use crate::route::Route;
use crate::trace::Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceEditorState {
    pub origin_id: Option<Uuid>,
    pub user: String,
    pub host: String,
    pub backend: Backend,
    pub session: String,
    pub label: String,
    pub routes: Vec<Route>,
    pub fingerprint: String,
}

pub struct AppStore {
    pub places: Vec<Place>,
    pub selected_id: Option<Uuid>,
    pub attaches: HashMap<Uuid, PlaceAttach>,
    pub editor: Option<PlaceEditorState>,
    pub debug_overlay: bool,
    pub catalog_error: Option<String>,
    progress: HashMap<Uuid, ConnectProgress>,
    catalog: CatalogIo,
}

impl AppStore {
    pub fn new(catalog: CatalogIo) -> Self {
        let mut store = Self {
            places: Vec::new(),
            selected_id: None,
            attaches: HashMap::new(),
            editor: None,
            debug_overlay: false,
            catalog_error: None,
            progress: HashMap::new(),
            catalog,
        };
        store.load_from_disk(true);
        store.start_watching();
        if let Some(id) = store.selected_id {
            store.select(id);
        }
        store.consume_pending_place();
        store
    }

    pub fn system() -> Self {
        Self::new(CatalogIo::system())
    }

    pub fn selected_place(&self) -> Option<&Place> {
        self.places
            .iter()
            .find(|place| Some(place.id) == self.selected_id)
    }

    pub fn select(&mut self, id: Uuid) {
        self.selected_id = Some(id);
        let was_idle = self
            .attaches
            .get(&id)
            .is_none_or(|attach| attach.phase() == &Phase::Idle);
        self.mutate_attach(id, PlaceAttach::select);
        if was_idle {
            self.ensure_progress(id).begin(true);
        }
    }

    pub fn select_place_at(&mut self, index: usize) {
        if let Some(place) = self.places.get(index) {
            let id = place.id;
            self.select(id);
        }
    }

    pub fn open_place_id(&mut self, id: Uuid) {
        if self.places.iter().any(|place| place.id == id) {
            self.select(id);
        }
    }

    pub fn open_place_switch_key(&mut self, switch_key: &str) {
        if let Ok(id) = Uuid::parse_str(switch_key) {
            self.open_place_id(id);
            return;
        }
        if let Some(existing) = self
            .places
            .iter()
            .find(|place| place.matches_switch_key(switch_key))
        {
            let id = existing.id;
            self.select(id);
            return;
        }
        let Some(parsed) = Place::parse_switch_key(switch_key) else {
            return;
        };
        if let Some(existing) = self
            .places
            .iter()
            .find(|place| place.matches_switch_key(&parsed.switch_key()))
        {
            let id = existing.id;
            self.select(id);
            return;
        }
        let id = parsed.id;
        self.places.push(parsed);
        self.attaches.insert(id, PlaceAttach::default());
        let _ = self.ensure_progress(id);
        self.persist();
        self.select(id);
    }

    pub fn consume_pending_place(&mut self) {
        let Some(key) = catalog::take_pending(&self.catalog.pending_place_file()) else {
            return;
        };
        self.open_place_switch_key(&key);
    }

    pub fn reconnect(&mut self, id: Uuid) {
        self.mutate_attach(id, PlaceAttach::reconnect);
        self.ensure_progress(id).begin(false);
        self.debug_overlay = false;
    }

    pub fn reconnect_selected(&mut self) {
        if let Some(id) = self.selected_id {
            self.reconnect(id);
        }
    }

    /// Add this machine as a place. The backend is Herdr and a missing user
    /// falls back to the login name.
    pub fn add_local_place(&mut self, user: impl Into<String>) -> Uuid {
        let user = user.into().trim().to_string();
        let user = if user.is_empty() { login_name() } else { user };
        let place = Place {
            id: Uuid::new_v4(),
            user,
            host: String::new(),
            backend: Backend::Herdr,
            session: None,
            label: None,
            routes: Vec::new(),
            fingerprint: None,
        };
        self.insert_place(place)
    }

    /// Add `route` as a way to reach the node whose SSH host key fingerprint is
    /// `fingerprint`. The fingerprint identifies the node, not the tab: a node
    /// may be open through several accesses (Herdr, a named tmux session, a
    /// screen session), and every one of them gets the route. A place without a
    /// fingerprint (v1 or a deep link) has no identity yet, so when the verified
    /// endpoint points at it, the place adopts the fingerprint instead of
    /// showing up twice in the list.
    pub fn add_endpoint(
        &mut self,
        user: impl Into<String>,
        address: impl Into<String>,
        fingerprint: impl Into<String>,
        route: Route,
    ) -> Uuid {
        let user = user.into().trim().to_string();
        let address = address.into().trim().to_string();
        let fingerprint = fingerprint.into().trim().to_string();
        // The user is its own field and the endpoint is used verbatim. The
        // endpoint may contain `@` that is not a login (ShellHub addresses are
        // `namespace.device@gateway`), so never move it into the user.
        let probe = [address.as_str(), route.host.as_str()];
        if !fingerprint.is_empty() {
            let login = if user.trim().is_empty() {
                login_name()
            } else {
                user.clone()
            };
            let owner = self.places.iter().position(|place| {
                place.fingerprint.as_deref() == Some(fingerprint.as_str())
                    && place.login_key() == login
            });
            let pending = owner.or_else(|| {
                self.places.iter().position(|place| {
                    !place.is_pinned() && probe.iter().any(|address| place.points_at(address))
                })
            });
            if let Some(index) = pending {
                let target = self.places[index].id;
                self.places[index].fingerprint = Some(fingerprint.clone());
                // Same access, no pin: it is the same tab from before the pin.
                self.absorb_unpinned(target, &probe);
                // Another access (tmux dev, screen ops): another tab of the node.
                self.adopt_unpinned(&fingerprint, target, &probe);
                self.remember_on_node(&fingerprint, route);
                // The endpoint the user typed is a route too.
                self.add_route_on_node(&fingerprint, Route::direct(address.clone()));
                self.persist();
                let chosen = self.add_tab_of(&fingerprint, target);
                self.select(chosen);
                return chosen;
            }
        }
        let mut routes = Vec::new();
        // Another login is another key, not another machine: it sees the same
        // endpoints.
        if !fingerprint.is_empty() {
            for place in &self.places {
                if place.fingerprint.as_deref() == Some(fingerprint.as_str()) {
                    crate::route::merge(&mut routes, place.routes.clone());
                }
            }
        }
        crate::route::remember(&mut routes, route);
        // The typed endpoint is a route too, so the route list is the truth.
        crate::route::push_unique(&mut routes, Route::direct(address.clone()));
        let place = Place {
            id: Uuid::new_v4(),
            user,
            host: address,
            backend: Backend::Herdr,
            session: None,
            label: None,
            routes,
            fingerprint: if fingerprint.is_empty() {
                None
            } else {
                Some(fingerprint)
            },
        };
        self.insert_place(place)
    }

    /// Open another access to the same node: the same user, endpoints, routes and
    /// fingerprint, with another backend and session. Selecting an access that
    /// already exists just focuses its tab.
    pub fn add_session(
        &mut self,
        source: Uuid,
        backend: Backend,
        session: impl Into<String>,
    ) -> Uuid {
        let session = session.into().trim().to_string();
        let session = if backend == Backend::Herdr || session.is_empty() {
            None
        } else {
            Some(session)
        };
        let Some(origin) = self.places.iter().find(|place| place.id == source).cloned() else {
            return source;
        };
        let existing = self.places.iter().position(|place| {
            place.id != source
                && same_node(&origin, place)
                && place.backend == backend
                && place.session == session
        });
        if let Some(index) = existing {
            let id = self.places[index].id;
            self.select(id);
            return id;
        }
        let place = Place {
            id: Uuid::new_v4(),
            user: origin.user.clone(),
            host: origin.host.clone(),
            backend,
            session,
            label: None,
            routes: origin.routes.clone(),
            fingerprint: origin.fingerprint.clone(),
        };
        let id = place.id;
        let at = self
            .places
            .iter()
            .position(|place| place.id == source)
            .map(|index| index + 1)
            .unwrap_or(self.places.len());
        self.places.insert(at, place);
        self.attaches.insert(id, PlaceAttach::default());
        let _ = self.ensure_progress(id);
        self.persist();
        self.select(id);
        id
    }

    /// Every tab that opens the same node as `id`. The tab itself is included.
    pub fn node_tabs(&self, id: Uuid) -> Vec<Place> {
        let Some(origin) = self.places.iter().find(|place| place.id == id) else {
            return Vec::new();
        };
        self.places
            .iter()
            .filter(|place| same_node(origin, place))
            .cloned()
            .collect()
    }

    /// The tab the Add flow should land on: Herdr is the default access, and Add
    /// never introduces a backend or session of its own.
    fn add_tab_of(&self, fingerprint: &str, fallback: Uuid) -> Uuid {
        let login = self
            .places
            .iter()
            .find(|place| place.id == fallback)
            .map(Place::login_key);
        self.places
            .iter()
            .find(|place| {
                place.fingerprint.as_deref() == Some(fingerprint)
                    && place.backend == Backend::Herdr
                    && place.session.is_none()
                    && login
                        .as_ref()
                        .is_none_or(|login| place.login_key() == *login)
            })
            .map(|place| place.id)
            .unwrap_or(fallback)
    }

    /// Mark `route` as the one that worked on every tab of the node: a route is
    /// a property of the node, not of the access.
    fn remember_on_node(&mut self, fingerprint: &str, route: Route) {
        for place in self
            .places
            .iter_mut()
            .filter(|place| place.fingerprint.as_deref() == Some(fingerprint))
        {
            crate::route::remember(&mut place.routes, route.clone());
        }
    }

    /// Add a route that is not the one in use (the typed endpoint) to every tab
    /// of the node, without changing which route is last.
    fn add_route_on_node(&mut self, fingerprint: &str, route: Route) {
        for place in self
            .places
            .iter_mut()
            .filter(|place| place.fingerprint.as_deref() == Some(fingerprint))
        {
            crate::route::push_unique(&mut place.routes, route.clone());
        }
    }

    /// Fold unpinned places that are the same access as `target` into it: they
    /// are the same tab from before its fingerprint was known.
    fn absorb_unpinned(&mut self, target: Uuid, addresses: &[&str]) -> bool {
        let key = self
            .places
            .iter()
            .find(|place| place.id == target)
            .map(|place| (place.login_key(), place.backend, place.session.clone()));
        let victims: Vec<(Uuid, Vec<Route>)> = self
            .places
            .iter()
            .filter(|place| place.id != target)
            .filter(|place| !place.is_pinned())
            .filter(|place| {
                key.as_ref().is_some_and(|key| {
                    key == &(place.login_key(), place.backend, place.session.clone())
                })
            })
            .filter(|place| addresses.iter().any(|address| place.points_at(address)))
            .map(|place| (place.id, place.routes.clone()))
            .collect();
        if victims.is_empty() {
            return false;
        }
        if let Some(place) = self.places.iter_mut().find(|place| place.id == target) {
            for (_, routes) in &victims {
                crate::route::merge(&mut place.routes, routes.clone());
            }
        }
        let dropped: HashSet<_> = victims.into_iter().map(|(id, _)| id).collect();
        self.places.retain(|place| !dropped.contains(&place.id));
        true
    }

    /// Give the pin to the unpinned places that are another access to the same
    /// node, so they become tabs of it instead of duplicates.
    fn adopt_unpinned(&mut self, fingerprint: &str, target: Uuid, addresses: &[&str]) -> bool {
        let routes = self
            .places
            .iter()
            .find(|place| place.id == target)
            .map(|place| place.routes.clone())
            .unwrap_or_default();
        let victims: Vec<Uuid> = self
            .places
            .iter()
            .filter(|place| place.id != target)
            .filter(|place| !place.is_pinned())
            .filter(|place| addresses.iter().any(|address| place.points_at(address)))
            .map(|place| place.id)
            .collect();
        for id in &victims {
            if let Some(place) = self.places.iter_mut().find(|place| place.id == *id) {
                place.fingerprint = Some(fingerprint.to_string());
                crate::route::merge(&mut place.routes, routes.clone());
            }
        }
        !victims.is_empty()
    }

    fn insert_place(&mut self, place: Place) -> Uuid {
        let id = place.id;
        self.places.push(place);
        self.attaches.insert(id, PlaceAttach::default());
        let _ = self.ensure_progress(id);
        self.persist();
        self.select(id);
        id
    }

    pub fn drop_all_connections(&mut self) {
        self.debug_overlay = false;
        let ids: Vec<_> = self.places.iter().map(|place| place.id).collect();
        for id in ids {
            self.mutate_attach(id, PlaceAttach::sleep);
            if let Some(progress) = self.progress.get_mut(&id) {
                progress.reset();
            }
        }
    }

    pub fn begin_add(&mut self) {
        self.editor = Some(PlaceEditorState {
            origin_id: None,
            user: String::new(),
            host: String::new(),
            backend: Backend::Herdr,
            session: String::new(),
            label: String::new(),
            routes: Vec::new(),
            fingerprint: String::new(),
        });
    }

    pub fn begin_edit(&mut self, place: &Place) {
        self.editor = Some(PlaceEditorState {
            origin_id: Some(place.id),
            user: place.user.clone(),
            host: place.host.clone(),
            backend: place.backend,
            session: place.session.clone().unwrap_or_default(),
            label: place.label.clone().unwrap_or_default(),
            routes: place.routes.clone(),
            fingerprint: place.fingerprint.clone().unwrap_or_default(),
        });
    }

    pub fn save_editor(&mut self) {
        let Some(mut draft) = self.editor.clone() else {
            return;
        };
        // The user and the endpoint are separate fields. Do not split `@` out
        // of the endpoint: ShellHub addresses are `namespace.device@gateway`
        // and the login that goes in front is the user field.
        draft.host = draft.host.trim().to_string();
        draft.user = draft.user.trim().to_string();
        if Place::is_loopback_host(&draft.host) {
            draft.host.clear();
            if draft.user.is_empty() {
                draft.user = login_name();
            }
        }
        if draft.backend == Backend::Herdr {
            draft.session.clear();
        } else if draft.session.trim().is_empty() {
            return;
        }
        let session = if draft.backend == Backend::Herdr {
            None
        } else {
            Some(draft.session.trim().to_string())
        };
        let label = draft.label.trim().to_string();
        let local = Place::is_loopback_host(&draft.host);
        let mut routes = Vec::new();
        if !local {
            for route in draft.routes {
                let route = route.normalized();
                if !route.host.is_empty() {
                    crate::route::push_unique(&mut routes, route);
                }
            }
            // The endpoint is just a route; keep it in the list so the list is
            // the whole truth.
            crate::route::push_unique(&mut routes, Route::direct(draft.host.clone()));
        }
        let fingerprint = if local {
            None
        } else {
            let value = draft.fingerprint.trim().to_string();
            if value.is_empty() { None } else { Some(value) }
        };
        let place = Place {
            id: draft.origin_id.unwrap_or_else(Uuid::new_v4),
            user: draft.user,
            host: draft.host,
            backend: draft.backend,
            session,
            label: if label.is_empty() { None } else { Some(label) },
            routes,
            fingerprint,
        };
        let should_reattach =
            if let Some(index) = self.places.iter().position(|item| item.id == place.id) {
                let idle = self
                    .attaches
                    .get(&place.id)
                    .is_none_or(|attach| attach.phase() == &Phase::Idle);
                let changed = self.places[index] != place && !idle;
                self.places[index] = place.clone();
                changed
            } else {
                self.places.push(place.clone());
                self.attaches.insert(place.id, PlaceAttach::default());
                let _ = self.ensure_progress(place.id);
                false
            };
        // Routes belong to the machine, so the other tabs of it follow. The
        // login is part of the tab key, so an edit must not rewrite siblings.
        if let Some(pin) = place.fingerprint.as_deref().filter(|pin| !pin.is_empty()) {
            for other in self
                .places
                .iter_mut()
                .filter(|item| item.id != place.id)
                .filter(|item| item.fingerprint.as_deref() == Some(pin))
            {
                crate::route::merge(&mut other.routes, place.routes.clone());
            }
        }
        self.editor = None;
        self.persist();
        if should_reattach {
            self.reconnect(place.id);
        } else {
            self.select(place.id);
        }
    }

    pub fn move_place(&mut self, id: Uuid, onto: Uuid) {
        if id == onto {
            return;
        }
        let Some(from) = self.places.iter().position(|place| place.id == id) else {
            return;
        };
        let Some(to) = self.places.iter().position(|place| place.id == onto) else {
            return;
        };
        // Swift `Array.move(fromOffsets:toOffset:)` with `toOffset = to > from ? to + 1 : to`.
        let to_offset = if to > from { to + 1 } else { to };
        let place = self.places.remove(from);
        let dest = if to_offset > from {
            to_offset - 1
        } else {
            to_offset
        };
        self.places.insert(dest.min(self.places.len()), place);
        self.persist();
    }

    pub fn delete(&mut self, id: Uuid) {
        self.places.retain(|place| place.id != id);
        self.attaches.remove(&id);
        self.progress.remove(&id);
        if self.selected_id == Some(id) {
            if let Some(next) = self.places.first().map(|place| place.id) {
                self.select(next);
            } else {
                self.selected_id = None;
            }
        }
        if self.editor.as_ref().and_then(|editor| editor.origin_id) == Some(id) {
            self.editor = None;
        }
        self.persist();
    }

    pub fn handle_exit(&mut self, id: Uuid, code: Option<i32>, generation: Option<u32>) {
        self.mutate_attach(id, |attach| attach.handle_exit(code, generation));
        let shows = self.attaches.get(&id).is_some_and(PlaceAttach::shows_pane);
        if !shows && let Some(progress) = self.progress.get_mut(&id) {
            progress.finish();
        }
    }

    pub fn connect_progress(&self, id: Uuid) -> Option<&ConnectProgress> {
        self.progress.get(&id)
    }

    pub fn connect_progress_mut(&mut self, id: Uuid) -> Option<&mut ConnectProgress> {
        self.progress.get_mut(&id)
    }

    pub fn tab_state(&self, id: Uuid) -> TabState {
        self.attaches
            .get(&id)
            .map(PlaceAttach::tab_state)
            .unwrap_or(TabState::Idle)
    }

    pub fn note_connect_timeout(&mut self, id: Uuid, generation: Option<u32>) {
        let Some(attach) = self.attaches.get(&id) else {
            return;
        };
        if let Some(generation) = generation
            && attach.generation() != generation
        {
            return;
        }
        if !attach.is_connecting() {
            return;
        }
        if attach.connect_attempt() >= MAX_CONNECT_ATTEMPTS {
            self.mutate_attach(id, PlaceAttach::fail_connect_timeout);
            if let Some(progress) = self.progress.get_mut(&id) {
                progress.finish();
            }
            return;
        }
        let next = attach.connect_attempt() + 1;
        self.reconnect(id);
        self.mutate_attach(id, |attach| attach.set_connect_attempt(next));
    }

    pub fn note_tty(&mut self, id: Uuid) {
        self.mutate_attach(id, PlaceAttach::note_tty);
        if let Some(progress) = self.progress.get_mut(&id) {
            progress.finish();
        }
    }

    pub fn learn_fingerprint(&mut self, id: Uuid, fingerprint: &str) {
        let fingerprint = fingerprint.trim();
        if fingerprint.is_empty() {
            return;
        }
        let Some(place) = self.places.iter_mut().find(|place| place.id == id) else {
            return;
        };
        if place.fingerprint.is_some() {
            return;
        }
        place.fingerprint = Some(fingerprint.to_string());
        // Learning the pin can turn this tab into a duplicate of an older tab
        // with the same login and access.
        let own = place.clone();
        let victims: Vec<Uuid> = self
            .places
            .iter()
            .filter(|other| other.id != id)
            .filter(|other| same_node(&own, other) && own.same_attachment(other))
            .map(|other| other.id)
            .collect();
        if !victims.is_empty() {
            let routes: Vec<Route> = victims
                .iter()
                .filter_map(|victim| {
                    self.places
                        .iter()
                        .find(|place| place.id == *victim)
                        .map(|place| place.routes.clone())
                })
                .flatten()
                .collect();
            if let Some(place) = self.places.iter_mut().find(|place| place.id == id) {
                crate::route::merge(&mut place.routes, routes);
            }
            let dropped: HashSet<_> = victims.iter().copied().collect();
            self.places.retain(|place| !dropped.contains(&place.id));
            for victim in &victims {
                self.attaches.remove(victim);
                self.progress.remove(victim);
            }
            if self.selected_id.is_some_and(|id| dropped.contains(&id)) {
                self.selected_id = Some(id);
            }
        }
        self.persist();
    }

    pub fn remember_route(&mut self, id: Uuid, route: &Route) {
        let fingerprint = self
            .places
            .iter()
            .find(|place| place.id == id)
            .filter(|place| place.is_pinned())
            .and_then(|place| place.fingerprint.clone());
        let changed = match fingerprint {
            Some(fingerprint) => {
                let mut changed = false;
                for place in self
                    .places
                    .iter_mut()
                    .filter(|place| place.fingerprint.as_deref() == Some(fingerprint.as_str()))
                {
                    let before = place.routes.clone();
                    crate::route::remember(&mut place.routes, route.clone());
                    changed |= place.routes != before;
                }
                changed
            }
            None => {
                let Some(place) = self.places.iter_mut().find(|place| place.id == id) else {
                    return;
                };
                let before = place.routes.clone();
                crate::route::remember(&mut place.routes, route.clone());
                place.routes != before
            }
        };
        if changed {
            self.persist();
        }
    }

    pub fn fail_attach(&mut self, id: Uuid, message: String, generation: Option<u32>) {
        self.mutate_attach(id, |attach| attach.fail(message, generation));
        if !self.attaches.get(&id).is_some_and(PlaceAttach::shows_pane)
            && let Some(progress) = self.progress.get_mut(&id)
        {
            progress.finish();
        }
    }

    pub fn note_progress(&mut self, id: Uuid, headline: &str) {
        self.mutate_attach(id, |attach| attach.note_progress(headline));
    }

    pub fn ingest_trace(&mut self, id: Uuid, raw: &str) {
        let effect = self
            .progress
            .get_mut(&id)
            .and_then(|progress| progress.ingest(raw));
        if let Some(Event::Progress(text)) = effect {
            self.note_progress(id, &text);
        }
    }

    pub fn reveal_catalog(&mut self) {
        if let Err(err) = self.catalog.prepare() {
            self.catalog_error = Some(err.to_string());
            return;
        }
        if !self.catalog.file_exists() {
            self.persist();
        }
        if let Err(err) = paths::open_directory(&self.catalog.directory) {
            self.catalog_error = Some(err.to_string());
        }
    }

    pub fn reload_from_disk(&mut self) {
        self.load_from_disk(false);
    }

    pub fn poll_fs(&mut self) {
        if self.catalog.poll_changes() {
            self.reload_from_disk();
            self.consume_pending_place();
        }
    }

    pub fn places_directory(&self) -> &std::path::Path {
        &self.catalog.directory
    }

    fn load_from_disk(&mut self, creating_if_missing: bool) {
        if let Err(err) = self.catalog.prepare() {
            self.catalog_error = Some(err.to_string());
            return;
        }
        if !self.catalog.file_exists() {
            if creating_if_missing {
                self.apply_places(fixture_places(), true);
                self.persist();
            }
            return;
        }
        match self.catalog.load() {
            Ok(document) => {
                let select_first = self.selected_id.is_none();
                self.apply_places(document.places, select_first);
                self.catalog_error = None;
            }
            Err(err) => self.catalog_error = Some(err.to_string()),
        }
    }

    fn persist(&mut self) {
        let document = CatalogDocument {
            version: codec::CURRENT_VERSION,
            places: self.places.clone(),
        };
        match self.catalog.save(&document) {
            Ok(()) => self.catalog_error = None,
            Err(err) => self.catalog_error = Some(err.to_string()),
        }
    }

    fn apply_places(&mut self, incoming: Vec<Place>, select_first: bool) {
        let incoming = normalize_places(incoming);
        let incoming_ids: HashSet<_> = incoming.iter().map(|place| place.id).collect();
        self.attaches.retain(|id, _| incoming_ids.contains(id));
        self.progress.retain(|id, _| incoming_ids.contains(id));
        for place in &incoming {
            self.attaches.entry(place.id).or_default();
            let _ = self.ensure_progress(place.id);
        }
        self.places = incoming;
        let selected_ok = self
            .selected_id
            .is_some_and(|id| incoming_ids.contains(&id));
        if select_first || !selected_ok {
            self.selected_id = self.places.first().map(|place| place.id);
        }
    }

    fn start_watching(&mut self) {
        if let Err(err) = self.catalog.start_watching() {
            self.catalog_error = Some(err);
        }
    }

    fn ensure_progress(&mut self, id: Uuid) -> &mut ConnectProgress {
        self.progress.entry(id).or_default()
    }

    fn mutate_attach(&mut self, id: Uuid, body: impl FnOnce(&mut PlaceAttach)) {
        let mut attach = self.attaches.get(&id).cloned().unwrap_or_default();
        let before = attach.clone();
        body(&mut attach);
        if attach != before {
            self.attaches.insert(id, attach);
        }
    }
}

fn fixture_places() -> Vec<Place> {
    vec![Place::new(login_name(), "", Backend::Herdr, None)]
}

/// Keep one place per node. A fingerprint is the identity; a place without one
/// joins the pinned node that shares an endpoint. Remaining unpinned duplicates
/// fold by switch key. This heals v1 catalogs and earlier Add runs that kept
/// the pre-fingerprint place next to the new one.
fn normalize_places(places: Vec<Place>) -> Vec<Place> {
    let mut places = places;
    for place in &mut places {
        if place.backend == Backend::Herdr {
            // Herdr names no session; an old record may still carry one.
            place.session = None;
        }
    }
    // An unpinned place that reaches a pinned node is a tab of that node from
    // before the pin, not a second node.
    let pins: Vec<(String, Vec<String>)> = places
        .iter()
        .filter(|place| place.is_pinned())
        .filter_map(|place| {
            place
                .fingerprint
                .clone()
                .map(|pin| (pin, place.endpoint_hosts()))
        })
        .collect();
    for place in places.iter_mut().filter(|place| !place.is_pinned()) {
        let hosts = place.endpoint_hosts();
        let owner = pins.iter().find(|(_, pin_hosts)| {
            !hosts.is_empty() && pin_hosts.iter().any(|host| hosts.contains(host))
        });
        if let Some((pin, _)) = owner {
            place.fingerprint = Some(pin.clone());
        }
    }
    // One tab per access: a duplicate needs the same node *and* the same backend
    // and session.
    let mut out: Vec<Place> = Vec::new();
    for place in places {
        let duplicate = out
            .iter()
            .position(|item| same_node(item, &place) && item.same_attachment(&place));
        if let Some(index) = duplicate {
            fold_place(&mut out[index], place);
        } else {
            out.push(place);
        }
    }
    share_routes(&mut out);
    out
}

/// Routes belong to the node, so every tab of it keeps the same list. A user
/// typed on any tab is the node's login too.
fn share_routes(places: &mut [Place]) {
    let mut pins: Vec<String> = places
        .iter()
        .filter(|place| place.is_pinned())
        .filter_map(|place| place.fingerprint.clone())
        .collect();
    pins.sort();
    pins.dedup();
    for pin in pins {
        let members: Vec<usize> = places
            .iter()
            .enumerate()
            .filter(|(_, place)| place.fingerprint.as_deref() == Some(pin.as_str()))
            .map(|(index, _)| index)
            .collect();
        let routes: Vec<Route> = members
            .iter()
            .flat_map(|index| places[*index].routes.clone())
            .collect();
        for index in members {
            crate::route::merge(&mut places[index].routes, routes.clone());
        }
    }
}

fn same_node(left: &Place, right: &Place) -> bool {
    let left_pin = left.fingerprint.as_deref().filter(|pin| !pin.is_empty());
    let right_pin = right.fingerprint.as_deref().filter(|pin| !pin.is_empty());
    match (left_pin, right_pin) {
        // The key of a tab is the login, the node's host key and the access.
        (Some(left_pin), Some(right_pin)) => {
            left_pin == right_pin && left.login_key() == right.login_key()
        }
        // Without a pin, the endpoints are all there is: overlapping routes
        // mean the same node, whatever name the user typed.
        _ => {
            let left_hosts = left.endpoint_hosts();
            let right_hosts = right.endpoint_hosts();
            left.login_key() == right.login_key()
                && left.is_local() == right.is_local()
                && (left_hosts.is_empty() && right_hosts.is_empty()
                    || left_hosts.iter().any(|host| right_hosts.contains(host)))
        }
    }
}

/// Fold a copy of a node into it: the copy's routes are added, and a label,
/// session, or user that only the copy carries is kept.
fn fold_place(node: &mut Place, copy: Place) {
    adopt_details(node, &copy);
    crate::route::merge(&mut node.routes, copy.routes);
}

fn adopt_details(node: &mut Place, copy: &Place) {
    if node.label.as_deref().is_none_or(str::is_empty)
        && copy.label.as_deref().is_some_and(|label| !label.is_empty())
    {
        node.label = copy.label.clone();
    }
    if node.session.is_none() {
        node.session = copy.session.clone();
    }
    if node.user.trim().is_empty() {
        node.user = copy.user.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attach::{CONNECTING_TITLE, Phase, TIMED_OUT_MESSAGE};
    use crate::place::{self, FILE_EXTENSION};
    use std::path::PathBuf;

    struct Tmp(PathBuf);

    impl Tmp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("rterm-store-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn store_in(tmp: &Tmp) -> AppStore {
        AppStore::new(CatalogIo::from_legacy_file(tmp.0.join("places.toml")))
    }

    #[test]
    fn consume_pending_place_selects_matching_place() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.begin_add();
        {
            let editor = store.editor.as_mut().unwrap();
            editor.user = "ada".into();
            editor.host = "remote.example".into();
            editor.backend = Backend::Herdr;
        }
        store.save_editor();
        let second = store.places.last().unwrap().id;
        store.select(first);
        catalog::write_pending(
            "ada@remote.example:herdr",
            &store.catalog.pending_place_file(),
        );
        store.consume_pending_place();
        assert_eq!(store.selected_id, Some(second));
        store.consume_pending_place();
        assert_eq!(store.selected_id, Some(second));
    }

    #[test]
    fn open_place_selects_matching_place() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.begin_add();
        {
            let editor = store.editor.as_mut().unwrap();
            editor.user = "ada".into();
            editor.host = "remote.example".into();
            editor.backend = Backend::Herdr;
        }
        store.save_editor();
        let second = store.places.last().unwrap().id;
        store.select(first);
        store.open_place_id(second);
        assert_eq!(store.selected_id, Some(second));
        store.select(first);
        store.open_place_switch_key("ada@remote.example:herdr");
        assert_eq!(store.selected_id, Some(second));
        store.open_place_id(Uuid::new_v4());
        assert_eq!(store.selected_id, Some(second));
    }

    #[test]
    fn remember_route_marks_the_route_used_last() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let id = store.places[0].id;
        store.remember_route(id, &Route::direct("riverwood"));
        store.remember_route(id, &Route::mdns("riverwood.local"));
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        let last: Vec<_> = place.routes.iter().filter(|route| route.last).collect();
        assert_eq!(last.len(), 1);
        assert_eq!(last[0].host, "riverwood.local");
    }

    #[test]
    fn add_endpoint_creates_a_node_for_a_new_fingerprint() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let count = store.places.len();
        let id = store.add_endpoint(
            "ada",
            "riverwood",
            "SHA256:NODE",
            Route::mdns("riverwood.local"),
        );
        assert_eq!(store.places.len(), count + 1);
        assert_eq!(store.selected_id, Some(id));
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(place.host, "riverwood");
        assert_eq!(place.fingerprint.as_deref(), Some("SHA256:NODE"));
        // The probed route, marked last, and the typed endpoint as another route.
        assert_eq!(place.routes.len(), 2);
        assert!(place.routes[0].last);
        assert_eq!(place.routes[0].host, "riverwood.local");
        assert!(place.routes.iter().any(
            |route| route.host == "riverwood" && route.kind == crate::route::RouteKind::Direct
        ));
    }

    #[test]
    fn add_endpoint_folds_a_route_into_the_node_with_that_fingerprint() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut node = Place::new("ada", "riverwood", Backend::Herdr, None);
        node.id = store.places[0].id;
        node.fingerprint = Some("SHA256:NODE".into());
        node.routes = vec![Route::direct("riverwood")];
        store.places[0] = node.clone();
        let count = store.places.len();
        let id = store.add_endpoint("ada", "riften", "SHA256:NODE", Route::mdns("riften.local"));
        assert_eq!(id, node.id);
        assert_eq!(store.places.len(), count);
        assert_eq!(store.selected_id, Some(node.id));
        let merged = store
            .places
            .iter()
            .find(|place| place.id == node.id)
            .unwrap();
        assert_eq!(merged.user, "ada");
        assert!(merged.routes.iter().any(|route| route.host == "riverwood"));
        assert!(
            merged
                .routes
                .iter()
                .any(|route| route.host == "riften.local" && route.last)
        );
    }

    #[test]
    fn add_endpoint_completes_an_unpinned_place_with_the_endpoint() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        store.places[0] = Place::new("ada", "riverwood", Backend::Herdr, None);
        let id = store.places[0].id;
        let returned = store.add_endpoint(
            "ada",
            "riverwood",
            "SHA256:NODE",
            Route::direct("riverwood"),
        );
        assert_eq!(returned, id);
        assert_eq!(store.places.len(), 1);
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(place.fingerprint.as_deref(), Some("SHA256:NODE"));
        assert_eq!(place.routes.len(), 1);
    }

    #[test]
    fn add_endpoint_absorbs_an_unpinned_duplicate_of_the_same_endpoint() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut pinned = Place::new("ada", "riverwood", Backend::Herdr, None);
        pinned.fingerprint = Some("SHA256:NODE".into());
        let mut stale = Place::new("ada", "riverwood", Backend::Herdr, None);
        stale.routes.push(Route::mdns("riverwood.local"));
        store.places = vec![pinned.clone(), stale];
        let id = store.add_endpoint(
            "ada",
            "riverwood",
            "SHA256:NODE",
            Route::direct("riverwood"),
        );
        assert_eq!(id, pinned.id);
        assert_eq!(store.places.len(), 1);
        assert!(
            store.places[0]
                .routes
                .iter()
                .any(|route| route.host == "riverwood.local")
        );
    }

    #[test]
    fn add_session_opens_another_tab_of_the_same_node() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        herdr.routes.push(Route::direct("riverwood"));
        store.places = vec![herdr.clone()];
        let id = store.add_session(herdr.id, Backend::Tmux, "dev");
        assert_ne!(id, herdr.id);
        assert_eq!(store.places.len(), 2);
        assert_eq!(store.selected_id, Some(id));
        assert_eq!(store.places[1].id, id);
        let tab = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(tab.fingerprint.as_deref(), Some("SHA256:NODE"));
        assert_eq!(tab.user, "ada");
        assert_eq!(tab.backend, Backend::Tmux);
        assert_eq!(tab.session.as_deref(), Some("dev"));
        assert!(tab.routes.iter().any(|route| route.host == "riverwood"));
    }

    #[test]
    fn add_session_reuses_an_existing_tab() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        let mut tmux = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        tmux.fingerprint = Some("SHA256:NODE".into());
        store.places = vec![herdr.clone(), tmux.clone()];
        let id = store.add_session(herdr.id, Backend::Tmux, "dev");
        assert_eq!(id, tmux.id);
        assert_eq!(store.places.len(), 2);
        assert_eq!(store.selected_id, Some(tmux.id));
    }

    #[test]
    fn add_endpoint_puts_the_route_on_every_tab_of_the_node() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        herdr.routes.push(Route::direct("riverwood"));
        let mut tmux = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        tmux.fingerprint = Some("SHA256:NODE".into());
        tmux.routes.push(Route::direct("riverwood"));
        store.places = vec![herdr, tmux];
        store.add_endpoint("ada", "riften", "SHA256:NODE", Route::mdns("riften.local"));
        assert_eq!(store.places.len(), 2);
        for place in &store.places {
            assert!(
                place
                    .routes
                    .iter()
                    .any(|route| route.host == "riften.local" && route.last)
            );
        }
    }

    #[test]
    fn remember_route_updates_every_tab_of_the_node() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        let mut tmux = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        tmux.fingerprint = Some("SHA256:NODE".into());
        store.places = vec![herdr.clone(), tmux];
        store.remember_route(herdr.id, &Route::mdns("riverwood.local"));
        for place in &store.places {
            assert!(
                place
                    .routes
                    .iter()
                    .any(|route| route.host == "riverwood.local" && route.last)
            );
        }
    }

    #[test]
    fn add_endpoint_adopts_another_session_of_an_unpinned_node() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        let stale = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        store.places = vec![herdr, stale.clone()];
        store.add_endpoint(
            "ada",
            "riverwood",
            "SHA256:NODE",
            Route::direct("riverwood"),
        );
        assert_eq!(store.places.len(), 2);
        let tab = store
            .places
            .iter()
            .find(|place| place.id == stale.id)
            .unwrap();
        assert_eq!(tab.fingerprint.as_deref(), Some("SHA256:NODE"));
        assert_eq!(tab.session.as_deref(), Some("dev"));
    }

    #[test]
    fn add_endpoint_with_another_login_opens_another_tab() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let mut node = Place::new("ada", "riverwood", Backend::Herdr, None);
        node.id = store.places[0].id;
        node.fingerprint = Some("SHA256:NODE".into());
        node.routes = vec![Route::direct("riverwood")];
        store.places[0] = node.clone();
        let id = store.add_endpoint("bob", "riften", "SHA256:NODE", Route::mdns("riften.local"));
        assert_ne!(id, node.id);
        assert_eq!(store.places.len(), 2);
        let tab = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(tab.user, "bob");
        assert_eq!(tab.fingerprint.as_deref(), Some("SHA256:NODE"));
        // The login is another key, but the machine's endpoints still count.
        assert!(tab.routes.iter().any(|route| route.host == "riverwood"));
        assert!(
            tab.routes
                .iter()
                .any(|route| route.host == "riften.local" && route.last)
        );
    }

    #[test]
    fn normalize_places_keeps_two_logins_of_one_node_apart() {
        let mut ada = Place::new("ada", "riverwood", Backend::Herdr, None);
        ada.fingerprint = Some("SHA256:NODE".into());
        let mut bob = Place::new("bob", "riverwood", Backend::Herdr, None);
        bob.fingerprint = Some("SHA256:NODE".into());
        let normalized = normalize_places(vec![ada, bob]);
        assert_eq!(normalized.len(), 2);
    }

    #[test]
    fn normalize_places_keeps_one_node_per_fingerprint() {
        let mut first = Place::new("ada", "riverwood", Backend::Herdr, None);
        first.fingerprint = Some("SHA256:NODE".into());
        first.routes.push(Route::direct("riverwood"));
        let mut second = Place::new("ada", "riften", Backend::Herdr, None);
        second.fingerprint = Some("SHA256:NODE".into());
        second.routes.push(Route::mdns("riften.local"));
        let normalized = normalize_places(vec![first.clone(), second]);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].id, first.id);
        assert!(
            normalized[0]
                .routes
                .iter()
                .any(|route| route.host == "riften.local")
        );
    }

    #[test]
    fn normalize_places_completes_the_unpinned_copy_of_a_pinned_node() {
        let mut pinned = Place::new("ada", "riverwood", Backend::Herdr, None);
        pinned.fingerprint = Some("SHA256:NODE".into());
        let stale = Place::new("ada", "riverwood", Backend::Herdr, None);
        let normalized = normalize_places(vec![stale.clone(), pinned]);
        assert_eq!(normalized.len(), 1);
        // The tab the user already had stays; it just learns the pin.
        assert_eq!(normalized[0].id, stale.id);
        assert_eq!(normalized[0].fingerprint.as_deref(), Some("SHA256:NODE"));
    }

    #[test]
    fn normalize_places_keeps_the_label_of_the_older_copy() {
        let mut stale = Place::new("ada", "riverwood", Backend::Tmux, Some("main".into()));
        stale.label = Some("Work".into());
        let mut pinned = Place::new("ada", "riverwood", Backend::Tmux, Some("main".into()));
        pinned.fingerprint = Some("SHA256:NODE".into());
        let normalized = normalize_places(vec![stale, pinned]);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].fingerprint.as_deref(), Some("SHA256:NODE"));
        assert_eq!(normalized[0].label.as_deref(), Some("Work"));
        assert_eq!(normalized[0].session.as_deref(), Some("main"));
    }

    #[test]
    fn normalize_places_keeps_two_sessions_of_one_node_as_two_tabs() {
        let mut herdr = Place::new("ada", "riverwood", Backend::Herdr, None);
        herdr.fingerprint = Some("SHA256:NODE".into());
        herdr.routes.push(Route::direct("riverwood"));
        let mut tmux = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        tmux.fingerprint = Some("SHA256:NODE".into());
        tmux.routes.push(Route::mdns("riverwood.local"));
        let normalized = normalize_places(vec![herdr, tmux]);
        assert_eq!(normalized.len(), 2);
        // Routes belong to the node, so both tabs see every endpoint.
        for place in &normalized {
            assert!(
                place
                    .routes
                    .iter()
                    .any(|route| route.host == "riverwood.local")
            );
        }
    }

    #[test]
    fn normalize_places_folds_unpinned_duplicates() {
        let first = Place::new("ada", "riverwood", Backend::Herdr, None);
        let mut second = Place::new("ada", "riverwood", Backend::Herdr, None);
        second.routes.push(Route::mdns("riverwood.local"));
        let normalized = normalize_places(vec![first.clone(), second]);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].id, first.id);
        assert!(
            normalized[0]
                .routes
                .iter()
                .any(|route| route.host == "riverwood.local")
        );
    }

    #[test]
    fn add_endpoint_keeps_the_at_of_a_shellhub_address() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let id = store.add_endpoint(
            "lucasew",
            "lucao.whiterun@cloud.shellhub.io",
            "SHA256:NODE",
            Route::direct("lucao.whiterun@cloud.shellhub.io"),
        );
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(place.user, "lucasew");
        assert_eq!(place.host, "lucao.whiterun@cloud.shellhub.io");
        assert_eq!(
            crate::driver::ssh_target(place),
            "lucasew@lucao.whiterun@cloud.shellhub.io"
        );
        assert_eq!(
            place.display_label(),
            "lucasew@lucao.whiterun@cloud.shellhub.io:herdr"
        );
    }

    #[test]
    fn normalize_places_keeps_the_at_of_the_address() {
        let place = Place::new(
            "lucasew",
            "lucao.whiterun@cloud.shellhub.io",
            Backend::Herdr,
            None,
        );
        let normalized = normalize_places(vec![place]);
        assert_eq!(normalized[0].user, "lucasew");
        assert_eq!(normalized[0].host, "lucao.whiterun@cloud.shellhub.io");
    }

    #[test]
    fn add_local_place_defaults_the_user_to_the_login_name() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let id = store.add_local_place("");
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        assert!(place.is_local());
        assert_eq!(place.user, login_name());
    }

    #[test]
    fn move_place_renumbers_files() {
        let tmp = Tmp::new();
        let directory = tmp.0.join("places");
        let first = Place::new("ada", "remote.example", Backend::Herdr, None);
        let second = Place::new("ada", "riverwood", Backend::Herdr, None);
        let third = Place::new("ada", "riften", Backend::Herdr, None);
        let io = CatalogIo::with_parts(directory.clone(), tmp.0.join("missing.toml"));
        io.save(&CatalogDocument {
            version: 1,
            places: vec![first.clone(), second.clone(), third.clone()],
        })
        .unwrap();
        let mut store = AppStore::new(io);
        store.move_place(first.id, third.id);
        assert_eq!(
            store
                .places
                .iter()
                .map(|place| place.id)
                .collect::<Vec<_>>(),
            vec![second.id, third.id, first.id]
        );
        store.move_place(first.id, second.id);
        assert_eq!(
            store
                .places
                .iter()
                .map(|place| place.id)
                .collect::<Vec<_>>(),
            vec![first.id, second.id, third.id]
        );
        store.move_place(first.id, first.id);
        assert_eq!(
            store
                .places
                .iter()
                .map(|place| place.id)
                .collect::<Vec<_>>(),
            vec![first.id, second.id, third.id]
        );
        assert_eq!(
            place_file_names(&directory),
            vec![
                place::file_name(0, &first),
                place::file_name(1, &second),
                place::file_name(2, &third),
            ]
        );
    }

    #[test]
    fn open_place_creates_missing_switch_key_once() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let before = store.places.len();
        store.open_place_switch_key("ada@riverwood:tmux(dev)");
        assert_eq!(store.places.len(), before + 1);
        assert_eq!(
            store.selected_place().unwrap().switch_key(),
            "ada@riverwood:tmux(dev)"
        );
        let id = store.selected_id.unwrap();
        store.open_place_switch_key("ada@riverwood:tmux(dev)");
        assert_eq!(store.places.len(), before + 1);
        assert_eq!(store.selected_id, Some(id));
    }

    #[test]
    fn sleep_drops_live_and_failed_tabs_without_changing_selection() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.select(first);
        store.handle_exit(first, Some(255), None);
        assert_eq!(
            store.attaches.get(&first).unwrap().phase(),
            &Phase::Failed("exited (255)".into())
        );
        assert!(!store.attaches.get(&first).unwrap().shows_pane());
        store.drop_all_connections();
        assert_eq!(store.selected_id, Some(first));
        assert_eq!(store.attaches.get(&first).unwrap().phase(), &Phase::Idle);
        assert!(!store.attaches.get(&first).unwrap().shows_pane());
    }

    #[test]
    fn save_editor_keeps_routes_and_pin() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        store.begin_add();
        {
            let editor = store.editor.as_mut().unwrap();
            editor.user = "ada".into();
            editor.host = "riverwood".into();
            editor.routes = vec![Route::mdns("riverwood.local")];
            editor.fingerprint = "SHA256:C8olMcLxERkizT4dlzO1z85UVxdq".into();
        }
        store.save_editor();
        let saved = store.places.last().unwrap().clone();
        assert_eq!(
            saved.routes,
            vec![Route::mdns("riverwood.local"), Route::direct("riverwood"),]
        );
        assert_eq!(
            saved.fingerprint.as_deref(),
            Some("SHA256:C8olMcLxERkizT4dlzO1z85UVxdq")
        );
        let reloaded = store_in(&tmp);
        let place = reloaded
            .places
            .iter()
            .find(|place| place.id == saved.id)
            .unwrap();
        assert_eq!(place.routes, saved.routes);
        assert_eq!(place.fingerprint, saved.fingerprint);
    }

    #[test]
    fn learning_a_fingerprint_never_overwrites_the_pin() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        store.begin_add();
        {
            let editor = store.editor.as_mut().unwrap();
            editor.user = "ada".into();
            editor.host = "riverwood".into();
        }
        store.save_editor();
        let id = store.places.last().unwrap().id;
        store.learn_fingerprint(id, "SHA256:ONE");
        store.learn_fingerprint(id, "SHA256:TWO");
        let place = store.places.iter().find(|place| place.id == id).unwrap();
        assert_eq!(place.fingerprint.as_deref(), Some("SHA256:ONE"));
    }

    #[test]
    fn learning_a_pin_folds_the_duplicate_of_the_same_access() {
        let mut pinned = Place::new("ada", "riverwood", Backend::Herdr, None);
        pinned.fingerprint = Some("SHA256:NODE".into());
        pinned.routes.push(Route::direct("riverwood"));
        let unpinned = Place::new("ada", "riverwood", Backend::Herdr, None);
        let mut store = store_in(&Tmp::new());
        store.places = vec![pinned, unpinned.clone()];
        store.learn_fingerprint(unpinned.id, "SHA256:NODE");
        assert_eq!(store.places.len(), 1);
        // The tab that just connected is the one that stays.
        assert_eq!(store.places[0].id, unpinned.id);
        assert!(
            store.places[0]
                .routes
                .iter()
                .any(|route| route.host == "riverwood")
        );
    }

    #[test]
    fn learning_a_pin_keeps_another_access_apart() {
        let mut pinned = Place::new("ada", "riverwood", Backend::Herdr, None);
        pinned.fingerprint = Some("SHA256:NODE".into());
        let tmux = Place::new("ada", "riverwood", Backend::Tmux, Some("dev".into()));
        let mut store = store_in(&Tmp::new());
        store.places = vec![pinned, tmux.clone()];
        store.learn_fingerprint(tmux.id, "SHA256:NODE");
        assert_eq!(store.places.len(), 2);
        let tab = store
            .places
            .iter()
            .find(|place| place.id == tmux.id)
            .unwrap();
        assert_eq!(tab.fingerprint.as_deref(), Some("SHA256:NODE"));
    }

    #[test]
    fn select_place_at_index() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.select_place_at(0);
        assert_eq!(store.selected_id, Some(first));
        store.select_place_at(99);
        assert_eq!(store.selected_id, Some(first));
    }

    #[test]
    fn connect_timeout_retries_then_gives_up() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.select(first);
        assert_eq!(store.attaches.get(&first).unwrap().connect_attempt(), 1);
        assert!(store.connect_progress(first).unwrap().is_visible());

        store.note_connect_timeout(first, None);
        assert_eq!(
            store.attaches.get(&first).unwrap().phase(),
            &Phase::Attaching(CONNECTING_TITLE.into())
        );
        assert_eq!(store.attaches.get(&first).unwrap().connect_attempt(), 2);
        assert_eq!(store.attaches.get(&first).unwrap().generation(), 1);
        assert!(store.connect_progress(first).unwrap().is_visible());

        store.note_connect_timeout(first, None);
        store.note_connect_timeout(first, None);
        assert_eq!(
            store.attaches.get(&first).unwrap().phase(),
            &Phase::Failed(TIMED_OUT_MESSAGE.into())
        );
        assert!(!store.connect_progress(first).unwrap().is_visible());

        let generation = store.attaches.get(&first).unwrap().generation();
        store.handle_exit(first, Some(255), Some(generation));
        assert_eq!(
            store.attaches.get(&first).unwrap().phase(),
            &Phase::Failed(TIMED_OUT_MESSAGE.into())
        );
    }

    #[test]
    fn connect_progress_stays_off_attach_and_survives_reconnect() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.select(first);
        {
            let progress = store.connect_progress_mut(first).unwrap();
            progress.ingest("debug1: Connecting to remote.example [10.0.0.2] port 22.");
            progress.ingest("debug1: Connection established.");
            assert_eq!(progress.crumbs().len(), 2);
        }
        store.reconnect(first);
        assert_eq!(store.connect_progress(first).unwrap().crumbs().len(), 2);
        assert_eq!(store.tab_state(first), TabState::Attaching);
        assert!(store.connect_progress(first).unwrap().is_visible());
        store.drop_all_connections();
        assert!(store.connect_progress(first).unwrap().crumbs().is_empty());
        assert!(!store.connect_progress(first).unwrap().is_visible());
        store.select(first);
        assert!(store.connect_progress(first).unwrap().crumbs().is_empty());
        assert!(store.connect_progress(first).unwrap().is_visible());
    }

    #[test]
    fn duplicate_progress_does_not_change_attach() {
        let tmp = Tmp::new();
        let mut store = store_in(&tmp);
        let first = store.places[0].id;
        store.select(first);
        let attach = store.attaches.get(&first).unwrap().clone();
        store.note_progress(first, CONNECTING_TITLE);
        assert_eq!(store.attaches.get(&first), Some(&attach));
    }

    fn place_file_names(directory: &std::path::Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some(FILE_EXTENSION) {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
    }
}
