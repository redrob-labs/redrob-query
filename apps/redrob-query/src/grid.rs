//! The result grid: one `QueryResultPage` shown as a virtualised table in monospace.
//!
//! Sorting and filtering act on the page the engine returned (at most the request's limit), never
//! on the database, so they cost nothing and promise nothing more than what was fetched; the page
//! controls at the bottom ask the tab for the next or previous page. Cells are formatted from
//! `DataValue` for display and parsed back on edit; an edit is only staged -- the Changes pane turns
//! the staged cells into a `CellEditSet` the engine applies in one transaction.
//!
//! Click selects a cell (its row is highlighted), Ctrl+C copies the cell, or the selected rows as
//! TSV when several are selected (Ctrl+click adds a row); header click sorts; column edges drag.

use std::collections::{BTreeMap, BTreeSet};

use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use redrob_query_core::{DataValue, QueryResultPage};

use crate::i18n::{t, tf};
use crate::theme::{self, Tokens};

const ROW_HEIGHT: f32 = 22.0;
const MAX_CELL_CHARS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// What the grid asked the tab for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridAction {
    None,
    PreviousPage,
    NextPage,
}

enum EditAction {
    Start(usize, usize),
    Draft(String),
    Commit,
    Cancel,
    SetNull(usize, usize),
    Revert(usize, usize),
}

pub struct ResultGrid {
    pub page: QueryResultPage,
    /// The request offset this page starts at (the page controls).
    pub offset: usize,
    sort: Option<(usize, SortDirection)>,
    pub filter: String,
    hidden: BTreeSet<usize>,
    /// Row indices of the page in display order after filter and sort.
    order: Vec<usize>,
    order_dirty: bool,
    /// (row, column) -> new value, staged by inline editing.
    pub staged: BTreeMap<(usize, usize), DataValue>,
    pub deleted: BTreeSet<usize>,
    editing: Option<(usize, usize, String)>,
    /// Columns of the primary key, when the result is one editable table; empty is read-only.
    pub key_columns: Vec<String>,
    pub editable: bool,
    pub json_view: bool,
    /// The clicked cell, and every row selected (Ctrl+click adds).
    pub selected_cell: Option<(usize, usize)>,
    pub selected_rows: BTreeSet<usize>,
}

impl ResultGrid {
    pub fn new(page: QueryResultPage, offset: usize) -> Self {
        let rows = page.rows.len();
        Self {
            page,
            offset,
            sort: None,
            filter: String::new(),
            hidden: BTreeSet::new(),
            order: (0..rows).collect(),
            order_dirty: false,
            staged: BTreeMap::new(),
            deleted: BTreeSet::new(),
            editing: None,
            key_columns: Vec::new(),
            editable: false,
            json_view: false,
            selected_cell: None,
            selected_rows: BTreeSet::new(),
        }
    }

    pub fn has_changes(&self) -> bool {
        !self.staged.is_empty() || !self.deleted.is_empty()
    }

    pub fn clear_changes(&mut self) {
        self.staged.clear();
        self.deleted.clear();
        self.editing = None;
    }

    fn rebuild_order(&mut self) {
        let filter = self.filter.trim().to_lowercase();
        let mut order: Vec<usize> = (0..self.page.rows.len())
            .filter(|&row| {
                filter.is_empty()
                    || self.page.rows[row]
                        .iter()
                        .any(|value| format_value(value).to_lowercase().contains(&filter))
            })
            .collect();
        if let Some((column, direction)) = self.sort {
            order.sort_by(|&a, &b| {
                let left = self.page.rows[a].get(column);
                let right = self.page.rows[b].get(column);
                let ordering = compare(left, right);
                match direction {
                    SortDirection::Ascending => ordering,
                    SortDirection::Descending => ordering.reverse(),
                }
            });
        }
        self.order = order;
        self.order_dirty = false;
    }

    /// The value shown in a cell: a staged edit wins over the fetched one.
    pub fn cell(&self, row: usize, column: usize) -> Option<&DataValue> {
        self.staged
            .get(&(row, column))
            .or_else(|| self.page.rows.get(row).and_then(|r| r.get(column)))
    }

