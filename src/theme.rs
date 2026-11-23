use eframe::egui::{self, Color32, Stroke, TextStyle, Theme, ThemePreference, Visuals};
use save_manager::Appearance;

const NORD0: Color32 = Color32::from_rgb(46, 52, 64);
const NORD1: Color32 = Color32::from_rgb(59, 66, 82);
const NORD2: Color32 = Color32::from_rgb(67, 76, 94);
const NORD3: Color32 = Color32::from_rgb(76, 86, 106);
const NORD4: Color32 = Color32::from_rgb(216, 222, 233);
const NORD6: Color32 = Color32::from_rgb(236, 239, 244);
const NORD8: Color32 = Color32::from_rgb(136, 192, 208);
const NORD11: Color32 = Color32::from_rgb(191, 97, 106);
const NORD13: Color32 = Color32::from_rgb(235, 203, 139);

pub fn configure(ctx: &egui::Context, appearance: Option<Appearance>) {
    let mut light = Visuals::light();
    light.panel_fill = Color32::from_rgb(240, 244, 249);
    light.window_fill = Color32::WHITE;
    light.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(218, 226, 237));
    light.faint_bg_color = Color32::from_rgb(245, 248, 252);
    light.text_edit_bg_color = Some(light.faint_bg_color);
    light.weak_text_color = Some(Color32::from_rgb(85, 101, 123));
    light.selection.bg_fill = Color32::from_rgb(219, 232, 251);
    light.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(30, 76, 141));
    light.error_fg_color = Color32::DARK_RED;
    ctx.set_visuals_of(Theme::Light, light);
    ctx.set_visuals_of(Theme::Dark, nord());
    ctx.options_mut(|options| options.fallback_theme = Theme::Light);
    ctx.set_theme(preference(appearance));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10., 9.);
        style.spacing.button_padding = egui::vec2(12., 7.);
        style.spacing.interact_size.y = 30.;
        style.text_styles.get_mut(&TextStyle::Body).unwrap().size = 14.;
        style.text_styles.get_mut(&TextStyle::Button).unwrap().size = 14.;
        style.text_styles.get_mut(&TextStyle::Heading).unwrap().size = 20.;
        let widgets = &mut style.visuals.widgets;
        for widget in [
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            widget.corner_radius = 7.into();
            widget.expansion = 0.;
        }
        style.visuals.window_corner_radius = 14.into();
        style.visuals.menu_corner_radius = 9.into();
    });
}

pub fn card(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().window_stroke)
        .corner_radius(14)
        .inner_margin(18)
}

pub fn primary(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    ui.scope(|ui| {
        let dark = ui.visuals().dark_mode;
        let (fill, hover, pressed, text) = if dark {
            (
                NORD8,
                Color32::from_rgb(151, 204, 218),
                Color32::from_rgb(116, 175, 192),
                NORD0,
            )
        } else {
            (
                Color32::from_rgb(34, 89, 167),
                Color32::from_rgb(28, 78, 151),
                Color32::from_rgb(24, 66, 130),
                Color32::WHITE,
            )
        };
        let widgets = &mut ui.visuals_mut().widgets;
        for (widget, color) in [
            (&mut widgets.inactive, fill),
            (&mut widgets.hovered, hover),
            (&mut widgets.active, pressed),
        ] {
            widget.weak_bg_fill = color;
            widget.bg_stroke = Stroke::new(1.0_f32, color);
            widget.fg_stroke.color = text;
        }
        ui.add_enabled(enabled, egui::Button::new(label))
    })
    .inner
}

fn nord() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = NORD0;
    visuals.window_fill = NORD1;
    visuals.window_stroke = Stroke::new(1.0_f32, NORD3);
    visuals.faint_bg_color = NORD0;
    visuals.extreme_bg_color = NORD0;
    visuals.text_edit_bg_color = Some(NORD0);
    visuals.code_bg_color = NORD1;
    visuals.weak_text_color = Some(NORD4);
    visuals.hyperlink_color = NORD8;
    visuals.warn_fg_color = NORD13;
    // Blend the Aurora red with Snow Storm for legibility on dark surfaces.
    visuals.error_fg_color = NORD11.lerp_to_gamma(NORD6, 0.4);
    visuals.selection.bg_fill = NORD2;
    visuals.selection.stroke = Stroke::new(1.0_f32, NORD8);
    visuals.text_cursor.stroke = Stroke::new(2.0_f32, NORD8);

    visuals.widgets.noninteractive.bg_fill = NORD1;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, NORD3);
    visuals.widgets.noninteractive.fg_stroke.color = NORD4;
    for (widget, background, outline) in [
        (&mut visuals.widgets.inactive, NORD1, NORD3),
        (&mut visuals.widgets.hovered, NORD2, NORD8),
        (&mut visuals.widgets.active, NORD3, NORD8),
        (&mut visuals.widgets.open, NORD2, NORD8),
    ] {
        widget.bg_fill = background;
        widget.weak_bg_fill = background;
        widget.bg_stroke = Stroke::new(1.0_f32, outline);
        widget.fg_stroke.color = NORD6;
    }
    visuals
}

pub fn preference(appearance: Option<Appearance>) -> ThemePreference {
    match appearance {
        None => ThemePreference::System,
        Some(Appearance::Light) => ThemePreference::Light,
        Some(Appearance::Dark) => ThemePreference::Dark,
    }
}

pub fn toggle(ui: &mut egui::Ui) -> Option<Appearance> {
    let (label, next) = match ui.ctx().theme() {
        Theme::Light => ("切换到 Nord 深色", Appearance::Dark),
        Theme::Dark => ("切换到浅色", Appearance::Light),
    };
    ui.button(label).clicked().then_some(next)
}
