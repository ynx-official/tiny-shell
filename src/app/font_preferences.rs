use gpui::{FontFallbacks, SharedString};

pub(crate) use crate::session::config_file::SYSTEM_MONO_FONT;

pub(crate) const SYSTEM_UI_FONT: &str = ".SystemUIFont";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedFontSelection {
    pub(crate) family: SharedString,
    pub(crate) preference_available: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalFontProfile {
    pub(crate) selection: ResolvedFontSelection,
    pub(crate) fallbacks: Option<FontFallbacks>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FontPlatform {
    Windows,
    MacOs,
    Linux,
}

impl FontPlatform {
    fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }

    fn terminal_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Windows => &["Consolas", "Cascadia Mono", "Cascadia Code", "Courier New"],
            Self::MacOs => &["Menlo", "Monaco", "Courier New"],
            Self::Linux => &[
                "DejaVu Sans Mono",
                "Noto Sans Mono",
                "Liberation Mono",
                "Ubuntu Mono",
            ],
        }
    }

    fn fallback_candidates(self) -> &'static [&'static str] {
        match self {
            Self::Windows => &[
                "Microsoft YaHei UI",
                "Microsoft YaHei",
                "SimSun",
                "Segoe UI Emoji",
                "Segoe UI Symbol",
            ],
            Self::MacOs => &[
                "PingFang SC",
                "Hiragino Sans GB",
                "Apple Color Emoji",
                "Apple Symbols",
            ],
            Self::Linux => &[
                "Noto Sans Mono CJK SC",
                "Noto Sans CJK SC",
                "WenQuanYi Micro Hei",
                "Noto Color Emoji",
                "Symbola",
            ],
        }
    }
}

pub(crate) fn resolve_ui_font(
    preference: &str,
    available_fonts: &[String],
) -> ResolvedFontSelection {
    if preference.trim().is_empty() || preference.eq_ignore_ascii_case(SYSTEM_UI_FONT) {
        return ResolvedFontSelection {
            family: SYSTEM_UI_FONT.into(),
            preference_available: true,
        };
    }

    match canonical_font_name(preference, available_fonts) {
        Some(family) => ResolvedFontSelection {
            family: family.into(),
            preference_available: true,
        },
        None => ResolvedFontSelection {
            family: SYSTEM_UI_FONT.into(),
            preference_available: false,
        },
    }
}

pub(crate) fn resolve_terminal_font(
    preference: &str,
    available_fonts: &[String],
) -> TerminalFontProfile {
    resolve_terminal_font_for(FontPlatform::current(), preference, available_fonts)
}

pub(crate) fn system_mono_family(available_fonts: &[String]) -> SharedString {
    system_mono_family_for(FontPlatform::current(), available_fonts).into()
}

fn system_mono_family_for(platform: FontPlatform, available_fonts: &[String]) -> &str {
    platform
        .terminal_candidates()
        .iter()
        .find_map(|candidate| canonical_font_name(candidate, available_fonts))
        .unwrap_or(SYSTEM_UI_FONT)
}

fn resolve_terminal_font_for(
    platform: FontPlatform,
    preference: &str,
    available_fonts: &[String],
) -> TerminalFontProfile {
    let use_system_default =
        preference.trim().is_empty() || preference.eq_ignore_ascii_case(SYSTEM_MONO_FONT);
    let configured_family = (!use_system_default)
        .then(|| canonical_font_name(preference, available_fonts))
        .flatten();
    let preference_available = use_system_default || configured_family.is_some();
    let family =
        configured_family.unwrap_or_else(|| system_mono_family_for(platform, available_fonts));

    let mut fallback_names = Vec::new();
    for candidate in platform.fallback_candidates() {
        let Some(canonical) = canonical_font_name(candidate, available_fonts) else {
            continue;
        };
        if canonical.eq_ignore_ascii_case(family)
            || fallback_names
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(canonical))
        {
            continue;
        }
        fallback_names.push(canonical.to_string());
    }

    TerminalFontProfile {
        selection: ResolvedFontSelection {
            family: family.into(),
            preference_available,
        },
        fallbacks: (!fallback_names.is_empty()).then(|| FontFallbacks::from_fonts(fallback_names)),
    }
}