    fn visible_columns(&self) -> Vec<usize> {
        (0..self.page.columns.len())
            .filter(|c| !self.hidden.contains(c))
            .collect()
    }

    fn rows_tsv(&self, rows: impl Iterator<Item = usize>, header: bool) -> String {
        let columns = self.visible_columns();
        let mut out = String::new();
        if header {
            out.push_str(
                &columns
                    .iter()
                    .map(|&c| self.page.columns[c].name.clone())
                    .collect::<Vec<_>>()
                    .join("\t"),
            );
            out.push('\n');
        }
        for row in rows {
            let line = columns
                .iter()
                .map(|&c| {
                    self.cell(row, c)
                        .map(format_value)
                        .unwrap_or_default()
                        .replace(['\t', '\n'], " ")
                })
                .collect::<Vec<_>>()
                .join("\t");
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    /// The whole page as tab-separated text (header first), hidden columns left out.
    pub fn to_tsv(&self) -> String {
        self.rows_tsv(self.order.iter().copied(), true)
    }

    /// Ctrl+C: the selected rows as TSV when more than one is selected, else the selected cell.
    pub fn copy_selection(&self, ctx: &egui::Context) -> bool {
        if self.selected_rows.len() > 1 {
            let rows: Vec<usize> = self
                .order
                .iter()
                .copied()
                .filter(|r| self.selected_rows.contains(r))
                .collect();
            ctx.copy_text(self.rows_tsv(rows.into_iter(), false));
            return true;
        }
        if let Some((row, column)) = self.selected_cell {
            ctx.copy_text(self.cell(row, column).map(format_value).unwrap_or_default());
            return true;
        }
        false
    }

    /// The page as a JSON array of objects, in display order.
    pub fn to_json(&self) -> String {
        let documents: Vec<serde_json::Value> =
            self.order.iter().map(|&row| self.row_json(row)).collect();
        serde_json::to_string_pretty(&documents).unwrap_or_default()
    }

    pub fn row_json(&self, row: usize) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        for (c, column) in self.page.columns.iter().enumerate() {
            if self.hidden.contains(&c) {
                continue;
            }
            if let Some(value) = self.cell(row, c) {
                object.insert(column.name.clone(), value_json(value));
            }
        }
        serde_json::Value::Object(object)
    }

    pub fn ui(&mut self, ui: &mut Ui) -> GridAction {
        if self.order_dirty || self.order.len() > self.page.rows.len() {
            self.rebuild_order();
        }
        self.toolbar(ui);
        let mut action = GridAction::None;
        egui::Panel::bottom(ui.id().with("grid-footer"))
            .exact_size(theme::STATUS_HEIGHT)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(6, 2)))
            .show(ui, |ui| {
                action = self.footer(ui);
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                if self.json_view {
                    self.json_ui(ui);
                } else {
                    self.table_ui(ui);
                }
            });
        action
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        ui.horizontal(|ui| {
            if self.editable {
                ui.label(RichText::new(t("editable")).small().color(tokens.success))
                    .on_hover_text(tf(
                        "Primary key: {key}",
                        &[("key", &self.key_columns.join(", "))],
                    ));
            } else {
                ui.label(RichText::new(t("read-only")).small().color(tokens.text_faint))
                    .on_hover_text(t(
                        "Edits need one table with a primary key and a writable connection",
                    ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(t("Export"), |ui| {
                    if ui.button(t("Copy as TSV")).clicked() {
                        ui.ctx().copy_text(self.to_tsv());
                        ui.close();
                    }
                    if ui.button(t("Copy as JSON")).clicked() {
                        ui.ctx().copy_text(self.to_json());
                        ui.close();
                    }
                    if ui.button(t("Save TSV…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("result.tsv")
                            .save_file()
                        {
                            let _ = std::fs::write(path, self.to_tsv());
                        }
                        ui.close();
                    }
                    if ui.button(t("Save JSON…")).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("result.json")
                            .save_file()
                        {
                            let _ = std::fs::write(path, self.to_json());
                        }
                        ui.close();
                    }
                });
                ui.menu_button(t("Columns"), |ui| {
                    for (index, column) in self.page.columns.iter().enumerate() {
                        let mut shown = !self.hidden.contains(&index);
                        if ui.checkbox(&mut shown, &column.name).changed() {
                            if shown {
                                self.hidden.remove(&index);
                            } else {
                                self.hidden.insert(index);
                            }
                        }
                    }
                    if !self.hidden.is_empty() {
                        ui.separator();
                        if ui.button(t("Show all")).clicked() {
                            self.hidden.clear();
                        }
                    }
                });
                ui.selectable_value(&mut self.json_view, true, "JSON");
                ui.selectable_value(&mut self.json_view, false, t("Table"));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text(t("Filter rows"))
                        .desired_width(160.0),
                );
                if response.changed() {
                    self.order_dirty = true;
                }
            });
        });
    }

    /// Row count and the page controls.
    fn footer(&mut self, ui: &mut Ui) -> GridAction {
        let tokens = Tokens::of(ui);
        let mut action = GridAction::None;
        ui.horizontal(|ui| {
            let fetched = self.page.rows.len();
            let first = if fetched == 0 { 0 } else { self.offset + 1 };
            let last = self.offset + fetched;
            let mut summary = tf(
                "rows {first}–{last}",
                &[("first", &first.to_string()), ("last", &last.to_string())],
            );
            if self.order.len() != fetched {
                summary.push_str(&tf(
                    " · {n} shown",
                    &[("n", &self.order.len().to_string())],
                ));
            }
            summary.push_str(&tf(
                " · {ms} ms",
                &[("ms", &self.page.stats.elapsed_ms.to_string())],
            ));
            ui.label(RichText::new(summary).small().color(tokens.text_dim));
            if self.page.stats.truncated {
                ui.label(
                    RichText::new(t("more rows available"))
                        .small()
                        .color(tokens.warning),
                );
            }
            if !self.selected_rows.is_empty() {
                ui.label(
                    RichText::new(tf(
                        "{n} selected",
                        &[("n", &self.selected_rows.len().to_string())],
                    ))
                    .small()
                    .color(tokens.text_faint),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let has_next = self.page.next_offset.is_some() || self.page.stats.truncated;
                if ui
                    .add_enabled(has_next, egui::Button::new("▶").small())
                    .on_hover_text(t("Next page"))
                    .clicked()
                {
                    action = GridAction::NextPage;
                }
                if ui
                    .add_enabled(self.offset > 0, egui::Button::new("◀").small())
                    .on_hover_text(t("Previous page"))
                    .clicked()
                {
                    action = GridAction::PreviousPage;
                }
            });
        });
        action
    }

    fn json_ui(&mut self, ui: &mut Ui) {
        egui::ScrollArea::both()
            .id_salt("json-view")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let text = self.to_json();
                let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                    let job = crate::highlight::layout(
                        ui,
                        text.as_str(),
                        crate::highlight::Dialect::MongoJson,
                        wrap,
                    );
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let mut shown = text;
                ui.add(
                    egui::TextEdit::multiline(&mut shown)
                        .code_editor()
                        .interactive(false)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
            });
    }

    fn table_ui(&mut self, ui: &mut Ui) {
        let tokens = Tokens::of(ui);
        let columns = self.visible_columns();
        if columns.is_empty() {
            ui.label(RichText::new(t("No columns to show.")).color(tokens.text_faint));
            return;
        }
        let order = self.order.clone();
        let mut sort_click: Option<usize> = None;
        let mut edit: Option<EditAction> = None;
        let mut toggle_delete: Option<usize> = None;
        let mut select: Option<(usize, usize, bool)> = None;
        let editing = self.editing.clone();
        let editable = self.editable;
        let ctrl = ui.input(|i| i.modifiers.command);
        let mono = egui::TextStyle::Monospace.resolve(ui.style());

        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(44.0))
            .min_scrolled_height(0.0);
        for _ in &columns {
            builder = builder.column(Column::initial(140.0).at_least(48.0).clip(true));
        }
        builder
            .header(ROW_HEIGHT + 2.0, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("#").small().color(tokens.text_faint));
                });
                for &c in &columns {
                    header.col(|ui| {
                        let column = &self.page.columns[c];
                        let arrow = match self.sort {
                            Some((s, SortDirection::Ascending)) if s == c => " ▲",
                            Some((s, SortDirection::Descending)) if s == c => " ▼",
                            _ => "",
                        };
                        let label = RichText::new(format!("{}{arrow}", column.name))
                            .monospace()
                            .strong();
                        let response = ui
                            .add(
                                egui::Label::new(label)
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            )
                            .on_hover_text(tf(
                                "{type} · click to sort",
                                &[("type", &column.data_type)],
                            ));
                        if response.clicked() {
                            sort_click = Some(c);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, order.len(), |mut row| {
                    let index = row.index();
                    let page_row = order[index];
                    let deleted = self.deleted.contains(&page_row);
                    row.set_selected(
                        self.selected_rows.contains(&page_row)
                            || self.selected_cell.is_some_and(|(r, _)| r == page_row),
                    );
                    row.col(|ui| {
                        let text = RichText::new((self.offset + page_row + 1).to_string())
                            .small()
                            .color(tokens.text_faint);
                        let text = if deleted {
                            text.strikethrough().color(tokens.danger)
                        } else {
                            text
                        };
                        let response = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                        if response.clicked() {
                            select = Some((page_row, 0, ctrl));
                        }
                        response.context_menu(|ui| {
                            if ui.button(t("Copy row as JSON")).clicked() {
                                ui.ctx().copy_text(
                                    serde_json::to_string_pretty(&self.row_json(page_row))
                                        .unwrap_or_default(),
                                );
                                ui.close();
                            }
                            if ui.button(t("Copy row as TSV")).clicked() {
                                ui.ctx()
                                    .copy_text(self.rows_tsv(std::iter::once(page_row), false));
                                ui.close();
                            }
                            if editable {
                                let label = if deleted {
                                    t("Undo delete")
                                } else {
                                    t("Delete row")
                                };
                                if ui.button(label).clicked() {
                                    toggle_delete = Some(page_row);
                                    ui.close();
                                }
                            }
                        });
                    });
                    for &c in &columns {
                        row.col(|ui| {
                            if let Some((er, ec, text)) = &editing
                                && *er == page_row
                                && *ec == c
                            {
                                let mut draft = text.clone();
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut draft)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::TextStyle::Monospace),
                                );
                                if !response.has_focus() && !response.lost_focus() {
                                    response.request_focus();
                                }
                                if draft != *text {
                                    edit = Some(EditAction::Draft(draft));
                                }
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if escape {
                                    edit = Some(EditAction::Cancel);
                                } else if enter || response.lost_focus() {
                                    edit = Some(EditAction::Commit);
                                }
                                return;
                            }
                            let value = self.cell(page_row, c);
                            let staged = self.staged.contains_key(&(page_row, c));
                            let is_null = matches!(value, Some(DataValue::Null) | None);
                            let shown = value.map_or_else(String::new, format_cell);
                            let mut text = RichText::new(&shown).monospace();
                            if deleted {
                                text = text.strikethrough().color(tokens.danger);
                            } else if staged {
                                text = text.color(tokens.warning);
                            } else if is_null {
                                text = text.color(tokens.text_faint).italics();
                            }
                            let cell_selected = self.selected_cell == Some((page_row, c));
                            if cell_selected {
                                ui.painter().rect_stroke(
                                    ui.max_rect(),
                                    0.0,
                                    egui::Stroke::new(1.0, tokens.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let width = ui.available_width();
                            let response = ui.add(
                                egui::Label::new(text)
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            );
                            if response.clicked() {
                                select = Some((page_row, c, ctrl));
                            }
                            if response.double_clicked() && editable && !deleted {
                                edit = Some(EditAction::Start(page_row, c));
                            }
                            if let Some(value) = value {
                                // The full value when the cell could not show all of it.
                                let needed = ui.fonts_mut(|f| {
                                    f.layout_no_wrap(shown.clone(), mono.clone(), tokens.text)
                                        .size()
                                        .x
                                });
                                let full = format_value(value);
                                if needed > width
                                    || full.chars().count() > MAX_CELL_CHARS
                                    || full.contains('\n')
                                {
                                    response.clone().on_hover_text(
                                        RichText::new(full.chars().take(2000).collect::<String>())
                                            .monospace(),
                                    );
                                }
                            }
                            response.context_menu(|ui| {
                                if ui.button(t("Copy value")).clicked() {
                                    ui.ctx()
                                        .copy_text(value.map(format_value).unwrap_or_default());
                                    ui.close();
                                }
                                if editable && !deleted && ui.button(t("Edit cell")).clicked() {
                                    edit = Some(EditAction::Start(page_row, c));
                                    ui.close();
                                }
                                if editable && !deleted && ui.button(t("Set NULL")).clicked() {
                                    edit = Some(EditAction::SetNull(page_row, c));
                                    ui.close();
                                }
                                if staged && ui.button(t("Revert cell")).clicked() {
                                    edit = Some(EditAction::Revert(page_row, c));
                                    ui.close();
                                }
                            });
                        });
                    }
                });
            });

        if let Some((row, column, add)) = select {
            self.selected_cell = Some((row, column));
            if add {
                if !self.selected_rows.remove(&row) {
                    self.selected_rows.insert(row);
                }
            } else {
                self.selected_rows.clear();
                self.selected_rows.insert(row);
            }
        }
        if let Some(c) = sort_click {
            self.sort = match self.sort {
                Some((s, SortDirection::Ascending)) if s == c => {
                    Some((c, SortDirection::Descending))
                }
                Some((s, SortDirection::Descending)) if s == c => None,
                _ => Some((c, SortDirection::Ascending)),
            };
            self.order_dirty = true;
        }
        match edit {
            Some(EditAction::Start(row, column)) => {
                let current = self.cell(row, column).map(format_value).unwrap_or_default();
                self.editing = Some((row, column, current));
            }
            Some(EditAction::Draft(text)) => {
                if let Some(state) = &mut self.editing {
                    state.2 = text;
                }
            }
            Some(EditAction::Commit) => {
                if let Some((row, column, text)) = self.editing.take() {
                    self.stage(row, column, &text);
                }
            }
            Some(EditAction::Cancel) => self.editing = None,
            Some(EditAction::SetNull(row, column)) => self.stage(row, column, "NULL"),
            Some(EditAction::Revert(row, column)) => {
                self.staged.remove(&(row, column));
            }
            None => {}
        }
        if let Some(row) = toggle_delete
            && !self.deleted.remove(&row)
        {
            self.deleted.insert(row);
        }
    }

    /// Stages `text` as the new value of a cell, parsed against the fetched value's type. The text
    /// `NULL` (any case) stages a NULL; a text equal to the fetched value un-stages the cell.
    pub fn stage(&mut self, row: usize, column: usize, text: &str) {
        let Some(original) = self.page.rows.get(row).and_then(|r| r.get(column)) else {
            return;
        };
        let parsed = parse_value(text, original);
        if parsed == *original {
            self.staged.remove(&(row, column));
        } else {
            self.staged.insert((row, column), parsed);
        }
    }
}

