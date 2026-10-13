//! Small app-owned additions alongside the existing embedded component icons.
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;
pub(crate) struct Assets;
const APP_ICONS: &[(&str, &[u8])] = &[
    (
        "icons/ai-sparkles.svg",
        include_bytes!("../../assets/icons/ai-sparkles.svg"),
    ),
    (
        "icons/server.svg",
        include_bytes!("../../assets/icons/server.svg"),
    ),
    (
        "icons/followup-add.svg",
        include_bytes!("../../assets/icons/followup-add.svg"),
    ),
];
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = APP_ICONS.iter().find(|(name, _)| *name == path) {
            Ok(Some(Cow::Borrowed(*bytes)))
        } else {
            gpui_component_assets::Assets.load(path)
        }
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut items = gpui_component_assets::Assets.list(path)?;
        for (name, _) in APP_ICONS.iter().filter(|(name, _)| name.starts_with(path)) {
            if !items.iter().any(|existing| existing.as_ref() == *name) {
                items.push((*name).into());
            }
        }
        Ok(items)
    }
}
