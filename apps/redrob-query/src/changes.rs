//! The Changes pane: the staged cell edits and row deletions of a result, reviewed as a list with
//! the SQL the engine will run, then applied through `apply_cell_edits` in one transaction (each
//! row update must hit exactly one row or everything rolls back). Nothing is written until Apply.

use egui::{RichText, Ui};
use redrob_query_core::{
    DataValue, DatabaseKind, MutationResult, Pending, Workspace,
    cell_edits::{CellChange, CellEditSet, RowEdit, build_row_updates},
};
use uuid::Uuid;

use crate::grid::{ResultGrid, format_value};
use crate::i18n::{t, tf};
use crate::theme::{self, Tokens};

/// Which table a result came from, when the editor could tell: a single-table SELECT, or a
/// preview opened from the schema tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultSource {
    pub connection_id: Uuid,
    pub schema: Option<String>,
    pub table: String,
}

/// The one-column primary key the result can be edited by, or why it cannot.
pub fn editable_key(grid: &ResultGrid) -> Result<(String, usize), &'static str> {
    match grid.key_columns.as_slice() {
        [] => Err("the table has no primary key"),
        [key] => grid
            .page
            .columns
            .iter()
            .position(|c| &c.name == key)
            .map(|index| (key.clone(), index))
            .ok_or("select the primary key column to edit"),
        _ => Err("composite primary keys cannot be edited here yet"),
    }
}