/// A value as text, in full.
pub fn format_value(value: &DataValue) -> String {
    match value {
        DataValue::Null => "NULL".to_owned(),
        DataValue::Boolean(b) => b.to_string(),
        DataValue::Integer(s) | DataValue::Decimal(s) | DataValue::Text(s) => s.clone(),
        DataValue::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{f:.1}")
            } else {
                f.to_string()
            }
        }
        DataValue::Binary(b) => format!("base64:{b}"),
        DataValue::Date(s) | DataValue::Time(s) | DataValue::DateTime(s) => s.clone(),
        DataValue::Uuid(u) => u.to_string(),
        DataValue::Json(v) | DataValue::Bson(v) => match v {
            serde_json::Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        },
    }
}

/// A value as it fits a grid cell: one line, bounded.
fn format_cell(value: &DataValue) -> String {
    let text = format_value(value);
    let one_line = text.replace(['\n', '\r'], "⏎");
    if one_line.chars().count() > MAX_CELL_CHARS {
        let mut cut: String = one_line.chars().take(MAX_CELL_CHARS).collect();
        cut.push('…');
        cut
    } else {
        one_line
    }
}

/// `text` parsed into the variant of `original`, so an edited integer stays an integer and an
/// edited JSON column stays JSON. Anything that does not parse is kept as text, which the database
/// then accepts or refuses with its own error.
pub fn parse_value(text: &str, original: &DataValue) -> DataValue {
    if text.eq_ignore_ascii_case("null") {
        return DataValue::Null;
    }
    let trimmed = text.trim();
    match original {
        DataValue::Boolean(_) => match trimmed.to_ascii_lowercase().as_str() {
            "true" | "t" | "1" | "yes" => DataValue::Boolean(true),
            "false" | "f" | "0" | "no" => DataValue::Boolean(false),
            _ => DataValue::Text(text.to_owned()),
        },
        DataValue::Integer(_) if trimmed.parse::<i128>().is_ok() => {
            DataValue::Integer(trimmed.to_owned())
        }
        DataValue::Float(_) => trimmed
            .parse::<f64>()
            .map_or_else(|_| DataValue::Text(text.to_owned()), DataValue::Float),
        DataValue::Decimal(_) if trimmed.parse::<f64>().is_ok() => {
            DataValue::Decimal(trimmed.to_owned())
        }
        DataValue::Uuid(_) => trimmed
            .parse()
            .map_or_else(|_| DataValue::Text(text.to_owned()), DataValue::Uuid),
        DataValue::Date(_) => DataValue::Date(trimmed.to_owned()),
        DataValue::Time(_) => DataValue::Time(trimmed.to_owned()),
        DataValue::DateTime(_) => DataValue::DateTime(trimmed.to_owned()),
        DataValue::Binary(_) => DataValue::Binary(trimmed.trim_start_matches("base64:").to_owned()),
        DataValue::Json(_) => serde_json::from_str(trimmed).map_or_else(
            |_| DataValue::Json(serde_json::Value::String(text.to_owned())),
            DataValue::Json,
        ),
        DataValue::Bson(_) => serde_json::from_str(trimmed).map_or_else(
            |_| DataValue::Bson(serde_json::Value::String(text.to_owned())),
            DataValue::Bson,
        ),
        // A NULL cell has no type to parse into: numbers become numbers, the rest text.
        DataValue::Null if trimmed.parse::<i64>().is_ok() => DataValue::Integer(trimmed.to_owned()),
        _ => DataValue::Text(text.to_owned()),
    }
}

