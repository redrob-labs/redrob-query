//! The Structure panel: the selected table's columns, primary key, foreign keys and indexes, and
//! the table-change form (create a table, add a column) that runs through the engine's `ddl`.

use egui::{RichText, Ui};
use redrob_query_core::{
    DatabaseKind, MetadataNode, MetadataRequest, Pending, Workspace,
    ddl::{ColumnSpec, ColumnType, TableChange},
    keys::TableKeys,
};
use uuid::Uuid;

use crate::i18n::t;
use crate::schema::split_table_id;
use crate::theme::{self, Tokens};

pub enum StructureEvent {
    /// A DDL change was applied: reload the schema tree of this connection.
    Changed(Uuid),
    Notice(String, bool),
}

#[derive(Default)]
pub struct StructurePanel {
    pub target: Option<Target>,
    columns: Option<Vec<MetadataNode>>,
    keys: Option<TableKeys>,
    columns_load: Option<Pending<Vec<MetadataNode>>>,
    keys_load: Option<Pending<TableKeys>>,
    form: Option<TableChangeForm>,
    apply: Option<Pending<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub connection_id: Uuid,
    pub kind: DatabaseKind,
    pub node: MetadataNode,
}

impl StructurePanel {
    pub fn show_table(
        &mut self,
        workspace: &Workspace,
        connection_id: Uuid,
        kind: DatabaseKind,
        node: MetadataNode,
    ) {
        let (schema, table) = split_table_id(&node, kind);
        self.columns = None;
        self.keys = None;
        self.columns_load = Some(workspace.load_metadata(MetadataRequest {
            connection_id,
            parent_id: Some(node.id.clone()),
        }));
        self.keys_load = Some(workspace.table_keys(connection_id, schema, table));
        self.target = Some(Target {
            connection_id,
            kind,
            node,
        });
    }

