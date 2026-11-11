//! Small toolbar widgets and canvas sizing shared by the views.

use super::*;

/// "minimal | detailed" switch at the start of a view's toolbar.
pub(super) fn mode_toggle(ui: &mut egui::Ui, detail: &mut bool) {
    ui.selectable_value(detail, false, "minimal")
        .on_hover_text("shapes and names only");
    ui.selectable_value(detail, true, "detailed")
        .on_hover_text("show what every trigger does");
}

/// "raw | details | tasm" switch: how triggers are written.
pub(super) fn listing_toggle(ui: &mut egui::Ui, mode: &mut Mode) {
    for m in Mode::ALL {
        ui.selectable_value(mode, m, m.name())
            .on_hover_text(m.hint());
    }
}

/// Size of the drawing canvas below a toolbar: the rest of the panel, never wider than what is
/// visible (an overflowing toolbar would otherwise widen the canvas under the side panel).
pub(super) fn canvas_size(ui: &egui::Ui) -> Vec2 {
    let avail = ui.available_size();
    vec2(
        avail
            .x
            .min(ui.clip_rect().right() - ui.cursor().left())
            .max(0.0),
        avail.y,
    )
}
