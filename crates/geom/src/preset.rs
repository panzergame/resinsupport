//! What every kind of preset's name must be (labels', and later supports', slicing's, …): the
//! names a user types for their own presets, and what a follower stores to name the one it follows.

/// A preset's name, in characters.
pub const MAX_PRESET_NAME: usize = 64;

/// Why `name` cannot name a preset, if it cannot: empty, longer than [`MAX_PRESET_NAME`], with
/// spaces round it or a control character.
pub fn check_preset_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("a preset's name must not be empty".into());
    }
    if name.chars().count() > MAX_PRESET_NAME {
        return Err(format!("a preset's name must be at most {MAX_PRESET_NAME} characters"));
    }
    if name.trim() != name {
        return Err("a preset's name must not start or end with spaces".into());
    }
    if name.chars().any(char::is_control) {
        return Err("a preset's name must have no control characters".into());
    }
    Ok(())
}

/// Whether `a` and `b` name the same preset: names are compared ignoring case.
pub fn same_preset_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked_and_compared_ignoring_case() {
        for bad in ["", " ", " A", "A ", "A\tB", &"x".repeat(MAX_PRESET_NAME + 1)] {
            assert!(check_preset_name(bad).is_err(), "{bad:?}");
        }
        assert!(check_preset_name(&"é".repeat(MAX_PRESET_NAME)).is_ok());
        assert!(check_preset_name("Light 0.05 mm").is_ok());
        assert!(same_preset_name("Calibration", "CALIBRATION") && !same_preset_name("Cal", "Cal "));
    }
}
