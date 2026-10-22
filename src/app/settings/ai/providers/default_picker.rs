//! Complete a missing default pair at save time without discarding the provider draft.
use super::*;
use gpui::WeakEntity;
use gpui_component::WindowExt as _;

struct DefaultPicker {
    owner: WeakEntity<ProviderForm>,
    providers: Vec<Provider>,
    provider: Entity<SelectState<SearchableVec<ProviderChoice>>>,
    model: Entity<SelectState<SearchableVec<ModelChoice>>>,
    models_for: Option<String>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl DefaultPicker {
    fn new(
        providers: Vec<Provider>,
        initial: Option<ModelSelection>,
        preferred: &str,
        owner: WeakEntity<ProviderForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let provider = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(
                    providers
                        .iter()
                        .map(|p| ProviderChoice {
                            id: p.id.clone(),
                            name: p.name.clone(),
                        })
                        .collect::<Vec<_>>(),
                ),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let model = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ModelChoice>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &provider,
                window,
                |this, _, event: &SelectEvent<SearchableVec<ProviderChoice>>, window, cx| {
                    if let SelectEvent::Confirm(Some(id)) = event {
                        this.set_provider(id, window, cx);
                        this.error = None;
                        cx.notify();
                    }
                },
            ),
            cx.subscribe(
                &model,
                |this, _, _: &SelectEvent<SearchableVec<ModelChoice>>, cx| {
                    this.error = None;
                    cx.notify();
                },
            ),
        ];
        let selected = initial
            .as_ref()
            .map(|s| s.provider_id.as_str())
            .filter(|id| providers.iter().any(|p| p.id == *id))
            .or_else(|| {
                providers
                    .iter()
                    .find(|p| p.id == preferred)
                    .map(|p| p.id.as_str())
            })
            .map(str::to_owned);
        let mut this = Self {
            owner,
            providers,
            provider,
            model,
            models_for: None,
            error: None,
            _subscriptions: subscriptions,
        };
        if let Some(id) = selected {
            this.provider
                .update(cx, |state, cx| state.set_selected_value(&id, window, cx));
            this.set_provider(&id, window, cx);
            if let Some(initial) = initial.filter(|selection| selection.provider_id == id) {
                this.model.update(cx, |state, cx| {
                    state.set_selected_value(&initial.model_id, window, cx)
                });
            }
        }
        this
    }

    fn set_provider(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.models_for.as_deref() == Some(id) {
            return;
        }
        let items = self
            .providers
            .iter()
            .find(|p| p.id == id)
            .map(|p| {
                p.models
                    .iter()
                    .map(|m| ModelChoice(m.id.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.model.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(items), window, cx);
            state.set_selected_index(None, window, cx);
        });
        self.models_for = Some(id.to_owned());
    }

    fn selection(&self, cx: &gpui::App) -> Option<ModelSelection> {
        let provider_id = self.provider.read(cx).selected_value()?;
        let model_id = self.model.read(cx).selected_value()?;
        self.providers.iter().find(|p| {
            p.id == *provider_id && p.enabled && p.models.iter().any(|m| m.id == *model_id)
        })?;
        Some(ModelSelection {
            provider_id: provider_id.clone(),
            model_id: model_id.clone(),
        })
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.selection(cx) else {
            self.error = Some("AI_DEFAULT_REQUIRED".into());
            cx.notify();
            return false;
        };
        let Some(owner) = self.owner.upgrade() else {
            return true;
        };
        let (saved, error) = owner.update(cx, |form, cx| {
            form.selection = Some(selection);
            (form.save(window, cx), form.error.clone())
        });
        self.error = error;
        cx.notify();
        saved
    }
}

