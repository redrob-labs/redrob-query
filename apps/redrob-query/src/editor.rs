//! Query tabs: the editor with syntax colouring above, the result area below (grid, messages,
//! changes), and each tab's connection, dirty state and file.
//!
//! Ctrl+Enter runs the selection, else the statement under the cursor (SQL) or the whole text
//! (Mongo JSON); F5 runs the whole text, a read-only batch statement by statement; Esc cancels the
//! run in flight. Ctrl+Shift+F formats. Ctrl+S saves the tab's text to a file (asking for one the
//! first time). Errors land in the Messages tab and on the status line, never in a dialog.

use std::path::PathBuf;
use std::time::Instant;

use egui::{RichText, Ui};
use redrob_query_core::{
    ConnectionProfile, DatabaseKind, Pending, QueryClassification, QueryRequest, QueryResultPage,
    Workspace, query::classify_sql,
};
use uuid::Uuid;

use crate::changes::{ChangesPanel, ResultSource};
use crate::grid::{GridAction, ResultGrid};
use crate::highlight::{self, Dialect};
use crate::i18n::{t, tf};
use crate::mongo::MongoStrip;
use crate::theme::{self, Tokens};

const DEFAULT_LIMIT: usize = 500;

/// A run that finished, for the history list.
pub struct Finished {
    pub connection_id: Option<Uuid>,
    pub connection_name: String,
    pub query: String,
    pub rows: usize,
    pub elapsed_ms: u64,
    pub ok: bool,
}

/// Which pane the result area shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ResultView {
    #[default]
    Grid,
    Messages,
    Changes,
}

/// One line of the Messages pane.
#[derive(Clone, Debug)]
pub struct Message {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// What the result area asked the app for this frame.
pub enum TabEvent {
    /// Put this on the status line (an error in the error colour).
    Status(String, bool),
}

pub struct QueryTab {
    pub title: String,
    pub text: String,
    /// The text as last opened or saved: a difference is the dirty dot.
    pub saved_text: String,
    pub connection_id: Option<Uuid>,
    pub dialect: Dialect,
    pub result: Option<ResultGrid>,
    pub source: Option<ResultSource>,
    pub messages: Vec<Message>,
    pub view: ResultView,
    pub mongo: MongoStrip,
    pub limit: usize,
    pub offset: usize,
    pub file: Option<PathBuf>,
    run: Option<Pending<QueryResultPage>>,
    run_started: Option<Instant>,
    keys: Option<Pending<Vec<String>>>,
    /// Statements still to run in a batch, and how many ran.
    queue: Vec<String>,
    ran_in_batch: usize,
    running_text: String,
    cursor_char: usize,
    finished: Vec<Finished>,
    pub read_only_profile: bool,
    pending_insert: Option<(String, bool)>,
    pub focus_editor: bool,
    events: Vec<TabEvent>,
}

impl QueryTab {
    pub fn new(title: String, text: String, connection: Option<&ConnectionProfile>) -> Self {
        let dialect = connection.map_or(Dialect::Sql, |p| Dialect::for_kind(p.kind));
        let mut mongo = MongoStrip::default();
        if dialect == Dialect::MongoJson {
            mongo.sync_from(&text);
        }
        Self {
            title,
            saved_text: text.clone(),
            text,
            connection_id: connection.map(|p| p.id),
            dialect,
            result: None,
            source: None,
            messages: Vec::new(),
            view: ResultView::Grid,
            mongo,
            limit: DEFAULT_LIMIT,
            offset: 0,
            file: None,
            run: None,
            run_started: None,
            keys: None,
            queue: Vec::new(),
            ran_in_batch: 0,
            running_text: String::new(),
            cursor_char: 0,
            finished: Vec::new(),
            read_only_profile: connection.is_none_or(|p| p.read_only),
            pending_insert: None,
            focus_editor: false,
            events: Vec::new(),
        }
    }

    pub fn for_connection(profile: &ConnectionProfile, index: usize) -> Self {
        let text = highlight::starter_query(profile.kind).to_owned();
        Self::new(tf("Query {n}", &[("n", &index.to_string())]), text, Some(profile))
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.text != self.saved_text
    }

    /// Runs finished since the last call.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// What the tab asked for since the last call.
    pub fn take_events(&mut self) -> Vec<TabEvent> {
        std::mem::take(&mut self.events)
    }

    /// Appends (or replaces an empty tab with) a draft from the AI dock or the history.
    pub fn insert(&mut self, draft: String) {
        self.pending_insert = Some((draft, false));
    }

