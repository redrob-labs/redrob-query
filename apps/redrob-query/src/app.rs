//! The shell: one window with the profiles and schema on the left, query tabs in the middle (editor
//! above, result below, structure beside), the Ask dock on the right and a status line at the
//! bottom. Every panel talks to the engine through the `Workspace` and reports back as events the
//! app routes here, so no panel reaches into another.

use egui::{RichText, Ui};
use redrob_query_core::{ConnectionProfile, DatabaseKind, MetadataNode, QueryLanguage, Workspace};
use uuid::Uuid;

use crate::account::AccountWatch;
use crate::ai_dock::{AiDock, AiEvent};
use crate::changes::{ChangesEvent, ChangesPanel};
use crate::editor::QueryTab;
use crate::highlight::Dialect;
use crate::profiles::{ProfilesEvent, ProfilesPanel};
use crate::schema::{SchemaBrowser, SchemaEvent, preview_query};
use crate::services;
use crate::structure::{StructureEvent, StructurePanel};
use crate::theme;
use crate::workspace::{HistoryEntry, SavedTab, WorkspaceFile, now_unix};

pub struct QueryApp {
    workspace: Workspace,
    profiles: ProfilesPanel,
    schema: SchemaBrowser,
    structure: StructurePanel,
    changes: ChangesPanel,
    ai: AiDock,
    account: AccountWatch,
    tabs: Vec<QueryTab>,
    active: usize,
    history: Vec<HistoryEntry>,
    show_structure: bool,
    show_ai: bool,
    show_history: bool,
    notice: Option<(String, bool)>,
    tab_counter: usize,
    dirty_since_save: bool,
}

impl QueryApp {
    pub fn new(ctx: &egui::Context) -> Self {
        theme::apply(ctx);
        let workspace = match services::config_dir() {
            Some(dir) => Workspace::persistent(&dir).or_else(|error| {
                log::warn!("profile storage unavailable ({error}); running without it");
                Workspace::ephemeral()
            }),
            None => Workspace::ephemeral(),
        }
        .unwrap_or_else(|error| {
            // Without a tokio runtime nothing can run; say so and exit rather than show a dead window.
            eprintln!("redrob-query: could not start the engine: {error}");
            std::process::exit(1);
        });
        let repaint = ctx.clone();
        workspace.set_wake(move || repaint.request_repaint());

        let saved = WorkspaceFile::load().unwrap_or_default();
        let mut tabs: Vec<QueryTab> = saved
            .tabs
            .iter()
            .map(|tab| QueryTab::new(tab.title.clone(), tab.text.clone(), None))
            .collect();
        for (tab, saved) in tabs.iter_mut().zip(&saved.tabs) {
            tab.connection_id = saved.connection_id;
        }
        let profiles = ProfilesPanel::new(&workspace);
        let ai = AiDock::new(&workspace);
        let account = AccountWatch::new(&workspace);
        let tab_counter = tabs.len();
        Self {
            profiles,
            schema: SchemaBrowser::default(),
            structure: StructurePanel::default(),
            changes: ChangesPanel::default(),
            ai,
            account,
            tabs,
            active: saved.active_tab.min(tab_counter.saturating_sub(1)),
            history: saved.history,
            show_structure: saved.structure_open,
            show_ai: saved.ai_dock_open,
            show_history: false,
            notice: None,
            tab_counter,
            dirty_since_save: false,
            workspace,
        }
    }

    fn notify(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        if is_error {
            log::warn!("{text}");
        }
        self.notice = Some((text, is_error));
    }

    fn profile(&self, id: Option<Uuid>) -> Option<&ConnectionProfile> {
        id.and_then(|id| self.profiles.profile(id))
    }

    fn save_workspace(&mut self) {
        let file = WorkspaceFile {
            schema: 1,
            tabs: self
                .tabs
                .iter()
                .map(|tab| SavedTab {
                    title: tab.title.clone(),
                    text: tab.text.clone(),
                    connection_id: tab.connection_id,
                })
                .collect(),
            active_tab: self.active,
            history: self.history.clone(),
            ai_dock_open: self.show_ai,
            structure_open: self.show_structure,
        };
        file.save();
        self.dirty_since_save = false;
    }

    fn open_tab_for(&mut self, profile: &ConnectionProfile, text: Option<String>) {
        // Re-use an open tab on this connection that is still its untouched starter.
        if text.is_none()
            && let Some(index) = self.tabs.iter().position(|t| {
                t.connection_id == Some(profile.id)
                    && t.text.trim() == crate::highlight::starter_query(profile.kind).trim()
                    && t.result.is_none()
            })
        {
            self.active = index;
            return;
        }
        self.tab_counter += 1;
        let mut tab = match text {
            Some(text) => QueryTab::new(format!("{} {}", profile.name, self.tab_counter), text, Some(profile)),
            None => QueryTab::for_connection(profile, self.tab_counter),
        };
        tab.focus_editor = true;
        if self.tabs.len() >= crate::workspace::MAX_TABS {
            self.notify(format!("At most {} tabs are kept; close one first.", crate::workspace::MAX_TABS), true);
            return;
        }
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.dirty_since_save = true;
    }