    pub fn forget(&mut self, connection_id: Uuid) {
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.connection_id == connection_id)
        {
            *self = Self::default();
        }
    }

    pub fn poll(&mut self, events: &mut Vec<StructureEvent>) {
        if let Some(load) = &mut self.columns_load
            && let Some(result) = load.poll()
        {
            self.columns_load = None;
            match result {
                Ok(columns) => self.columns = Some(columns),
                Err(error) => events.push(StructureEvent::Notice(error.to_string(), true)),
            }
        }
        if let Some(load) = &mut self.keys_load
            && let Some(result) = load.poll()
        {
            self.keys_load = None;
            match result {
                Ok(keys) => self.keys = Some(keys),
                // Keys are a nicety: a dialect without them still shows its columns.
                Err(error) => {
                    log::info!("table keys unavailable: {error}");
                    self.keys = Some(TableKeys::default());
                }
            }
        }
        if let Some(apply) = &mut self.apply
            && let Some(result) = apply.poll()
        {
            self.apply = None;
            match result {
                Ok(()) => {
                    if let Some(form) = self.form.take() {
                        events.push(StructureEvent::Notice(
                            format!("Applied: {}", form.describe()),
                            false,
                        ));
                        events.push(StructureEvent::Changed(form.connection_id));
                    }
                }
                Err(error) => {
                    if let Some(form) = &mut self.form {
                        form.error = Some(error.to_string());
                    }
                }
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, workspace: &Workspace, read_only_profile: bool) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Structure")).font(theme::semibold(12.5)));
            if let Some(target) = &self.target {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if target.kind != DatabaseKind::MongoDb
                        && ui
                            .small_button(t("＋ Column"))
                            .on_hover_text(t("Add a column to this table"))
                            .clicked()
                    {
                        let (schema, table) = split_table_id(&target.node, target.kind);
                        self.form = Some(TableChangeForm::add_column(
                            target.connection_id,
                            target.kind,
                            schema,
                            table,
                        ));
                    }
                });
            }
        });
        let Some(target) = self.target.clone() else {
            ui.label(RichText::new(t("Select a table in the schema tree.")).weak());
            return;
        };
        ui.label(RichText::new(&target.node.name).strong().monospace());
        if let Some(schema) = target.node.attributes.get("schema") {
            ui.label(RichText::new(format!("schema {schema}")).small().weak());
        }
        egui::ScrollArea::vertical()
            .id_salt("structure")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new(t("Columns")).small().strong());
                match &self.columns {
                    None => {
                        ui.spinner();
                    }
                    Some(columns) if columns.is_empty() => {
                        ui.label(RichText::new(t("No columns reported.")).weak());
                    }
                    Some(columns) => {
                        let keys = self.keys.clone().unwrap_or_default();
                        egui::Grid::new("structure-columns")
                            .num_columns(3)
                            .striped(true)
                            .spacing([12.0, 2.0])
                            .show(ui, |ui| {
                                for column in columns {
                                    let is_key = keys.primary.contains(&column.name);
                                    let name = RichText::new(&column.name).monospace();
                                    ui.label(if is_key {
                                        name.color(Tokens::of(ui).accent_text)
                                    } else {
                                        name
                                    });
                                    ui.label(
                                        RichText::new(column.data_type.clone().unwrap_or_default())
                                            .small()
                                            .weak(),
                                    );
                                    let mut flags = Vec::new();
                                    if is_key {
                                        flags.push("PK");
                                    }
                                    if column.nullable == Some(false) {
                                        flags.push("not null");
                                    }
                                    if keys
                                        .foreign
                                        .iter()
                                        .any(|fk| fk.columns.contains(&column.name))
                                    {
                                        flags.push("FK");
                                    }
                                    ui.label(RichText::new(flags.join(" · ")).small());
                                    ui.end_row();
                                }
                            });
                    }
                }
                if let Some(keys) = &self.keys {
                    if !keys.foreign.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Foreign keys")).small().strong());
                        for fk in &keys.foreign {
                            let mut line = format!(
                                "{} → {} ({})",
                                fk.columns.join(", "),
                                fk.referenced_table,
                                fk.referenced_columns.join(", ")
                            );
                            if !fk.on_delete.is_empty() && fk.on_delete != "NO ACTION" {
                                line.push_str(&format!(
                                    " on delete {}",
                                    fk.on_delete.to_lowercase()
                                ));
                            }
                            ui.label(RichText::new(line).monospace().small())
                                .on_hover_text(if fk.name.is_empty() {
                                    "unnamed constraint".to_owned()
                                } else {
                                    fk.name.clone()
                                });
                        }
                    }
                    if !keys.indexes.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(t("Indexes")).small().strong());
                        for index in &keys.indexes {
                            let mut tags = Vec::new();
                            if index.primary {
                                tags.push("primary");
                            } else if index.unique {
                                tags.push("unique");
                            }
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&index.name).monospace().small());
                                ui.label(
                                    RichText::new(format!("({})", index.columns.join(", ")))
                                        .small()
                                        .weak(),
                                );
                                if !tags.is_empty() {
                                    ui.label(
                                        RichText::new(tags.join(", "))
                                            .small()
                                            .color(Tokens::of(ui).accent_text),
                                    );
                                }
                            });
                        }
                    }
                } else if self.keys_load.is_some() {
                    ui.spinner();
                }
            });
        if let Some(form) = &mut self.form {
            let mut keep = true;
            let mut submit = false;
            form.ui(
                ui.ctx(),
                &mut keep,
                &mut submit,
                read_only_profile,
                self.apply.is_some(),
            );
            if submit {
                match form.change() {
                    Ok(change) => {
                        form.error = None;
                        self.apply = Some(workspace.apply_table_change(form.connection_id, change));
                    }
                    Err(reason) => form.error = Some(reason),
                }
            }
            if !keep {
                self.form = None;
            }
        }
    }
}

#[derive(Clone)]
struct ColumnRow {
    name: String,
    column_type: ColumnType,
    nullable: bool,
    primary_key: bool,
}

impl Default for ColumnRow {
    fn default() -> Self {
        Self {
            name: String::new(),
            column_type: ColumnType::Text,
            nullable: true,
            primary_key: false,
        }
    }
}

pub struct TableChangeForm {
    pub connection_id: Uuid,
    kind: DatabaseKind,
    schema: String,
    table: String,
    create: bool,
    columns: Vec<ColumnRow>,
    pub error: Option<String>,
}

impl TableChangeForm {
    fn add_column(
        connection_id: Uuid,
        kind: DatabaseKind,
        schema: Option<String>,
        table: String,
    ) -> Self {
        Self {
            connection_id,
            kind,
            schema: schema.unwrap_or_default(),
            table,
            create: false,
            columns: vec![ColumnRow::default()],
            error: None,
        }
    }

    fn describe(&self) -> String {
        if self.create {
            format!("created table {}", self.table)
        } else {
            format!(
                "added column {} to {}",
                self.columns.first().map_or("", |c| c.name.as_str()),
                self.table
            )
        }
    }

    fn specs(&self) -> Vec<ColumnSpec> {
        self.columns
            .iter()
            .map(|c| ColumnSpec {
                name: c.name.trim().to_owned(),
                column_type: c.column_type,
                nullable: c.nullable && !c.primary_key,
                primary_key: c.primary_key,
            })
            .collect()
    }

