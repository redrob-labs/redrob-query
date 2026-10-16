//! The Profiles tree and the connection form.
//!
//! Built-in profiles (the Demo `SQLite` database) are immutable; user profiles can be added, edited,
//! tested and removed. Passwords go to the OS keyring through the engine and are never kept here
//! beyond the form's own text field, which is cleared when the form closes.

use std::collections::HashMap;

use egui::{RichText, Ui};
use redrob_query_core::{
    ConnectionConfig, ConnectionProfile, ConnectionState, ConnectionStatus, DatabaseKind, Pending,
    RemoveProfileOutcome, SaveProfileOutcome, Workspace,
};
use uuid::Uuid;

use crate::theme;

/// What the panel asks the app to do.
#[derive(Debug)]
pub enum ProfilesEvent {
    /// A profile connected (or was already connected): open or focus a query tab on it.
    OpenQuery(ConnectionProfile),
    /// The schema tree for this profile should be (re)loaded.
    Connected(Uuid),
    Disconnected(Uuid),
    /// Something worth the status line.
    Notice(String, bool),
}

pub struct ProfilesPanel {
    pub profiles: Vec<ConnectionProfile>,
    pub status: HashMap<Uuid, ConnectionStatus>,
    pub selected: Option<Uuid>,
    list: Option<Pending<Vec<ConnectionProfile>>>,
    connects: HashMap<Uuid, Pending<ConnectionStatus>>,
    disconnects: HashMap<Uuid, Pending<()>>,
    removes: HashMap<Uuid, Pending<RemoveProfileOutcome>>,
    pub form: Option<ProfileForm>,
    confirm_remove: Option<Uuid>,
    pub load_warnings: Vec<String>,
}

impl ProfilesPanel {
    pub fn new(workspace: &Workspace) -> Self {
        Self {
            profiles: Vec::new(),
            status: HashMap::new(),
            selected: None,
            list: Some(workspace.list_profiles()),
            connects: HashMap::new(),
            disconnects: HashMap::new(),
            removes: HashMap::new(),
            form: None,
            confirm_remove: None,
            load_warnings: workspace.profile_load_warnings(),
        }
    }

    pub fn refresh(&mut self, workspace: &Workspace) {
        self.list = Some(workspace.list_profiles());
    }

    pub fn profile(&self, id: Uuid) -> Option<&ConnectionProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn is_connected(&self, id: Uuid) -> bool {
        self.status
            .get(&id)
            .is_some_and(|s| s.state == ConnectionState::Connected)
    }

    pub fn connect(&mut self, workspace: &Workspace, id: Uuid) {
        self.connects
            .entry(id)
            .or_insert_with(|| workspace.connect(id));
    }

