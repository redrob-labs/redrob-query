//! The shell's look, the same system Redrob Canvas draws with: one set of colour and shape tokens
//! per ground (dark, light), one egui style built from them at start-up and never reset by a view,
//! and one font set (Inter for the interface, `JetBrains Mono` for SQL and grid cells, a system CJK
//! face as the fallback for Hangul, Han and Kana) installed once.
//!
//! The dark ground is the Pro (Medium Gray) palette, the light ground Pro (Light); both come from
//! the canvas design tokens so the two apps sit together. Which ground draws follows the operating
//! system unless Help > Theme pins one ([`ThemePref`]). Widgets read [`Tokens::of`] instead of
//! hard-coding colours.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};

/// Redrob Blue (`--blue-6`): the one brand accent, for the primary button and the selected tab.
pub const REDROB_BLUE: Color32 = Color32::from_rgb(0x2B, 0x52, 0xFF);

/// Help > Theme: follow the system, or pin a ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePref {
    pub const ALL: [ThemePref; 3] = [ThemePref::System, ThemePref::Light, ThemePref::Dark];

    /// The preference file's value.
    pub fn id(self) -> &'static str {
        match self {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "light" => ThemePref::Light,
            "dark" => ThemePref::Dark,
            _ => ThemePref::System,
        }
    }

    /// The menu entry (an English source string for `i18n::t`).
    pub fn label(self) -> &'static str {
        match self {
            ThemePref::System => "System",
            ThemePref::Light => "Light",
            ThemePref::Dark => "Dark",
        }
    }

    fn preference(self) -> egui::ThemePreference {
        match self {
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Colour and shape tokens of one ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// Window chrome: the top bar, the status line, the panels.
    pub chrome: Color32,
    /// The ground behind the editor and the grid.
    pub canvas: Color32,
    /// The dock behind cards.
    pub dock: Color32,
    pub card: Color32,
    pub card_border: Color32,
    /// Inputs, fields, dropdowns.
    pub field: Color32,
    pub field_border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_border: Color32,
    pub accent_text: Color32,
    pub separator: Color32,
    pub shadow: Color32,
    pub primary_bg: Color32,
    pub primary_text: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub radius_sm: u8,
    pub radius: u8,
    pub radius_lg: u8,
    /// The tab strip above a panel's content.
    pub tab_strip: Color32,
    /// The selected list row.
    pub row_selected: Color32,
    /// Every other grid row.
    pub row_stripe: Color32,
}

impl Tokens {
    /// The dark ground: canvas's Pro (Medium Gray).
    pub const DARK: Tokens = Tokens {
        dark: true,
        chrome: Color32::from_rgb(83, 83, 83),
        canvas: Color32::from_rgb(40, 40, 40),
        dock: Color32::from_rgb(66, 66, 66),
        card: Color32::from_rgb(83, 83, 83),
        card_border: Color32::from_rgb(66, 66, 66),
        field: Color32::from_rgb(69, 69, 69),
        field_border: Color32::from_rgb(104, 104, 104),
        hover: Color32::from_rgb(98, 98, 98),
        pressed: Color32::from_rgb(112, 112, 112),
        text: Color32::from_rgb(238, 238, 238),
        text_dim: Color32::from_rgb(212, 212, 212),
        text_faint: Color32::from_rgb(160, 160, 160),
        accent: Color32::from_rgb(55, 142, 240),
        accent_soft: Color32::from_rgb(110, 110, 110),
        accent_border: Color32::from_rgb(55, 142, 240),
        accent_text: Color32::WHITE,
        separator: Color32::from_rgb(62, 62, 62),
        shadow: Color32::from_black_alpha(150),
        primary_bg: Color32::from_rgb(55, 142, 240),
        primary_text: Color32::WHITE,
        danger: Color32::from_rgb(236, 91, 98),
        warning: Color32::from_rgb(232, 176, 70),
        success: Color32::from_rgb(92, 196, 132),
        radius_sm: 3,
        radius: 4,
        radius_lg: 6,
        tab_strip: Color32::from_rgb(74, 74, 74),
        row_selected: Color32::from_rgb(107, 107, 107),
        row_stripe: Color32::from_rgb(46, 46, 46),
    };

    /// The light ground: canvas's Pro (Light).
    pub const LIGHT: Tokens = Tokens {
        dark: false,
        chrome: Color32::from_rgb(240, 240, 240),
        canvas: Color32::from_rgb(252, 252, 252),
        dock: Color32::from_rgb(226, 226, 226),
        card: Color32::from_rgb(240, 240, 240),
        card_border: Color32::from_rgb(214, 214, 214),
        field: Color32::WHITE,
        field_border: Color32::from_rgb(186, 186, 186),
        hover: Color32::from_rgb(226, 226, 226),
        pressed: Color32::from_rgb(206, 206, 206),
        text: Color32::from_rgb(20, 20, 20),
        text_dim: Color32::from_rgb(64, 64, 64),
        text_faint: Color32::from_rgb(120, 120, 120),
        accent: Color32::from_rgb(55, 142, 240),
        accent_soft: Color32::from_rgb(214, 214, 214),
        accent_border: Color32::from_rgb(55, 142, 240),
        accent_text: Color32::from_rgb(16, 98, 196),
        separator: Color32::from_rgb(214, 214, 214),
        shadow: Color32::from_black_alpha(50),
        primary_bg: Color32::from_rgb(55, 142, 240),
        primary_text: Color32::WHITE,
        danger: Color32::from_rgb(200, 48, 56),
        warning: Color32::from_rgb(176, 112, 10),
        success: Color32::from_rgb(28, 140, 84),
        radius_sm: 3,
        radius: 4,
        radius_lg: 6,
        tab_strip: Color32::from_rgb(226, 226, 226),
        row_selected: Color32::from_rgb(204, 204, 204),
        row_stripe: Color32::from_rgb(244, 244, 244),
    };

    /// The tokens of the ground `ui` draws in.
    pub fn of(ui: &egui::Ui) -> Tokens {
        if ui.visuals().dark_mode {
            Tokens::DARK
        } else {
            Tokens::LIGHT
        }
    }

    fn visuals(&self) -> Visuals {
        let t = self;
        let mut v = if t.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = t.chrome;
        v.window_fill = t.card;
        v.window_stroke = Stroke::new(1.0, t.card_border);
        v.extreme_bg_color = t.field;
        v.faint_bg_color = t.row_stripe;
        v.code_bg_color = t.field;
        v.override_text_color = Some(t.text);
        v.hyperlink_color = t.accent;
        v.warn_fg_color = t.warning;
        v.error_fg_color = t.danger;
        v.window_corner_radius = CornerRadius::same(t.radius_lg);
        v.menu_corner_radius = CornerRadius::same(t.radius);
        v.window_shadow = egui::Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 0,
            color: t.shadow,
        };
        v.popup_shadow = egui::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: t.shadow,
        };
        v.selection.bg_fill = t.accent;
        v.selection.stroke = Stroke::new(1.0, t.accent_text);
        v.text_cursor.stroke.color = t.accent;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.striped = true;
        v.indent_has_left_vline = false;
        let r = CornerRadius::same(t.radius_sm);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = t.card;
        w.noninteractive.weak_bg_fill = t.card;
        w.noninteractive.bg_stroke = Stroke::new(1.0, t.separator);
        w.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
        w.noninteractive.corner_radius = r;
        for (wv, bg, stroke) in [
            (&mut w.inactive, t.field, t.field_border),
            (&mut w.hovered, t.hover, t.field_border),
            (&mut w.active, t.pressed, t.accent_border),
            (&mut w.open, t.hover, t.field_border),
        ] {
            wv.bg_fill = bg;
            wv.weak_bg_fill = bg;
            wv.bg_stroke = Stroke::new(1.0, stroke);
            wv.fg_stroke = Stroke::new(1.0, t.text);
            wv.corner_radius = r;
            wv.expansion = 0.0;
        }
        v
    }
}

