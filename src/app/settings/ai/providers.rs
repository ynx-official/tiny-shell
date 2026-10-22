//! Provider editing is independent from the single global provider/model selection.
use super::{PermissionsAction, error_text, field, note, service};
use crate::{
    ai::{Model, ModelSelection, Provider, Settings, Snapshot},
    app::confirmation_dialog::ConfirmationDialog,
};
use gpui::{
    AppContext as _, Context, Entity, IntoElement as _, ParentElement as _, Render, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _, Size, StyleSized as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    v_flex,
};
use rust_i18n::t;
mod default_picker;
mod discovery;
mod selectors;
#[cfg(test)]
mod tests;
mod view;
use discovery::{DiscoveredModel, DiscoveredModels};
use selectors::{ModelChoice, ProviderChoice};

enum Pending {
    Save,
    Delete,
}

pub(super) struct ProviderForm {
    permissions: PermissionsAction,
    draft: Provider,
    baseline: Provider,
    observed_settings: Settings,
    selection: Option<ModelSelection>,
    name: Entity<InputState>,
    endpoint: Entity<InputState>,
    key: Entity<InputState>,
    model: Entity<InputState>,
    search: Entity<InputState>,
    default_provider: Entity<SelectState<SearchableVec<ProviderChoice>>>,
    default_model: Entity<SelectState<SearchableVec<ModelChoice>>>,
    provider_items: Vec<ProviderChoice>,
    model_items: Vec<String>,
    discovered: Entity<SelectState<DiscoveredModels>>,
    discovered_choices: Vec<DiscoveredModel>,
    discovery: Option<(String, String, Vec<String>)>,
    loaded: bool,
    advanced: bool,
    error: Option<String>,
    pending: Option<Pending>,
    _subscriptions: Vec<Subscription>,
}

