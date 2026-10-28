//! The app's own colors around the terminals: the `:root` tokens of
//! `src/styles.css`, from the same theme as the terminals. The builtin
//! theme keeps the app's colors exactly; devaslife has its own, picked by
//! hand; anything else is derived from its terminal colors.

use cmux_protocol::{ResolvedColors, UiColors};

/// Today's colors, exactly as `:root` in `src/styles.css` has them
/// (Catppuccin Mocha).
pub fn builtin_ui() -> UiColors {
    UiColors {
        bg: "#1e1e2e".into(),
        bg_alt: "#181825".into(),
        surface: "#313244".into(),
        text: "#cdd6f4".into(),
        subtext: "#a6adc8".into(),
        muted: "#6c7086".into(),
        accent: "#89b4fa".into(),
        overlay: "#45475a".into(),
        status_working: "#89b4fa".into(),
        status_needs_you: "#fab387".into(),
        status_idle: "#a6e3a1".into(),
        danger: "#f38ba8".into(),
        warn: "#f9e2af".into(),
        merged: "#cba6f7".into(),
        tint_needs_you_row: "#3a2f2c".into(),
        tint_needs_you_head: "#2c2630".into(),
        tint_needs_you_area: "#2a2430".into(),
        tint_needs_you_panel: "#211f2a".into(),
        tint_hover: "#262637".into(),
        tint_info: "#1e2433".into(),
        tint_warn: "#2e2c25".into(),
        tint_error_row: "#3b2a33".into(),
        on_needs_you: "#181825".into(),
        categorical: ["#89b4fa", "#a6e3a1", "#f9e2af", "#f38ba8", "#cba6f7", "#94e2d5", "#fab387", "#b4befe"]
            .map(String::from)
            .to_vec(),
    }
}

/// devaslife's colors: Solarized's bases and accents, on the darker
/// background its terminals have.
pub fn devaslife_ui() -> UiColors {
    let bg = "#001b21";
    let (orange, blue, yellow, red) = ("#cb4b16", "#268bd2", "#b58900", "#dc322f");
    let text = "#93a1a1"; // base1
    let t = tints(bg, text, orange, blue, yellow, red);
    UiColors {
        bg: bg.into(),
        bg_alt: "#00141a".into(),
        surface: "#073642".into(), // base02
        text: text.into(),
        subtext: "#839496".into(), // base0
        muted: "#586e75".into(),   // base01
        accent: blue.into(),
        overlay: "#0d4251".into(),
        status_working: blue.into(),
        status_needs_you: orange.into(),
        status_idle: "#859900".into(), // green
        danger: red.into(),
        warn: yellow.into(),
        merged: "#6c71c4".into(), // violet
        on_needs_you: ink(orange, "#00141a", text),
        // Solarized's eight accents, in the order of builtin's hues.
        categorical: ["#268bd2", "#859900", "#b58900", "#dc322f", "#d33682", "#2aa198", "#cb4b16", "#6c71c4"]
            .map(String::from)
            .to_vec(),
        ..t
    }
}

/// Catppuccin Latte's colors: the light counterpart of builtin, each token
/// the same Catppuccin role builtin takes from Mocha (base, mantle,
/// surface0, surface1, text, subtext0, overlay0, blue, peach, green, red,
/// yellow, mauve), from catppuccin/palette's palette.json.
pub fn catppuccin_latte_ui() -> UiColors {
    let (base, text, blue, peach, yellow, red) = ("#eff1f5", "#4c4f69", "#1e66f5", "#fe640b", "#df8e1d", "#d20f39");
    let mantle = "#e6e9ef";
    let t = tints(base, text, peach, blue, yellow, red);
    UiColors {
        bg: base.into(),
        bg_alt: mantle.into(),
        surface: "#ccd0da".into(), // surface0
        text: text.into(),
        subtext: "#6c6f85".into(), // subtext0
        muted: "#9ca0b0".into(),   // overlay0
        accent: blue.into(),
        overlay: "#bcc0cc".into(), // surface1
        status_working: blue.into(),
        status_needs_you: peach.into(),
        status_idle: "#40a02b".into(), // green
        danger: red.into(),
        warn: yellow.into(),
        merged: "#8839ef".into(), // mauve
        on_needs_you: ink(peach, mantle, text),
        // Builtin's order: blue, green, yellow, red, mauve, teal, peach,
        // lavender.
        categorical: ["#1e66f5", "#40a02b", "#df8e1d", "#d20f39", "#8839ef", "#179299", "#fe640b", "#7287fd"]
            .map(String::from)
            .to_vec(),
        ..t
    }
}