/// The one style, built from the tokens: both of egui's theme branches get their ground's
/// palette, so following the system needs no re-apply. Called once at start-up.
pub fn apply(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(pref.preference());
    for (theme, tokens) in [
        (egui::Theme::Dark, Tokens::DARK),
        (egui::Theme::Light, Tokens::LIGHT),
    ] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = tokens.visuals();
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(10.5)),
            (TextStyle::Body, FontId::proportional(12.5)),
            (TextStyle::Button, FontId::proportional(12.5)),
            (TextStyle::Heading, semibold(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 4.0);
        style.spacing.interact_size = egui::vec2(24.0, 24.0);
        style.spacing.slider_width = 150.0;
        style.spacing.combo_width = 120.0;
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.window_margin = egui::Margin::same(16);
        style.spacing.icon_width = 14.0;
        style.spacing.tooltip_width = 320.0;
        style.interaction.tooltip_delay = TOOLTIP_DELAY;
        let mut scroll = egui::style::ScrollStyle::thin();
        scroll.interact_handle_opacity = 1.0;
        scroll.interact_background_opacity = 0.4;
        scroll.floating_width = 4.0;
        style.spacing.scroll = scroll;
        ctx.set_style_of(theme, style);
    }
}

/// Help > Theme: re-point egui at a ground without rebuilding the styles.
pub fn set_pref(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(pref.preference());
}

