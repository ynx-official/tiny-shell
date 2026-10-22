use super::*;
use gpui::{InteractiveElement as _, SharedString, StatefulInteractiveElement as _};
use gpui_component::{Icon, IconName, select::SelectItem, tooltip::Tooltip};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ProviderChoice {
    pub id: String,
    pub name: String,
}
impl SelectItem for ProviderChoice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.name.clone().into()
    }
    fn value(&self) -> &String {
        &self.id
    }

    fn render(&self, window: &mut Window, _: &mut gpui::App) -> impl gpui::IntoElement {
        choice_label(&self.id, &self.name, window)
    }
}

#[derive(Clone)]
pub(super) struct ModelChoice(pub String);

impl SelectItem for ModelChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.0.clone().into()
    }

    fn value(&self) -> &String {
        &self.0
    }

    fn render(&self, window: &mut Window, _: &mut gpui::App) -> impl gpui::IntoElement {
        choice_label(&self.0, &self.0, window)
    }
}

pub(super) fn choice_label(id: &str, label: &str, window: &Window) -> gpui::Stateful<gpui::Div> {
    let full_label = label.to_owned();
    // The virtual list measures one row for all items. Keep rows single-line and show
    // the complete identifier in a wrapping tooltip instead of varying row heights.
    div()
        .id(format!("ai-choice-{id}"))
        .w(select_menu_width(window) - px(56.))
        .truncate()
        .child(label.to_owned())
        .tooltip(move |window, cx| {
            let label = full_label.clone();
            let width = select_menu_width(window) - px(32.);
            Tooltip::element(move |_, _| div().w(width).whitespace_normal().child(label.clone()))
                .build(window, cx)
        })
}

