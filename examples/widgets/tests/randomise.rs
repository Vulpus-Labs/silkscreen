//! Random sounds: the browser's Random button, one undo step,
//! every param inside its range.

use silkscreen::random::Seed;
use silkscreen::{MemoryStore, PresetBrowser};
use silkscreen_harness::Harness;
use widgets::{PARAMS, faceplate::ids};

fn values(h: &Harness) -> Vec<f32> {
    h.editor().state().param_values().collect()
}

#[test]
fn every_param_lands_in_its_range_and_on_a_position() {
    let mut h = Harness::new(widgets::editor, 1.0);
    for seed in 0..200 {
        h.editor_mut()
            .state
            .randomise_preset_seeded(Seed::from_bits(seed));
        for (spec, v) in PARAMS.iter().zip(values(&h)) {
            assert!((spec.min..=spec.max).contains(&v), "{} = {v}", spec.name);
            if !spec.position_labels().is_empty() {
                assert_eq!(v, v.round(), "{} on a position", spec.name);
            }
        }
    }
}

#[test]
fn the_same_seed_gives_the_same_sound() {
    let mut a = Harness::new(widgets::editor, 1.0);
    let mut b = Harness::new(widgets::editor, 1.0);
    let seed = Seed::parse("3F2AK").unwrap();
    a.editor_mut().state.randomise_preset_seeded(seed);
    b.editor_mut().state.randomise_preset_seeded(seed);
    assert_eq!(values(&a), values(&b));
    assert_eq!(a.editor().state().preset_name(), Some("Random"));
    assert_eq!(
        a.editor().state().preset_comment(),
        Some("Random, seed 3F2AK")
    );
}

#[test]
fn the_browsers_random_button_makes_a_new_sound_as_one_step() {
    let mut h = Harness::new(widgets::editor, 1.0);
    let b = PresetBrowser::new(
        PresetBrowser::centred(widgets::faceplate::LAYOUT.size),
        Box::new(MemoryStore::default()),
        widgets::FACTORY,
        h.editor().state(),
    );
    let i = b
        .footer_labels()
        .iter()
        .position(|l| *l == "Random")
        .unwrap();
    let button = b.footer_button(i).center();
    let before = values(&h);
    h.click(ids::PROGRAM);
    h.click_at(button);
    let first = values(&h);
    assert_ne!(first, before);
    let s = h.editor().state();
    let comment = s.preset_comment().unwrap().to_owned();
    let seed = comment
        .strip_prefix("Random, seed ")
        .expect("its seed in the comment");
    assert!(Seed::parse(seed).is_some(), "{comment}");
    assert_eq!(s.preset_place(), None, "saved nowhere");
    h.click_at(button);
    assert_ne!(values(&h), first, "a different sound each time");
    assert_ne!(
        h.editor().state().preset_comment(),
        Some(comment.as_str()),
        "a new seed"
    );
    h.raw(silkscreen::core::RawKind::Key {
        key: silkscreen::core::Key::Escape,
        mods: silkscreen::core::Modifiers::NONE,
        pressed: true,
    });
    assert!(h.editor_mut().undo());
    assert_eq!(values(&h), first, "one undo step each");
}
