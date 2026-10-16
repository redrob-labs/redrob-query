//! The shell, shaped like Redrob Canvas: a thin top bar (brand, File / Connection / Query / View /
//! Help menus, the right slot with the active connection and the account), the connections and
//! schema trees on the left, tabbed editors in the middle with the result area under a splitter,
//! the Ask dock on the right (Ctrl+Shift+A), and a status line at the bottom that carries every
//! message and error -- there is no error dialog anywhere. Every panel talks to the engine through
//! the `Workspace` and reports back as events the app routes here, so no panel reaches into another.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use egui::{RichText, Ui};
use redrob_query_core::{ConnectionProfile, DatabaseKind, MetadataNode, QueryLanguage, Workspace};
use uuid::Uuid;

use crate::account::AccountWatch;
use crate::ai_dock::{AiDock, AiEvent};
use crate::changes::{ChangesEvent, ChangesPanel};
use crate::editor::{QueryTab, TabEvent};
use crate::highlight::Dialect;
use crate::i18n::{self, t, tf};
use crate::profiles::{ProfilesEvent, ProfilesPanel, kind_name};
use crate::schema::{SchemaBrowser, SchemaEvent, preview_query};
use crate::services;
use crate::structure::{StructureEvent, StructurePanel};
use crate::theme::{self, ThemePref, Tokens};
use crate::workspace::{HistoryEntry, SavedTab, WorkspaceFile, now_unix};

/// How long a status line stays before it has faded out, in seconds.
const STATUS_SECS: f32 = 6.0;

/// The 512px app icon, for the About window.
const ABOUT_ICON_PNG: &[u8] =
    include_bytes!("../../../resources/icons/hicolor/512x512/apps/redrob-query.png");

/// The shell's status line: one line that fades out after [`STATUS_SECS`].
#[derive(Clone, Debug)]
struct Status {
    text: String,
    error: bool,
    since: Instant,
}

impl Status {
    /// The line's opacity at `now`: full, then fading through its last second; `None` once gone.
    fn alpha(&self, now: Instant) -> Option<f32> {
        let left = STATUS_SECS - now.saturating_duration_since(self.since).as_secs_f32();
        (left > 0.0).then_some(left.min(1.0))
    }
}

/// The shortcuts the shell owns, for Help > Shortcuts and the menus.
const SHORTCUTS: &[(&str, &str)] = &[
    (
        "Ctrl+Enter",
        "Run the selection or the statement under the cursor",
    ),
    ("F5", "Run the whole text"),
    ("Esc", "Cancel the run in flight"),
    ("Ctrl+T", "New query tab on the active connection"),
    ("Ctrl+W", "Close the tab"),
    ("Ctrl+1 … Ctrl+9", "Go to tab 1 … 9"),
    ("Ctrl+Shift+F", "Format the query"),
    ("Ctrl+O", "Open a .sql file into the tab"),
    ("Ctrl+S / Ctrl+Shift+S", "Save the tab's text / save as"),
    (
        "Ctrl+C",
        "Copy the selected cell, or the selected rows as TSV",
    ),
    ("Ctrl+Shift+A", "Show or hide the Ask dock"),
    ("Ctrl+N", "New connection"),
    ("Double-click", "Schema tree: insert the name at the caret"),
];

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
    shortcuts_open: bool,
    about_open: bool,
    about_icon: Option<egui::TextureHandle>,
    status: Option<Status>,
    language: i18n::Language,
    theme_pref: ThemePref,
    tab_counter: usize,
    dirty_since_save: bool,
}