    /// Inserts `text` at the caret (a name from the schema tree).
    pub fn insert_at_cursor(&mut self, text: String) {
        self.pending_insert = Some((text, true));
        self.focus_editor = true;
    }

    pub fn set_connection(&mut self, profile: &ConnectionProfile) {
        self.connection_id = Some(profile.id);
        self.dialect = Dialect::for_kind(profile.kind);
        self.read_only_profile = profile.read_only;
    }

    fn message(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        self.messages.push(Message {
            text: text.clone(),
            error,
            at: Instant::now(),
        });
        if self.messages.len() > 200 {
            let drop = self.messages.len() - 200;
            self.messages.drain(..drop);
        }
        if error {
            self.view = ResultView::Messages;
        }
        self.events.push(TabEvent::Status(text, error));
    }

    pub fn poll(&mut self, workspace: &Workspace, profile: Option<&ConnectionProfile>) {
        if let Some(run) = &mut self.run
            && let Some(result) = run.poll()
        {
            self.run = None;
            self.run_started = None;
            let (rows, elapsed, ok) = match &result {
                Ok(page) => (page.stats.rows_returned, page.stats.elapsed_ms, true),
                Err(_) => (0, 0, false),
            };
            self.finished.push(Finished {
                connection_id: self.connection_id,
                connection_name: profile.map(|p| p.name.clone()).unwrap_or_default(),
                query: self.running_text.clone(),
                rows,
                elapsed_ms: elapsed,
                ok,
            });
            match result {
                Ok(page) => {
                    let mut line = tf(
                        "{rows} rows in {ms} ms",
                        &[
                            ("rows", &page.stats.rows_returned.to_string()),
                            ("ms", &page.stats.elapsed_ms.to_string()),
                        ],
                    );
                    if page.stats.rows_affected > 0 {
                        line.push_str(&tf(
                            " · {n} affected",
                            &[("n", &page.stats.rows_affected.to_string())],
                        ));
                    }
                    if page.stats.truncated {
                        line.push_str(" · ");
                        line.push_str(t("truncated at the limit"));
                    }
                    let mut grid = ResultGrid::new(page, self.offset);
                    grid.json_view = self.dialect == Dialect::MongoJson;
                    self.result = Some(grid);
                    self.view = ResultView::Grid;
                    self.message(line, false);
                    self.ran_in_batch += 1;
                    if self.queue.is_empty() {
                        self.resolve_source(workspace, profile);
                    } else if let Some(connection_id) = self.connection_id {
                        let next = self.queue.remove(0);
                        self.start(workspace, connection_id, next);
                    }
                }
                Err(error) => {
                    self.queue.clear();
                    self.message(error.to_string(), true);
                }
            }
        }
        if let Some(keys) = &mut self.keys
            && let Some(result) = keys.poll()
        {
            self.keys = None;
            if let (Ok(columns), Some(grid)) = (result, &mut self.result) {
                grid.key_columns = columns;
                grid.editable = grid.key_columns.len() == 1 && !self.read_only_profile;
            }
        }
    }

    /// Works out which table the result came from (a single-table SELECT), then asks for its key.
    fn resolve_source(&mut self, workspace: &Workspace, profile: Option<&ConnectionProfile>) {
        self.source = None;
        let Some(profile) = profile else { return };
        if profile.kind == DatabaseKind::MongoDb {
            return;
        }
        if let Some((schema, table)) = single_table_of(&self.running_text, profile.kind) {
            self.source = Some(ResultSource {
                connection_id: profile.id,
                schema: schema.clone(),
                table: table.clone(),
            });
            self.keys = Some(workspace.primary_key_columns(profile.id, schema, table));
        }
    }

    fn start(&mut self, workspace: &Workspace, connection_id: Uuid, query: String) {
        self.running_text.clone_from(&query);
        self.run_started = Some(Instant::now());
        self.run = Some(workspace.execute_query(QueryRequest {
            connection_id,
            query,
            language: self.dialect.language(),
            parameters: Vec::new(),
            limit: self.limit,
            offset: self.offset,
            timeout_ms: 30_000,
        }));
    }

    /// Runs `selection` when given, else the statement under the cursor; a read-only batch runs
    /// statement by statement.
    pub fn run_now(&mut self, workspace: &Workspace, selection: Option<String>) {
        let text = match selection {
            Some(s) if !s.trim().is_empty() => s,
            _ => highlight::statement_at(&self.text, self.cursor_char, self.dialect),
        };
        self.offset = 0;
        self.run_text(workspace, text);
    }