/// Tokyo Night's colors, the night variant, from folke/tokyonight.nvim's
/// palette (lua/tokyonight/colors/storm.lua, with night.lua's darker
/// backgrounds): its editor's own roles for each token.
pub fn tokyo_night_ui() -> UiColors {
    let (bg, fg, blue, orange, yellow, red) = ("#1a1b26", "#c0caf5", "#7aa2f7", "#ff9e64", "#e0af68", "#f7768e");
    let bg_dark = "#16161e";
    let t = tints(bg, fg, orange, blue, yellow, red);
    UiColors {
        bg: bg.into(),
        bg_alt: bg_dark.into(),
        surface: "#292e42".into(), // bg_highlight
        text: fg.into(),
        subtext: "#a9b1d6".into(), // fg_dark
        muted: "#565f89".into(),   // comment
        accent: blue.into(),
        overlay: "#3b4261".into(), // fg_gutter
        status_working: blue.into(),
        status_needs_you: orange.into(),
        status_idle: "#9ece6a".into(), // green
        danger: red.into(),
        warn: yellow.into(),
        merged: "#bb9af7".into(), // magenta
        on_needs_you: ink(orange, bg_dark, fg),
        // Builtin's order of hue: blue, green, yellow, red, magenta, cyan,
        // orange, purple.
        categorical: ["#7aa2f7", "#9ece6a", "#e0af68", "#f7768e", "#bb9af7", "#7dcfff", "#ff9e64", "#9d7cd8"]
            .map(String::from)
            .to_vec(),
        ..t
    }
}

/// The app's colors for any other theme, from its terminal colors: the
/// background and foreground and mixes of them for the surfaces and
/// text, the palette's blue, red, yellow, green and magenta for the
/// accents. A color that does not parse falls back to builtin's.
pub fn derive_ui(c: &ResolvedColors) -> UiColors {
    let builtin = builtin_ui();
    let (Some(_), Some(_)) = (rgb(&c.background), rgb(&c.foreground)) else {
        return builtin;
    };
    let bg = c.background.as_str();
    let fg = c.foreground.as_str();
    let ansi = |i: usize, fallback: &str| -> String {
        c.palette.get(i).filter(|p| rgb(p).is_some()).cloned().unwrap_or_else(|| fallback.to_string())
    };
    let (red, green, yellow, blue, magenta) = (
        ansi(1, &builtin.danger),
        ansi(2, &builtin.status_idle),
        ansi(3, &builtin.warn),
        ansi(4, &builtin.accent),
        ansi(5, &builtin.merged),
    );
    // Orange, between the two: told apart from danger's red and warn's yellow.
    let needs_you = mix(&red, &yellow, 0.5);
    // A light background's panel is a shade darker, as a dark one's is.
    let alt = if luminance(bg) > 0.5 { 0.04 } else { 0.2 };
    let bg_alt = mix(bg, "#000000", alt);
    let t = tints(bg, fg, &needs_you, &blue, &yellow, &red);
    // Blue, green, yellow, red, magenta, cyan, then bright red and bright
    // magenta: never the bright green, yellow, blue or cyan, which
    // Solarized-style palettes make greys.
    let categorical = [4, 2, 3, 1, 5, 6, 9, 13]
        .iter()
        .zip(&builtin.categorical)
        .map(|(&i, fallback)| ansi(i, fallback))
        .collect();
    UiColors {
        bg: bg.into(),
        on_needs_you: ink(&needs_you, &bg_alt, fg),
        bg_alt,
        surface: mix(bg, fg, 0.11),
        text: fg.into(),
        subtext: mix(fg, bg, 0.22),
        muted: mix(fg, bg, 0.55),
        accent: blue.clone(),
        overlay: mix(bg, fg, 0.22),
        status_working: blue,
        status_needs_you: needs_you,
        status_idle: green,
        danger: red,
        warn: yellow,
        merged: magenta,
        categorical,
        ..t
    }
}