impl QueryApp {
    pub fn new(ctx: &egui::Context) -> Self {
        let saved = WorkspaceFile::load().unwrap_or_default();
        let theme_pref = ThemePref::from_id(&saved.theme);
        ctx.set_fonts(theme::font_definitions());
        theme::apply(ctx, theme_pref);
        let language = i18n::Language::load();
        i18n::set_current(language.effective());

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
            shortcuts_open: false,
            about_open: false,
            about_icon: None,
            status: None,
            language,
            theme_pref,
            tab_counter,
            dirty_since_save: false,
            workspace,
        }
    }

    /// Show `text` on the status line (an error in the error colour).
    fn notify(&mut self, text: impl Into<String>, is_error: bool) {
        let text = text.into();
        if is_error {
            log::warn!("{text}");
        }
        self.status = Some(Status {
            text,
            error: is_error,
            since: Instant::now(),
        });
    }

    fn profile(&self, id: Option<Uuid>) -> Option<&ConnectionProfile> {
        id.and_then(|id| self.profiles.profile(id))
    }

    /// The connection the shell acts on: the active tab's, else the selected one.
    fn active_profile(&self) -> Option<&ConnectionProfile> {
        self.tabs
            .get(self.active)
            .and_then(|tab| self.profile(tab.connection_id))
            .or_else(|| self.profiles.active())
    }

    fn kinds(&self) -> HashMap<Uuid, DatabaseKind> {
        self.profiles
            .profiles
            .iter()
            .map(|p| (p.id, p.kind))
            .collect()
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
            theme: self.theme_pref.id().to_owned(),
        };
        file.save();
        self.dirty_since_save = false;
    }

    fn push_tab(&mut self, mut tab: QueryTab) {
        if self.tabs.len() >= crate::workspace::MAX_TABS {
            self.notify(
                tf(
                    "At most {n} tabs are kept; close one first.",
                    &[("n", &crate::workspace::MAX_TABS.to_string())],
                ),
                true,
            );
            return;
        }
        tab.focus_editor = true;
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
        self.dirty_since_save = true;
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
            self.tabs[index].focus_editor = true;
            return;
        }
        self.tab_counter += 1;
        let tab = match text {
            Some(text) => QueryTab::new(
                tf("Query {n}", &[("n", &self.tab_counter.to_string())]),
                text,
                Some(profile),
            ),
            None => QueryTab::for_connection(profile, self.tab_counter),
        };
        self.push_tab(tab);
    }

    /// Ctrl+T / File > New tab: on the active connection, connecting it first if need be.
    fn new_tab(&mut self) {
        let selected = self.active_profile().cloned().or_else(|| {
            self.profiles
                .profiles
                .iter()
                .find(|p| self.profiles.is_connected(p.id))
                .cloned()
        });
        match selected {
            Some(profile) if self.profiles.is_connected(profile.id) => {
                self.tab_counter += 1;
                let tab = QueryTab::for_connection(&profile, self.tab_counter);
                self.push_tab(tab);
            }
            Some(profile) => self.profiles.connect(&self.workspace, profile.id),
            None => self.notify(t("Select or connect a connection first"), true),
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.cancel();
        }
        self.tabs.remove(index);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
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

    fn handle_profile_events(&mut self, events: Vec<ProfilesEvent>) {
        for event in events {
            match event {
                ProfilesEvent::OpenQuery(profile) => self.open_tab_for(&profile, None),
                ProfilesEvent::Connected(id) => {
                    self.schema.load_root(&self.workspace, id);
                    if let Some(name) = self.profiles.profile(id).map(|p| p.name.clone()) {
                        self.notify(tf("Connected to {name}", &[("name", &name)]), false);
                    }
                }
                ProfilesEvent::Disconnected(id) => {
                    self.schema.forget(id);
                    self.structure.forget(id);
                }
                ProfilesEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }
    }

    fn poll(&mut self) {
        let mut profile_events = Vec::new();
        self.profiles.poll(&self.workspace, &mut profile_events);
        self.handle_profile_events(profile_events);
        self.bind_restored_tabs();

        let mut schema_events = Vec::new();
        self.schema.poll(&self.kinds(), &mut schema_events);
        self.handle_schema_events(schema_events);

        let mut structure_events = Vec::new();
        self.structure.poll(&mut structure_events);
        for event in structure_events {
            match event {
                StructureEvent::Changed(id) => {
                    self.schema.load_root(&self.workspace, id);
                    self.notify(t("Table change applied"), false);
                }
                StructureEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }

        let mut changes_events = Vec::new();
        self.changes.poll(&mut changes_events);
        for event in changes_events {
            match event {
                ChangesEvent::Applied(result) => {
                    self.notify(
                        tf(
                            "Applied {rows} row change(s) in {ms} ms",
                            &[
                                ("rows", &result.rows_affected.to_string()),
                                ("ms", &result.elapsed_ms.to_string()),
                            ],
                        ),
                        false,
                    );
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
        self.handle_ai_events(ai_events);

        if self.account.poll(&self.workspace) {
            self.ai.refresh_status(&self.workspace);
            let text = if self.account.status.signed_in() {
                tf("Signed in: {name}", &[("name", &self.account.status.label)])
            } else {
                t("Signed out of Redrob").to_owned()
            };
            self.notify(text, false);
        }

        for index in 0..self.tabs.len() {
            let profile = self.tabs[index]
                .connection_id
                .and_then(|id| self.profiles.profile(id))
                .cloned();
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
            for event in self.tabs[index].take_events() {
                match event {
                    TabEvent::Status(text, error) => {
                        if index == self.active || error {
                            self.notify(text, error);
                        }
                    }
                }
            }
        }
    }

    fn handle_ai_events(&mut self, events: Vec<AiEvent>) {
        for event in events {
            match event {
                AiEvent::Insert(draft) => {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.insert(draft);
                        tab.focus_editor = true;
                    } else {
                        self.notify(t("Open a query tab to insert the draft into"), true);
                    }
                }
                AiEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }
    }

    fn handle_schema_events(&mut self, events: Vec<SchemaEvent>) {
        for event in events {
            match event {
                SchemaEvent::Select {
                    connection_id,
                    node,
                } => {
                    if self.show_structure
                        && let Some(kind) = self.profiles.profile(connection_id).map(|p| p.kind)
                    {
                        self.structure
                            .show_table(&self.workspace, connection_id, kind, node);
                    }
                }
                SchemaEvent::Preview {
                    connection_id,
                    node,
                } => {
                    if let Some(profile) = self.profiles.profile(connection_id).cloned() {
                        let query = preview_query(profile.kind, &node);
                        self.open_tab_for(&profile, Some(query));
                        if let Some(tab) = self.tabs.get_mut(self.active) {
                            tab.title.clone_from(&node.name);
                            tab.saved_text.clone_from(&tab.text);
                            tab.run_now(&self.workspace, None);
                        }
                    }
                }
                SchemaEvent::Insert(name) => match self.tabs.get_mut(self.active) {
                    Some(tab) => tab.insert_at_cursor(name),
                    None => self.notify(t("Open a query tab first (Ctrl+T)"), true),
                },
                SchemaEvent::OpenText {
                    connection_id,
                    title,
                    text,
                } => {
                    if let Some(profile) = self.profiles.profile(connection_id).cloned() {
                        self.open_tab_for(&profile, Some(text));
                        if let Some(tab) = self.tabs.get_mut(self.active) {
                            tab.title = title;
                            tab.saved_text.clone_from(&tab.text);
                        }
                    }
                }
                SchemaEvent::Notice(text, is_error) => self.notify(text, is_error),
            }
        }
    }

    // ---- shortcuts ----------------------------------------------------------------------------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        // A modal form owns the keyboard while it is up.
        if self.profiles.form.is_some() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::T)) {
            self.new_tab();
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::W)) {
            self.close_tab(self.active);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::N)) {
            self.profiles.new_connection();
        }
        if ctx.input_mut(|i| i.consume_key(shift, Key::A)) {
            self.show_ai = !self.show_ai;
            self.dirty_since_save = true;
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F5))
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.run_all(&self.workspace);
        }
        if ctx.input(|i| i.key_pressed(Key::Escape))
            && let Some(tab) = self.tabs.get_mut(self.active)
            && tab.is_running()
        {
            tab.cancel();
        }
        for (n, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ]
        .into_iter()
        .enumerate()
        {
            if ctx.input_mut(|i| i.consume_key(cmd, key)) && n < self.tabs.len() {
                self.active = n;
            }
        }
        if ctx.input_mut(|i| i.consume_key(shift, Key::S)) {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                tab.save_to_file(true);
            }
        } else if ctx.input_mut(|i| i.consume_key(cmd, Key::S))
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.save_to_file(false);
        }
        if ctx.input_mut(|i| i.consume_key(cmd, Key::O)) {
            self.open_file();
        }
        // Ctrl+C with no text field focused copies from the grid.
        let text_focused = ctx.memory(egui::Memory::focused).is_some();
        if !text_focused
            && ctx.input_mut(|i| i.consume_key(cmd, Key::C))
            && let Some(grid) = self.tabs.get(self.active).and_then(|t| t.result.as_ref())
            && grid.copy_selection(ctx)
        {
            self.notify(t("Copied"), false);
        }
    }

    fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SQL", &["sql", "txt", "json"])
            .pick_file()
        else {
            return;
        };
        if self.tabs.is_empty() {
            self.new_tab();
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.open_file(path);
            self.dirty_since_save = true;
        }
    }

    // ---- top bar -----------------------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        let frame = theme::bar_frame(ui);
        egui::Panel::top("top-bar")
            .exact_size(theme::TOP_BAR_HEIGHT)
            .frame(frame)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(services::APP_NAME).font(theme::semibold(13.0)));
                    ui.add_space(8.0);
                    ui.separator();
                    ui.menu_button(t("File"), |ui| self.file_menu(ui));
                    ui.menu_button(t("Connection"), |ui| self.connection_menu(ui));
                    ui.menu_button(t("Query"), |ui| self.query_menu(ui));
                    ui.menu_button(t("View"), |ui| self.view_menu(ui));
                    ui.menu_button(t("Help"), |ui| self.help_menu(ui));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        self.right_slot(ui);
                    });
                });
            });
    }

    fn menu_item(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> bool {
        let clicked = ui
            .add_enabled(enabled, egui::Button::new(label).shortcut_text(shortcut))
            .clicked();
        if clicked {
            ui.close();
        }
        clicked
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        let has_tab = !self.tabs.is_empty();
        if Self::menu_item(ui, t("New tab"), "Ctrl+T", true) {
            self.new_tab();
        }
        if Self::menu_item(ui, t("Close tab"), "Ctrl+W", has_tab) {
            self.close_tab(self.active);
        }
        ui.separator();
        if Self::menu_item(ui, t("Open…"), "Ctrl+O", true) {
            self.open_file();
        }
        if Self::menu_item(ui, t("Save"), "Ctrl+S", has_tab)
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.save_to_file(false);
        }
        if Self::menu_item(ui, t("Save as…"), "Ctrl+Shift+S", has_tab)
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.save_to_file(true);
        }
        ui.separator();
        if Self::menu_item(ui, t("Quit"), "", true) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn connection_menu(&mut self, ui: &mut Ui) {
        let active = self.active_profile().cloned();
        let connected = active
            .as_ref()
            .is_some_and(|p| self.profiles.is_connected(p.id));
        if Self::menu_item(ui, t("New connection…"), "Ctrl+N", true) {
            self.profiles.new_connection();
        }
        ui.separator();
        let name = active.as_ref().map_or_else(String::new, |p| p.name.clone());
        if Self::menu_item(
            ui,
            &tf("Connect {name}", &[("name", &name)]),
            "",
            active.is_some() && !connected,
        ) && let Some(p) = &active
        {
            self.profiles.connect(&self.workspace, p.id);
        }
        if Self::menu_item(
            ui,
            &tf("Disconnect {name}", &[("name", &name)]),
            "",
            connected,
        ) && let Some(p) = &active
        {
            self.profiles.disconnect(&self.workspace, p.id);
        }
        if Self::menu_item(ui, t("Reload schema"), "", connected)
            && let Some(p) = &active
        {
            self.schema.load_root(&self.workspace, p.id);
        }
        ui.separator();
        let editable = active.as_ref().is_some_and(|p| !p.built_in);
        if Self::menu_item(ui, t("Edit…"), "", editable)
            && let Some(p) = &active
        {
            self.profiles.edit(p.id);
        }
        if Self::menu_item(ui, t("Remove…"), "", editable)
            && let Some(p) = &active
        {
            self.profiles.ask_remove(p.id);
        }
    }

    fn query_menu(&mut self, ui: &mut Ui) {
        let (can_run, running) = self.tabs.get(self.active).map_or((false, false), |t| {
            (t.connection_id.is_some() && !t.is_running(), t.is_running())
        });
        if Self::menu_item(ui, t("Run"), "Ctrl+Enter", can_run)
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.run_now(&self.workspace, None);
        }
        if Self::menu_item(ui, t("Run all"), "F5", can_run)
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.run_all(&self.workspace);
        }
        if Self::menu_item(ui, t("Cancel"), "Esc", running)
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.cancel();
        }
        ui.separator();
        if Self::menu_item(ui, t("Format"), "Ctrl+Shift+F", !self.tabs.is_empty())
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.format_text();
        }
        if Self::menu_item(ui, t("Clear result"), "", !self.tabs.is_empty())
            && let Some(tab) = self.tabs.get_mut(self.active)
        {
            tab.result = None;
            tab.source = None;
        }
    }

    fn view_menu(&mut self, ui: &mut Ui) {
        if ui
            .checkbox(&mut self.show_structure, t("Structure panel"))
            .changed()
        {
            self.dirty_since_save = true;
            ui.close();
        }
        if ui
            .checkbox(&mut self.show_history, t("History panel"))
            .changed()
        {
            ui.close();
        }
        if ui
            .add(egui::Checkbox::new(&mut self.show_ai, t("Ask dock")))
            .on_hover_text("Ctrl+Shift+A")
            .changed()
        {
            self.dirty_since_save = true;
            ui.close();
        }
    }

    fn help_menu(&mut self, ui: &mut Ui) {
        if Self::menu_item(ui, t("Shortcuts"), "", true) {
            self.shortcuts_open = true;
        }
        ui.menu_button(t("Theme"), |ui| {
            for pref in ThemePref::ALL {
                if ui.radio(self.theme_pref == pref, t(pref.label())).clicked() {
                    self.theme_pref = pref;
                    theme::set_pref(ui.ctx(), pref);
                    self.dirty_since_save = true;
                    ui.close();
                }
            }
        });
        ui.menu_button(t("Language"), |ui| self.language_menu(ui));
        ui.separator();
        if Self::menu_item(ui, t("Report an issue…"), "", true) {
            let _ = webbrowser::open("https://github.com/redrob-labs/redrob-query/issues");
        }
        if Self::menu_item(
            ui,
            &tf("About {app}", &[("app", services::APP_NAME)]),
            "",
            true,
        ) {
            self.about_open = true;
        }
    }

    fn language_menu(&mut self, ui: &mut Ui) {
        let system = self.language.system();
        if ui
            .radio(
                self.language.is_auto(),
                tf("System ({name})", &[("name", system.name)]),
            )
            .on_hover_text(t("Follow the operating system's language"))
            .clicked()
        {
            self.language.set(i18n::AUTO);
            ui.close();
        }
        ui.separator();
        let effective = self.language.effective();
        for lang in i18n::LANGUAGES {
            let selected = !self.language.is_auto() && effective.code == lang.code;
            if ui.radio(selected, lang.name).clicked() {
                self.language.set(lang.code);
                ui.close();
            }
        }
    }

    /// The right side of the top bar: the Ask toggle, the active connection with its mode badge,
    /// and the account.
    fn right_slot(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        let ask = ui
            .selectable_label(self.show_ai, t("Ask"))
            .on_hover_text(tf(
                "{what}  (Ctrl+Shift+A)",
                &[("what", t("Show or hide the Ask dock"))],
            ));
        if ask.clicked() {
            self.show_ai = !self.show_ai;
            self.dirty_since_save = true;
        }
        ui.separator();
        // The account chip.
        let status = &self.account.status;
        if status.signed_in() {
            theme::chip(ui, Some(tokens.success), &status.label).on_hover_text(format!(
                "{}\n{}",
                t("Signed in with Redrob Launcher"),
                self.account.describe_paths()
            ));
        } else if ui
            .small_button(t("Connect Redrob"))
            .on_hover_text(format!(
                "{}\n{}",
                t("Sign in with Redrob Launcher to use Ask"),
                self.account.describe_paths()
            ))
            .clicked()
        {
            let _ = webbrowser::open(Workspace::launcher_url());
        }
        // The active connection chip, with its read-only / read-write badge.
        if let Some(profile) = self.active_profile().cloned() {
            let connected = self.profiles.is_connected(profile.id);
            let (badge_text, badge_fill) = if profile.read_only {
                (t("read-only"), tokens.text_faint)
            } else {
                (t("read-write"), tokens.warning)
            };
            theme::badge(ui, badge_text, badge_fill, egui::Color32::WHITE);
            let dot = if connected {
                tokens.success
            } else if self.profiles.is_connecting(profile.id) {
                tokens.warning
            } else {
                tokens.text_faint
            };
            let chip = theme::chip(ui, Some(dot), &profile.name).on_hover_text(format!(
                "{} · {}",
                kind_name(profile.kind),
                if connected {
                    t("connected")
                } else {
                    t("not connected")
                }
            ));
            if chip.double_clicked() && !connected {
                self.profiles.connect(&self.workspace, profile.id);
            }
        }
    }

    // ---- status line -------------------------------------------------------------------------

    fn status_line(&mut self, ui: &mut Ui) {
        let frame = theme::bar_frame(ui);
        egui::Panel::bottom("status-line")
            .exact_size(theme::STATUS_HEIGHT)
            .frame(frame)
            .show(ui, |ui| {
                let tokens = Tokens::of(ui);
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    let running = self.tabs.iter().any(QueryTab::is_running);
                    if running {
                        ui.spinner();
                        ui.label(
                            RichText::new(t("Running… Esc cancels"))
                                .small()
                                .color(tokens.text_dim),
                        );
                        ui.separator();
                    }
                    match self.status.clone() {
                        Some(status) => match status.alpha(Instant::now()) {
                            Some(alpha) => {
                                let colour = if status.error {
                                    tokens.danger
                                } else {
                                    tokens.text
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&status.text)
                                            .small()
                                            .color(colour.gamma_multiply(alpha)),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&status.text);
                                ui.ctx().request_repaint_after(Duration::from_millis(100));
                            }
                            None => self.status = None,
                        },
                        None => {
                            ui.label(RichText::new(t("Ready")).small().color(tokens.text_faint));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let connected = self
                            .profiles
                            .profiles
                            .iter()
                            .filter(|p| self.profiles.is_connected(p.id))
                            .count();
                        ui.label(
                            RichText::new(tf(
                                "{connected} connected · {tabs} tabs",
                                &[
                                    ("connected", &connected.to_string()),
                                    ("tabs", &self.tabs.len().to_string()),
                                ],
                            ))
                            .small()
                            .color(tokens.text_faint),
                        );
                    });
                });
            });
    }

    // ---- centre ------------------------------------------------------------------------------

    fn tab_bar(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        let mut close: Option<usize> = None;
        let mut new_tab = false;
        egui::Frame::NONE
            .fill(tokens.tab_strip)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("tabs")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, tab) in self.tabs.iter().enumerate() {
                                let selected = index == self.active;
                                let connection = tab
                                    .connection_id
                                    .and_then(|id| self.profiles.profile(id))
                                    .map_or_else(
                                        || t("no connection").to_owned(),
                                        |p| p.name.clone(),
                                    );
                                let body = egui::TextStyle::Body.resolve(ui.style());
                                let small = egui::TextStyle::Small.resolve(ui.style());
                                let mut job = egui::text::LayoutJob::default();
                                let title_colour =
                                    if selected { tokens.accent } else { tokens.text };
                                job.append(
                                    &tab.title,
                                    0.0,
                                    egui::TextFormat::simple(body.clone(), title_colour),
                                );
                                job.append(
                                    &format!("  {connection}"),
                                    0.0,
                                    egui::TextFormat::simple(small, tokens.text_faint),
                                );
                                if tab.is_running() {
                                    job.append(
                                        "  ⟳",
                                        0.0,
                                        egui::TextFormat::simple(body.clone(), tokens.warning),
                                    );
                                } else if tab.is_dirty()
                                    || tab
                                        .result
                                        .as_ref()
                                        .is_some_and(crate::grid::ResultGrid::has_changes)
                                {
                                    job.append(
                                        "  ●",
                                        0.0,
                                        egui::TextFormat::simple(body, tokens.accent),
                                    );
                                }
                                let response = ui.selectable_label(selected, job);
                                if response.clicked() {
                                    self.active = index;
                                }
                                if response.middle_clicked() {
                                    close = Some(index);
                                }
                                response.context_menu(|ui| {
                                    if ui.button(t("Close tab")).clicked() {
                                        close = Some(index);
                                        ui.close();
                                    }
                                });
                            }
                            if ui
                                .add(egui::Button::new("＋").frame(false))
                                .on_hover_text(tf(
                                    "{what}  (Ctrl+T)",
                                    &[("what", t("New query tab on the active connection"))],
                                ))
                                .clicked()
                            {
                                new_tab = true;
                            }
                        });
                    });
            });
        if let Some(index) = close {
            self.close_tab(index);
        }
        if new_tab {
            self.new_tab();
        }
    }

    fn history_ui(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("History")).font(theme::semibold(12.5)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    self.show_history = false;
                }
                if !self.history.is_empty() && ui.small_button(t("Clear")).clicked() {
                    self.history.clear();
                    self.dirty_since_save = true;
                }
            });
        });
        ui.label(
            RichText::new(t(
                "Click to insert into the editor; double-click opens a tab.",
            ))
            .small()
            .color(tokens.text_faint),
        );
        let mut insert: Option<String> = None;
        let mut open: Option<HistoryEntry> = None;
        egui::ScrollArea::vertical()
            .id_salt("history")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.history.is_empty() {
                    ui.label(RichText::new(t("Nothing has run yet.")).color(tokens.text_faint));
                }
                for entry in self.history.iter().rev() {
                    let one_line: String =
                        entry.query.split_whitespace().collect::<Vec<_>>().join(" ");
                    let shown: String = one_line.chars().take(90).collect();
                    let colour = if entry.ok { tokens.text } else { tokens.danger };
                    let response = ui.add(
                        egui::Label::new(RichText::new(shown).monospace().small().color(colour))
                            .sense(egui::Sense::click())
                            .truncate(),
                    );
                    let meta = tf(
                        "{name} · {rows} rows · {ms} ms",
                        &[
                            ("name", &entry.connection_name),
                            ("rows", &entry.rows.to_string()),
                            ("ms", &entry.elapsed_ms.to_string()),
                        ],
                    );
                    ui.label(RichText::new(meta).small().color(tokens.text_faint));
                    if response.double_clicked() {
                        open = Some(entry.clone());
                    } else if response.clicked() {
                        insert = Some(entry.query.clone());
                    }
                    response.on_hover_text(RichText::new(&entry.query).monospace());
                    ui.separator();
                }
            });
        if let Some(text) = insert {
            match self.tabs.get_mut(self.active) {
                Some(tab) => {
                    tab.insert(text);
                    tab.focus_editor = true;
                }
                None => self.notify(t("Open a query tab first (Ctrl+T)"), true),
            }
        }
        if let Some(entry) = open {
            if let Some(profile) = self.profile(entry.connection_id).cloned() {
                self.open_tab_for(&profile, Some(entry.query));
            } else {
                self.notify(t("That connection is not available any more"), true);
            }
        }
    }

    /// First run, or no tab open: one primary action.
    fn empty_state(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.28);
            ui.label(RichText::new(t("Connect a database")).font(theme::semibold(20.0)));
            ui.add_space(6.0);
            ui.label(
                RichText::new(t(
                    "Add a PostgreSQL, MySQL, SQLite or MongoDB connection, then query it in tabs.",
                ))
                .color(tokens.text_dim),
            );
            ui.add_space(16.0);
            if ui
                .add(theme::primary_button(ui, t("New connection")))
                .clicked()
            {
                self.profiles.new_connection();
            }
            let demo = self
                .profiles
                .profiles
                .iter()
                .find(|p| p.built_in && p.capabilities.connect)
                .cloned();
            if let Some(demo) = demo {
                ui.add_space(8.0);
                if ui
                    .link(
                        RichText::new(tf("Try the {name} database", &[("name", &demo.name)]))
                            .small(),
                    )
                    .clicked()
                {
                    if self.profiles.is_connected(demo.id) {
                        self.open_tab_for(&demo, None);
                    } else {
                        self.profiles.connect(&self.workspace, demo.id);
                    }
                }
            }
        });
    }

    fn central(&mut self, ui: &mut Ui) {
        if self.tabs.is_empty() {
            self.empty_state(ui);
            return;
        }
        self.tab_bar(ui);
        let active = self.active.min(self.tabs.len() - 1);
        self.active = active;
        let profile = self.tabs[active]
            .connection_id
            .and_then(|id| self.profiles.profile(id))
            .cloned();
        let collections: Vec<MetadataNode> = profile
            .as_ref()
            .filter(|p| p.kind == DatabaseKind::MongoDb)
            .map(|p| self.schema.roots(p.id).to_vec())
            .unwrap_or_default();
        let kind = profile.as_ref().map(|p| p.kind);

        let tokens = Tokens::of(ui);
        let editor_frame = egui::Frame::NONE
            .fill(tokens.canvas)
            .inner_margin(egui::Margin::same(6));
        egui::Panel::top("editor-pane")
            .resizable(true)
            .default_size(260.0)
            .size_range(100.0..=f32::INFINITY)
            .frame(editor_frame)
            .show(ui, |ui| {
                self.tabs[active].editor_ui(ui, &self.workspace, profile.as_ref(), &collections);
            });
        let tab = &mut self.tabs[active];
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(tokens.canvas))
            .show(ui, |ui| {
                tab.result_ui(ui, &self.workspace, &mut self.changes, kind);
            });
    }

    // ---- windows -----------------------------------------------------------------------------

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let mut open = self.shortcuts_open;
        egui::Window::new(t("Shortcuts"))
            .id(egui::Id::new("shortcuts"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let tokens = Tokens::of(ui);
                egui::Grid::new("shortcuts-grid")
                    .num_columns(2)
                    .spacing([18.0, 6.0])
                    .show(ui, |ui| {
                        for (keys, what) in SHORTCUTS {
                            ui.label(RichText::new(*keys).monospace().color(tokens.accent_text));
                            ui.label(t(what));
                            ui.end_row();
                        }
                    });
            });
        self.shortcuts_open = open;
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        if self.about_icon.is_none()
            && let Ok(icon) = eframe::icon_data::from_png_bytes(ABOUT_ICON_PNG)
        {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            self.about_icon =
                Some(ctx.load_texture("about-icon", image, egui::TextureOptions::LINEAR));
        }
        let mut open = self.about_open;
        egui::Window::new(tf("About {app}", &[("app", services::APP_NAME)]))
            .id(egui::Id::new("about"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                let tokens = Tokens::of(ui);
                ui.set_width(380.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    if let Some(texture) = &self.about_icon {
                        ui.add(
                            egui::Image::from_texture((texture.id(), texture.size_vec2()))
                                .fit_to_exact_size(egui::vec2(64.0, 64.0)),
                        );
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(services::APP_NAME).font(theme::semibold(16.0)));
                    ui.label(tf(
                        "Version {version}",
                        &[("version", env!("CARGO_PKG_VERSION"))],
                    ));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(t(
                            "GPL-3.0-or-later. Third-party notices: NOTICE and UPSTREAM_NOTICES.md.",
                        ))
                        .small()
                        .color(tokens.text_dim),
                    );
                    ui.add_space(8.0);
                });
            });
        self.about_open = open;
    }
}

