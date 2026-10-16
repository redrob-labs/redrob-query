//! The AI dock: ask for a query in plain words, get a draft back, put it in the editor.
//!
//! Each turn sends only the prompt and the active tab's text (never result rows or credentials)
//! to the console with the shared account. Fenced `sql` / `mql` / `json` blocks in the answer each
//! get an Insert button. Without a signed-in account the dock says so and offers the launcher.

use egui::{RichText, Ui};
use redrob_query_core::{
    AiAssistantRequest, AiChatResponse, DatabaseKind, Pending, QueryLanguage, Workspace,
    ai::{AiKeySource, AiKeyStatus, NOT_SIGNED_IN_MESSAGE},
};

use crate::theme;

#[derive(Clone, Debug)]
pub struct Turn {
    pub user: bool,
    pub text: String,
}

#[derive(Default)]
pub struct AiDock {
    pub prompt: String,
    pub turns: Vec<Turn>,
    pending: Option<Pending<AiChatResponse>>,
    error: Option<String>,
    pub status: AiKeyStatus,
}

pub enum AiEvent {
    /// Put this draft in the active editor tab.
    Insert(String),
    Notice(String, bool),
}

impl AiDock {
    pub fn new(workspace: &Workspace) -> Self {
        Self {
            status: workspace.ai_key_status(),
            ..Self::default()
        }
    }

    pub fn refresh_status(&mut self, workspace: &Workspace) {
        self.status = workspace.ai_key_status();
    }

    pub fn poll(&mut self, events: &mut Vec<AiEvent>) {
        if let Some(pending) = &mut self.pending
            && let Some(result) = pending.poll()
        {
            self.pending = None;
            match result {
                Ok(response) => self.turns.push(Turn {
                    user: false,
                    text: response.message.content,
                }),
                Err(error) => {
                    let text = error.to_string();
                    self.error = Some(text.clone());
                    events.push(AiEvent::Notice(text, true));
                }
            }
        }
    }

    pub fn ui(
        &mut self,
        ui: &mut Ui,
        workspace: &Workspace,
        active: Option<(DatabaseKind, QueryLanguage, String)>,
        events: &mut Vec<AiEvent>,
    ) {
        ui.horizontal(|ui| {
            ui.heading("Ask");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Clear").clicked() {
                    self.turns.clear();
                    self.error = None;
                }
            });
        });
        match self.status.source {
            AiKeySource::None => {
                ui.label(RichText::new(NOT_SIGNED_IN_MESSAGE).color(theme::CAUTION));
                if ui.button("Open Redrob Launcher").clicked() {
                    let _ = webbrowser::open(Workspace::launcher_url());
                }
            }
            AiKeySource::Launcher => {
                ui.label(
                    RichText::new(format!("{} · via Redrob Launcher", self.status.label))
                        .small()
                        .weak(),
                );
            }
            AiKeySource::Own => {
                ui.label(
                    RichText::new(format!("{} · this app's own key", self.status.label))
                        .small()
                        .weak(),
                );
            }
        }
        ui.separator();

        let signed_in = self.status.signed_in();
        let scroll_height = (ui.available_height() - 120.0).max(80.0);
        egui::ScrollArea::vertical()
            .id_salt("ai-turns")
            .auto_shrink([false, false])
            .max_height(scroll_height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.turns.is_empty() {
                    ui.label(RichText::new(
                        "Describe the query you want. The active tab's text is sent along as context; result rows never are.",
                    ).weak());
                }
                for (index, turn) in self.turns.iter().enumerate() {
                    let who = if turn.user { "You" } else { "Redrob" };
                    ui.label(RichText::new(who).small().strong().color(if turn.user { ui.visuals().text_color() } else { theme::REDROB_BLUE }));
                    if turn.user {
                        ui.label(&turn.text);
                    } else {
                        render_answer(ui, index, &turn.text, events);
                    }
                    ui.add_space(6.0);
                }
                if self.pending.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Thinking…").weak());
                    });
                }
                if let Some(error) = &self.error {
                    ui.label(RichText::new(error).color(theme::DANGER));
                }
            });

        ui.separator();
        let can_ask = signed_in && self.pending.is_none() && active.is_some();
        let response = ui.add_enabled(
            signed_in,
            egui::TextEdit::multiline(&mut self.prompt)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text(if active.is_some() {
                    "e.g. customers who ordered twice this month, newest first"
                } else {
                    "Open a query tab first"
                }),
        );
        let submit = response.has_focus()
            && ui.input(|i| {
                i.key_pressed(egui::Key::Enter) && (i.modifiers.command || i.modifiers.ctrl)
            });
        ui.horizontal(|ui| {
            if (ui
                .add_enabled(
                    can_ask && !self.prompt.trim().is_empty(),
                    theme::primary_button("Ask (Ctrl+Enter)"),
                )
                .clicked()
                || (submit && can_ask))
                && let Some((kind, language, text)) = active
            {
                self.ask(workspace, kind, language, text);
            }
        });
    }

    fn ask(
        &mut self,
        workspace: &Workspace,
        kind: DatabaseKind,
        language: QueryLanguage,
        active: String,
    ) {
        let prompt = self.prompt.trim().to_owned();
        if prompt.is_empty() {
            return;
        }
        self.error = None;
        self.turns.push(Turn {
            user: true,
            text: prompt.clone(),
        });
        self.prompt.clear();
        let language = match language {
            QueryLanguage::Auto => {
                if kind == DatabaseKind::MongoDb {
                    QueryLanguage::MongoJson
                } else {
                    QueryLanguage::Sql
                }
            }
            other => other,
        };
        self.pending = Some(workspace.ai_chat(AiAssistantRequest {
            prompt,
            active_query: Some(active).filter(|t| !t.trim().is_empty()),
            database_kind: kind,
            query_language: language,
        }));
    }
}

/// Renders an answer: prose as labels, fenced code blocks as code with an Insert button.
fn render_answer(ui: &mut Ui, index: usize, text: &str, events: &mut Vec<AiEvent>) {
    for (block, piece) in split_fences(text) {
        if block {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(piece.trim()).monospace().small()).wrap(),
                    );
                });
                ui.horizontal(|ui| {
                    if ui
                        .add(theme::primary_button("Insert into editor"))
                        .clicked()
                    {
                        events.push(AiEvent::Insert(piece.trim().to_owned()));
                    }
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(piece.trim().to_owned());
                    }
                });
            });
        } else if !piece.trim().is_empty() {
            ui.push_id(("ai-prose", index, piece.len()), |ui| {
                ui.add(egui::Label::new(piece.trim()).wrap());
            });
        }
    }
}

/// Splits markdown into `(is_code_block, text)` pieces by triple-backtick fences; the language tag is dropped.
pub fn split_fences(text: &str) -> Vec<(bool, String)> {
    let mut pieces = Vec::new();
    let mut in_block = false;
    let mut current = String::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if !current.is_empty() || in_block {
                pieces.push((in_block, std::mem::take(&mut current)));
            }
            in_block = !in_block;
            continue;
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        pieces.push((in_block, current));
    }
    pieces
}