/// The staged state as the engine's change set. Rows whose key is NULL are refused.
pub fn build_edit_set(grid: &ResultGrid, source: &ResultSource) -> Result<CellEditSet, String> {
    let (key, key_index) = editable_key(grid).map_err(|e| t(e).to_owned())?;
    let key_of = |row: usize| -> Result<DataValue, String> {
        match grid.page.rows.get(row).and_then(|r| r.get(key_index)) {
            Some(DataValue::Null) | None => Err(tf(
                "row {n} has no primary key value",
                &[("n", &(row + 1).to_string())],
            )),
            Some(value) => Ok(value.clone()),
        }
    };
    let mut rows: Vec<RowEdit> = Vec::new();
    for (&(row, column), value) in &grid.staged {
        if grid.deleted.contains(&row) {
            continue;
        }
        if column == key_index {
            return Err(t("the primary key column itself cannot be edited").to_owned());
        }
        let change = CellChange {
            column: grid.page.columns[column].name.clone(),
            value: value.clone(),
        };
        let key_value = key_of(row)?;
        match rows.iter_mut().find(|r| r.key == key_value) {
            Some(existing) => existing.changes.push(change),
            None => rows.push(RowEdit {
                key: key_value,
                changes: vec![change],
            }),
        }
    }
    let deletes = grid
        .deleted
        .iter()
        .map(|&row| key_of(row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CellEditSet {
        schema: source.schema.clone(),
        table: source.table.clone(),
        primary_key: key,
        rows,
        inserts: Vec::new(),
        deletes,
    })
}

/// The statements the engine will run, with their bound values written beside the placeholders,
/// for the review list. The values are shown, never spliced into the SQL: the engine binds them.
pub fn sql_preview(kind: DatabaseKind, set: &CellEditSet) -> Result<Vec<String>, String> {
    let updates = build_row_updates(kind, set).map_err(|e| e.to_string())?;
    Ok(updates
        .into_iter()
        .map(|update| {
            let values = update
                .parameters
                .iter()
                .enumerate()
                .map(|(i, v)| format!("${} = {}", i + 1, literal(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{};  -- {values}", update.statement)
        })
        .collect())
}

/// A value as it would read in SQL, for the preview only.
fn literal(value: &DataValue) -> String {
    match value {
        DataValue::Null => "NULL".to_owned(),
        DataValue::Boolean(_)
        | DataValue::Integer(_)
        | DataValue::Float(_)
        | DataValue::Decimal(_) => format_value(value),
        other => format!("'{}'", format_value(other).replace('\'', "''")),
    }
}

#[derive(Default)]
pub struct ChangesPanel {
    apply: Option<Pending<MutationResult>>,
    pub last: Option<(String, bool)>,
}

/// What the pane asks the app to do after an apply.
pub enum ChangesEvent {
    Applied(MutationResult),
    Notice(String, bool),
}

impl ChangesPanel {
    pub fn busy(&self) -> bool {
        self.apply.is_some()
    }

    pub fn poll(&mut self, events: &mut Vec<ChangesEvent>) {
        if let Some(apply) = &mut self.apply
            && let Some(result) = apply.poll()
        {
            self.apply = None;
            match result {
                Ok(outcome) => {
                    self.last = Some((
                        tf(
                            "Applied: {rows} rows changed in {ms} ms",
                            &[
                                ("rows", &outcome.rows_affected.to_string()),
                                ("ms", &outcome.elapsed_ms.to_string()),
                            ],
                        ),
                        true,
                    ));
                    events.push(ChangesEvent::Applied(outcome));
                }
                Err(error) => {
                    self.last = Some((error.to_string(), false));
                    events.push(ChangesEvent::Notice(error.to_string(), true));
                }
            }
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        grid: &mut ResultGrid,
        source: Option<&ResultSource>,
        kind: Option<DatabaseKind>,
        read_only_profile: bool,
    ) {
        let tokens = Tokens::of(ui);
        if let Some((text, ok)) = &self.last {
            ui.label(RichText::new(text).color(if *ok { tokens.success } else { tokens.danger }));
        }
        let Some(source) = source else {
            ui.add_space(8.0);
            ui.label(
                RichText::new(t("Edits need a result from one table.")).color(tokens.text_faint),
            );
            return;
        };
        if !grid.has_changes() {
            ui.add_space(8.0);
            ui.label(
                RichText::new(t(
                    "Double-click a cell to edit it; right-click a row number to delete the row. Nothing is written until Apply.",
                ))
                .color(tokens.text_faint),
            );
            return;
        }
        if read_only_profile {
            ui.label(
                RichText::new(t(
                    "This connection is read-only. Untick Read-only in its connection form to allow reviewed edits.",
                ))
                .color(tokens.warning),
            );
        }
        let set = build_edit_set(grid, source);
        let preview = match (&set, kind) {
            (Ok(set), Some(kind)) => Some(sql_preview(kind, set)),
            _ => None,
        };
        egui::Panel::bottom(ui.id().with("changes-actions"))
            .exact_size(theme::STATUS_HEIGHT + 8.0)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 4)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let can_apply =
                        set.is_ok() && grid.has_changes() && !self.busy() && !read_only_profile;
                    if ui
                        .add_enabled(can_apply, theme::primary_button(ui, t("Apply")))
                        .clicked()
                        && let Ok(set) = &set
                    {
                        self.last = None;
                        self.apply =
                            Some(workspace.apply_cell_edits(source.connection_id, set.clone()));
                    }
                    if ui
                        .add_enabled(
                            grid.has_changes() && !self.busy(),
                            egui::Button::new(t("Discard")),
                        )
                        .clicked()
                    {
                        grid.clear_changes();
                        self.last = None;
                    }
                    if self.busy() {
                        ui.spinner();
                    }
                });
            });
        egui::ScrollArea::vertical()
            .id_salt("changes")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (&(row, column), value) in &grid.staged {
                    let before = grid
                        .page
                        .rows
                        .get(row)
                        .and_then(|r| r.get(column))
                        .map(format_value)
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(tf("row {n}", &[("n", &(row + 1).to_string())]))
                                .small()
                                .color(tokens.text_faint),
                        );
                        ui.label(RichText::new(&grid.page.columns[column].name).monospace());
                        ui.label(
                            RichText::new(before)
                                .monospace()
                                .color(tokens.text_faint)
                                .strikethrough(),
                        );
                        ui.label("→");
                        ui.label(
                            RichText::new(format_value(value))
                                .monospace()
                                .color(tokens.warning),
                        );
                    });
                }
                for &row in &grid.deleted {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(tf("row {n}", &[("n", &(row + 1).to_string())]))
                                .small()
                                .color(tokens.text_faint),
                        );
                        ui.label(RichText::new(t("delete")).color(tokens.danger));
                    });
                }
                ui.add_space(6.0);
                match (&set, preview) {
                    (Ok(set), Some(Ok(statements))) => {
                        ui.label(
                            RichText::new(tf(
                                "SQL the engine will run on {table}, keyed by {key}, in one transaction:",
                                &[
                                    (
                                        "table",
                                        &format!(
                                            "{}{}",
                                            set.schema
                                                .as_deref()
                                                .map(|s| format!("{s}."))
                                                .unwrap_or_default(),
                                            set.table
                                        ),
                                    ),
                                    ("key", &set.primary_key),
                                ],
                            ))
                            .small()
                            .color(tokens.text_dim),
                        );
                        for statement in statements {
                            ui.add(
                                egui::Label::new(RichText::new(statement).monospace().small())
                                    .wrap(),
                            );
                        }
                    }
                    (Ok(_), Some(Err(reason))) => {
                        ui.label(RichText::new(reason).small().color(tokens.warning));
                    }
                    (Err(reason), _) => {
                        ui.label(RichText::new(reason).small().color(tokens.warning));
                    }
                    (Ok(_), None) => {}
                }
            });
    }
}
