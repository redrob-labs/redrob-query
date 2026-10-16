//! The Schema tree of the active connection: database > (schema >) tables and views > columns,
//! with columns loaded on expand. Nodes come from the engine's `load_metadata`, which returns the
//! root (tables) for a connection and the children (columns, or a Mongo collection's sampled
//! fields) for a node id.
//!
//! Double-click inserts the name into the editor; the context menu offers Select top 100, Copy
//! name and DDL (a CREATE TABLE built from the column metadata, opened in a new tab).

use std::collections::HashMap;

use egui::{RichText, Ui};
use redrob_query_core::{
    ConnectionProfile, DatabaseKind, MetadataKind, MetadataNode, MetadataRequest, Pending,
    Workspace,
};
use uuid::Uuid;

use crate::i18n::{t, tf};
use crate::theme::{self, Tokens};

/// What the tree asks the app to do.
#[derive(Debug)]
pub enum SchemaEvent {
    /// A table was clicked: show it in the structure panel.
    Select {
        connection_id: Uuid,
        node: MetadataNode,
    },
    /// Select top 100: open a tab with a bounded SELECT on the table and run it.
    Preview {
        connection_id: Uuid,
        node: MetadataNode,
    },
    /// Put this name at the editor's caret.
    Insert(String),
    /// Open a new tab with this text (the DDL).
    OpenText {
        connection_id: Uuid,
        title: String,
        text: String,
    },
    Notice(String, bool),
}

#[derive(Default)]
struct Tree {
    roots: Vec<MetadataNode>,
    children: HashMap<String, Vec<MetadataNode>>,
    loading: HashMap<Option<String>, Pending<Vec<MetadataNode>>>,
    expanded: HashMap<String, bool>,
    filter: String,
    /// Tables whose DDL was asked for before their columns were loaded.
    ddl_wanted: Vec<String>,
}

#[derive(Default)]
pub struct SchemaBrowser {
    trees: HashMap<Uuid, Tree>,
    pub selected: Option<(Uuid, String)>,
}

impl SchemaBrowser {
    pub fn load_root(&mut self, workspace: &Workspace, connection_id: Uuid) {
        let tree = self.trees.entry(connection_id).or_default();
        tree.roots.clear();
        tree.children.clear();
        tree.loading.insert(
            None,
            workspace.load_metadata(MetadataRequest {
                connection_id,
                parent_id: None,
            }),
        );
    }

    pub fn forget(&mut self, connection_id: Uuid) {
        self.trees.remove(&connection_id);
        if self
            .selected
            .as_ref()
            .is_some_and(|(id, _)| *id == connection_id)
        {
            self.selected = None;
        }
    }

    /// The root nodes of a connection, for the editor's completion and the structure panel.
    pub fn roots(&self, connection_id: Uuid) -> &[MetadataNode] {
        self.trees
            .get(&connection_id)
            .map_or(&[], |tree| tree.roots.as_slice())
    }

    pub fn poll(&mut self, kinds: &HashMap<Uuid, DatabaseKind>, events: &mut Vec<SchemaEvent>) {
        for (connection_id, tree) in &mut self.trees {
            let mut done = Vec::new();
            for (parent, pending) in &mut tree.loading {
                if let Some(result) = pending.poll() {
                    done.push((parent.clone(), result));
                }
            }
            for (parent, result) in done {
                tree.loading.remove(&parent);
                match (parent, result) {
                    (None, Ok(nodes)) => tree.roots = nodes,
                    (Some(id), Ok(nodes)) => {
                        tree.children.insert(id.clone(), nodes);
                        if let Some(at) = tree.ddl_wanted.iter().position(|w| *w == id)
                            && let Some(node) = tree.roots.iter().find(|n| n.id == id)
                            && let Some(kind) = kinds.get(connection_id)
                        {
                            tree.ddl_wanted.remove(at);
                            let columns = tree.children.get(&id).cloned().unwrap_or_default();
                            events.push(SchemaEvent::OpenText {
                                connection_id: *connection_id,
                                title: tf("DDL {table}", &[("table", &node.name)]),
                                text: ddl_for(*kind, node, &columns),
                            });
                        }
                    }
                    (_, Err(error)) => events.push(SchemaEvent::Notice(error.to_string(), true)),
                }
            }
        }
    }

