//! Query tabs: the editor with syntax colouring, the run / format / save shortcuts, and each tab's
//! result grid with its source table (for the changes and structure panels).
//!
//! Ctrl+Enter runs the selection, else the statement under the cursor (SQL) or the whole text
//! (Mongo JSON); a batch of several read-only statements runs them one by one and shows the last
//! result. Ctrl+Shift+F formats. Ctrl+S saves the tab's text to a file (asking for one the first
//! time). The workspace itself (tabs, history) is saved by the app.

use std::path::PathBuf;

use egui::{RichText, Ui};
use redrob_query_core::{
    ConnectionProfile, DatabaseKind, Pending, QueryClassification, QueryRequest, QueryResultPage,
    Workspace, query::classify_sql,
};
use uuid::Uuid;

use crate::changes::ResultSource;
use crate::grid::ResultGrid;
use crate::highlight::{self, Dialect};
use crate::mongo::MongoStrip;
use crate::theme;

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

pub struct QueryTab {
    pub title: String,
    pub text: String,
    pub connection_id: Option<Uuid>,
    pub dialect: Dialect,
    pub result: Option<ResultGrid>,
    pub source: Option<ResultSource>,
    pub error: Option<String>,
    pub mongo: MongoStrip,
    pub limit: usize,
    pub file: Option<PathBuf>,
    run: Option<Pending<QueryResultPage>>,
    keys: Option<Pending<Vec<String>>>,
    /// Statements still to run in a batch, and how many ran.
    queue: Vec<String>,
    ran_in_batch: usize,
    running_text: String,
    cursor_char: usize,
    finished: Vec<Finished>,
    pub read_only_profile: bool,
    pending_insert: Option<String>,
    pub focus_editor: bool,
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
            text,
            connection_id: connection.map(|p| p.id),
            dialect,
            result: None,
            source: None,
            error: None,
            mongo,
            limit: DEFAULT_LIMIT,
            file: None,
            run: None,
            keys: None,
            queue: Vec::new(),
            ran_in_batch: 0,
            running_text: String::new(),
            cursor_char: 0,
            finished: Vec::new(),
            read_only_profile: connection.is_none_or(|p| p.read_only),
            pending_insert: None,
            focus_editor: false,
        }
    }

    pub fn for_connection(profile: &ConnectionProfile, index: usize) -> Self {
        let text = highlight::starter_query(profile.kind).to_owned();
        Self::new(format!("{} {}", profile.name, index), text, Some(profile))
    }

    pub fn is_running(&self) -> bool {
        self.run.is_some()
    }

    /// Runs finished since the last call.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    /// Appends (or replaces an empty tab with) a draft from the AI dock.
    pub fn insert(&mut self, draft: String) {
        self.pending_insert = Some(draft);
    }

    pub fn set_connection(&mut self, profile: &ConnectionProfile) {
        self.connection_id = Some(profile.id);
        self.dialect = Dialect::for_kind(profile.kind);
        self.read_only_profile = profile.read_only;
    }

    pub fn poll(&mut self, workspace: &Workspace, profile: Option<&ConnectionProfile>) {
        if let Some(run) = &mut self.run
            && let Some(result) = run.poll()
        {
            self.run = None;
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
                    self.error = None;
                    let mut grid = ResultGrid::new(page);
                    grid.json_view = self.dialect == Dialect::MongoJson;
                    self.result = Some(grid);
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
                    self.error = Some(error.to_string());
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
        self.running_text = query.clone();
        self.run = Some(workspace.execute_query(QueryRequest {
            connection_id,
            query,
            language: self.dialect.language(),
            parameters: Vec::new(),
            limit: self.limit,
            offset: 0,
            timeout_ms: 30_000,
        }));
    }

    /// Runs `selection` when given, else the statement under the cursor; a read-only batch runs
    /// statement by statement.
    pub fn run_now(&mut self, workspace: &Workspace, selection: Option<String>) {
        let Some(connection_id) = self.connection_id else {
            self.error = Some("choose a connection for this tab first".to_owned());
            return;
        };
        if self.run.is_some() {
            return;
        }
        self.error = None;
        self.ran_in_batch = 0;
        let text = match selection {
            Some(s) if !s.trim().is_empty() => s,
            _ => highlight::statement_at(&self.text, self.cursor_char, self.dialect),
        };
        if text.trim().is_empty() {
            self.error = Some("nothing to run".to_owned());
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

    pub fn format_text(&mut self) {
        self.text = highlight::format(&self.text, self.dialect);
    }

    /// Ctrl+S: write the tab's text to its file, asking for one the first time.
    pub fn save_to_file(&mut self, save_as: bool) -> Option<String> {
        if save_as || self.file.is_none() {
            let extension = if self.dialect == Dialect::MongoJson { "json" } else { "sql" };
            let picked = rfd::FileDialog::new()
                .set_file_name(format!("{}.{extension}", self.title.replace(' ', "-").to_lowercase()))
                .save_file()?;
            self.file = Some(picked);
        }
        let path = self.file.clone()?;
        match std::fs::write(&path, &self.text) {
            Ok(()) => {
                self.title = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| self.title.clone());
                Some(format!("Saved {}", path.display()))
            }
            Err(error) => {
                self.error = Some(format!("could not save: {error}"));
                None
            }
        }
    }

    /// Draws the editor half of the tab. Returns a status-line message when there is one.
    pub fn editor_ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        profile: Option<&ConnectionProfile>,
        collections: &[redrob_query_core::MetadataNode],
    ) -> Option<String> {
        let mut notice = None;
        if let Some(draft) = self.pending_insert.take() {
            if self.text.trim().is_empty() {
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
            if ui.add_enabled(can_run, theme::primary_button("▶ Run")).on_hover_text("Ctrl+Enter: the selection or the statement under the cursor").clicked() {
                self.run_now(workspace, None);
            }
            if running {
                ui.spinner();
                if !self.queue.is_empty() || self.ran_in_batch > 0 {
                    ui.label(RichText::new(format!("statement {} of {}", self.ran_in_batch + 1, self.ran_in_batch + 1 + self.queue.len())).small());
                }
            }
            if ui.button("Format").on_hover_text("Ctrl+Shift+F").clicked() {
                self.format_text();
            }
            if ui.button("Save").on_hover_text("Ctrl+S: save the text to a file").clicked() {
                notice = self.save_to_file(false);
            }
            ui.label(RichText::new("Limit").small());
            ui.add(egui::DragValue::new(&mut self.limit).range(1..=10_000).speed(10));
            if let Some(profile) = profile {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let classification = if self.dialect == Dialect::Sql {
                        Some(classify_sql(&highlight::statement_at(&self.text, self.cursor_char, self.dialect)))
                    } else {
                        None
                    };
                    if let Some(c) = classification {
                        let (text, colour) = match c {
                            QueryClassification::ReadOnly => ("read-only", theme::SUCCESS),
                            QueryClassification::Mutation => ("mutation (use the grid to edit)", theme::CAUTION),
                            QueryClassification::Destructive => ("destructive (refused)", theme::DANGER),
                            QueryClassification::TransactionControl => ("transaction control (refused)", theme::CAUTION),
                            QueryClassification::Unknown => ("unknown (refused)", theme::CAUTION),
                        };
                        ui.label(RichText::new(text).small().color(colour));
                    }
                    ui.label(RichText::new(format!("{} · {}", profile.name, crate::profiles::kind_name(profile.kind))).small().weak());
                });
            } else {
                ui.label(RichText::new("no connection: connect a profile, then open a tab from it").small().color(theme::CAUTION));
            }
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
        let output = egui::TextEdit::multiline(&mut self.text)
            .code_editor()
            .id_salt("query-editor")
            .desired_width(f32::INFINITY)
            .desired_rows(8)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        if self.focus_editor {
            output.response.request_focus();
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
        let (run, format, save, save_as) = ui.input_mut(|i| {
            let cmd = egui::Modifiers::COMMAND;
            (
                focused && i.consume_key(cmd, egui::Key::Enter),
                focused && i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::F),
                i.consume_key(cmd, egui::Key::S),
                i.consume_key(cmd | egui::Modifiers::SHIFT, egui::Key::S),
            )
        });
        if run {
            self.run_now(workspace, selection);
        }
        if format {
            self.format_text();
        }
        if save_as {
            notice = self.save_to_file(true);
        } else if save {
            notice = self.save_to_file(false);
        }
        if self.dialect == Dialect::MongoJson && output.response.changed() {
            self.mongo.sync_from(&self.text);
        }
        if let Some(error) = &self.error {
            ui.label(RichText::new(error).color(theme::DANGER));
        }
        notice
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
    if words.iter().any(|w| matches!(*w, "JOIN" | "UNION" | "GROUP" | "DISTINCT" | "INTERSECT" | "EXCEPT"))
        || upper.matches("SELECT").count() != 1
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