/// The tints, each the background with a little of the color it marks.
/// The amounts follow builtin's hand-made tints.
fn tints(bg: &str, text: &str, needs_you: &str, info: &str, warn: &str, error: &str) -> UiColors {
    UiColors {
        tint_needs_you_row: mix(bg, needs_you, 0.13),
        tint_needs_you_head: mix(bg, needs_you, 0.07),
        tint_needs_you_area: mix(bg, needs_you, 0.06),
        tint_needs_you_panel: mix(bg, needs_you, 0.03),
        tint_hover: mix(bg, text, 0.04),
        tint_info: mix(bg, info, 0.05),
        tint_warn: mix(bg, warn, 0.07),
        tint_error_row: mix(bg, error, 0.12),
        ..builtin_ui()
    }
}

/// The ink for small text on `fill`: the panel colour, else the text, if
/// it reads at 4.5:1 (builtin's dark ink on its peach); for a fill too
/// mid-toned for either, whichever of the four reads best, black and
/// white included. Not simply the best of the four: black beats builtin's
/// own ink on peach, and builtin keeps the colours it has.
fn ink(fill: &str, bg_alt: &str, text: &str) -> String {
    if let Some(c) = [bg_alt, text].into_iter().find(|c| contrast(c, fill) >= 4.5) {
        return c.to_string();
    }
    [bg_alt, text, "#000000", "#ffffff"]
        .into_iter()
        .fold((String::new(), 0.0), |best, c| {
            let ratio = contrast(c, fill);
            if ratio > best.1 { (c.to_string(), ratio) } else { best }
        })
        .0
}

fn rgb(hex: &str) -> Option<[f64; 3]> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(f64::from);
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// `a` moved `t` of the way toward `b`; `a` itself when either does not
/// parse.
fn mix(a: &str, b: &str, t: f64) -> String {
    let (Some(x), Some(y)) = (rgb(a), rgb(b)) else {
        return a.to_string();
    };
    let c = |i: usize| (x[i] + (y[i] - x[i]) * t).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(0), c(1), c(2))
}