    /// Drains finished operations; call once per frame before drawing.
    pub fn poll(&mut self, workspace: &Workspace, events: &mut Vec<ProfilesEvent>) {
        if let Some(list) = &mut self.list
            && let Some(result) = list.poll()
        {
            self.list = None;
            match result {
                Ok(mut profiles) => {
                    profiles.sort_by(|a, b| {
                        b.built_in
                            .cmp(&a.built_in)
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });
                    self.profiles = profiles;
                }
                Err(error) => events.push(ProfilesEvent::Notice(error.to_string(), true)),
            }
        }
        let mut done = Vec::new();
        for (id, pending) in &mut self.connects {
            if let Some(result) = pending.poll() {
                done.push((*id, result));
            }
        }
        for (id, result) in done {
            self.connects.remove(&id);
            match result {
                Ok(status) => {
                    let connected = status.state == ConnectionState::Connected;
                    let message = status.message.clone();
                    self.status.insert(id, status);
                    if connected {
                        events.push(ProfilesEvent::Connected(id));
                        if let Some(profile) = self.profile(id).cloned() {
                            events.push(ProfilesEvent::OpenQuery(profile));
                        }
                    } else {
                        events.push(ProfilesEvent::Notice(message, true));
                    }
                }
                Err(error) => events.push(ProfilesEvent::Notice(error.to_string(), true)),
            }
        }
        let mut done = Vec::new();
        for (id, pending) in &mut self.disconnects {
            if let Some(result) = pending.poll() {
                done.push((*id, result));
            }
        }
        for (id, result) in done {
            self.disconnects.remove(&id);
            self.status.remove(&id);
            events.push(ProfilesEvent::Disconnected(id));
            if let Err(error) = result {
                events.push(ProfilesEvent::Notice(error.to_string(), true));
            }
        }
        let mut done = Vec::new();
        for (id, pending) in &mut self.removes {
            if let Some(result) = pending.poll() {
                done.push((*id, result));
            }
        }
        for (id, result) in done {
            self.removes.remove(&id);
            match result {
                Ok(outcome) => {
                    self.status.remove(&id);
                    if self.selected == Some(id) {
                        self.selected = None;
                    }
                    events.push(ProfilesEvent::Disconnected(id));
                    if let Some(warning) = outcome.warning {
                        events.push(ProfilesEvent::Notice(warning, false));
                    }
                    self.refresh(workspace);
                }
                Err(error) => events.push(ProfilesEvent::Notice(error.to_string(), true)),
            }
        }
        if let Some(form) = &mut self.form
            && let Some(saved) = form.poll(events)
        {
            self.form = None;
            self.selected = Some(saved.id);
            self.refresh(workspace);
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, workspace: &Workspace, events: &mut Vec<ProfilesEvent>) {
        ui.horizontal(|ui| {
            ui.heading("Profiles");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("＋ New")
                    .on_hover_text("Add a connection profile")
                    .clicked()
                {
                    self.form = Some(ProfileForm::new_profile());
                }
                if ui
                    .small_button("⟳")
                    .on_hover_text("Reload profiles")
                    .clicked()
                {
                    self.refresh(workspace);
                }
            });
        });
        for warning in &self.load_warnings {
            ui.label(RichText::new(warning).small().color(theme::CAUTION));
        }
        ui.separator();

        let mut action: Option<(Uuid, RowAction)> = None;
        egui::ScrollArea::vertical()
            .id_salt("profiles")
            .auto_shrink([false, true])
            .max_height(ui.available_height() * 0.45)
            .show(ui, |ui| {
                let (built_in, user): (Vec<_>, Vec<_>) =
                    self.profiles.iter().partition(|p| p.built_in);
                if !built_in.is_empty() {
                    ui.label(RichText::new("Built-in").small().weak());
                    for profile in built_in {
                        if let Some(a) = self.profile_row(ui, profile) {
                            action = Some(a);
                        }
                    }
                }
                ui.label(RichText::new("Your connections").small().weak());
                if user.is_empty() {
                    ui.label(RichText::new("None yet. Press New to add one.").weak());
                }
                for profile in user {
                    if let Some(a) = self.profile_row(ui, profile) {
                        action = Some(a);
                    }
                }
            });

        if let Some((id, action)) = action {
            match action {
                RowAction::Select => self.selected = Some(id),
                RowAction::Connect => {
                    self.selected = Some(id);
                    if self.is_connected(id) {
                        if let Some(profile) = self.profile(id).cloned() {
                            events.push(ProfilesEvent::OpenQuery(profile));
                        }
                    } else {
                        self.connect(workspace, id);
                    }
                }
                RowAction::Disconnect => {
                    self.disconnects
                        .entry(id)
                        .or_insert_with(|| workspace.disconnect(id));
                }
                RowAction::Edit => {
                    if let Some(profile) = self.profile(id).cloned() {
                        self.form = Some(ProfileForm::edit(profile));
                    }
                }
                RowAction::Remove => self.confirm_remove = Some(id),
            }
        }