    fn change(&self) -> Result<TableChange, String> {
        let schema = Some(self.schema.trim().to_owned()).filter(|s| !s.is_empty());
        let table = self.table.trim().to_owned();
        if table.is_empty() {
            return Err("name the table".to_owned());
        }
        if self.columns.iter().any(|c| c.name.trim().is_empty()) {
            return Err("every column needs a name".to_owned());
        }
        let change = if self.create {
            TableChange::CreateTable {
                schema,
                table,
                columns: self.specs(),
            }
        } else {
            let column = self
                .specs()
                .into_iter()
                .next()
                .ok_or_else(|| "name the column".to_owned())?;
            TableChange::AddColumn {
                schema,
                table,
                column,
            }
        };
        // The engine renders the statement; a refusal here is the same one it would give.
        change.statement(self.kind).map_err(|e| e.to_string())?;
        Ok(change)
    }

    fn ui(
        &mut self,
        ctx: &egui::Context,
        keep: &mut bool,
        submit: &mut bool,
        read_only: bool,
        busy: bool,
    ) {
        let title = if self.create {
            t("Create table")
        } else {
            t("Add column")
        };
        let mut open = true;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                egui::Grid::new("ddl-head")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        if matches!(self.kind, DatabaseKind::PostgreSql | DatabaseKind::MySql) {
                            ui.label(t("Schema"));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.schema)
                                    .desired_width(200.0)
                                    .hint_text("default"),
                            );
                            ui.end_row();
                        }
                        ui.label(t("Table"));
                        ui.add_enabled(
                            self.create,
                            egui::TextEdit::singleline(&mut self.table).desired_width(300.0),
                        );
                        ui.end_row();
                    });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(if self.create {
                        t("Columns")
                    } else {
                        t("Column")
                    })
                    .small()
                    .strong(),
                );
                let mut remove: Option<usize> = None;
                let create = self.create;
                let count = self.columns.len();
                egui::Grid::new("ddl-columns")
                    .num_columns(5)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new(t("Name")).small().weak());
                        ui.label(RichText::new(t("Type")).small().weak());
                        ui.label(RichText::new(t("Nullable")).small().weak());
                        ui.label(RichText::new(t("Primary")).small().weak());
                        ui.label("");
                        ui.end_row();
                        for (index, column) in self.columns.iter_mut().enumerate() {
                            ui.add(
                                egui::TextEdit::singleline(&mut column.name).desired_width(160.0),
                            );
                            egui::ComboBox::from_id_salt(("ddl-type", index))
                                .selected_text(type_name(column.column_type))
                                .show_ui(ui, |ui| {
                                    for t in [
                                        ColumnType::Text,
                                        ColumnType::Integer,
                                        ColumnType::Decimal,
                                        ColumnType::Boolean,
                                        ColumnType::Date,
                                        ColumnType::Timestamp,
                                    ] {
                                        ui.selectable_value(
                                            &mut column.column_type,
                                            t,
                                            type_name(t),
                                        );
                                    }
                                });
                            ui.add_enabled(
                                !column.primary_key,
                                egui::Checkbox::without_text(&mut column.nullable),
                            );
                            ui.add_enabled(
                                create,
                                egui::Checkbox::without_text(&mut column.primary_key),
                            );
                            if create && count > 1 && ui.small_button("✕").clicked() {
                                remove = Some(index);
                            } else if !create {
                                ui.label("");
                            }
                            ui.end_row();
                        }
                    });
                if let Some(index) = remove {
                    self.columns.remove(index);
                }
                if self.create && ui.small_button(t("＋ column")).clicked() {
                    self.columns.push(ColumnRow::default());
                }
                if let Ok(change) = self.change()
                    && let Ok(statement) = change.statement(self.kind)
                {
                    ui.add_space(6.0);
                    ui.label(RichText::new(statement).monospace().small().weak());
                }
                if read_only {
                    ui.label(
                        RichText::new(t(
                            "This connection is read-only; the change will be refused.",
                        ))
                        .color(Tokens::of(ui).warning),
                    );
                }
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(Tokens::of(ui).danger));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!busy && !read_only, theme::primary_button(ui, t("Apply")))
                        .clicked()
                    {
                        *submit = true;
                    }
                    if ui.button(t("Cancel")).clicked() {
                        *keep = false;
                    }
                    if busy {
                        ui.spinner();
                    }
                });
            });
        if !open {
            *keep = false;
        }
    }
}

fn type_name(t: ColumnType) -> &'static str {
    match t {
        ColumnType::Text => "text",
        ColumnType::Integer => "integer",
        ColumnType::Decimal => "decimal",
        ColumnType::Boolean => "boolean",
        ColumnType::Date => "date",
        ColumnType::Timestamp => "timestamp",
    }
}
