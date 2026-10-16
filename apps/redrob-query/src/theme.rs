//! The shell's look: egui's dark and light themes with the design system's tokens where they are
//! what distinguishes a Redrob window (redrob-design-system `20-color.md`: "Blue acts", ground
//! `deep` is Redrob Black). Every colour literal in the shell lives here, named after its token.

use egui::Color32;

/// `--blue-6`, Redrob Blue: the one accent. Buttons that act, the selected tab, the caret.
pub const REDROB_BLUE: Color32 = Color32::from_rgb(0x2B, 0x52, 0xFF);
/// `--redrob-black`: the deep ground behind the brand lockup (kept for the About surface).
#[allow(dead_code)]
pub const REDROB_BLACK: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x0C);
/// `--red-6`, danger: destructive classifications and failed runs.
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x3E, 0x3E);
/// `--green-6`, success: a connected profile, an applied change set.
pub const SUCCESS: Color32 = Color32::from_rgb(0x1F, 0xA9, 0x71);
/// `--amber-6`, caution: an elevated-risk mutation, a truncated result.
pub const CAUTION: Color32 = Color32::from_rgb(0xE0, 0x9A, 0x1F);

/// Applies the shell's style on top of egui's theme: the accent, slightly roomier spacing, and
/// a monospace size that reads as code without crowding the grid.
pub fn apply(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.visuals.selection.bg_fill = REDROB_BLUE.linear_multiply(0.35);
        style.visuals.selection.stroke.color = REDROB_BLUE;
        style.visuals.hyperlink_color = REDROB_BLUE;
        style.visuals.text_cursor.stroke.color = REDROB_BLUE;
        style.visuals.widgets.active.bg_fill = REDROB_BLUE.linear_multiply(0.6);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        if let Some(mono) = style.text_styles.get_mut(&egui::TextStyle::Monospace) {
            mono.size = 13.0;
        }
    });
}

/// A filled accent button (the primary action of a panel).
pub fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE)).fill(REDROB_BLUE)
}

/// A small status dot: green connected, grey not, red unsupported.
pub fn status_dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, colour);
}

/// The caution/danger/neutral colour for a message that may be a warning.
pub fn tone_for_error(ui: &egui::Ui, is_error: bool) -> Color32 {
    if is_error {
        DANGER
    } else {
        ui.visuals().text_color()
    }
}
