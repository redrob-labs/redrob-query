//! The Changes panel: the staged cell edits and row deletions of a result, reviewed as a list,
//! then applied through `apply_cell_edits` in one transaction (each row update must hit exactly
//! one row or everything rolls back). Nothing is written until Apply is pressed.

use egui::{RichText, Ui};
use redrob_query_core::{
    DataValue, MutationResult, Pending, Workspace,
    cell_edits::{CellChange, CellEditSet, RowEdit},
};
use uuid::Uuid;

use crate::grid::{ResultGrid, format_value};
use crate::theme;

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
    let (key, key_index) = editable_key(grid).map_err(str::to_owned)?;
    let key_of = |row: usize| -> Result<DataValue, String> {
        match grid.page.rows.get(row).and_then(|r| r.get(key_index)) {
            Some(DataValue::Null) | None => {
                Err(format!("row {} has no primary key value", row + 1))
            }
            Some(value) => Ok(value.clone()),
        }
    };
    let mut rows: Vec<RowEdit> = Vec::new();
    for (&(row, column), value) in &grid.staged {
        if grid.deleted.contains(&row) {
            continue;
        }
        if column == key_index {
            return Err("the primary key column itself cannot be edited".to_owned());
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

#[derive(Default)]
pub struct ChangesPanel {
    apply: Option<Pending<MutationResult>>,
    pub last: Option<(String, bool)>,
}

/// What the panel asks the app to do after an apply.
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
                        format!(
                            "Applied: {} rows changed in {} ms",
                            outcome.rows_affected, outcome.elapsed_ms
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
        read_only_profile: bool,
    ) {
        ui.horizontal(|ui| {
            ui.heading("Changes");
            let count = grid.staged.len() + grid.deleted.len();
            ui.label(RichText::new(format!("{count} staged")).weak());
        });
        if let Some((text, ok)) = &self.last {
            ui.label(RichText::new(text).color(if *ok { theme::SUCCESS } else { theme::DANGER }));
        }
        let Some(source) = source else {
            ui.label(RichText::new("Edits need a result from one table.").weak());
            return;
        };
        if read_only_profile {
            ui.label(RichText::new("This profile is read-only. Untick \"Read-only\" in its connection form to allow reviewed edits.").color(theme::CAUTION));
        }
        let set = build_edit_set(grid, source);
        egui::ScrollArea::vertical()
            .id_salt("changes")
            .auto_shrink([false, true])
            .max_height(220.0)
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
                        ui.label(RichText::new(format!("row {}", row + 1)).small().weak());
                        ui.label(RichText::new(&grid.page.columns[column].name).monospace());
                        ui.label(RichText::new(before).weak().strikethrough());
                        ui.label("→");
                        ui.label(RichText::new(format_value(value)).color(theme::CAUTION));
                    });
                }
                for &row in &grid.deleted {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("row {}", row + 1)).small().weak());
                        ui.label(RichText::new("delete").color(theme::DANGER));
                    });
                }
            });
        match &set {
            Ok(set) => {
                ui.label(
                    RichText::new(format!(
                        "{} row update(s), {} delete(s) on {}{} keyed by {}",
                        set.rows.len(),
                        set.deletes.len(),
                        set.schema
                            .as_deref()
                            .map(|s| format!("{s}."))
                            .unwrap_or_default(),
                        set.table,
                        set.primary_key
                    ))
                    .small(),
                );
            }
            Err(reason) => {
                ui.label(RichText::new(reason).small().color(theme::CAUTION));
            }
        }
        ui.horizontal(|ui| {
            let can_apply = set.is_ok() && grid.has_changes() && !self.busy() && !read_only_profile;
            if ui
                .add_enabled(can_apply, theme::primary_button("Apply"))
                .clicked()
                && let Ok(set) = set
            {
                self.last = None;
                self.apply = Some(workspace.apply_cell_edits(source.connection_id, set));
            }
            if ui
                .add_enabled(
                    grid.has_changes() && !self.busy(),
                    egui::Button::new("Discard"),
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
    }
}