impl eframe::App for QueryApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.poll();
        self.handle_shortcuts(ui.ctx());

        self.top_bar(ui);
        self.status_line(ui);

        let panel_frame = theme::panel_frame(ui);
        egui::Panel::left("side")
            .resizable(true)
            .default_size(280.0)
            .size_range(200.0..=600.0)
            .frame(panel_frame)
            .show(ui, |ui| {
                let mut events = Vec::new();
                self.profiles.ui(ui, &self.workspace, &mut events);
                self.handle_profile_events(events);
                ui.separator();
                let profile = self.active_profile().cloned();
                let connected = profile
                    .as_ref()
                    .is_some_and(|p| self.profiles.is_connected(p.id));
                let mut events = Vec::new();
                self.schema.ui(
                    ui,
                    &self.workspace,
                    profile.as_ref(),
                    connected,
                    &mut events,
                );
                self.handle_schema_events(events);
            });
        if self.show_ai {
            egui::Panel::right("ai-dock")
                .resizable(true)
                .default_size(340.0)
                .size_range(260.0..=700.0)
                .frame(panel_frame)
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
                    self.handle_ai_events(events);
                });
        }
        if self.show_history {
            egui::Panel::right("history-pane")
                .resizable(true)
                .default_size(300.0)
                .frame(panel_frame)
                .show(ui, |ui| self.history_ui(ui));
        }
        if self.show_structure {
            let read_only = self.active_profile().is_none_or(|p| p.read_only);
            egui::Panel::right("structure-pane")
                .resizable(true)
                .default_size(300.0)
                .frame(panel_frame)
                .show(ui, |ui| {
                    self.structure.ui(ui, &self.workspace, read_only);
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.central(ui));

        self.shortcuts_window(ui.ctx());
        self.about_window(ui.ctx());

        if self.dirty_since_save {
            self.save_workspace();
        }
    }

    fn on_exit(&mut self) {
        self.save_workspace();
    }
}