        if let Some(id) = self.confirm_remove {
            let name = self.profile(id).map(|p| p.name.clone()).unwrap_or_default();
            let mut open = true;
            let mut decided = None;
            egui::Window::new("Remove profile")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Remove \"{name}\" and its saved password? The database itself is not touched."
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("Remove").color(egui::Color32::WHITE)).fill(theme::DANGER))
                            .clicked()
                        {
                            decided = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decided = Some(false);
                        }
                    });
                });
            match decided {
                Some(true) => {
                    self.confirm_remove = None;
                    self.removes.insert(id, workspace.remove_profile(id));
                }
                Some(false) => self.confirm_remove = None,
                None if !open => self.confirm_remove = None,
                None => {}
            }
        }

        if let Some(form) = &mut self.form {
            let mut keep = true;
            form.ui(ui.ctx(), workspace, &mut keep);
            if !keep {
                self.form = None;
            }
        }
    }

    fn profile_row(&self, ui: &mut Ui, profile: &ConnectionProfile) -> Option<(Uuid, RowAction)> {
        let mut action = None;
        let connected = self.is_connected(profile.id);
        let connecting = self.connects.contains_key(&profile.id);
        let selected = self.selected == Some(profile.id);
        ui.horizontal(|ui| {
            let dot = if connected {
                theme::SUCCESS
            } else if !profile.capabilities.connect {
                theme::DANGER
            } else {
                ui.visuals().weak_text_color()
            };
            theme::status_dot(ui, dot);
            let label = format!("{}  {}", kind_glyph(profile.kind), profile.name);
            let response = ui.selectable_label(selected, label);
            if response.clicked() {
                action = Some((profile.id, RowAction::Select));
            }
            if response.double_clicked() {
                action = Some((profile.id, RowAction::Connect));
            }
            response.context_menu(|ui| {
                if connected {
                    if ui.button("New query tab").clicked() {
                        action = Some((profile.id, RowAction::Connect));
                        ui.close();
                    }
                    if ui.button("Disconnect").clicked() {
                        action = Some((profile.id, RowAction::Disconnect));
                        ui.close();
                    }
                } else if ui.button("Connect").clicked() {
                    action = Some((profile.id, RowAction::Connect));
                    ui.close();
                }
                if !profile.built_in {
                    ui.separator();
                    if ui.button("Edit…").clicked() {
                        action = Some((profile.id, RowAction::Edit));
                        ui.close();
                    }
                    if ui.button("Remove…").clicked() {
                        action = Some((profile.id, RowAction::Remove));
                        ui.close();
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if connecting {
                    ui.spinner();
                } else if connected {
                    if ui.small_button("Query").clicked() {
                        action = Some((profile.id, RowAction::Connect));
                    }
                } else if profile.capabilities.connect && ui.small_button("Connect").clicked() {
                    action = Some((profile.id, RowAction::Connect));
                }
            });
        });
        if selected {
            ui.indent("detail", |ui| {
                let text = describe(profile);
                ui.label(RichText::new(text).small().weak());
                if let Some(status) = self.status.get(&profile.id) {
                    let mut line = status.message.clone();
                    if let Some(ms) = status.latency_ms {
                        line.push_str(&format!(" ({ms} ms)"));
                    }
                    ui.label(RichText::new(line).small());
                }
            });
        }
        action
    }
}

enum RowAction {
    Select,
    Connect,
    Disconnect,
    Edit,
    Remove,
}

pub fn kind_glyph(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::PostgreSql => "PG",
        DatabaseKind::MySql => "My",
        DatabaseKind::SQLite => "Lite",
        DatabaseKind::MongoDb => "Mongo",
        DatabaseKind::SqlServer => "MSSQL",
    }
}

pub fn kind_name(kind: DatabaseKind) -> &'static str {
    match kind {
        DatabaseKind::PostgreSql => "PostgreSQL",
        DatabaseKind::MySql => "MySQL",
        DatabaseKind::SQLite => "SQLite",
        DatabaseKind::MongoDb => "MongoDB",
        DatabaseKind::SqlServer => "SQL Server",
    }
}

fn describe(profile: &ConnectionProfile) -> String {
    let c = &profile.config;
    let mut text = kind_name(profile.kind).to_owned();
    match profile.kind {
        DatabaseKind::SQLite => {
            text.push_str(" · ");
            text.push_str(c.file_path.as_deref().unwrap_or(":memory:"));
        }
        _ => {
            if let Some(host) = &c.host {
                text.push_str(" · ");
                text.push_str(host);
                if let Some(port) = c.port.or(profile.kind.default_port()) {
                    text.push_str(&format!(":{port}"));
                }
            }
            if let Some(db) = c.database.as_deref().filter(|d| !d.is_empty()) {
                text.push_str(&format!(" / {db}"));
            }
        }
    }
    if profile.read_only {
        text.push_str(" · read-only");
    }
    text
}