impl ProviderForm {
    pub(super) fn new(
        permissions: PermissionsAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let endpoint =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/v1"));
        let key = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || t!("ai_key_hint").to_string())
                .masked(true)
        });
        let model = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || {
                t!("ai_model_hint").to_string()
            })
        });
        let search = cx.new(|cx| {
            crate::app::localization::localized_input(window, cx, || {
                t!("ai_provider_search").to_string()
            })
        });
        let discovered = cx.new(|cx| {
            SelectState::new(DiscoveredModels::new(Vec::new()), None, window, cx).searchable(true)
        });
        let default_provider = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ProviderChoice>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let default_model = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ModelChoice>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        // Never observe the keyed-state owner: form notifications already propagate to it.
        let mut subscriptions = Vec::new();
        for input in [&name, &endpoint, &key, &model, &search] {
            subscriptions.push(cx.subscribe(input, |_, _, _: &InputEvent, cx| cx.notify()));
        }
        subscriptions.push(cx.subscribe_in(
            &discovered,
            window,
            |this, picker, event: &SelectEvent<DiscoveredModels>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    if !this.draft.models.iter().any(|model| model.id == *id) {
                        this.model
                            .update(cx, |i, cx| i.set_value(id.clone(), window, cx));
                        this.add_model(window, cx);
                    }
                    // This picker adds an item; it is not a second default-model selector.
                    picker.update(cx, |state, cx| state.set_selected_index(None, window, cx));
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            &default_provider,
            |this, _, event: &SelectEvent<SearchableVec<ProviderChoice>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.choose_default_provider(id);
                    cx.notify();
                }
            },
        ));
        subscriptions.push(cx.subscribe(
            &default_model,
            |this, _, event: &SelectEvent<SearchableVec<ModelChoice>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event
                    && let Some(selection) = &mut this.selection
                {
                    selection.model_id = id.clone();
                    this.error = None;
                    cx.notify();
                }
            },
        ));
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        let draft = initial_provider(&snapshot.settings);
        name.update(cx, |i, cx| i.set_value(draft.name.clone(), window, cx));
        endpoint.update(cx, |i, cx| i.set_value(draft.base_url.clone(), window, cx));
        Self {
            permissions,
            baseline: draft.clone(),
            draft,
            selection: snapshot.settings.default_selection(),
            observed_settings: snapshot.settings,
            name,
            endpoint,
            key,
            model,
            search,
            default_provider,
            default_model,
            provider_items: Vec::new(),
            model_items: Vec::new(),
            discovered,
            discovered_choices: Vec::new(),
            discovery: None,
            loaded: snapshot.ready,
            advanced: false,
            error: None,
            pending: None,
            _subscriptions: subscriptions,
        }
    }

    fn choose_default_provider(&mut self, id: &str) {
        if self.selection.as_ref().is_none_or(|s| s.provider_id != id) {
            // Even identical model IDs at two providers are distinct choices.
            self.selection = Some(ModelSelection {
                provider_id: id.into(),
                model_id: String::new(),
            });
        }
        self.error = None;
    }

    fn select(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |i, cx| i.set_value(provider.name.clone(), window, cx));
        self.endpoint.update(cx, |i, cx| {
            i.set_value(provider.base_url.clone(), window, cx)
        });
        self.key.update(cx, |i, cx| i.set_value("", window, cx));
        self.model.update(cx, |i, cx| i.set_value("", window, cx));
        self.baseline = provider.clone();
        self.draft = provider;
        self.error = None;
        cx.notify();
    }

    fn request_select(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        if provider.id == self.draft.id {
            return;
        }
        if self.provider_dirty(cx) {
            let form = cx.entity();
            ConfirmationDialog::new(
                t!("ai_unsaved_title").to_string(),
                t!("ai_unsaved_provider_notice").to_string(),
            )
            .confirm_label(t!("ai_discard_switch").to_string())
            .on_ok(move |_, window, cx| {
                form.update(cx, |this, cx| this.select(provider.clone(), window, cx));
                true
            })
            .open(window, cx);
        } else {
            self.select(provider, window, cx);
        }
    }

    fn provider(&self, cx: &gpui::App) -> Provider {
        let mut provider = self.draft.clone();
        provider.name = self.name.read(cx).value().trim().into();
        provider.base_url = self.endpoint.read(cx).value().trim().into();
        provider
    }

    fn provider_dirty(&self, cx: &gpui::App) -> bool {
        self.provider(cx) != self.baseline || !self.key.read(cx).value().is_empty()
    }

    fn key(&self, cx: &gpui::App) -> Option<String> {
        let key = self.key.read(cx).value().to_string();
        (!key.is_empty()).then_some(key)
    }

    fn providers(&self, settings: &Settings, cx: &gpui::App) -> Vec<Provider> {
        let mut providers = settings.providers.clone();
        let draft = self.provider(cx);
        if let Some(provider) = providers.iter_mut().find(|p| p.id == draft.id) {
            *provider = draft;
        } else {
            providers.push(draft);
        }
        providers
    }

    fn add_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.model.read(cx).value().trim().to_string();
        if id.is_empty() || id.len() > 256 {
            self.error = Some("AI_NO_MODEL".into());
        } else {
            if !self.draft.models.iter().any(|m| m.id == id) {
                self.draft.models.push(Model {
                    id: id.clone(),
                    supports_tools: true,
                });
            }
            if self.draft.default_model.is_empty() {
                self.draft.default_model = id;
            }
            self.model.update(cx, |i, cx| i.set_value("", window, cx));
            self.error = None;
        }
        self.sync_discovered_choices(cx);
        cx.notify();
    }

    fn remove_model(&mut self, id: &str, cx: &mut Context<Self>) {
        self.draft.models.retain(|m| m.id != id);
        if let Some(selection) = &mut self.selection
            && selection.provider_id == self.draft.id
            && selection.model_id == id
        {
            selection.model_id.clear();
        }
        self.sync_discovered_choices(cx);
        cx.notify();
    }

    fn request_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = service()
            .map(|s| s.snapshot().settings)
            .unwrap_or_else(|_| self.observed_settings.clone());
        let validation = settings.with_provider_default(self.provider(cx), self.selection.as_ref());
        if validation.is_err_and(|error| error.to_string() == "AI_DEFAULT_REQUIRED") {
            let providers = self
                .providers(&settings, cx)
                .into_iter()
                .filter(|p| p.enabled && !p.models.is_empty())
                .collect::<Vec<_>>();
            if !providers.is_empty() {
                self.error = None;
                default_picker::open(
                    providers,
                    self.selection.clone(),
                    &self.draft.id,
                    cx.entity().downgrade(),
                    window,
                    cx,
                );
                cx.notify();
                return;
            }
        }
        self.save(window, cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let result = service().and_then(|s| {
            s.save_provider(self.provider(cx), self.key(cx), self.selection.clone())
                .map_err(|e| e.to_string())
        });
        self.error = super::feedback::report(result, "feedback_saved", window, cx);
        if self.error.is_none() {
            self.pending = Some(Pending::Save);
        }
        cx.notify();
        self.error.is_none()
    }

    fn request_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.entity();
        let id = self.draft.id.clone();
        ConfirmationDialog::new(
            t!("ai_delete_provider").to_string(),
            t!("ai_delete_provider_notice").to_string(),
        )
        .danger(true)
        .on_ok(move |_, window, cx| {
            form.update(cx, |this, cx| {
                let result = service()
                    .and_then(|s| s.remove_provider(id.clone()).map_err(|e| e.to_string()));
                this.error = super::feedback::report(result, "feedback_deleted", window, cx);
                if this.error.is_none() {
                    this.pending = Some(Pending::Delete);
                }
                cx.notify();
            });
            true
        })
        .open(window, cx);
    }

    fn refresh(&mut self, snapshot: &Snapshot, window: &mut Window, cx: &mut Context<Self>) {
        if !self.loaded && snapshot.ready {
            self.loaded = true;
            self.selection = snapshot.settings.default_selection();
            self.observed_settings = snapshot.settings.clone();
            self.select(initial_provider(&snapshot.settings), window, cx);
        }
        if !snapshot.busy
            && let Some(pending) = self.pending.take()
        {
            if snapshot.notice.as_deref() == Some("AI_SAVED") {
                let provider = match pending {
                    Pending::Save => snapshot
                        .settings
                        .providers
                        .iter()
                        .find(|p| p.id == self.draft.id)
                        .cloned()
                        .unwrap_or_else(|| initial_provider(&snapshot.settings)),
                    Pending::Delete => initial_provider(&snapshot.settings),
                };
                self.selection = snapshot.settings.default_selection();
                self.select(provider, window, cx);
            } else {
                self.error = snapshot.notice.clone();
            }
        }
        if self.observed_settings != snapshot.settings {
            if self.selection == self.observed_settings.default_selection() {
                self.selection = snapshot.settings.default_selection();
            }
            if !self.provider_dirty(cx) {
                let provider = snapshot
                    .settings
                    .providers
                    .iter()
                    .find(|p| p.id == self.draft.id)
                    .cloned();
                if let Some(provider) = provider {
                    self.select(provider, window, cx);
                } else if self
                    .observed_settings
                    .providers
                    .iter()
                    .any(|p| p.id == self.draft.id)
                {
                    self.select(initial_provider(&snapshot.settings), window, cx);
                }
            }
            self.observed_settings = snapshot.settings.clone();
        }
        let discovery = snapshot
            .discovered_models
            .as_ref()
            .filter(|(id, url, _)| {
                id == &self.draft.id && url == self.endpoint.read(cx).value().trim()
            })
            .cloned();
        if self.discovery != discovery {
            let items = discovery
                .as_ref()
                .map(|(_, _, items)| items.clone())
                .unwrap_or_default();
            self.discovered_choices = items
                .into_iter()
                .map(|id| {
                    let added = self.draft.models.iter().any(|m| m.id == id);
                    DiscoveredModel::new(id, added)
                })
                .collect();
            self.discovered.update(cx, |s, cx| {
                s.set_items(
                    DiscoveredModels::new(self.discovered_choices.clone()),
                    window,
                    cx,
                );
                s.set_selected_index(None, window, cx);
            });
            self.discovery = discovery;
        }
        self.sync_discovered_choices(cx);
        self.refresh_selectors(&snapshot.settings, window, cx);
    }

    fn sync_discovered_choices(&self, cx: &mut Context<Self>) {
        let mut changed = false;
        for choice in &self.discovered_choices {
            changed |=
                choice.set_added(self.draft.models.iter().any(|model| model.id == choice.id));
        }
        if changed {
            // Shared row flags update both the full and searched list without resetting its query.
            self.discovered.update(cx, |_, cx| cx.notify());
        }
    }
}

