//! Appearance tokens for Rencord.
//!
//! Colors, type, radii, spacing, and shadows live here. The desktop shell
//! reads the resolved [`Theme`] and does not embed hex colors of its own.
//! Custom themes are free: nothing in this crate checks for a subscription.

mod appearance;
mod color;
mod derive;
mod file;
mod image_fx;
mod preset;
mod theme;

pub use appearance::{read_appearance, write_appearance, AppearanceFile};
pub use color::{best_ink, contrast_ratio, ensure_contrast, Color, Oklch};
pub use derive::derive_theme;
pub use file::{
    dir_stamp, load_theme_dir, theme_from_code, theme_from_file, theme_from_json, theme_from_toml,
    validate_file, ThemeError, ThemeFile, THEME_CODE_PREFIX,
};
pub use image_fx::{gradient_png, grain_png, prepare_image};
pub use preset::{
    build_preset, guild_tile, map_discord_appearance, role_color, GuildTile, PresetId,
};
pub use theme::{
    contrast_warnings, ease_out_cubic, warnings_text, Background, BackgroundKind, Chrome,
    ColorMode, Colors, ContrastWarning, FontFace, Metrics, Theme,
};

/// Swatches the appearance editor can offer without embedding hex in UI code.
pub const EDITOR_SWATCHES: [u32; 12] = [
    0x000000, 0x1e1f22, 0x313338, 0x5865f2, 0x4f6bff, 0x0e5e6b, 0xf7f8fb, 0xffffff, 0xc2410c,
    0x14804a, 0x88c0d0, 0x1a1040,
];