/// Seconds the pointer rests on a control before its tooltip shows.
pub const TOOLTIP_DELAY: f32 = 0.35;

/// Height of the top bar, in points (canvas's).
pub const TOP_BAR_HEIGHT: f32 = 28.0;
/// Height of the status line, in points.
pub const STATUS_HEIGHT: f32 = 24.0;

// ---------------------------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------------------------

const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// System fonts with Hangul, Han and Kana, in the order tried. The first that reads is appended
/// to every family; none means those glyphs draw as boxes, as egui's defaults would.
const CJK_CANDIDATES: &[&str] = &[
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
    // macOS
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
    // Windows
    "C:\\Windows\\Fonts\\malgun.ttf",
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\YuGothR.ttc",
];

/// The font set: Inter first in the proportional family, `JetBrains Mono` first in the monospace
/// family, the `medium` and `semibold` weights as named families, egui's defaults behind them for
/// symbols and emoji, and the system CJK fallback last in every family.
pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("Inter", INTER_REGULAR),
        ("Inter-Medium", INTER_MEDIUM),
        ("Inter-SemiBold", INTER_SEMIBOLD),
        ("JetBrainsMono", JETBRAINS_MONO),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".to_owned());
    let fallback: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (family, primary) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts
            .families
            .insert(FontFamily::Name(family.into()), stack);
    }
    if let Some((path, bytes)) = CJK_CANDIDATES
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
    {
        log::info!("CJK fallback font: {path}");
        fonts.font_data.insert(
            "system-cjk".to_owned(),
            Arc::new(FontData::from_owned(bytes)),
        );
        for stack in fonts.families.values_mut() {
            stack.push("system-cjk".to_owned());
        }
    } else {
        log::info!("no CJK system font found; Hangul, Han and Kana draw as boxes");
    }
    fonts
}

/// Inter `SemiBold` at `size` (headings, the brand).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

// ---------------------------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------------------------

/// A filled accent button (the primary action of a panel).
pub fn primary_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    let t = Tokens::of(ui);
    egui::Button::new(egui::RichText::new(text).color(t.primary_text))
        .fill(t.primary_bg)
        .stroke(Stroke::new(1.0, t.primary_bg))
}

/// A filled danger button (Remove, Discard).
pub fn danger_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    let t = Tokens::of(ui);
    egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
        .fill(t.danger)
        .stroke(Stroke::new(1.0, t.danger))
}

/// A small status dot: green connected, grey not, red unsupported.
pub fn status_dot(ui: &mut egui::Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, colour);
}

/// The frame every side panel and bar draws in: the chrome fill with a 1 px separator stroke.
pub fn panel_frame(ui: &egui::Ui) -> egui::Frame {
    let visuals = ui.visuals();
    egui::Frame::NONE
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::same(8))
        .stroke(Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
}

/// A bar frame (top bar, status line): the chrome fill and the separator, no inner margin.
pub fn bar_frame(ui: &egui::Ui) -> egui::Frame {
    let visuals = ui.visuals();
    egui::Frame::NONE
        .fill(visuals.panel_fill)
        .stroke(Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
}

/// A small rounded badge: `text` on `fill` (a count, read-only / read-write).
pub fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, text_colour: Color32) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), FontId::proportional(10.0), text_colour);
    let size = galley.size() + egui::vec2(10.0, 3.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, text_colour);
    response
}

/// A chip in the top bar's right slot: a dot, then text, in a bordered pill.
pub fn chip(ui: &mut egui::Ui, dot: Option<Color32>, text: &str) -> egui::Response {
    let t = Tokens::of(ui);
    egui::Frame::NONE
        .fill(t.field)
        .stroke(Stroke::new(1.0, t.field_border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(colour) = dot {
                    status_dot(ui, colour);
                }
                ui.label(egui::RichText::new(text).small());
            });
        })
        .response
}