fn initial_provider(settings: &Settings) -> Provider {
    settings
        .providers
        .iter()
        .find(|p| Some(&p.id) == settings.default_provider.as_ref())
        .or_else(|| settings.providers.first())
        .cloned()
        .unwrap_or_default()
}

fn select_trigger(select: impl gpui::IntoElement) -> gpui::Div {
    // Select's outer element uses size_full; sizing only the Select styles its inner input.
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .input_h(Size::Small)
        .child(select)
}

fn select_menu_width(window: &Window) -> gpui::Pixels {
    (window.viewport_size().width - px(48.))
        .min(px(560.))
        .max(px(160.))
}

fn select_menu_max_height(window: &Window) -> gpui::Pixels {
    (window.viewport_size().height * 0.4)
        .min(px(280.))
        .max(px(120.))
}

fn discovered_model_select(
    state: &Entity<SelectState<DiscoveredModels>>,
    disabled: bool,
    window: &Window,
) -> gpui::Div {
    select_trigger(
        Select::new(state)
            .w_full()
            .small()
            .disabled(disabled)
            .placeholder(t!("ai_select_discovered_model").to_string())
            .search_placeholder(t!("ai_model_search").to_string())
            .menu_width(select_menu_width(window))
            .menu_max_h(select_menu_max_height(window)),
    )
}

impl Render for ProviderForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        self.refresh(&snapshot, window, cx);
        let disabled = !self.loaded || snapshot.busy;
        let global = self.render_default(disabled, window, cx);
        let sidebar = self.render_sidebar(&snapshot.settings, disabled, cx);
        let editor = self.render_editor(&snapshot, disabled, window, cx);
        v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .gap_4()
            .child(global)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .items_start()
                    .flex_wrap()
                    .gap_4()
                    .child(sidebar)
                    .child(editor),
            )
    }
}