    /// Tabs that were restored with a connection id get the live profile's dialect once the
    /// profile list arrives.
    fn bind_restored_tabs(&mut self) {
        for tab in &mut self.tabs {
            if let Some(profile) = tab.connection_id.and_then(|id| self.profiles.profile(id)) {
                let dialect = Dialect::for_kind(profile.kind);
                if tab.dialect != dialect || tab.read_only_profile != profile.read_only {
                    tab.set_connection(profile);
                }
            }
        }
    }

    fn poll(&mut self) {
        let mut profile_events = Vec::new();
        self.profiles.poll(&self.workspace, &mut profile_events);
        let mut connected_now = Vec::new();
        for event in profile_events {
            match event {
                ProfilesEvent::OpenQuery(profile) => self.open_tab_for(&profile, None),
                ProfilesEvent::Connected(id) => connected_now.push(id),
                ProfilesEvent::Disconnected(id) => {
                    self.schema.forget(id);
                    self.structure.forget(id);
                }
                ProfilesEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }
        for id in connected_now {
            self.schema.load_root(&self.workspace, id);
        }
        self.bind_restored_tabs();

        let mut schema_events = Vec::new();
        self.schema.poll(&mut schema_events);
        self.handle_schema_events(schema_events);

        let mut structure_events = Vec::new();
        self.structure.poll(&mut structure_events);
        for event in structure_events {
            match event {
                StructureEvent::Changed(id) => {
                    self.schema.load_root(&self.workspace, id);
                    self.notify("Table change applied", false);
                }
                StructureEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }

        let mut changes_events = Vec::new();
        self.changes.poll(&mut changes_events);
        for event in changes_events {
            match event {
                ChangesEvent::Applied(result) => {
                    self.notify(format!("Applied {} row change(s) in {} ms", result.rows_affected, result.elapsed_ms), false);
                    // Re-run the tab so the grid shows the committed state.
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        if let Some(grid) = &mut tab.result {
                            grid.clear_changes();
                        }
                        tab.run_now(&self.workspace, None);
                    }
                }
                ChangesEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }

        let mut ai_events = Vec::new();
        self.ai.poll(&mut ai_events);
        for event in ai_events {
            match event {
                AiEvent::Insert(draft) => {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.insert(draft);
                        tab.focus_editor = true;
                    } else {
                        self.notify("Open a query tab to insert the draft into", true);
                    }
                }
                AiEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }

        if self.account.poll(&self.workspace) {
            self.ai.refresh_status(&self.workspace);
            let text = if self.account.status.signed_in() {
                format!("Signed in: {}", self.account.status.label)
            } else {
                "Signed out of Redrob".to_owned()
            };
            self.notify(text, false);
        }

        for index in 0..self.tabs.len() {
            let profile = self.tabs[index].connection_id.and_then(|id| self.profiles.profile(id)).cloned();
            self.tabs[index].poll(&self.workspace, profile.as_ref());
            for finished in self.tabs[index].take_finished() {
                self.history.push(HistoryEntry {
                    connection_id: finished.connection_id,
                    connection_name: finished.connection_name,
                    query: finished.query,
                    ran_at_unix: now_unix(),
                    rows: finished.rows,
                    elapsed_ms: finished.elapsed_ms,
                    ok: finished.ok,
                });
                if self.history.len() > crate::workspace::MAX_HISTORY {
                    let drop = self.history.len() - crate::workspace::MAX_HISTORY;
                    self.history.drain(..drop);
                }
                self.dirty_since_save = true;
            }
        }
    }

    fn handle_schema_events(&mut self, events: Vec<SchemaEvent>) {
        for event in events {
            match event {
                SchemaEvent::Select { connection_id, node } => {
                    if let Some(kind) = self.profiles.profile(connection_id).map(|p| p.kind) {
                        self.structure.show_table(&self.workspace, connection_id, kind, node);
                        self.show_structure = true;
                    }
                }
                SchemaEvent::Preview { connection_id, node } => {
                    if let Some(profile) = self.profiles.profile(connection_id).cloned() {
                        let query = preview_query(profile.kind, &node);
                        self.open_tab_for(&profile, Some(query));
                        if let Some(tab) = self.tabs.get_mut(self.active) {
                            tab.title = node.name.clone();
                            tab.run_now(&self.workspace, None);
                        }
                    }
                }
                SchemaEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(services::APP_NAME).strong().size(16.0));
            ui.label(RichText::new(env!("CARGO_PKG_VERSION")).small().weak());
            ui.separator();
            ui.toggle_value(&mut self.show_structure, "Structure");
            ui.toggle_value(&mut self.show_history, "History");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.toggle_value(&mut self.show_ai, "Ask").changed() {
                    self.dirty_since_save = true;
                }
                ui.separator();
                let status = &self.account.status;
                if status.signed_in() {
                    theme::status_dot(ui, theme::SUCCESS);
                    ui.label(RichText::new(&status.label).small())
                        .on_hover_text(self.account.describe_paths());
                } else {
                    let weak = ui.visuals().weak_text_color();
                    theme::status_dot(ui, weak);
                    if ui.small_button("Sign in with Redrob Launcher").on_hover_text(self.account.describe_paths()).clicked() {
                        let _ = webbrowser::open(Workspace::launcher_url());
                    }
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            match &self.notice {
                Some((text, is_error)) => {
                    let colour = theme::tone_for_error(ui, *is_error);
                    ui.label(RichText::new(text).small().color(colour));
                }
                None => {
                    ui.label(RichText::new("Ready").small().weak());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let connected = self
                    .profiles
                    .profiles
                    .iter()
                    .filter(|p| self.profiles.is_connected(p.id))
                    .count();
                ui.label(RichText::new(format!("{connected} connected · {} tabs", self.tabs.len())).small().weak());
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let mut close: Option<usize> = None;
        let mut new_tab = false;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let mut title = tab.title.clone();
                        if tab.is_running() {
                            title.push_str(" ⟳");
                        } else if tab.result.as_ref().is_some_and(|g| g.has_changes()) {
                            title.push_str(" •");
                        }
                        let selected = index == self.active;
                        let text = if selected {
                            RichText::new(title).color(theme::REDROB_BLUE).strong()
                        } else {
                            RichText::new(title)
                        };
                        let response = ui.selectable_label(selected, text);
                        if response.clicked() {
                            self.active = index;
                        }
                        if response.middle_clicked() {
                            close = Some(index);
                        }
                        response.context_menu(|ui| {
                            if ui.button("Close").clicked() {
                                close = Some(index);
                                ui.close();
                            }
                        });
                    }
                    if ui.button("＋").on_hover_text("New tab on the selected profile").clicked() {
                        new_tab = true;
                    }
                });
            });
        if let Some(index) = close {
            self.tabs.remove(index);
            if self.active >= self.tabs.len() {
                self.active = self.tabs.len().saturating_sub(1);
            }
            self.dirty_since_save = true;
        }
        if new_tab {
            let selected = self
                .profiles
                .selected
                .and_then(|id| self.profiles.profile(id))
                .cloned()
                .or_else(|| {
                    self.profiles
                        .profiles
                        .iter()
                        .find(|p| self.profiles.is_connected(p.id))
                        .cloned()
                });
            match selected {
                Some(profile) if self.profiles.is_connected(profile.id) => {
                    self.tab_counter += 1;
                    let mut tab = QueryTab::for_connection(&profile, self.tab_counter);
                    tab.focus_editor = true;
                    self.tabs.push(tab);
                    self.active = self.tabs.len() - 1;
                    self.dirty_since_save = true;
                }
                Some(profile) => self.profiles.connect(&self.workspace, profile.id),
                None => self.notify("Select or connect a profile first", true),
            }
        }
    }

    fn history_ui(&mut self, ui: &mut Ui) {
        ui.heading("History");
        let mut rerun: Option<HistoryEntry> = None;
        egui::ScrollArea::vertical()
            .id_salt("history")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.history.is_empty() {
                    ui.label(RichText::new("Nothing has run yet.").weak());
                }
                for entry in self.history.iter().rev() {
                    let one_line: String = entry.query.split_whitespace().collect::<Vec<_>>().join(" ");
                    let shown: String = one_line.chars().take(90).collect();
                    let colour = if entry.ok { ui.visuals().text_color() } else { theme::DANGER };
                    let response = ui.add(
                        egui::Label::new(RichText::new(shown).monospace().small().color(colour))
                            .sense(egui::Sense::click())
                            .truncate(),
                    );
                    let meta = format!(
                        "{} · {} rows · {} ms",
                        entry.connection_name, entry.rows, entry.elapsed_ms
                    );
                    ui.label(RichText::new(meta).small().weak());
                    if response.double_clicked() {
                        rerun = Some(entry.clone());
                    }
                    response.on_hover_text(format!("{}\n\nDouble-click to open in a tab", entry.query));
                    ui.separator();
                }
            });
        if let Some(entry) = rerun {
            if let Some(profile) = self.profile(entry.connection_id).cloned() {
                self.open_tab_for(&profile, Some(entry.query));
            } else {
                self.notify("That connection is not available any more", true);
            }
        }
    }

    fn central(&mut self, ui: &mut Ui) {
        self.tab_bar(ui);
        ui.separator();
        if self.tabs.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.3);
                ui.label(RichText::new("Connect a profile on the left, then open a query tab.").weak());
                ui.label(RichText::new("The built-in Demo SQLite database works offline.").small().weak());
            });
            return;
        }
        let active = self.active.min(self.tabs.len() - 1);
        self.active = active;
        let profile = self.tabs[active].connection_id.and_then(|id| self.profiles.profile(id)).cloned();
        let read_only = profile.as_ref().is_none_or(|p| p.read_only);
        let collections: Vec<MetadataNode> = profile
            .as_ref()
            .filter(|p| p.kind == DatabaseKind::MongoDb)
            .map(|p| self.schema.roots(p.id).to_vec())
            .unwrap_or_default();

        let mut notice = None;
        egui::Panel::top("editor-pane")
            .resizable(true)
            .default_size(260.0)
            .size_range(120.0..=f32::INFINITY)
            .show(ui, |ui| {
                notice = self.tabs[active].editor_ui(ui, &self.workspace, profile.as_ref(), &collections);
            });
        if let Some(text) = notice {
            self.notify(text, false);
        }

        if self.show_structure {
            egui::Panel::right("structure-pane")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| {
                    self.structure.ui(ui, &self.workspace, read_only);
                });
        }
        let tab = &mut self.tabs[active];
        if let Some(grid) = &mut tab.result
            && grid.has_changes()
        {
            egui::Panel::bottom("changes-pane")
                .resizable(true)
                .default_size(160.0)
                .show(ui, |ui| {
                    self.changes.ui(ui, &self.workspace, grid, tab.source.as_ref(), read_only);
                });
        }
        egui::CentralPanel::default().show(ui, |ui| match &mut tab.result {
            Some(grid) => grid.ui(ui),
            None => {
                ui.label(RichText::new("Run a query to see its rows here (Ctrl+Enter).").weak());
            }
        });
    }
}

