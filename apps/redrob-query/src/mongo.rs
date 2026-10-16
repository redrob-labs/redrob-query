//! Mongo tabs: a strip above the editor that writes a `find` or `aggregate` request body for a
//! collection, with `explain`, so the JSON the engine accepts does not have to be typed from
//! memory. The engine (`connection.rs`) accepts exactly these keys: `collection`, `operation`
//! (`find` | `aggregate`), `filter`, `projection`, `sort`, `limit`, `explain` (a verbosity string),
//! `pipeline`.

use egui::{RichText, Ui};
use redrob_query_core::MetadataNode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MongoOperation {
    Find,
    Aggregate,
}

#[derive(Clone, Debug)]
pub struct MongoStrip {
    pub collection: String,
    pub operation: MongoOperation,
    pub explain: bool,
}

impl Default for MongoStrip {
    fn default() -> Self {
        Self {
            collection: String::new(),
            operation: MongoOperation::Find,
            explain: false,
        }
    }
}

impl MongoStrip {
    /// Draws the strip; returns a request body to put in the editor when a template was asked for.
    pub fn ui(&mut self, ui: &mut Ui, collections: &[MetadataNode]) -> Option<String> {
        let mut template = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Collection").small());
            egui::ComboBox::from_id_salt("mongo-collection")
                .selected_text(if self.collection.is_empty() {
                    "choose…"
                } else {
                    self.collection.as_str()
                })
                .width(180.0)
                .show_ui(ui, |ui| {
                    for node in collections {
                        ui.selectable_value(&mut self.collection, node.name.clone(), &node.name);
                    }
                });
            ui.selectable_value(&mut self.operation, MongoOperation::Find, "find");
            ui.selectable_value(&mut self.operation, MongoOperation::Aggregate, "aggregate");
            ui.checkbox(&mut self.explain, "explain");
            if ui
                .button("Insert template")
                .on_hover_text("Replace the editor text with a request for this collection")
                .clicked()
            {
                template = Some(self.template());
            }
        });
        template
    }

    pub fn template(&self) -> String {
        let collection = serde_json::Value::String(self.collection.clone());
        // `explain` is a verbosity, as the engine reads it (query_planner | execution_stats | all_plans_execution).
        let explain = if self.explain {
            ",\n  \"explain\": \"execution_stats\""
        } else {
            ""
        };
        match self.operation {
            MongoOperation::Find => format!(
                "{{\n  \"collection\": {collection},\n  \"operation\": \"find\",\n  \"filter\": {{}},\n  \"sort\": {{ \"_id\": 1 }},\n  \"limit\": 50{explain}\n}}"
            ),
            MongoOperation::Aggregate => format!(
                "{{\n  \"collection\": {collection},\n  \"operation\": \"aggregate\",\n  \"pipeline\": [\n    {{ \"$match\": {{}} }},\n    {{ \"$limit\": 50 }}\n  ]{explain}\n}}"
            ),
        }
    }

    /// Picks up the collection named in the editor text, so the strip follows a typed query.
    pub fn sync_from(&mut self, text: &str) {
        if let Ok(serde_json::Value::Object(body)) = serde_json::from_str::<serde_json::Value>(text)
        {
            if let Some(serde_json::Value::String(name)) = body.get("collection") {
                self.collection.clone_from(name);
            }
            self.operation = match body.get("operation").and_then(|v| v.as_str()) {
                Some("aggregate") => MongoOperation::Aggregate,
                _ => MongoOperation::Find,
            };
            self.explain = body.get("explain").is_some_and(|v| !v.is_null());
        }
    }
}
