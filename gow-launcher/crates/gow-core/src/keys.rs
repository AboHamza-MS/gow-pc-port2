//! Configurable keyboard and mouse map. Codes are GLFW key codes (physical position, as in the game) and
//! `MOUSE_BASE + button` for the mouse. The map goes to the game in `PC_KEYS` (`ps2_pad.cpp` reads it).

use std::collections::BTreeMap;

pub const MOUSE_BASE: i64 = 1000;

/// Actions in tab order.
pub const ACTIONS: [&str; 31] = [
    "ls_up", "ls_down", "ls_left", "ls_right", "rs_mod", "square", "triangle", "circle", "cross", "l1", "r1",
    "artemis", "r2", "l2", "rage", "poseidon", "medusa", "zeus", "hades", "up", "right", "down", "left", "start",
    "select", "rs_up", "rs_down", "rs_left", "rs_right", "l3", "r3",
];

pub type Bindings = BTreeMap<String, Vec<i64>>;

/// Default scheme: WASD moves, Ctrl + WASD dodges, 1-4 the named magic, left/right click attack, middle click draws
/// the Blade of Artemis (L1 + R1), arrows are the d-pad, Enter Start, Tab Select.
pub fn default_bindings() -> Bindings {
    let table: [(&str, &[i64]); 31] = [
        ("ls_up", &[87]),
        ("ls_down", &[83]),
        ("ls_left", &[65]),
        ("ls_right", &[68]),
        ("rs_mod", &[341]),
        ("square", &[MOUSE_BASE]),
        ("triangle", &[MOUSE_BASE + 1]),
        ("circle", &[70]),
        ("cross", &[32]),
        ("l1", &[81]),
        ("r1", &[69]),
        ("artemis", &[MOUSE_BASE + 2]),
        ("r2", &[341]),
        ("l2", &[340]),
        ("rage", &[86]),
        ("poseidon", &[49]),
        ("medusa", &[50]),
        ("zeus", &[51]),
        ("hades", &[52]),
        ("up", &[265]),
        ("right", &[262]),
        ("down", &[264]),
        ("left", &[263]),
        ("start", &[257]),
        ("select", &[258]),
        ("rs_up", &[73]),
        ("rs_down", &[75]),
        ("rs_left", &[74]),
        ("rs_right", &[76]),
        ("l3", &[]),
        ("r3", &[]),
    ];
    table.iter().map(|(id, codes)| (id.to_string(), codes.to_vec())).collect()
}

/// Saved map (JSON) with the missing actions taken from the default scheme. Anything unreadable gives the defaults.
pub fn read_bindings(text: &str) -> Bindings {
    let mut base = default_bindings();
    if text.trim().is_empty() {
        return base;
    }
    let Ok(serde_json::Value::Object(mut saved)) = serde_json::from_str::<serde_json::Value>(text) else {
        return base;
    };
    // Maps from before the per-magic actions: 1-4 were on the d-pad (1 up, 2 right, 3 down, 4 left). They now go to
    // the magic's default keys and are removed from the d-pad so it does not press two directions at once.
    if !saved.get("poseidon").is_some_and(|v| v.is_array()) {
        for dpad in ["up", "right", "down", "left"] {
            if let Some(serde_json::Value::Array(items)) = saved.get_mut(dpad) {
                items.retain(|c| !c.as_f64().is_some_and(|c| (49.0..=52.0).contains(&c)));
            }
        }
    }
    for id in ACTIONS {
        if let Some(serde_json::Value::Array(items)) = saved.get(id) {
            let codes: Vec<i64> =
                items.iter().take(2).filter_map(|c| c.as_f64()).filter(|c| *c > 0.0).map(|c| c as i64).collect();
            base.insert(id.to_string(), codes);
        }
    }
    base
}

/// `action=code,code;...`: the format of `PC_KEYS`.
pub fn serialize_bindings(map: &Bindings) -> String {
    ACTIONS
        .iter()
        .filter_map(|id| {
            let codes = map.get(*id)?;
            if codes.is_empty() {
                return None;
            }
            let joined = codes.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            Some(format!("{id}={joined}"))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Left and right Shift, Ctrl and Alt are the same key code in the game.
pub fn normal(code: i64) -> i64 {
    match code {
        344 => 340,
        345 => 341,
        346 => 342,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scheme_serializes_in_action_order() {
        let text = serialize_bindings(&default_bindings());
        assert!(text.starts_with("ls_up=87;ls_down=83;ls_left=65;ls_right=68;rs_mod=341;square=1000;triangle=1001;"));
        assert!(text.ends_with("rs_up=73;rs_down=75;rs_left=74;rs_right=76"));
        assert!(!text.contains("l3="));
    }

    #[test]
    fn empty_or_broken_input_gives_the_defaults() {
        assert_eq!(read_bindings(""), default_bindings());
        assert_eq!(read_bindings("not json"), default_bindings());
        assert_eq!(read_bindings("[1,2]"), default_bindings());
    }

    #[test]
    fn saved_actions_override_and_the_rest_stay_default() {
        let map = read_bindings(r#"{"cross":[90,1001,5],"l3":[0,88],"r3":[-4,0,88],"poseidon":[49]}"#);
        assert_eq!(map["cross"], vec![90, 1001]); // at most two
        assert_eq!(map["l3"], vec![88]); // non-positive codes dropped
        assert!(map["r3"].is_empty()); // the cut to two happens before the filter, as in the QML launcher
        assert_eq!(map["start"], vec![257]);
    }

    #[test]
    fn old_maps_lose_the_digits_on_the_dpad() {
        let map = read_bindings(r#"{"up":[265,49],"right":[262,50],"down":[264,51],"left":[263,52]}"#);
        assert_eq!(map["up"], vec![265]);
        assert_eq!(map["left"], vec![263]);
        assert_eq!(map["poseidon"], vec![49]); // default again
    }

    #[test]
    fn new_maps_keep_digits_on_the_dpad() {
        let map = read_bindings(r#"{"poseidon":[90],"up":[265,49]}"#);
        assert_eq!(map["up"], vec![265, 49]);
        assert_eq!(map["poseidon"], vec![90]);
    }

    #[test]
    fn sides_of_modifiers_are_the_same_key() {
        assert_eq!(normal(344), 340);
        assert_eq!(normal(345), 341);
        assert_eq!(normal(346), 342);
        assert_eq!(normal(65), 65);
    }
}