    /// F5: the whole text.
    pub fn run_all(&mut self, workspace: &Workspace) {
        self.offset = 0;
        let text = self.text.clone();
        self.run_text(workspace, text);
    }

    /// The same statement again at the current offset (the page controls).
    fn rerun_page(&mut self, workspace: &Workspace) {
        let text = self.running_text.clone();
        if text.trim().is_empty() {
            return;
        }
        self.run_text(workspace, text);
    }

    fn run_text(&mut self, workspace: &Workspace, text: String) {
        let Some(connection_id) = self.connection_id else {
            self.message(t("Choose a connection for this tab first"), true);
            return;
        };
        if self.run.is_some() {
            return;
        }
        self.ran_in_batch = 0;
        if text.trim().is_empty() {
            self.message(t("Nothing to run"), true);
            return;
        }
        if self.dialect == Dialect::Sql
            && let Some(mut statements) = Workspace::split_read_only_batch(&text)
            && statements.len() > 1
        {
            let first = statements.remove(0);
            self.queue = statements;
            self.start(workspace, connection_id, first);
            return;
        }
        self.queue.clear();
        self.start(workspace, connection_id, text);
    }

    /// Esc: stop waiting for the run in flight.
    pub fn cancel(&mut self) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        run.cancel();
        self.run = None;
        self.run_started = None;
        self.queue.clear();
        self.message(t("Cancelled"), false);
        true
    }

    pub fn format_text(&mut self) {
        self.text = highlight::format(&self.text, self.dialect);
    }

