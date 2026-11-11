//! Reading typed values out of raw object properties, plus name / number formatting helpers.

use super::*;

pub fn get_group(obj: &GDObject, prop: u16) -> Option<Gid> {
    let g = match obj.get_property(prop)? {
        GDValue::Group(g) | GDValue::Item(g) | GDValue::Short(g) => g,
        GDValue::Int(i) => i16::try_from(i).ok()?,
        _ => return None,
    };
    (g > 0).then_some(g)
}

pub(super) fn get_item(obj: &GDObject, prop: u16) -> Option<i16> {
    get_group(obj, prop)
}

pub fn flag(obj: &GDObject, prop: u16) -> bool {
    matches!(obj.get_property(prop), Some(GDValue::Bool(true)))
}

pub fn float(obj: &GDObject, prop: u16) -> Option<f32> {
    match obj.get_property(prop)? {
        GDValue::Float(f) => Some(f),
        GDValue::Double(d) => Some(d as f32),
        GDValue::Int(i) => Some(i as f32),
        _ => None,
    }
}

pub(super) fn item_ns(obj: &GDObject, type_prop: u16) -> ItemNs {
    match obj.get_property(type_prop) {
        Some(GDValue::ItemType(t)) => match t {
            ItemType::Counter => ItemNs::Counter,
            ItemType::Timer => ItemNs::Timer,
            ItemType::Points => ItemNs::Points,
            ItemType::MainTime => ItemNs::MainTime,
            ItemType::Attempts => ItemNs::Attempts,
            _ => ItemNs::Counter,
        },
        _ => ItemNs::Counter,
    }
}

pub fn property_name(prop: u16) -> &'static str {
    PROPERTY_TABLE.get(&prop).map(|(n, _)| *n).unwrap_or("?")
}

/// Human-friendly rendering of a property value.
pub fn fmt_value(v: &GDValue) -> String {
    match v {
        GDValue::Int(_)
        | GDValue::Short(_)
        | GDValue::Float(_)
        | GDValue::Double(_)
        | GDValue::Bool(_)
        | GDValue::Group(_)
        | GDValue::Item(_)
        | GDValue::String(_)
        | GDValue::GroupList(_)
        | GDValue::ProbabilitiesList(_)
        | GDValue::SpawnRemapsList(_) => v.to_string(),
        other => format!("{other:?}"),
    }
}

pub(super) fn mnemonic(name: &str) -> String {
    let base = name
        .trim_end_matches(" Trigger")
        .trim_end_matches(" trigger");
    base.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

pub(super) fn fmt_f(f: f32) -> String {
    let s = format!("{f:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.to_string()
    }
}
