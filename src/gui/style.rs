//! Colours: edge kinds, flow kinds, activation tags, and the per-trigger-type shades that tell
//! triggers of the same kind apart.

use super::*;

pub(super) fn kind_color(k: EdgeKind) -> Color32 {
    let (r, g, b) = k.rgb();
    Color32::from_rgb(r, g, b)
}

pub(super) fn act_color(a: Activation) -> Color32 {
    match a {
        Activation::Spawn => Color32::from_rgb(80, 200, 120),
        Activation::Position => Color32::from_rgb(240, 150, 60),
        Activation::Touch => Color32::from_rgb(90, 170, 255),
        Activation::Player => Color32::from_rgb(200, 120, 255),
        // logic objects that aren't "run" (collision blocks, counter labels)
        Activation::None => Color32::from_rgb(110, 200, 220),
    }
}

pub(super) fn flow_color(k: Option<FlowKind>) -> Color32 {
    match k {
        Some(k) => {
            let (r, g, b) = k.rgb();
            Color32::from_rgb(r, g, b)
        }
        None => Color32::from_gray(150),
    }
}

/// Opaque colour scaled toward black (unlike `gamma_multiply`, which also fades alpha).
pub(super) fn shade(c: Color32, f: f32) -> Color32 {
    let s = |v: u8| (v as f32 * f).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(s(c.r()), s(c.g()), s(c.b()))
}

/// Map fill of logic objects; their outline carries the activation colour.
pub(super) const TRIGGER_FILL: Color32 = Color32::from_gray(50);

pub(super) const WARN_COLOR: Color32 = Color32::from_rgb(230, 170, 60);

pub(super) const CYCLE_COLOR: Color32 = Color32::from_rgb(60, 190, 190);

pub(super) const PLATE_BG: Color32 = Color32::from_rgba_premultiplied(8, 8, 10, 215);

/// Trigger types in the order their shade variants are handed out, grouped by flow kind so that
/// types sharing a kind colour get neighbouring indices, which never share a variant.
pub(super) const VARIANT_ORDER: &[i32] = {
    use gdlib::cclocallevels::gdobj::ids::objects as o;
    &[
        // decision
        o::ITEM_COMPARE_TRIGGER,
        o::RANDOM_TRIGGER,
        o::ADVANCED_RANDOM_TRIGGER,
        o::INSTANT_COUNT_TRIGGER,
        o::INSTANT_COLLISION_TRIGGER,
        // stop / end
        o::STOP_TRIGGER,
        o::RESET_TRIGGER,
        o::END_TRIGGER,
        // event
        o::COUNT_TRIGGER,
        o::COLLISION_TRIGGER,
        o::ON_DEATH_TRIGGER,
        o::TIME_EVENT_TRIGGER,
        o::EVENT_TRIGGER,
        o::COLLISION_STATE_BLOCK,
        o::PLAYER_TOUCH_TOGGLE_BLOCK,
        o::CHECKPOINT,
        // item
        o::ITEM_EDIT_TRIGGER,
        o::PICKUP_TRIGGER,
        o::PERSISTENT_ITEM_SETUP_TRIGGER,
        o::TIME_TRIGGER,
        o::TIME_CONTROL_TRIGGER,
        o::COUNTER_LABEL,
        o::COLLISION_BLOCK,
        // call
        o::SPAWN_TRIGGER,
        o::TOGGLE_TRIGGER,
        o::TOUCH_TRIGGER,
        // process
        o::MOVE_TRIGGER,
        o::ROTATE_TRIGGER,
        o::SCALE_TRIGGER,
        o::ALPHA_TRIGGER,
        o::COLOR_TRIGGER,
        o::PULSE_TRIGGER,
        o::FOLLOW_TRIGGER,
        o::FOLLOW_PLAYER_Y_TRIGGER,
        o::ANIMATE_TRIGGER,
        o::LINK_VISIBLE_TRIGGER,
        o::SPAWN_PARTICLE_TRIGGER,
        o::TELEPORT_TRIGGER,
        o::TIMEWARP_TRIGGER,
        o::SHAKE_TRIGGER,
        o::UI_TRIGGER,
    ]
};

/// Per trigger type tweak of a kind colour: (brightness factor, hue shift, stroke width factor).
/// Every trigger type always gets the same variant, and types sharing a colour (e.g. spawn and
/// item edit, both green when spawn-triggered) stay tellable apart.
pub(super) fn trigger_variant(obj_id: i32) -> (f32, f32, f32) {
    const BRIGHT: [f32; 6] = [1.0, 0.72, 1.18, 0.86, 1.09, 0.79];
    const HUE: [f32; 3] = [0.0, 0.025, -0.025];
    const WIDTH: [f32; 4] = [1.0, 1.6, 0.7, 1.3];
    // listed types: consecutive indices differ in brightness and width, and the first 36 are unique
    let k = VARIANT_ORDER
        .iter()
        .position(|&i| i == obj_id)
        .unwrap_or_else(|| {
            let h = (obj_id as u32).wrapping_mul(2_654_435_761);
            (h ^ (h >> 15)) as usize
        });
    (BRIGHT[k % 6], HUE[k / 6 % 3], WIDTH[k % 4])
}

/// `base` shaded for one trigger type (see [`trigger_variant`]).
pub(super) fn trigger_shade(base: Color32, obj_id: i32) -> Color32 {
    let (bright, hue, _) = trigger_variant(obj_id);
    let mut c = egui::ecolor::Hsva::from(base);
    c.h = (c.h + hue).rem_euclid(1.0);
    if bright > 1.0 {
        // lighter: towards white, since bright colours have no headroom left in value
        c.s *= 2.0 - bright;
        c.v = (c.v * bright).min(1.0);
    } else {
        c.v *= bright;
    }
    Color32::from(c)
}