    /// The tree of `profile` (the active connection), or a hint when nothing is connected.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        profile: Option<&ConnectionProfile>,
        connected: bool,
        events: &mut Vec<SchemaEvent>,
    ) {
        let tokens = Tokens::of(ui);
        ui.label(RichText::new(t("Schema")).font(theme::semibold(12.5)));
        let Some(profile) = profile else {
            ui.label(
                RichText::new(t("Select a connection to browse its tables."))
                    .small()
                    .color(tokens.text_faint),
            );
            return;
        };
        if !connected {
            ui.label(
                RichText::new(tf(
                    "{name} is not connected. Double-click it to connect.",
                    &[("name", &profile.name)],
                ))
                .small()
                .color(tokens.text_faint),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("schema")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.connection_ui(ui, workspace, profile, events);
            });
    }

    fn connection_ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        profile: &ConnectionProfile,
        events: &mut Vec<SchemaEvent>,
    ) {
        let tokens = Tokens::of(ui);
        let connection_id = profile.id;
        let kind = profile.kind;
        let tree = self.trees.entry(connection_id).or_default();
        let root_loading = tree.loading.contains_key(&None);
        let mut to_load: Vec<String> = Vec::new();
        let mut reload = false;
        let mut selected = self.selected.clone();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut tree.filter)
                    .hint_text(t("Filter tables"))
                    .desired_width(ui.available_width() - 32.0),
            );
            if ui.small_button("⟳").on_hover_text(t("Reload")).clicked() {
                reload = true;
            }
        });
        let database = database_name(profile);
        let header = egui::CollapsingHeader::new(RichText::new(&database).strong())
            .id_salt(("schema-root", connection_id))
            .default_open(true);
        header.show(ui, |ui| {
            if root_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(t("Loading…")).color(tokens.text_faint));
                });
            } else if tree.roots.is_empty() {
                ui.label(
                    RichText::new(t("No tables or collections."))
                        .small()
                        .color(tokens.text_faint),
                );
            }
            let filter = tree.filter.to_lowercase();
            // Group by schema attribute when present (PostgreSQL, MySQL).
            let mut groups: Vec<(String, Vec<&MetadataNode>)> = Vec::new();
            for node in &tree.roots {
                if !filter.is_empty() && !node.name.to_lowercase().contains(&filter) {
                    continue;
                }
                let schema = node.attributes.get("schema").cloned().unwrap_or_default();
                match groups.iter_mut().find(|(s, _)| *s == schema) {
                    Some((_, nodes)) => nodes.push(node),
                    None => groups.push((schema, vec![node])),
                }
            }
            let grouped = groups.len() > 1 || groups.first().is_some_and(|(s, _)| !s.is_empty());
            for (schema, nodes) in groups {
                let body = |ui: &mut Ui| {
                    for node in nodes {
                        let expanded = tree.expanded.get(&node.id).copied().unwrap_or(false);
                        let is_selected = selected
                            .as_ref()
                            .is_some_and(|(c, id)| *c == connection_id && *id == node.id);
                        let glyph = match node.kind {
                            MetadataKind::View => "▤",
                            MetadataKind::Collection => "{}",
                            _ => "▦",
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let arrow = if expanded { "▾" } else { "▸" };
                            if ui
                                .add(egui::Button::new(RichText::new(arrow).small()).frame(false))
                                .clicked()
                            {
                                let now = !expanded;
                                tree.expanded.insert(node.id.clone(), now);
                                if now
                                    && !tree.children.contains_key(&node.id)
                                    && !tree.loading.contains_key(&Some(node.id.clone()))
                                {
                                    to_load.push(node.id.clone());
                                }
                            }
                            let response = ui.selectable_label(
                                is_selected,
                                RichText::new(format!("{glyph} {}", node.name)).monospace(),
                            );
                            if response.clicked() {
                                selected = Some((connection_id, node.id.clone()));
                                events.push(SchemaEvent::Select {
                                    connection_id,
                                    node: node.clone(),
                                });
                            }
                            if response.double_clicked() {
                                events.push(SchemaEvent::Insert(quoted_name(kind, node)));
                            }
                            response.context_menu(|ui| {
                                if ui.button(t("Select top 100")).clicked() {
                                    events.push(SchemaEvent::Preview {
                                        connection_id,
                                        node: node.clone(),
                                    });
                                    ui.close();
                                }
                                if ui.button(t("Copy name")).clicked() {
                                    ui.ctx().copy_text(node.name.clone());
                                    ui.close();
                                }
                                if ui.button(t("Insert name")).clicked() {
                                    events.push(SchemaEvent::Insert(quoted_name(kind, node)));
                                    ui.close();
                                }
                                if kind != DatabaseKind::MongoDb && ui.button("DDL").clicked() {
                                    match tree.children.get(&node.id) {
                                        Some(columns) => events.push(SchemaEvent::OpenText {
                                            connection_id,
                                            title: tf("DDL {table}", &[("table", &node.name)]),
                                            text: ddl_for(kind, node, columns),
                                        }),
                                        None => {
                                            tree.ddl_wanted.push(node.id.clone());
                                            if !tree.loading.contains_key(&Some(node.id.clone())) {
                                                to_load.push(node.id.clone());
                                            }
                                        }
                                    }
                                    ui.close();
                                }
                            });
                        });
                        if expanded {
                            ui.indent(("cols", &node.id), |ui| match tree.children.get(&node.id) {
                                Some(children) => {
                                    for column in children {
                                        if let Some(name) = column_row(ui, column) {
                                            events.push(SchemaEvent::Insert(name));
                                        }
                                    }
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(
                                            RichText::new(t("Loading…"))
                                                .small()
                                                .color(tokens.text_faint),
                                        );
                                    });
                                }
                            });
                        }
                    }
                };
                if grouped {
                    egui::CollapsingHeader::new(RichText::new(if schema.is_empty() {
                        t("(default)").to_owned()
                    } else {
                        schema.clone()
                    }))
                    .id_salt(("schema-group", connection_id, &schema))
                    .default_open(true)
                    .show(ui, body);
                } else {
                    body(ui);
                }
            }
        });
        self.selected = selected;
        for id in to_load {
            let request = MetadataRequest {
                connection_id,
                parent_id: Some(id.clone()),
            };
            if let Some(tree) = self.trees.get_mut(&connection_id) {
                tree.loading
                    .insert(Some(id), workspace.load_metadata(request));
            }
        }
        if reload {
            self.load_root(workspace, connection_id);
        }
    }
}