impl ProviderForm {
    pub(super) fn refresh_selectors(
        &mut self,
        settings: &Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let providers = self.providers(settings, cx);
        let items = providers
            .iter()
            .filter(|p| p.enabled && !p.models.is_empty())
            .map(|p| ProviderChoice {
                id: p.id.clone(),
                name: if p.name.is_empty() {
                    t!("ai_new_provider").to_string()
                } else {
                    p.name.clone()
                },
            })
            .collect::<Vec<_>>();
        let selected_provider = self
            .selection
            .as_ref()
            .filter(|s| items.iter().any(|p| p.id == s.provider_id))
            .map(|s| s.provider_id.clone());
        let models = providers
            .iter()
            .find(|p| Some(&p.id) == selected_provider.as_ref())
            .map(|p| p.models.iter().map(|m| m.id.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let selected_model = self
            .selection
            .as_ref()
            .filter(|s| models.contains(&s.model_id))
            .map(|s| s.model_id.clone());
        if self.provider_items != items
            || self.default_provider.read(cx).selected_value() != selected_provider.as_ref()
        {
            self.default_provider.update(cx, |state, cx| {
                state.set_items(SearchableVec::new(items.clone()), window, cx);
                if let Some(id) = &selected_provider {
                    state.set_selected_value(id, window, cx);
                } else {
                    state.set_selected_index(None, window, cx);
                }
            });
            self.provider_items = items;
        }
        if self.model_items != models
            || self.default_model.read(cx).selected_value() != selected_model.as_ref()
        {
            self.default_model.update(cx, |state, cx| {
                state.set_items(
                    SearchableVec::new(models.iter().cloned().map(ModelChoice).collect::<Vec<_>>()),
                    window,
                    cx,
                );
                if let Some(id) = &selected_model {
                    state.set_selected_value(id, window, cx);
                } else {
                    state.set_selected_index(None, window, cx);
                }
            });
            self.model_items = models;
        }
    }

    pub(super) fn render_default(
        &self,
        disabled: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let valid = self.default_provider.read(cx).selected_value().is_some()
            && self.default_model.read(cx).selected_value().is_some();
        v_flex()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .gap_2()
            .pb_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .flex_wrap()
                    .gap_3()
                    .items_end()
                    .child(
                        v_flex()
                            .min_w(px(150.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(t!("ai_global_default").to_string()),
                            )
                            .child(note(t!("ai_global_default_hint").to_string(), cx)),
                    )
                    .child(
                        div().flex_1().min_w(px(140.)).child(field(
                            "ai_provider_label",
                            select_trigger(
                                Select::new(&self.default_provider)
                                    .small()
                                    .w_full()
                                    .placeholder(t!("ai_choose_provider").to_string())
                                    .search_placeholder(t!("ai_provider_search").to_string())
                                    .menu_max_h(select_menu_max_height(window))
                                    .menu_width(select_menu_width(window))
                                    .disabled(disabled || self.provider_items.is_empty()),
                            )
                            .debug_selector(|| "ai-default-provider-picker".into())
                            .into_any_element(),
                            cx,
                        )),
                    )
                    .child(
                        div().flex_1().min_w(px(200.)).child(field(
                            "ai_model_label",
                            select_trigger(
                                Select::new(&self.default_model)
                                    .small()
                                    .w_full()
                                    .placeholder(t!("ai_choose_model").to_string())
                                    .search_placeholder(t!("ai_model_search").to_string())
                                    .menu_max_h(select_menu_max_height(window))
                                    .menu_width(select_menu_width(window))
                                    .disabled(disabled || self.model_items.is_empty()),
                            )
                            .debug_selector(|| "ai-default-model-picker".into())
                            .into_any_element(),
                            cx,
                        )),
                    ),
            )
            .when(valid, |section| {
                let provider = self
                    .provider_items
                    .iter()
                    .find(|p| Some(&p.id) == self.default_provider.read(cx).selected_value());
                let model = self.default_model.read(cx).selected_value();
                section.when_some(provider.zip(model), |section, (provider, model)| {
                    section.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .whitespace_normal()
                            .text_sm()
                            .debug_selector(|| "ai-selected-model-summary".into())
                            .child(
                                t!(
                                    "ai_selected_default",
                                    provider = provider.name,
                                    model = model
                                )
                                .to_string(),
                            ),
                    )
                })
            })
            .child(note(
                t!(if self.provider_items.is_empty() {
                    "ai_default_empty"
                } else if !valid {
                    "ai_default_required"
                } else {
                    "ai_default_save_hint"
                })
                .to_string(),
                cx,
            ))
    }

    pub(super) fn render_sidebar(
        &self,
        settings: &Settings,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let query = self.search.read(cx).value().trim().to_lowercase();
        let providers = self.providers(settings, cx);
        let mut list = v_flex()
            .id("ai-provider-list")
            .w_full()
            .max_h(px(360.))
            .overflow_y_scroll()
            .gap_1();
        let mut visible = 0;
        for provider in &providers {
            let name = if provider.name.is_empty() {
                t!("ai_new_provider").to_string()
            } else {
                provider.name.clone()
            };
            if !query.is_empty() && !name.to_lowercase().contains(&query) {
                continue;
            }
            visible += 1;
            let selected = provider.id == self.draft.id;
            let saved = settings
                .providers
                .iter()
                .find(|p| p.id == provider.id)
                .cloned()
                .unwrap_or_else(|| provider.clone());
            list = list.child(
                Button::new(format!("ai-provider-row-{}", provider.id))
                    .ghost()
                    .w_full()
                    .h_auto()
                    .py_2()
                    .px_2()
                    .when(selected, |b| {
                        b.bg(cx.theme().accent)
                            .border_l_2()
                            .border_color(cx.theme().primary)
                    })
                    .disabled(disabled)
                    .tooltip(name.clone())
                    .child(
                        v_flex()
                            .w_full()
                            .min_w_0()
                            .items_start()
                            .gap_1()
                            .child(div().w_full().min_w_0().truncate().text_sm().child(name))
                            .child(note(
                                t!(
                                    "ai_provider_summary",
                                    count = provider.models.len(),
                                    status = t!(if provider.enabled {
                                        "ai_status_enabled"
                                    } else {
                                        "ai_status_disabled"
                                    })
                                )
                                .to_string(),
                                cx,
                            )),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.request_select(saved.clone(), window, cx)
                    })),
            );
        }
        if visible == 0 {
            list = list.child(note(t!("no_results").to_string(), cx));
        }
        v_flex()
            .w(px(180.))
            .flex_shrink_0()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_sm().child(t!("ai_provider_label").to_string()))
                    .child(note(providers.len().to_string(), cx)),
            )
            .child(
                Button::new("ai-provider-new")
                    .small()
                    .outline()
                    .w_full()
                    .icon(IconName::Plus)
                    .label(t!("ai_new_provider").to_string())
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request_select(Provider::default(), window, cx)
                    })),
            )
            .child(
                Input::new(&self.search)
                    .small()
                    .prefix(Icon::new(IconName::Search).size(px(14.)))
                    .disabled(disabled),
            )
            .child(list)
    }
}