    /// Ctrl+O's result: the file's text replaces the tab's.
    pub fn open_file(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                if let Some(stem) = path.file_stem() {
                    self.title = stem.to_string_lossy().into_owned();
                }
                self.text.clone_from(&text);
                self.saved_text = text;
                self.file = Some(path);
            }
            Err(error) => self.message(
                tf("Could not open {path}: {error}", &[
                    ("path", &path.display().to_string()),
                    ("error", &error.to_string()),
                ]),
                true,
            ),
        }
    }

    /// Ctrl+S: write the tab's text to its file, asking for one the first time.
    pub fn save_to_file(&mut self, save_as: bool) {
        if save_as || self.file.is_none() {
            let extension = if self.dialect == Dialect::MongoJson {
                "json"
            } else {
                "sql"
            };
            let Some(picked) = rfd::FileDialog::new()
                .set_file_name(format!(
                    "{}.{extension}",
                    self.title.replace(' ', "-").to_lowercase()
                ))
                .save_file()
            else {
                return;
            };
            self.file = Some(picked);
        }
        let Some(path) = self.file.clone() else { return };
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                if let Some(stem) = path.file_stem() {
                    self.title = stem.to_string_lossy().into_owned();
                }
                self.saved_text.clone_from(&self.text);
                self.events.push(TabEvent::Status(
                    tf("Saved {path}", &[("path", &path.display().to_string())]),
                    false,
                ));
            }
            Err(error) => self.message(
                tf("Could not save: {error}", &[("error", &error.to_string())]),
                true,
            ),
        }
    }

    /// The classification of the statement under the cursor, for the editor's toolbar.
    fn classification(&self) -> Option<QueryClassification> {
        (self.dialect == Dialect::Sql).then(|| {
            classify_sql(&highlight::statement_at(
                &self.text,
                self.cursor_char,
                self.dialect,
            ))
        })
    }

    /// Draws the editor half of the tab.
    pub fn editor_ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        profile: Option<&ConnectionProfile>,
        collections: &[redrob_query_core::MetadataNode],
    ) {
        let tokens = Tokens::of(ui);
        if let Some((draft, at_cursor)) = self.pending_insert.take() {
            if at_cursor {
                let at = self
                    .text
                    .char_indices()
                    .nth(self.cursor_char)
                    .map_or(self.text.len(), |(i, _)| i);
                self.text.insert_str(at, &draft);
                self.cursor_char += draft.chars().count();
            } else if self.text.trim().is_empty() {
                self.text = draft;
            } else {
                if !self.text.ends_with('\n') {
                    self.text.push('\n');
                }
                self.text.push('\n');
                self.text.push_str(&draft);
            }
        }
        // Toolbar.
        ui.horizontal(|ui| {
            let running = self.run.is_some();
            let can_run = self.connection_id.is_some() && !running;
            if ui
                .add_enabled(can_run, theme::primary_button(ui, t("▶ Run")))
                .on_hover_text(t("Ctrl+Enter: the selection or the statement under the cursor"))
                .clicked()
            {
                self.run_now(workspace, None);
            }
            if ui
                .add_enabled(can_run, egui::Button::new(t("Run all")))
                .on_hover_text(t("F5: the whole text"))
                .clicked()
            {
                self.run_all(workspace);
            }
            if running {
                if ui
                    .button(t("Cancel"))
                    .on_hover_text(t("Esc"))
                    .clicked()
                {
                    self.cancel();
                }
                if !self.queue.is_empty() || self.ran_in_batch > 0 {
                    ui.label(
                        RichText::new(tf(
                            "statement {i} of {n}",
                            &[
                                ("i", &(self.ran_in_batch + 1).to_string()),
                                (
                                    "n",
                                    &(self.ran_in_batch + 1 + self.queue.len()).to_string(),
                                ),
                            ],
                        ))
                        .small(),
                    );
                }
            }
            if ui
                .button(t("Format"))
                .on_hover_text("Ctrl+Shift+F")
                .clicked()
            {
                self.format_text();
            }
            ui.label(RichText::new(t("Limit")).small().color(tokens.text_faint));
            ui.add(
                egui::DragValue::new(&mut self.limit)
                    .range(1..=10_000)
                    .speed(10),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if profile.is_some() {
                    if let Some(c) = self.classification() {
                        let (text, colour) = match c {
                            QueryClassification::ReadOnly => (t("read-only"), tokens.success),
                            QueryClassification::Mutation => {
                                (t("mutation (edit through the grid)"), tokens.warning)
                            }
                            QueryClassification::Destructive => {
                                (t("destructive (refused)"), tokens.danger)
                            }
                            QueryClassification::TransactionControl => {
                                (t("transaction control (refused)"), tokens.warning)
                            }
                            QueryClassification::Unknown => {
                                (t("unknown (refused)"), tokens.warning)
                            }
                        };
                        ui.label(RichText::new(text).small().color(colour));
                    }
                } else {
                    ui.label(
                        RichText::new(t("No connection: pick one in the Connection menu"))
                            .small()
                            .color(tokens.warning),
                    );
                }
            });
        });
        if self.dialect == Dialect::MongoJson
            && let Some(template) = self.mongo.ui(ui, collections)
        {
            self.text = template;
        }

        // The editor itself.
        let dialect = self.dialect;
        let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
            let job = highlight::layout(ui, text.as_str(), dialect, wrap);
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let height = ui.available_height();
        let mut output = egui::TextEdit::multiline(&mut self.text)
            .code_editor()
            .id_salt("query-editor")
            .desired_width(f32::INFINITY)
            .desired_rows(6)
            .min_size(egui::vec2(0.0, height))
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        if self.focus_editor {
            output.response.request_focus();
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), output.response.id) {
                let range = egui::text::CCursorRange::one(egui::text::CCursor::new(
                    self.cursor_char,
                ));
                state.cursor.set_char_range(Some(range));
                state.store(ui.ctx(), output.response.id);
                output.cursor_range = Some(range);
            }
            self.focus_editor = false;
        }
        if let Some(range) = output.cursor_range {
            self.cursor_char = range.primary.index.0;
        }
        let selection = output
            .cursor_range
            .filter(|r| !r.is_empty())
            .map(|r| r.slice_str(&self.text).to_owned());
        let focused = output.response.has_focus();
        let run = ui.input_mut(|i| {
            focused && i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)
        });
        let format = ui.input_mut(|i| {
            focused
                && i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::F,
                )
        });
        if run {
            self.run_now(workspace, selection);
        }
        if format {
            self.format_text();
        }
        if self.dialect == Dialect::MongoJson && output.response.changed() {
            self.mongo.sync_from(&self.text);
        }
    }

    /// The result area: a tab strip (grid, messages, changes with its badge) and the pane.
    pub fn result_ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        changes: &mut ChangesPanel,
        kind: Option<DatabaseKind>,
    ) {
        let tokens = Tokens::of(ui);
        let staged = self
            .result
            .as_ref()
            .map_or(0, |g| g.staged.len() + g.deleted.len());
        let errors = self.messages.iter().filter(|m| m.error).count();
        egui::Frame::NONE
            .fill(tokens.tab_strip)
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (view, label, count, count_colour) in [
                        (ResultView::Grid, t("Grid"), 0, tokens.accent),
                        (ResultView::Messages, t("Messages"), errors, tokens.danger),
                        (ResultView::Changes, t("Changes"), staged, tokens.warning),
                    ] {
                        let selected = self.view == view;
                        let text = if selected {
                            RichText::new(label).color(tokens.accent).strong()
                        } else {
                            RichText::new(label)
                        };
                        if ui.selectable_label(selected, text).clicked() {
                            self.view = view;
                        }
                        if count > 0 {
                            theme::badge(ui, &count.to_string(), count_colour, egui::Color32::WHITE);
                        }
                        ui.add_space(4.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(started) = self.run_started {
                            ui.spinner();
                            ui.label(
                                RichText::new(tf(
                                    "running… {s} s",
                                    &[("s", &format!("{:.1}", started.elapsed().as_secs_f32()))],
                                ))
                                .small()
                                .color(tokens.text_faint),
                            );
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_millis(100));
                        }
                    });
                });
            });
        match self.view {
            ResultView::Grid => match &mut self.result {
                Some(grid) => match grid.ui(ui) {
                    GridAction::None => {}
                    GridAction::PreviousPage => {
                        self.offset = self.offset.saturating_sub(self.limit);
                        self.rerun_page(workspace);
                    }
                    GridAction::NextPage => {
                        self.offset += self.limit;
                        self.rerun_page(workspace);
                    }
                },
                None => {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(t("Run a statement to see its rows here (Ctrl+Enter)."))
                                .color(tokens.text_faint),
                        );
                    });
                }
            },
            ResultView::Messages => self.messages_ui(ui),
            ResultView::Changes => match &mut self.result {
                Some(grid) => changes.ui(
                    ui,
                    workspace,
                    grid,
                    self.source.as_ref(),
                    kind,
                    self.read_only_profile,
                ),
                None => {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(t("Edits need a result from one table."))
                                .color(tokens.text_faint),
                        );
                    });
                }
            },
        }
    }

    fn messages_ui(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(t("Clear")).clicked() {
                    self.messages.clear();
                }
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("messages")
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.label(RichText::new(t("No messages yet.")).color(tokens.text_faint));
                }
                for message in &self.messages {
                    let colour = if message.error {
                        tokens.danger
                    } else {
                        tokens.text_dim
                    };
                    let ago = message.at.elapsed().as_secs();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(tf("{s} s ago", &[("s", &ago.to_string())]))
                                .small()
                                .color(tokens.text_faint),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(&message.text).monospace().color(colour))
                                .wrap(),
                        );
                    });
                }
            });
    }
}

