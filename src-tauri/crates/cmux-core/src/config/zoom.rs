//! Zoom, a step at a time: the terminals' font size (`fontSize`). The
//! steps are worked out here, from the size the app is showing, and
//! written back to mirador.json like any other setting, so a zoom outlives
//! a restart and shows in the Settings pane.

use std::path::Path;

use serde_json::{json, Value};

use super::settings::set_config_value_in;

/// The terminal font sizes a file, the Settings pane or a zoom can set, in
/// points. Outside them a terminal is unreadable or a few cells wide.
pub const FONT_SIZE_MIN: f32 = 6.0;
pub const FONT_SIZE_MAX: f32 = 72.0;

/// `current` moved `steps` whole points, up for a positive count: a size
/// between two whole points goes to the nearer one in that direction
/// first, so 12.5 is 13 a step up and 12 a step down. Kept to
/// [`FONT_SIZE_MIN`]..=[`FONT_SIZE_MAX`].
///
/// One sum, not a step at a time: `steps` comes from the webview, and a
/// loop over `i32::MIN` steps held the main thread for seconds.
pub fn step_font_size(current: f32, steps: i32) -> f32 {
    let size = current.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    let stepped = match steps {
        0 => size,
        s if s > 0 => size.floor() + s as f32,
        s => size.ceil() + s as f32,
    };
    stepped.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX)
}

/// A number as mirador.json should hold it: `14`, not `14.0`.
fn tidy(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e9 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

/// Zooms the terminals from `current` points: `steps` whole points, or
/// with 0 back to the default, which removes `fontSize` from the file at
/// `path` (the theme's size or 13 again). The new size when one is set.
pub(super) fn zoom_terminals_in(path: &Path, current: f32, steps: i32) -> Result<Option<f32>, String> {
    if steps == 0 {
        set_config_value_in(path, "fontSize", None)?;
        return Ok(None);
    }
    let size = step_font_size(current, steps);
    set_config_value_in(path, "fontSize", Some(tidy(f64::from(size))))?;
    Ok(Some(size))
}

/// [`zoom_terminals_in`] on the user's mirador.json.
pub fn zoom_terminals(current: f32, steps: i32) -> Result<Option<f32>, String> {
    zoom_terminals_in(&super::config_path(), current, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_size_steps_by_whole_points_within_6_to_72() {
        assert_eq!(step_font_size(13.0, 1), 14.0);
        assert_eq!(step_font_size(13.0, -1), 12.0);
        assert_eq!(step_font_size(13.0, 3), 16.0);
        assert_eq!(step_font_size(13.0, -3), 10.0);
        // Between two whole points: the nearer one in that direction first.
        assert_eq!(step_font_size(12.5, 1), 13.0);
        assert_eq!(step_font_size(12.5, -1), 12.0);
        assert_eq!(step_font_size(12.5, 2), 14.0);
        assert_eq!(step_font_size(12.5, -2), 11.0);
        // The ends hold, and a size outside them comes back in.
        assert_eq!(step_font_size(72.0, 1), 72.0);
        assert_eq!(step_font_size(6.0, -1), 6.0);
        assert_eq!(step_font_size(100.0, -1), 71.0);
        assert_eq!(step_font_size(13.0, 0), 13.0);
    }

    /// The count comes from the webview: any `i32` is one sum, not a loop
    /// on the main thread, and lands on an end.
    #[test]
    fn any_step_count_lands_within_the_ends_at_once() {
        assert_eq!(step_font_size(13.0, i32::MAX), FONT_SIZE_MAX);
        assert_eq!(step_font_size(13.0, i32::MIN), FONT_SIZE_MIN);
        assert_eq!(step_font_size(12.5, i32::MAX), FONT_SIZE_MAX);
        assert_eq!(step_font_size(12.5, i32::MIN), FONT_SIZE_MIN);
        assert_eq!(step_font_size(FONT_SIZE_MAX, i32::MIN + 1), FONT_SIZE_MIN);
        assert_eq!(step_font_size(FONT_SIZE_MIN, i32::MAX - 1), FONT_SIZE_MAX);
    }

    fn temp_config(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-zoom-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("mirador").join("mirador.json")
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn zooming_writes_whole_numbers_keeps_other_keys_and_resets_by_removing() {
        let path = temp_config("write");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{ "shell": "pwsh", "fontSize": 12.5 }"#).unwrap();

        assert_eq!(zoom_terminals_in(&path, 12.5, 2).unwrap(), Some(14.0));
        assert_eq!(read(&path), json!({ "shell": "pwsh", "fontSize": 14 }));
        assert_eq!(zoom_terminals_in(&path, 14.0, i32::MAX).unwrap(), Some(72.0));
        assert_eq!(read(&path)["fontSize"], json!(72));
        assert_eq!(zoom_terminals_in(&path, 72.0, i32::MIN).unwrap(), Some(6.0));
        assert_eq!(read(&path)["fontSize"], json!(6));

        // 0 is the reset: the key goes, so the default (or the theme's
        // font size) is back.
        assert_eq!(zoom_terminals_in(&path, 6.0, 0).unwrap(), None);
        assert_eq!(read(&path), json!({ "shell": "pwsh" }));
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_file_that_does_not_parse_is_not_zoomed_over() {
        let path = temp_config("broken");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ broken").unwrap();
        assert!(zoom_terminals_in(&path, 13.0, 1).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