/// WCAG relative luminance, 0 for black to 1 for white.
fn luminance(hex: &str) -> f64 {
    let Some(c) = rgb(hex) else { return 0.0 };
    let lin = |v: f64| {
        let s = v / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG contrast ratio, 1 to 21.
fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(background: &str, foreground: &str, palette: [&str; 16]) -> ResolvedColors {
        ResolvedColors {
            background: background.into(),
            foreground: foreground.into(),
            cursor: foreground.into(),
            selection_background: background.into(),
            palette: palette.map(String::from).to_vec(),
            ui: builtin_ui(),
        }
    }

    // Dracula, and a light theme (Solarized light's palette on white).
    fn dracula() -> ResolvedColors {
        terminal("#282a36", "#f8f8f2", [
            "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
            "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
        ])
    }

    fn light() -> ResolvedColors {
        terminal("#ffffff", "#24292f", [
            "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
            "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
        ])
    }

    #[test]
    fn builtin_is_what_styles_css_has_on_root() {
        // Read at test time rather than hard-coded twice, so the two cannot
        // drift: a token changed in the stylesheet fails here.
        let css = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/styles.css"))
            .expect("src/styles.css");
        let root = css.split(":root {").nth(1).and_then(|r| r.split("\n}").next()).expect("a :root block");
        let mut tokens = serde_json::Map::new();
        let mut hues = Vec::new();
        for line in root.lines() {
            let Some((name, value)) = line.trim().strip_prefix("--").and_then(|l| l.split_once(':')) else {
                continue;
            };
            let value = value.trim().trim_end_matches(';');
            if let Some(i) = name.strip_prefix("hue-") {
                hues.push((i.parse::<usize>().unwrap(), value.to_string()));
                continue;
            }
            // bg-alt → bgAlt, as UiColors serializes its fields.
            let mut key = String::new();
            let mut upper = false;
            for ch in name.chars() {
                if ch == '-' {
                    upper = true;
                } else {
                    key.push(if upper { ch.to_ascii_uppercase() } else { ch });
                    upper = false;
                }
            }
            tokens.insert(key, value.into());
        }
        let mut ui = serde_json::to_value(builtin_ui()).unwrap();
        // The hues are --hue-0 to --hue-7, one per place in the list.
        ui.as_object_mut().unwrap().remove("categorical");
        assert_eq!(serde_json::Value::Object(tokens), ui);
        hues.sort();
        assert_eq!(hues.into_iter().map(|(_, v)| v).collect::<Vec<_>>(), builtin_ui().categorical);
    }

    #[test]
    fn eight_hues_always() {
        // Builtin's are the graph's lane colours from before themes.
        assert_eq!(
            builtin_ui().categorical,
            ["#89b4fa", "#a6e3a1", "#f9e2af", "#f38ba8", "#cba6f7", "#94e2d5", "#fab387", "#b4befe"]
        );
        for ui in [builtin_ui(), devaslife_ui(), derive_ui(&dracula()), derive_ui(&light())] {
            assert_eq!(ui.categorical.len(), 8);
        }
        // Slots 4, 2, 3, 1, 5, 6, 9, 13 of the palette; never a grey from
        // the bright green, yellow, blue or cyan (10, 11, 12, 14).
        assert_eq!(
            derive_ui(&light()).categorical,
            ["#268bd2", "#859900", "#b58900", "#dc322f", "#d33682", "#2aa198", "#cb4b16", "#6c71c4"]
        );
        // A palette too short falls back to builtin's hue in that place.
        let mut short = dracula();
        short.palette.truncate(8);
        assert_eq!(derive_ui(&short).categorical[7], builtin_ui().categorical[7]);
    }

    #[test]
    fn a_light_background_keeps_its_text_darker() {
        let ui = derive_ui(&light());
        assert_eq!(ui.bg, "#ffffff");
        assert!(luminance(&ui.text) < luminance(&ui.bg));
        assert!(luminance(&ui.muted) < luminance(&ui.bg));
        assert!(luminance(&ui.bg_alt) < luminance(&ui.bg), "the panel a shade darker");
        assert!(luminance(&ui.surface) < luminance(&ui.bg));
    }

    #[test]
    fn text_reads_on_its_background() {
        for (name, ui) in [
            ("builtin", builtin_ui()),
            ("devaslife", devaslife_ui()),
            ("dracula", derive_ui(&dracula())),
            ("light", derive_ui(&light())),
        ] {
            let ratio = contrast(&ui.text, &ui.bg);
            assert!(ratio >= 4.5, "{name}: text on bg is {ratio:.2}:1");
        }
    }

    #[test]
    fn ink_reads_on_the_needs_you_fill() {
        // Builtin keeps the dark ink it always had, and the rule agrees.
        let builtin = builtin_ui();
        assert_eq!(ink(&builtin.status_needs_you, &builtin.bg_alt, &builtin.text), "#181825");
        for (name, ui) in [("builtin", builtin), ("devaslife", devaslife_ui()), ("light", derive_ui(&light()))] {
            let ratio = contrast(&ui.on_needs_you, &ui.status_needs_you);
            assert!(ratio >= 4.5, "{name}: {} on {} is {ratio:.2}:1", ui.on_needs_you, ui.status_needs_you);
        }
        // Too mid-toned for the panel or the text: black or white then.
        assert_eq!(derive_ui(&light()).on_needs_you, "#000000");
        assert_eq!(devaslife_ui().on_needs_you, "#ffffff");
    }

    #[test]
    fn every_named_theme_reads() {
        // An entry added to the table is checked here without being named.
        for t in super::super::NAMED_THEMES {
            let ui = (t.ui)();
            let terminal = (t.terminal)();
            let name = t.name;
            // Its own colours apply only while the background is its own.
            assert_eq!(Some(&ui.bg), terminal.background.as_ref(), "{name}: bg is its terminal's");
            let text = contrast(&ui.text, &ui.bg);
            assert!(text >= 4.5, "{name}: text on bg is {text:.2}:1");
            let ink = contrast(&ui.on_needs_you, &ui.status_needs_you);
            assert!(ink >= 4.5, "{name}: ink on needs-you is {ink:.2}:1");
            assert_eq!(ui.categorical.len(), 8, "{name}: eight hues");
            let mut json = serde_json::to_value(&ui).unwrap();
            let hues = json.as_object_mut().unwrap().remove("categorical").unwrap();
            let terminal_colors = [&terminal.background, &terminal.foreground, &terminal.cursor, &terminal.selection_background]
                .into_iter()
                .chain(&terminal.palette)
                .map(|c| c.clone().expect("every terminal colour set"));
            for value in json.as_object().unwrap().values().chain(hues.as_array().unwrap()) {
                assert!(rgb(value.as_str().unwrap()).is_some(), "{name}: {value}");
            }
            for c in terminal_colors {
                assert!(rgb(&c).is_some(), "{name}: terminal {c}");
            }
        }
    }

    #[test]
    fn latte_is_catppuccins_own() {
        // palette.json's latte base, text and blue: a typo shows here.
        let ui = catppuccin_latte_ui();
        assert_eq!((ui.bg.as_str(), ui.text.as_str(), ui.accent.as_str()), ("#eff1f5", "#4c4f69", "#1e66f5"));
        // On a light background a tint is the background darkened toward
        // its colour, never lighter than it.
        for (name, tint) in [
            ("needs-you row", &ui.tint_needs_you_row),
            ("needs-you head", &ui.tint_needs_you_head),
            ("needs-you area", &ui.tint_needs_you_area),
            ("needs-you panel", &ui.tint_needs_you_panel),
            ("hover", &ui.tint_hover),
            ("info", &ui.tint_info),
            ("warn", &ui.tint_warn),
            ("error row", &ui.tint_error_row),
        ] {
            assert!(luminance(tint) < luminance(&ui.bg), "latte {name}: {tint} is not darker than {}", ui.bg);
        }
    }

    #[test]
    fn a_derived_ui_takes_its_accents_from_the_palette() {
        let ui = derive_ui(&dracula());
        assert_eq!((ui.bg.as_str(), ui.text.as_str()), ("#282a36", "#f8f8f2"));
        assert_eq!((ui.danger.as_str(), ui.status_idle.as_str(), ui.warn.as_str()), ("#ff5555", "#50fa7b", "#f1fa8c"));
        assert_eq!((ui.accent.as_str(), ui.status_working.as_str(), ui.merged.as_str()), ("#bd93f9", "#bd93f9", "#ff79c6"));
        // Needs-you between red and yellow, so it is neither.
        assert_ne!(ui.status_needs_you, ui.danger);
        assert_ne!(ui.status_needs_you, ui.warn);
        // Every color is #rrggbb.
        let mut json = serde_json::to_value(&ui).unwrap();
        let hues = json.as_object_mut().unwrap().remove("categorical").unwrap();
        for value in json.as_object().unwrap().values().chain(hues.as_array().unwrap()) {
            assert!(rgb(value.as_str().unwrap()).is_some(), "{value}");
        }
    }

    #[test]
    fn colors_that_do_not_parse_fall_back_to_builtin() {
        let mut c = dracula();
        c.background = "red".into();
        assert_eq!(derive_ui(&c), builtin_ui());
        let mut c = dracula();
        c.palette[1] = "nope".into();
        assert_eq!(derive_ui(&c).danger, builtin_ui().danger);
    }

    #[test]
    fn mixing() {
        assert_eq!(mix("#000000", "#ffffff", 0.5), "#808080");
        assert_eq!(mix("#102030", "#102030", 0.7), "#102030");
        assert_eq!(mix("#000000", "#ffffff", 0.0), "#000000");
        assert_eq!(mix("bad", "#ffffff", 0.5), "bad");
        assert!((luminance("#ffffff") - 1.0).abs() < 1e-9);
        assert_eq!(luminance("#000000"), 0.0);
    }
}