impl Render for DefaultPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let mut form = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(note(t!("ai_default_before_save_hint").to_string(), cx))
            .child(field(
                "ai_provider_label",
                select_trigger(
                    Select::new(&self.provider)
                        .small()
                        .w_full()
                        .placeholder(t!("ai_choose_provider").to_string())
                        .search_placeholder(t!("ai_provider_search").to_string())
                        .menu_width(select_menu_width(window))
                        .menu_max_h(select_menu_max_height(window)),
                )
                .into_any_element(),
                cx,
            ))
            .child(field(
                "ai_model_label",
                select_trigger(
                    Select::new(&self.model)
                        .small()
                        .w_full()
                        .placeholder(t!("ai_choose_model").to_string())
                        .search_placeholder(t!("ai_model_search").to_string())
                        .disabled(self.provider.read(cx).selected_value().is_none())
                        .menu_width(select_menu_width(window))
                        .menu_max_h(select_menu_max_height(window)),
                )
                .into_any_element(),
                cx,
            ));
        if let Some(selection) = self.selection(cx) {
            let provider = self
                .providers
                .iter()
                .find(|p| p.id == selection.provider_id)
                .map(|p| p.name.as_str())
                .unwrap_or_default();
            form = form.child(
                div().w_full().whitespace_normal().text_sm().child(
                    t!(
                        "ai_selected_default",
                        provider = provider,
                        model = selection.model_id
                    )
                    .to_string(),
                ),
            );
        }
        if let Some(error) = &self.error {
            form = form.child(
                div()
                    .whitespace_normal()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error_text(error)),
            );
        }
        form
    }
}

pub(super) fn open(
    providers: Vec<Provider>,
    initial: Option<ModelSelection>,
    preferred: &str,
    owner: WeakEntity<ProviderForm>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let picker = cx.new(|cx| DefaultPicker::new(providers, initial, preferred, owner, window, cx));
    // This is an input form, not a two-action confirmation; use the native Dialog.
    window.open_dialog(cx, move |dialog, window, _| {
        let save = picker.clone();
        let scale = (window.rem_size() / px(14.)).max(1.);
        let layout = crate::app::dialog_layout::centered_dialog_layout(window, px(380.) * scale, 0);
        dialog
            .title(t!("ai_default_before_save").to_string())
            .w((window.viewport_size().width - px(32.)).min(px(480.) * scale))
            .h(layout.height)
            .margin_top(layout.margin_top)
            .overlay_closable(false)
            .close_button(false)
            .keyboard(false)
            .child(picker.clone())
            .footer(
                h_flex()
                    .justify_end()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("ai-default-cancel")
                            .outline()
                            .label(t!("cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("ai-default-save")
                            .primary()
                            .label(t!("ai_choose_default_save").to_string())
                            .on_click(move |_, window, cx| {
                                let save = save.clone();
                                window.defer(cx, move |window, cx| {
                                    if save.update(cx, |picker, cx| picker.submit(window, cx)) {
                                        window.close_dialog(cx);
                                    }
                                });
                            }),
                    ),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn completing_default_requires_a_model_and_keeps_owner_untouched_before_save(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        cx.add_window(|window, cx| {
            let owner = cx.new(|cx| ProviderForm::new(std::rc::Rc::new(|_, _| {}), window, cx));
            let provider = Provider {
                name: "Fixture".into(),
                models: vec![Model {
                    id: "chosen".into(),
                    supports_tools: false,
                }],
                ..Provider::default()
            };
            let picker = cx.new(|cx| {
                DefaultPicker::new(
                    vec![provider.clone()],
                    None,
                    &provider.id,
                    owner.downgrade(),
                    window,
                    cx,
                )
            });
            picker.update(cx, |picker, cx| {
                assert!(picker.selection(cx).is_none());
                assert!(!picker.submit(window, cx));
                assert_eq!(picker.error.as_deref(), Some("AI_DEFAULT_REQUIRED"));
                picker.model.update(cx, |state, cx| {
                    state.set_selected_value(&"chosen".into(), window, cx)
                });
                assert_eq!(
                    picker.selection(cx),
                    Some(ModelSelection {
                        provider_id: provider.id,
                        model_id: "chosen".into()
                    })
                );
                assert!(owner.read(cx).selection.is_none());
            });
            gpui_component::Root::new(picker, window, cx)
        });
    }
}