/// The connection form: a window over the app with the fields of one profile.
pub struct ProfileForm {
    pub profile: ConnectionProfile,
    pub password: String,
    pub editing: bool,
    port_text: String,
    auth_source: String,
    test: Option<Pending<ConnectionStatus>>,
    test_result: Option<(String, bool)>,
    save: Option<Pending<SaveProfileOutcome>>,
    error: Option<String>,
}

impl ProfileForm {
    pub fn new_profile() -> Self {
        let kind = DatabaseKind::PostgreSql;
        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            name: String::new(),
            kind,
            config: ConnectionConfig {
                host: Some("localhost".to_owned()),
                port: kind.default_port(),
                ..ConnectionConfig::default()
            },
            read_only: true,
            built_in: false,
            capabilities: kind.capabilities(),
        };
        Self::over(profile, false)
    }

    pub fn edit(profile: ConnectionProfile) -> Self {
        Self::over(profile, true)
    }

    fn over(profile: ConnectionProfile, editing: bool) -> Self {
        let port_text = profile
            .config
            .port
            .map(|p| p.to_string())
            .unwrap_or_default();
        let auth_source = profile
            .config
            .options
            .get("authSource")
            .cloned()
            .unwrap_or_default();
        Self {
            profile,
            password: String::new(),
            editing,
            port_text,
            auth_source,
            test: None,
            test_result: None,
            save: None,
            error: None,
        }
    }

    fn poll(&mut self, events: &mut Vec<ProfilesEvent>) -> Option<ConnectionProfile> {
        if let Some(test) = &mut self.test
            && let Some(result) = test.poll()
        {
            self.test = None;
            self.test_result = Some(match result {
                Ok(status) => {
                    let ok = status.state == ConnectionState::Connected;
                    let mut text = status.message;
                    if let Some(ms) = status.latency_ms {
                        text.push_str(&format!(" ({ms} ms)"));
                    }
                    (text, ok)
                }
                Err(error) => (error.to_string(), false),
            });
        }
        if let Some(save) = &mut self.save
            && let Some(result) = save.poll()
        {
            self.save = None;
            match result {
                Ok(outcome) => {
                    if let Some(warning) = outcome.warning {
                        events.push(ProfilesEvent::Notice(warning, false));
                    }
                    events.push(ProfilesEvent::Notice(
                        format!("Saved profile \"{}\"", outcome.profile.name),
                        false,
                    ));
                    self.password.clear();
                    return Some(outcome.profile);
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        None
    }

    /// Reads the form fields back into the profile; the text port becomes a number or nothing.
    fn commit_fields(&mut self) {
        let kind = self.profile.kind;
        self.profile.capabilities = kind.capabilities();
        let config = &mut self.profile.config;
        config.port = self.port_text.trim().parse::<u16>().ok();
        config.options.clear();
        if kind == DatabaseKind::MongoDb && !self.auth_source.trim().is_empty() {
            config
                .options
                .insert("authSource".to_owned(), self.auth_source.trim().to_owned());
        }
        if kind != DatabaseKind::MongoDb {
            config.srv = false;
        }
        if kind == DatabaseKind::SQLite {
            config.host = None;
            config.port = None;
            config.username = None;
            config.database = None;
            config.tls = false;
        } else {
            config.file_path = None;
            for field in [&mut config.host, &mut config.username, &mut config.database] {
                if field.as_deref().is_some_and(|s| s.trim().is_empty()) {
                    *field = None;
                }
            }
        }
    }

    fn secret(&self) -> Option<String> {
        (!self.password.is_empty() && self.profile.kind != DatabaseKind::SQLite)
            .then(|| self.password.clone())
    }

    pub fn ui(&mut self, ctx: &egui::Context, workspace: &Workspace, keep: &mut bool) {
        let title = if self.editing {
            "Edit connection"
        } else {
            "New connection"
        };
        let mut open = true;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| self.fields(ui, workspace, keep));
        if !open {
            *keep = false;
        }
    }

    fn fields(&mut self, ui: &mut Ui, workspace: &Workspace, keep: &mut bool) {
        let busy = self.test.is_some() || self.save.is_some();
        egui::Grid::new("profile-form")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut self.profile.name).desired_width(300.0));
                ui.end_row();

                ui.label("Database");
                let before = self.profile.kind;
                egui::ComboBox::from_id_salt("kind")
                    .selected_text(kind_name(self.profile.kind))
                    .show_ui(ui, |ui| {
                        for kind in [
                            DatabaseKind::PostgreSql,
                            DatabaseKind::MySql,
                            DatabaseKind::SQLite,
                            DatabaseKind::MongoDb,
                        ] {
                            ui.selectable_value(&mut self.profile.kind, kind, kind_name(kind));
                        }
                    });
                if before != self.profile.kind {
                    self.port_text = self
                        .profile
                        .kind
                        .default_port()
                        .map(|p| p.to_string())
                        .unwrap_or_default();
                    self.test_result = None;
                }
                ui.end_row();

                if self.profile.kind == DatabaseKind::SQLite {
                    ui.label("File");
                    ui.horizontal(|ui| {
                        let path = self
                            .profile
                            .config
                            .file_path
                            .get_or_insert_with(String::new);
                        ui.add(
                            egui::TextEdit::singleline(path)
                                .desired_width(220.0)
                                .hint_text(":memory:"),
                        );
                        if ui.button("Browse…").clicked()
                            && let Some(picked) = rfd::FileDialog::new()
                                .add_filter("SQLite", &["db", "sqlite", "sqlite3"])
                                .pick_file()
                        {
                            *path = picked.display().to_string();
                        }
                    });
                    ui.end_row();
                } else {
                    ui.label("Host");
                    ui.add(
                        egui::TextEdit::singleline(
                            self.profile.config.host.get_or_insert_with(String::new),
                        )
                        .desired_width(300.0),
                    );
                    ui.end_row();
                    if !(self.profile.kind == DatabaseKind::MongoDb && self.profile.config.srv) {
                        ui.label("Port");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.port_text).desired_width(100.0),
                        );
                        ui.end_row();
                    }
                    ui.label("Database");
                    ui.add(
                        egui::TextEdit::singleline(
                            self.profile.config.database.get_or_insert_with(String::new),
                        )
                        .desired_width(300.0),
                    );
                    ui.end_row();
                    ui.label("User");
                    ui.add(
                        egui::TextEdit::singleline(
                            self.profile.config.username.get_or_insert_with(String::new),
                        )
                        .desired_width(300.0),
                    );
                    ui.end_row();
                    ui.label("Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .desired_width(300.0)
                            .hint_text(if self.editing {
                                "unchanged unless typed"
                            } else {
                                ""
                            }),
                    );
                    ui.end_row();
                    ui.label("Options");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.profile.config.tls, "TLS");
                        if self.profile.kind == DatabaseKind::MongoDb {
                            ui.checkbox(&mut self.profile.config.srv, "SRV (mongodb+srv)");
                        }
                    });
                    ui.end_row();
                    if self.profile.kind == DatabaseKind::MongoDb {
                        ui.label("authSource");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.auth_source)
                                .desired_width(200.0)
                                .hint_text("admin"),
                        );
                        ui.end_row();
                    }
                }
                ui.label("Edits");
                ui.checkbox(
                    &mut self.profile.read_only,
                    "Read-only (uncheck to allow reviewed edits)",
                );
                ui.end_row();
            });

        if let Some((text, ok)) = &self.test_result {
            let colour = if *ok { theme::SUCCESS } else { theme::DANGER };
            ui.label(RichText::new(text).color(colour));
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(theme::DANGER));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Test")).clicked() {
                self.commit_fields();
                self.error = None;
                self.test_result = None;
                match self.profile.validate() {
                    Ok(()) => {
                        self.test =
                            Some(workspace.test_connection(self.profile.clone(), self.secret()))
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            if ui
                .add_enabled(!busy, theme::primary_button("Save"))
                .clicked()
            {
                self.commit_fields();
                self.error = None;
                match self.profile.validate() {
                    Ok(()) => {
                        self.save =
                            Some(workspace.save_profile(self.profile.clone(), self.secret()))
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            if ui.button("Cancel").clicked() {
                self.password.clear();
                *keep = false;
            }
            if busy {
                ui.spinner();
            }
        });
    }
}
