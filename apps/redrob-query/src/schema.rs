//! The Schema browser: tables/views/collections of each connected profile, with columns loaded on
//! expand. Nodes come from the engine's `load_metadata`, which returns the root (tables) for a
//! connection and the children (columns, or a Mongo collection's sampled fields) for a node id.

use std::collections::HashMap;

use egui::{RichText, Ui};
use redrob_query_core::{
    ConnectionProfile, DatabaseKind, MetadataKind, MetadataNode, MetadataRequest, Pending,
    Workspace,
};
use uuid::Uuid;

use crate::theme;

/// What the browser asks the app to do.
#[derive(Debug)]
pub enum SchemaEvent {
    /// Show this table in the structure panel and (double-click) run a starter SELECT on it.
    Select {
        connection_id: Uuid,
        node: MetadataNode,
    },
    Preview {
        connection_id: Uuid,
        node: MetadataNode,
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

    pub fn poll(&mut self, events: &mut Vec<SchemaEvent>) {
        for tree in self.trees.values_mut() {
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
                        tree.children.insert(id, nodes);
                    }
                    (_, Err(error)) => events.push(SchemaEvent::Notice(error.to_string(), true)),
                }
            }
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        connected: &[&ConnectionProfile],
        events: &mut Vec<SchemaEvent>,
    ) {
        ui.heading("Schema");
        if connected.is_empty() {
            ui.label(RichText::new("Connect a profile to browse its tables.").weak());
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("schema")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for profile in connected {
                    self.connection_ui(ui, workspace, profile, events);
                }
            });
    }

    fn connection_ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        profile: &ConnectionProfile,
        events: &mut Vec<SchemaEvent>,
    ) {
        let connection_id = profile.id;
        let tree = self.trees.entry(connection_id).or_default();
        let root_loading = tree.loading.contains_key(&None);
        let header = egui::CollapsingHeader::new(RichText::new(&profile.name).strong())
            .id_salt(("schema-root", connection_id))
            .default_open(true);
        let mut to_load: Vec<String> = Vec::new();
        let mut reload = false;
        let mut selected = self.selected.clone();
        header.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut tree.filter)
                        .hint_text("Filter tables")
                        .desired_width(ui.available_width() - 40.0),
                );
                if ui.small_button("⟳").on_hover_text("Reload").clicked() {
                    reload = true;
                }
            });
            if root_loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Loading…").weak());
                });
            } else if tree.roots.is_empty() {
                ui.label(RichText::new("No tables or collections.").weak());
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
                            let arrow = if expanded { "▾" } else { "▸" };
                            if ui.small_button(arrow).clicked() {
                                let now = !expanded;
                                tree.expanded.insert(node.id.clone(), now);
                                if now
                                    && !tree.children.contains_key(&node.id)
                                    && !tree.loading.contains_key(&Some(node.id.clone()))
                                {
                                    to_load.push(node.id.clone());
                                }
                            }
                            let response =
                                ui.selectable_label(is_selected, format!("{glyph} {}", node.name));
                            if response.clicked() {
                                selected = Some((connection_id, node.id.clone()));
                                events.push(SchemaEvent::Select {
                                    connection_id,
                                    node: node.clone(),
                                });
                            }
                            if response.double_clicked() {
                                events.push(SchemaEvent::Preview {
                                    connection_id,
                                    node: node.clone(),
                                });
                            }
                            response.context_menu(|ui| {
                                if ui.button("Preview rows").clicked() {
                                    events.push(SchemaEvent::Preview {
                                        connection_id,
                                        node: node.clone(),
                                    });
                                    ui.close();
                                }
                                if ui.button("Copy name").clicked() {
                                    ui.ctx().copy_text(node.name.clone());
                                    ui.close();
                                }
                            });
                        });
                        if expanded {
                            ui.indent(("cols", &node.id), |ui| match tree.children.get(&node.id) {
                                Some(children) => {
                                    for column in children {
                                        column_row(ui, column);
                                    }
                                }
                                None => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(RichText::new("Loading…").weak().small());
                                    });
                                }
                            });
                        }
                    }
                };
                if grouped {
                    egui::CollapsingHeader::new(RichText::new(if schema.is_empty() {
                        "(default)".to_owned()
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

fn column_row(ui: &mut Ui, column: &MetadataNode) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&column.name).monospace().small());
        if let Some(data_type) = &column.data_type {
            ui.label(RichText::new(data_type).small().weak());
        }
        if column.nullable == Some(false) {
            ui.label(RichText::new("not null").small().color(theme::CAUTION));
        }
    });
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

/// A bounded SELECT (or find) on a table, for a preview tab.
pub fn preview_query(kind: DatabaseKind, node: &MetadataNode) -> String {
    let (schema, table) = split_table_id(node, kind);
    match kind {
        DatabaseKind::MongoDb => format!(
            "{{\n  \"collection\": {},\n  \"operation\": \"find\",\n  \"filter\": {{}},\n  \"limit\": 50\n}}",
            serde_json::Value::String(table)
        ),
        DatabaseKind::MySql => {
            let target = match schema {
                Some(s) => format!("`{}`.`{}`", s.replace('`', "``"), table.replace('`', "``")),
                None => format!("`{}`", table.replace('`', "``")),
            };
            format!("SELECT *\nFROM {target}\nLIMIT 200;")
        }
        _ => {
            let quote = |n: &str| format!("\"{}\"", n.replace('"', "\"\""));
            let target = match schema {
                Some(s) => format!("{}.{}", quote(&s), quote(&table)),
                None => quote(&table),
            };
            format!("SELECT *\nFROM {target}\nLIMIT 200;")
        }
    }
}