fn canonical_font_name<'a>(requested: &str, available_fonts: &'a [String]) -> Option<&'a str> {
    let requested = requested.trim();
    available_fonts
        .iter()
        .find(|font| font.eq_ignore_ascii_case(requested))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn system_mono_uses_platform_first_choice() {
        let windows = resolve_terminal_font_for(
            FontPlatform::Windows,
            SYSTEM_MONO_FONT,
            &fonts(&["Cascadia Mono", "Consolas"]),
        );
        let macos = resolve_terminal_font_for(
            FontPlatform::MacOs,
            SYSTEM_MONO_FONT,
            &fonts(&["Monaco", "Menlo"]),
        );
        let linux = resolve_terminal_font_for(
            FontPlatform::Linux,
            SYSTEM_MONO_FONT,
            &fonts(&["Noto Sans Mono", "DejaVu Sans Mono"]),
        );

        assert_eq!(windows.selection.family.as_ref(), "Consolas");
        assert_eq!(macos.selection.family.as_ref(), "Menlo");
        assert_eq!(linux.selection.family.as_ref(), "DejaVu Sans Mono");
    }

    #[test]
    fn empty_terminal_preference_uses_system_mono() {
        let profile = resolve_terminal_font_for(FontPlatform::Windows, "", &fonts(&["Consolas"]));

        assert_eq!(profile.selection.family.as_ref(), "Consolas");
        assert!(profile.selection.preference_available);
    }

    #[test]
    fn custom_font_match_is_case_insensitive_and_canonical() {
        let profile = resolve_terminal_font_for(
            FontPlatform::Windows,
            "cAsCaDiA mOnO",
            &fonts(&["Cascadia Mono", "Consolas"]),
        );

        assert_eq!(profile.selection.family.as_ref(), "Cascadia Mono");
        assert!(profile.selection.preference_available);
    }

    #[test]
    fn missing_legacy_maple_is_preserved_as_unavailable_and_falls_back() {
        let profile = resolve_terminal_font_for(
            FontPlatform::Windows,
            "Maple Mono NF CN",
            &fonts(&["Consolas"]),
        );

        assert_eq!(profile.selection.family.as_ref(), "Consolas");
        assert!(!profile.selection.preference_available);
    }

    #[test]
    fn installed_legacy_maple_remains_selected() {
        let profile = resolve_terminal_font_for(
            FontPlatform::MacOs,
            "Maple Mono NF CN",
            &fonts(&["Menlo", "Maple Mono NF CN"]),
        );

        assert_eq!(profile.selection.family.as_ref(), "Maple Mono NF CN");
        assert!(profile.selection.preference_available);
    }

    #[test]
    fn terminal_fallbacks_only_include_installed_unique_fonts() {
        let profile = resolve_terminal_font_for(
            FontPlatform::Windows,
            SYSTEM_MONO_FONT,
            &fonts(&[
                "Consolas",
                "Microsoft YaHei UI",
                "microsoft yahei ui",
                "Segoe UI Emoji",
            ]),
        );
        let fallbacks = profile.fallbacks.expect("installed fallbacks");

        assert_eq!(
            fallbacks.fallback_list(),
            &["Microsoft YaHei UI", "Segoe UI Emoji"]
        );
    }

    #[test]
    fn missing_platform_candidates_fall_back_to_system_ui() {
        let profile = resolve_terminal_font_for(
            FontPlatform::Linux,
            SYSTEM_MONO_FONT,
            &fonts(&[SYSTEM_UI_FONT]),
        );

        assert_eq!(profile.selection.family.as_ref(), SYSTEM_UI_FONT);
        assert!(profile.fallbacks.is_none());
    }

    #[test]
    fn missing_ui_font_uses_system_ui_without_overwriting_preference() {
        let selection = resolve_ui_font("Maple Mono NF CN", &fonts(&[SYSTEM_UI_FONT]));

        assert_eq!(selection.family.as_ref(), SYSTEM_UI_FONT);
        assert!(!selection.preference_available);
    }
}
