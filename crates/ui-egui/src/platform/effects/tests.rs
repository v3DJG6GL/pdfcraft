use super::*;

fn effects(mode: ReducedEffects, adapter: Option<bool>) -> Effects {
    let mut e = Effects { mode, ..Effects::default() };
    if let Some(cpu) = adapter {
        e.set_software_renderer(cpu);
    }
    e
}

#[test]
fn reduced_effects_follow_the_override_then_the_adapter() {
    for adapter in [Some(true), Some(false), None] {
        assert!(effects(ReducedEffects::On, adapter).reduced(), "on is reduced on {adapter:?}");
        assert!(!effects(ReducedEffects::Off, adapter).reduced(), "off is never reduced, {adapter:?}");
    }
    assert!(effects(ReducedEffects::Auto, Some(true)).reduced(), "a software rasterizer reduces them");
    assert!(!effects(ReducedEffects::Auto, Some(false)).reduced(), "a GPU doesn't");
    assert!(!effects(ReducedEffects::Auto, None).reduced(), "nor does a start that reported no adapter");

    assert_eq!(ReducedEffects::from_env(Some("1")), ReducedEffects::On);
    assert_eq!(ReducedEffects::from_env(Some(" 1 ")), ReducedEffects::On);
    assert_eq!(ReducedEffects::from_env(Some("0")), ReducedEffects::Off);
    for other in [Some(""), Some("yes"), Some("on"), None] {
        assert_eq!(ReducedEffects::from_env(other), ReducedEffects::Auto, "{other:?}");
    }
    for mode in ReducedEffects::ALL {
        assert_eq!(ReducedEffects::parse(mode.name()), Some(mode));
        assert_eq!(ReducedEffects::parse(&mode.name().to_uppercase()), Some(mode));
    }
    assert_eq!(ReducedEffects::parse("maybe"), None);
}

#[test]
fn dithering_is_off_only_after_a_software_start_or_when_forced() {
    for last in [true, false] {
        assert!(!ReducedEffects::On.dithering_at_start(last), "forced on, after a software start: {last}");
        assert!(ReducedEffects::Off.dithering_at_start(last), "forced off, after a software start: {last}");
    }
    assert!(!ReducedEffects::Auto.dithering_at_start(true));
    assert!(ReducedEffects::Auto.dithering_at_start(false));
}

#[test]
fn the_software_flag_survives_a_save_and_a_start_without_an_adapter() {
    let mut first = effects(ReducedEffects::On, Some(true));
    let saved_json = serde_json::json!({ KEY: first.saved_value() });
    assert!(saved(&saved_json));

    // An OpenGL start reports no adapter and keeps what the last start said.
    let mut next = Effects::default();
    next.restore(&saved_json);
    assert!(next.saved_value());
    assert!(!next.reduced(), "the saved flag alone doesn't reduce effects");
    next.set_software_renderer(false);
    assert!(!next.saved_value(), "a GPU start replaces it");

    for malformed in [serde_json::json!({}), serde_json::json!({ KEY: 1 }), serde_json::json!("["), serde_json::Value::Null] {
        assert!(!saved(&malformed), "{malformed}");
    }
    first.mode = ReducedEffects::Off;
    assert!(first.saved_value(), "the session override is not what gets saved");
}

#[test]
fn the_state_is_handed_to_egui_once_per_change() {
    let ctx = egui::Context::default();
    assert!(!reduced(&ctx));
    let mut e = effects(ReducedEffects::Auto, Some(true));
    assert!(e.publish(&ctx));
    assert!(reduced(&ctx));
    assert!(!e.publish(&ctx), "unchanged");
    e.mode = ReducedEffects::Off;
    assert!(e.publish(&ctx));
    assert!(!reduced(&ctx));
}