/// `(schema, table)` of a SELECT that reads from exactly one table with no join, so the result can
/// be edited by its primary key. Quoted identifiers are unquoted; anything else is `None`.
pub fn single_table_of(sql: &str, kind: DatabaseKind) -> Option<(Option<String>, String)> {
    let upper = sql.to_ascii_uppercase();
    let words: Vec<&str> = upper.split_whitespace().collect();
    if words.first().copied() != Some("SELECT") {
        return None;
    }
    if words.iter().any(|w| {
        matches!(
            *w,
            "JOIN" | "UNION" | "GROUP" | "DISTINCT" | "INTERSECT" | "EXCEPT"
        )
    }) || upper.matches("SELECT").count() != 1
        || upper.contains(" FROM (")
    {
        return None;
    }
    // The token after FROM, in the original text's case.
    let from_at = upper.find(" FROM ")?;
    let after = sql[from_at + 6..].trim_start();
    let end = after
        .find(|c: char| c.is_whitespace() || c == ';' || c == ',')
        .unwrap_or(after.len());
    let target = &after[..end];
    if target.is_empty() || target.contains('(') {
        return None;
    }
    // An alias or a second table after a comma means more than one relation.
    let rest = after[end..].trim_start();
    if rest.starts_with(',') {
        return None;
    }
    let unquote = |s: &str| -> String {
        let s = s.trim();
        let quoted = (s.starts_with('"') && s.ends_with('"'))
            || (s.starts_with('`') && s.ends_with('`'))
            || (s.starts_with('[') && s.ends_with(']'));
        if quoted && s.len() >= 2 {
            s[1..s.len() - 1].replace("\"\"", "\"").replace("``", "`")
        } else {
            s.to_owned()
        }
    };
    let parts: Vec<&str> = split_qualified(target);
    match (parts.as_slice(), kind) {
        ([table], _) => Some((None, unquote(table))),
        ([schema, table], DatabaseKind::PostgreSql | DatabaseKind::MySql) => {
            Some((Some(unquote(schema)), unquote(table)))
        }
        _ => None,
    }
}

/// Splits `a.b` on dots outside quotes.
fn split_qualified(target: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    for (i, c) in target.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '`') => quote = Some(c),
            (None, '[') => quote = Some(']'),
            (None, '.') => {
                parts.push(&target[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&target[start..]);
    parts
}
