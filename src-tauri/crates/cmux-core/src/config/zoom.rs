//! Zoom, a step at a time: the terminals' font size (`fontSize`) and the
//! whole window's scale (`uiZoom`). The steps are worked out here, from
//! the value the app is showing, and written back to mirador.json like
//! any other setting, so a zoom outlives a restart and shows in the
//! Settings pane.

use std::path::Path;

use serde_json::{json, Value};

use super::settings::set_config_value_in;

/// The terminal font sizes a file, the Settings pane or a zoom can set, in
/// points. Outside them a terminal is unreadable or a few cells wide.
pub const FONT_SIZE_MIN: f32 = 6.0;
pub const FONT_SIZE_MAX: f32 = 72.0;

/// The window's zoom levels, a browser's: what zooming steps through. A
/// file may hold any value between the ends; a step goes to the next
/// level from wherever it is.
pub const UI_ZOOM_LEVELS: [f64; 13] = [0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// The window's zoom with nothing set: 100 %.
pub const UI_ZOOM_DEFAULT: f64 = 1.0;

/// The ends of [`UI_ZOOM_LEVELS`], which is what `uiZoom` takes.
pub fn ui_zoom_range() -> (f64, f64) {
    (UI_ZOOM_LEVELS[0], UI_ZOOM_LEVELS[UI_ZOOM_LEVELS.len() - 1])
}

/// `current` moved `steps` whole points, up for a positive count: a size
/// between two whole points goes to the nearer one in that direction
/// first, so 12.5 is 13 a step up and 12 a step down. Kept to
/// [`FONT_SIZE_MIN`]..=[`FONT_SIZE_MAX`].
pub fn step_font_size(current: f32, steps: i32) -> f32 {
    let mut size = current.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    for _ in 0..steps.unsigned_abs() {
        size = if steps > 0 { size.floor() + 1.0 } else { size.ceil() - 1.0 };
    }
    size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX)
}

/// `current` moved `steps` levels of [`UI_ZOOM_LEVELS`], up for a positive
/// count, stopping at either end. A value between two levels goes to the
/// next one in that direction, as a browser's zoom does.
pub fn step_ui_zoom(current: f64, steps: i32) -> f64 {
    let (lowest, highest) = ui_zoom_range();
    let mut zoom = current.clamp(lowest, highest);
    for _ in 0..steps.unsigned_abs() {
        zoom = if steps > 0 {
            UI_ZOOM_LEVELS.iter().copied().find(|l| *l > zoom + 1e-6).unwrap_or(highest)
        } else {
            UI_ZOOM_LEVELS.iter().rev().copied().find(|l| *l < zoom - 1e-6).unwrap_or(lowest)
        };
    }
    zoom
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

/// Zooms the whole window from `current`: `steps` levels, or with 0 back
/// to 100 %, which removes `uiZoom` from the file at `path`. The new zoom.
pub(super) fn zoom_window_in(path: &Path, current: f64, steps: i32) -> Result<f64, String> {
    if steps == 0 {
        set_config_value_in(path, "uiZoom", None)?;
        return Ok(UI_ZOOM_DEFAULT);
    }
    let zoom = step_ui_zoom(current, steps);
    set_config_value_in(path, "uiZoom", Some(tidy(zoom)))?;
    Ok(zoom)
}

/// [`zoom_terminals_in`] on the user's mirador.json.
pub fn zoom_terminals(current: f32, steps: i32) -> Result<Option<f32>, String> {
    zoom_terminals_in(&super::config_path(), current, steps)
}

/// [`zoom_window_in`] on the user's mirador.json.
pub fn zoom_window(current: f64, steps: i32) -> Result<f64, String> {
    zoom_window_in(&super::config_path(), current, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_size_steps_by_whole_points_within_6_to_72() {
        assert_eq!(step_font_size(13.0, 1), 14.0);
        assert_eq!(step_font_size(13.0, -1), 12.0);
        assert_eq!(step_font_size(13.0, 3), 16.0);
        // Between two whole points: the nearer one in that direction first.
        assert_eq!(step_font_size(12.5, 1), 13.0);
        assert_eq!(step_font_size(12.5, -1), 12.0);
        // The ends hold, and a size outside them comes back in.
        assert_eq!(step_font_size(72.0, 1), 72.0);
        assert_eq!(step_font_size(6.0, -1), 6.0);
        assert_eq!(step_font_size(100.0, -1), 71.0);
        assert_eq!(step_font_size(13.0, 0), 13.0);
    }

    #[test]
    fn the_window_zoom_steps_through_the_levels_and_stops_at_the_ends() {
        assert_eq!(step_ui_zoom(1.0, 1), 1.1);
        assert_eq!(step_ui_zoom(1.0, -1), 0.9);
        assert_eq!(step_ui_zoom(1.0, 4), 1.75);
        // Between two levels: the next one in that direction.
        assert_eq!(step_ui_zoom(1.2, 1), 1.25);
        assert_eq!(step_ui_zoom(1.2, -1), 1.1);
        assert_eq!(step_ui_zoom(3.0, 1), 3.0);
        assert_eq!(step_ui_zoom(0.5, -1), 0.5);
        assert_eq!(step_ui_zoom(9.0, -1), 2.5);
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
        assert_eq!(zoom_window_in(&path, 1.0, 1).unwrap(), 1.1);
        assert_eq!(read(&path), json!({ "shell": "pwsh", "fontSize": 14, "uiZoom": 1.1 }));
        assert_eq!(zoom_window_in(&path, 1.1, 8).unwrap(), 3.0);
        assert_eq!(read(&path)["uiZoom"], json!(3));

        // 0 is the reset: the key goes, so the default (or the theme's
        // font size) is back.
        assert_eq!(zoom_terminals_in(&path, 14.0, 0).unwrap(), None);
        assert_eq!(zoom_window_in(&path, 3.0, 0).unwrap(), 1.0);
        assert_eq!(read(&path), json!({ "shell": "pwsh" }));
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_file_that_does_not_parse_is_not_zoomed_over() {
        let path = temp_config("broken");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ broken").unwrap();
        assert!(zoom_terminals_in(&path, 13.0, 1).is_err());
        assert!(zoom_window_in(&path, 1.0, 1).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
