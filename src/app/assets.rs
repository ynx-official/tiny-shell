//! Small app-owned additions alongside the existing embedded component icons.
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;
pub(crate) struct Assets;
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if path == "icons/ai-sparkles.svg" {
            Ok(Some(Cow::Borrowed(include_bytes!(
                "../../assets/icons/ai-sparkles.svg"
            ))))
        } else {
            gpui_component_assets::Assets.load(path)
        }
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        let mut items = gpui_component_assets::Assets.list(path)?;
        if "icons/ai-sparkles.svg".starts_with(path) {
            items.push("icons/ai-sparkles.svg".into());
        }
        Ok(items)
    }
}