impl eframe::App for QueryApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.poll();

        egui::Panel::top("top-bar").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status-bar").show(ui, |ui| self.status_bar(ui));
        egui::Panel::left("side")
            .resizable(true)
            .default_size(280.0)
            .size_range(200.0..=600.0)
            .show(ui, |ui| {
                let mut events = Vec::new();
                self.profiles.ui(ui, &self.workspace, &mut events);
                for event in events {
                    match event {
                        ProfilesEvent::OpenQuery(profile) => self.open_tab_for(&profile, None),
                        ProfilesEvent::Connected(id) => self.schema.load_root(&self.workspace, id),
                        ProfilesEvent::Disconnected(id) => {
                            self.schema.forget(id);
                            self.structure.forget(id);
                        }
                        ProfilesEvent::Notice(text, is_error) => self.notify(text, is_error),
                    }
                }
                ui.separator();
                let connected: Vec<ConnectionProfile> = self
                    .profiles
                    .profiles
                    .iter()
                    .filter(|p| self.profiles.is_connected(p.id))
                    .cloned()
                    .collect();
                let refs: Vec<&ConnectionProfile> = connected.iter().collect();
                let mut events = Vec::new();
                self.schema.ui(ui, &self.workspace, &refs, &mut events);
                self.handle_schema_events(events);
            });
        if self.show_ai {
            egui::Panel::right("ai-dock")
                .resizable(true)
                .default_size(340.0)
                .size_range(260.0..=700.0)
                .show(ui, |ui| {
                    let active = self.tabs.get(self.active).and_then(|tab| {
                        let profile = tab.connection_id.and_then(|id| self.profiles.profile(id))?;
                        let language = match tab.dialect {
                            Dialect::Sql => QueryLanguage::Sql,
                            Dialect::MongoJson => QueryLanguage::MongoJson,
                        };
                        Some((profile.kind, language, tab.text.clone()))
                    });
                    let mut events = Vec::new();
                    self.ai.ui(ui, &self.workspace, active, &mut events);
                    for event in events {
                        match event {
                            AiEvent::Insert(draft) => {
                                if let Some(tab) = self.tabs.get_mut(self.active) {
                                    tab.insert(draft);
                                    tab.focus_editor = true;
                                }
                            }
                            AiEvent::Notice(text, is_error) => self.notify(text, is_error),
                        }
                    }
                });
        }
        if self.show_history {
            egui::Panel::right("history-pane")
                .resizable(true)
                .default_size(300.0)
                .show(ui, |ui| self.history_ui(ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.central(ui));

        if self.dirty_since_save {
            self.save_workspace();
        }
    }

    fn on_exit(&mut self) {
        self.save_workspace();
    }
}