/// A column row: name in monospace, type dimmed; double-click gives the name to insert.
fn column_row(ui: &mut Ui, column: &MetadataNode) -> Option<String> {
    let tokens = Tokens::of(ui);
    let mut insert = None;
    ui.horizontal(|ui| {
        let response = ui
            .add(
                egui::Label::new(RichText::new(&column.name).monospace().small())
                    .sense(egui::Sense::click()),
            )
            .on_hover_text(t("Double-click to insert"));
        if response.double_clicked() {
            insert = Some(column.name.clone());
        }
        response.context_menu(|ui| {
            if ui.button(t("Copy name")).clicked() {
                ui.ctx().copy_text(column.name.clone());
                ui.close();
            }
            if ui.button(t("Insert name")).clicked() {
                insert = Some(column.name.clone());
                ui.close();
            }
        });
        if let Some(data_type) = &column.data_type {
            ui.label(RichText::new(data_type).small().color(tokens.text_faint));
        }
        if column.nullable == Some(false) {
            ui.label(RichText::new("not null").small().color(tokens.warning));
        }
    });
    insert
}

/// The database the tree is rooted at: the configured database, the `SQLite` file name, or the host.
fn database_name(profile: &ConnectionProfile) -> String {
    let c = &profile.config;
    match profile.kind {
        DatabaseKind::SQLite => c
            .file_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map_or_else(
                || ":memory:".to_owned(),
                |n| n.to_string_lossy().into_owned(),
            ),
        _ => c
            .database
            .clone()
            .filter(|d| !d.is_empty())
            .or_else(|| c.host.clone())
            .unwrap_or_else(|| profile.name.clone()),
    }
}

/// `schema`/`table` out of a root node id (`table:<schema>.<name>` or `table:<name>`,
/// `collection:<name>`), the way the engine addresses them.
pub fn split_table_id(node: &MetadataNode, kind: DatabaseKind) -> (Option<String>, String) {
    let schema = node
        .attributes
        .get("schema")
        .cloned()
        .filter(|s| !s.is_empty());
    match kind {
        DatabaseKind::PostgreSql | DatabaseKind::MySql => (schema, node.name.clone()),
        _ => (None, node.name.clone()),
    }
}

fn quote(kind: DatabaseKind, name: &str) -> String {
    match kind {
        DatabaseKind::MySql => format!("`{}`", name.replace('`', "``")),
        DatabaseKind::MongoDb => name.to_owned(),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// The table's qualified, quoted name as the dialect writes it.
fn quoted_name(kind: DatabaseKind, node: &MetadataNode) -> String {
    let (schema, table) = split_table_id(node, kind);
    match schema {
        Some(s) => format!("{}.{}", quote(kind, &s), quote(kind, &table)),
        None => quote(kind, &table),
    }
}

/// A bounded SELECT (or find) on a table, for a preview tab.
pub fn preview_query(kind: DatabaseKind, node: &MetadataNode) -> String {
    let (schema, table) = split_table_id(node, kind);
    match kind {
        DatabaseKind::MongoDb => format!(
            "{{\n  \"collection\": {},\n  \"operation\": \"find\",\n  \"filter\": {{}},\n  \"limit\": 100\n}}",
            serde_json::Value::String(table)
        ),
        _ => {
            let target = match schema {
                Some(s) => format!("{}.{}", quote(kind, &s), quote(kind, &table)),
                None => quote(kind, &table),
            };
            format!("SELECT *\nFROM {target}\nLIMIT 100;")
        }
    }
}

/// A CREATE TABLE statement rebuilt from the column metadata (name, type, nullability): what the
/// tree knows, not the server's own DDL, and said so in a leading comment.
pub fn ddl_for(kind: DatabaseKind, node: &MetadataNode, columns: &[MetadataNode]) -> String {
    let mut out = format!(
        "-- {}\n",
        t("Rebuilt from the column metadata; constraints and defaults are not included.")
    );
    let name = quoted_name(kind, node);
    if node.kind == MetadataKind::View {
        out.push_str(&format!("-- {name} is a view.\n"));
    }
    out.push_str(&format!("CREATE TABLE {name} (\n"));
    let lines: Vec<String> = columns
        .iter()
        .map(|c| {
            let mut line = format!("  {}", quote(kind, &c.name));
            if let Some(data_type) = &c.data_type {
                line.push(' ');
                line.push_str(data_type);
            }
            if c.nullable == Some(false) {
                line.push_str(" NOT NULL");
            }
            line
        })
        .collect();
    out.push_str(&lines.join(",\n"));
    out.push_str("\n);\n");
    out
}