/// A value as JSON, for the JSON view and exports.
pub fn value_json(value: &DataValue) -> serde_json::Value {
    match value {
        DataValue::Null => serde_json::Value::Null,
        DataValue::Boolean(b) => serde_json::Value::Bool(*b),
        DataValue::Integer(s) => s.parse::<i64>().map_or_else(
            |_| serde_json::Value::String(s.clone()),
            serde_json::Value::from,
        ),
        DataValue::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        DataValue::Json(v) | DataValue::Bson(v) => v.clone(),
        other => serde_json::Value::String(format_value(other)),
    }
}

fn compare(left: Option<&DataValue>, right: Option<&DataValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (left, right) {
        (None | Some(DataValue::Null), None | Some(DataValue::Null)) => Ordering::Equal,
        (None | Some(DataValue::Null), _) => Ordering::Greater,
        (_, None | Some(DataValue::Null)) => Ordering::Less,
        (Some(a), Some(b)) => match (numeric(a), numeric(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => format_value(a).cmp(&format_value(b)),
        },
    }
}

fn numeric(value: &DataValue) -> Option<f64> {
    match value {
        DataValue::Integer(s) | DataValue::Decimal(s) => s.parse().ok(),
        DataValue::Float(f) => Some(*f),
        DataValue::Boolean(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}