pub const SCHEMA: &str = include_str!("../../../docs/themes/theme.schema.json");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onyx_ink_keeps_its_ink() {
        let theme = Theme::preset(PresetId::OnyxInk);
        assert_eq!(theme.colors.bg_base.to_u32(), 0x000000);
        assert_eq!(theme.colors.bg_chat.to_u32(), 0x000000);
        assert_eq!(theme.colors.accent.to_u32(), 0x4f6bff);
        assert_eq!(theme.colors.accent_fg.to_u32(), 0x000000);
        assert_eq!(theme.colors.text_link.to_u32(), 0xa8b8ff);
        assert_eq!(theme.colors.border_hairline.to_u32(), 0x15152a);
        assert!(theme.metrics.ink_chrome);
        assert_eq!(theme.metrics.font_ui.family(), "Inter");
        assert_eq!(theme.metrics.font_mono.family(), "JetBrains Mono");
        assert!(theme.metrics.gap < 0.5);
        assert!(theme.metrics.radius_panel < 0.5);
    }

    #[test]
    fn crisp_keeps_the_previous_dark_canvas() {
        assert_eq!(
            Theme::preset(PresetId::Crisp).colors.bg_base.to_u32(),
            0x16181d
        );
        assert_eq!(
            Theme::preset(PresetId::OnyxInk).preset.unwrap().cycle(),
            PresetId::Dark
        );
    }

    #[test]
    fn every_preset_meets_wcag_aa() {
        let mut failures = Vec::new();
        for id in PresetId::ALL {
            let theme = Theme::preset(id);
            let warnings = contrast_warnings(&theme);
            if !warnings.is_empty() {
                failures.push(format!("{}: {}", id.label(), warnings_text(&theme)));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn derived_palette_is_accessible() {
        let theme = derive_theme(
            "Lab",
            Color::rgb(0x12_14_1C),
            Color::rgb(0x7C_8C_FF),
            None,
            Chrome::Classic,
        );
        assert!(
            contrast_warnings(&theme).is_empty(),
            "{}",
            warnings_text(&theme)
        );
        let light = derive_theme(
            "Paper",
            Color::rgb(0xF4_F1_EA),
            Color::rgb(0x0E_5E_6B),
            Some(ColorMode::Light),
            Chrome::Classic,
        );
        assert!(light.mode == ColorMode::Light);
        assert!(
            contrast_warnings(&light).is_empty(),
            "{}",
            warnings_text(&light)
        );
    }

    #[test]
    fn files_and_theme_codes_roundtrip() {
        let theme = Theme::preset(PresetId::MidnightBlurple);
        let toml = theme.to_toml().unwrap();
        let json = theme.to_json().unwrap();
        let from_toml = theme_from_toml(&toml).unwrap();
        let from_json = theme_from_json(&json).unwrap();
        assert_eq!(
            from_toml.colors.accent.to_u32(),
            theme.colors.accent.to_u32()
        );
        assert_eq!(from_json.name, theme.name);
        assert!((from_toml.background.grain - theme.background.grain).abs() < 0.0001);
        let code = theme.to_code().unwrap();
        assert!(code.starts_with(THEME_CODE_PREFIX));
        let decoded = theme_from_code(&code).unwrap();
        assert_eq!(
            decoded.colors.bg_chat.to_u32(),
            theme.colors.bg_chat.to_u32()
        );
        assert!(matches!(
            decoded.background.kind,
            BackgroundKind::Gradient { .. }
        ));
    }

    #[test]
    fn schema_lists_every_color_token() {
        for key in Colors::keys() {
            assert!(
                SCHEMA.contains(&format!("\"{key}\"")),
                "schema missing {key}"
            );
        }
        let bad = ThemeFile {
            version: 1,
            name: "Nope".into(),
            id: "nope".into(),
            mode: "dark".into(),
            chrome: "classic".into(),
            derive: false,
            base: None,
            accent: None,
            colors: [("not.a.token".into(), "#ffffff".into())].into(),
            metrics: Default::default(),
            background: Default::default(),
        };
        assert!(validate_file(&bad).is_err());
        assert!(theme_from_toml("version = 1\nname = \"\"\nid = \"x\"\n").is_err());
    }

    #[test]
    fn discord_mapping_is_read_only_and_named() {
        assert_eq!(
            map_discord_appearance(Some("dark"), Some("onyx")),
            Some(PresetId::Onyx)
        );
        assert_eq!(
            map_discord_appearance(Some("light"), None),
            Some(PresetId::Light)
        );
        assert_eq!(
            map_discord_appearance(Some("dark"), Some("amoled")),
            Some(PresetId::Amoled)
        );
        assert_eq!(map_discord_appearance(None, None), None);
    }

    #[test]
    fn high_contrast_and_colorblind_and_desaturate_hold() {
        let theme = Theme::preset(PresetId::Dark);
        let boosted = theme.clone().high_contrast();
        assert!(
            contrast_ratio(boosted.colors.text_muted, boosted.colors.bg_chat)
                >= contrast_ratio(theme.colors.text_muted, theme.colors.bg_chat) - 0.05
        );
        let safe = theme.clone().colorblind_status();
        assert_ne!(
            safe.colors.status_online.to_u32(),
            theme.colors.status_online.to_u32()
        );
        assert!(contrast_warnings(&safe)
            .iter()
            .all(|w| !w.pair.contains("online") || w.ratio >= 3.0));
        let flat = theme.desaturate(0.4);
        assert!(
            contrast_warnings(&flat).is_empty(),
            "{}",
            warnings_text(&flat)
        );
    }

    #[test]
    fn lerp_keeps_endpoints() {
        let a = Theme::preset(PresetId::Light);
        let b = Theme::preset(PresetId::OnyxInk);
        let start = a.lerp(&b, 0.0);
        let end = a.lerp(&b, 1.0);
        assert_eq!(start.colors.bg_chat.to_u32(), a.colors.bg_chat.to_u32());
        assert_eq!(end.colors.accent.to_u32(), b.colors.accent.to_u32());
        assert!((ease_out_cubic(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn role_and_guild_tiles_stay_readable() {
        for id in [PresetId::OnyxInk, PresetId::Light, PresetId::Dark] {
            let theme = Theme::preset(id);
            for name in ["Ada", "Charles", "Lovelace", "Grace"] {
                let color = role_color(name, &theme);
                assert!(
                    contrast_ratio(color, theme.colors.bg_chat) >= 4.5,
                    "{id:?} {name}"
                );
            }
            let tile = guild_tile("Analytical Engines", true, &theme);
            assert!(contrast_ratio(Color::rgb(tile.letter), Color::rgb(tile.fill)) >= 4.5);
        }
    }

    #[test]
    fn write_theme_examples_when_asked() {
        if std::env::var("RENCORD_WRITE_THEMES").is_err() {
            return;
        }
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/themes/examples");
        std::fs::create_dir_all(&dir).unwrap();
        let ink = Theme::preset(PresetId::OnyxInk);
        std::fs::write(dir.join("onyx-ink.toml"), ink.to_toml().unwrap()).unwrap();
        let midnight = Theme::preset(PresetId::MidnightBlurple);
        std::fs::write(
            dir.join("midnight-blurple.json"),
            midnight.to_json().unwrap(),
        )
        .unwrap();
        let custom = derive_theme(
            "Harbor",
            Color::rgb(0x10243A),
            Color::rgb(0x7EB6FF),
            Some(ColorMode::Dark),
            Chrome::Classic,
        );
        let mut custom = custom;
        custom.background.kind = BackgroundKind::Gradient {
            angle: 148.0,
            stops: vec![
                Color::rgb(0x10243A),
                Color::rgb(0x1A1048),
                Color::rgb(0x12324A),
            ],
        };
        custom.background.grain = 0.02;
        std::fs::write(dir.join("harbor.toml"), custom.to_toml().unwrap()).unwrap();
    }

    #[test]
    fn gradient_png_and_dim_roundtrip() {
        let png = gradient_png(&[Color::rgb(0x000000), Color::rgb(0xFFFFFF)], 0.0, 8);
        assert!(png.starts_with(b"\x89PNG"));
        let dimmed = prepare_image(&png, 0.0, 1.0, 8).unwrap();
        let image = image::load_from_memory(&dimmed).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(0, 0)[0], 0);
    }
}
