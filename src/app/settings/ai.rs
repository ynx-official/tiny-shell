//! Provider form owns drafts; background service owns secrets and persistence.
use super::page::{SettingGroup, SettingItem, SettingPage};
use crate::{
    TinyShell,
    ai::{Model, Provider},
    app::{
        ai::{error_text, service},
        confirmation_dialog::ConfirmationDialog,
    },
};
use gpui::{
    AppContext as _, Context, Entity, IntoElement as _, ParentElement as _, Render, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    v_flex,
};
use rust_i18n::t;
use std::rc::Rc;
mod execution;
mod followups;
mod notifications;

type PermissionsAction = Rc<dyn Fn(&mut Window, &mut gpui::App)>;

pub(crate) fn page(owner: &Entity<TinyShell>) -> SettingPage {
    let owner = owner.clone();
    let audit_owner = owner.clone();
    SettingPage::new(t!("settings_ai").to_string())
        .icon(crate::app::ai::icon())
        .default_open(true)
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_provider").to_string())
                .item(SettingItem::render(move |_, window, cx| {
                    window
                        .use_keyed_state("ai-provider-form", cx, |window, cx| {
                            let weak = owner.downgrade();
                            ProviderForm::new(
                                Rc::new(move |window, cx| {
                                    if let Some(owner) = weak.upgrade() {
                                        owner.update(cx, |o, cx| {
                                            o.mcp_requested_task = None;
                                            o.show_ai_settings(5, window, cx);
                                        });
                                    }
                                }),
                                window,
                                cx,
                            )
                        })
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_budget").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state("ai-budget-settings", cx, execution::BudgetForm::new)
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_notifications").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state(
                            "ai-notification-settings",
                            cx,
                            notifications::NotificationForm::new,
                        )
                        .into_any_element()
                })),
        )
        .group(
            SettingGroup::new()
                .title(t!("audit_title").to_string())
                .on_open(move |_, cx| {
                    crate::app::audit::open(
                        tiny_shell_mcp::audit::Filter::default(),
                        audit_owner.clone(),
                        cx,
                    );
                }),
        )
        .group(
            SettingGroup::new()
                .title(t!("ai_settings_followups").to_string())
                .item(SettingItem::render(|_, window, cx| {
                    window
                        .use_keyed_state("ai-followup-settings", cx, followups::FollowupForm::new)
                        .into_any_element()
                })),
        )
}
struct ProviderForm {
    permissions: PermissionsAction,
    draft: Provider,
    name: Entity<InputState>,
    endpoint: Entity<InputState>,
    key: Entity<InputState>,
    model: Entity<InputState>,
    discovered: Entity<SelectState<SearchableVec<String>>>,
    discovery: Option<(String, String, Vec<String>)>,
    loaded: bool,
    default: bool,
    error: Option<String>,
    submitted: bool,
    _subscriptions: Vec<Subscription>,
}
impl ProviderForm {
    fn new(permissions: PermissionsAction, window: &mut Window, cx: &mut Context<Self>) -> Self {
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
        let discovered = cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::<String>::new()), None, window, cx)
                .searchable(true)
        });
        // Keyed state forwards form notifications to its rendering view. Observing that view
        // in return forms a cycle when the settings page is embedded in the workspace.
        let mut subscriptions = Vec::new();
        for i in [&name, &endpoint, &key, &model] {
            subscriptions.push(cx.subscribe(i, |_, _, _: &InputEvent, cx| cx.notify()));
        }
        subscriptions.push(cx.subscribe_in(
            &discovered,
            window,
            |this, _, event: &SelectEvent<SearchableVec<String>>, window, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    this.model
                        .update(cx, |i, cx| i.set_value(id.clone(), window, cx));
                    this.add_model(window, cx);
                }
            },
        ));
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        let draft = snapshot
            .settings
            .providers
            .iter()
            .find(|p| Some(&p.id) == snapshot.settings.default_provider.as_ref())
            .or_else(|| snapshot.settings.providers.first())
            .cloned()
            .unwrap_or_default();
        name.update(cx, |i, cx| i.set_value(draft.name.clone(), window, cx));
        endpoint.update(cx, |i, cx| i.set_value(draft.base_url.clone(), window, cx));
        let default = snapshot.settings.default_provider.as_deref() == Some(&draft.id);
        Self {
            permissions,
            draft,
            name,
            endpoint,
            key,
            model,
            discovered,
            discovery: None,
            loaded: snapshot.ready,
            default,
            error: None,
            submitted: false,
            _subscriptions: subscriptions,
        }
    }
    fn select(&mut self, p: Provider, window: &mut Window, cx: &mut Context<Self>) {
        self.name
            .update(cx, |i, cx| i.set_value(p.name.clone(), window, cx));
        self.endpoint
            .update(cx, |i, cx| i.set_value(p.base_url.clone(), window, cx));
        self.key.update(cx, |i, cx| i.set_value("", window, cx));
        self.model.update(cx, |i, cx| i.set_value("", window, cx));
        self.default = service()
            .is_ok_and(|s| s.snapshot().settings.default_provider.as_deref() == Some(&p.id));
        self.draft = p;
        self.error = None;
        cx.notify();
    }
    fn provider(&self, cx: &gpui::App) -> Provider {
        let mut p = self.draft.clone();
        p.name = self.name.read(cx).value().trim().into();
        p.base_url = self.endpoint.read(cx).value().trim().into();
        p
    }
    fn key(&self, cx: &gpui::App) -> Option<String> {
        let key = self.key.read(cx).value().to_string();
        (!key.is_empty()).then_some(key)
    }
    fn add_model(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.model.read(cx).value().trim().to_string();
        if id.is_empty() || id.len() > 256 {
            self.error = Some("AI_NO_MODEL".into());
            return;
        }
        if !self.draft.models.iter().any(|m| m.id == id) {
            self.draft.models.push(Model {
                id: id.clone(),
                supports_tools: false,
            });
        }
        if self.draft.default_model.is_empty() {
            self.draft.default_model = id;
        }
        self.model.update(cx, |i, cx| i.set_value("", window, cx));
        cx.notify();
    }
    fn save(&mut self, cx: &mut Context<Self>) {
        let p = self.provider(cx);
        let result = service().and_then(|s| {
            s.save_provider(p, self.key(cx), self.default)
                .map_err(|e| e.to_string())
        });
        self.error = result.err();
        self.submitted = self.error.is_none();
        cx.notify();
    }
    fn checkbox(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        cx: &mut Context<Self>,
        action: impl Fn(&mut Self, bool, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui::AnyElement {
        Checkbox::new(id)
            .label(t!(label).to_string())
            .checked(checked)
            .disabled(!self.loaded || service().is_ok_and(|s| s.snapshot().busy))
            .on_click(cx.listener(move |this, checked, window, cx| {
                action(this, *checked, window, cx);
                cx.notify();
            }))
            .into_any_element()
    }
}
impl Render for ProviderForm {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let snapshot = service().map(|s| s.snapshot()).unwrap_or_default();
        if !self.loaded && snapshot.ready {
            self.loaded = true;
            if let Some(p) = snapshot
                .settings
                .providers
                .iter()
                .find(|p| Some(&p.id) == snapshot.settings.default_provider.as_ref())
                .or_else(|| snapshot.settings.providers.first())
            {
                self.select(p.clone(), window, cx);
            }
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
            self.discovered.update(cx, |s, cx| {
                s.set_items(SearchableVec::new(items), window, cx);
                s.set_selected_index(None, window, cx);
            });
            self.discovery = discovery;
        }
        if self.submitted && !snapshot.busy && snapshot.notice.as_deref() == Some("AI_SAVED") {
            self.key.update(cx, |i, cx| i.set_value("", window, cx));
            self.submitted = false;
        }
        let view = cx.entity();
        let providers = snapshot.settings.providers.clone();
        let provider_select = Button::new("ai-provider-select")
            .outline()
            .small()
            .flex_1()
            .min_w_0()
            .label(if self.draft.name.is_empty() {
                t!("ai_new_provider").to_string()
            } else {
                self.draft.name.clone()
            })
            .dropdown_menu(move |mut menu, window, _| {
                menu = menu.scrollable(true).max_h(px(280.));
                for p in &providers {
                    let p = p.clone();
                    menu = menu.item(PopupMenuItem::new(p.name.clone()).on_click(
                        window.listener_for(&view, move |this, _, window, cx| {
                            this.select(p.clone(), window, cx)
                        }),
                    ));
                }
                menu
            });
        let mut content = v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .child(
                h_flex().gap_2().child(provider_select).child(
                    Button::new("ai-provider-new")
                        .small()
                        .outline()
                        .icon(IconName::Plus)
                        .label(t!("ai_new_provider").to_string())
                        .disabled(snapshot.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.select(Provider::default(), window, cx)
                        })),
                ),
            )
            .child(field(
                "ai_provider_name",
                Input::new(&self.name)
                    .small()
                    .disabled(snapshot.busy)
                    .into_any_element(),
                cx,
            ))
            .child(note(t!("ai_protocol").to_string(), cx))
            .child(field(
                "ai_endpoint",
                Input::new(&self.endpoint)
                    .small()
                    .disabled(snapshot.busy)
                    .into_any_element(),
                cx,
            ))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .justify_between()
                    .child(note(
                        t!(if snapshot.keys_saved.contains(&self.draft.id) {
                            "ai_key_saved"
                        } else {
                            "ai_key_not_saved"
                        })
                        .to_string(),
                        cx,
                    ))
                    .child(
                        Button::new("ai-clear-key")
                            .ghost()
                            .xsmall()
                            .label(t!("ai_clear_key").to_string())
                            .disabled(
                                snapshot.busy || !snapshot.keys_saved.contains(&self.draft.id),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                let id = this.draft.id.clone();
                                ConfirmationDialog::new(
                                    t!("ai_clear_key").to_string(),
                                    t!("ai_clear_key_notice").to_string(),
                                )
                                .danger(true)
                                .on_ok(move |_, _, _| {
                                    service().is_ok_and(|s| s.clear_key(id.clone()).is_ok())
                                })
                                .open(window, cx);
                            })),
                    ),
            )
            .child(field(
                "ai_api_key",
                Input::new(&self.key)
                    .small()
                    .disabled(snapshot.busy)
                    .mask_toggle()
                    .into_any_element(),
                cx,
            ))
            .child(
                div()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .pt_3()
                    .child(t!("ai_models").to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.model).small())
                    .child(
                        Button::new("ai-model-add")
                            .small()
                            .outline()
                            .icon(IconName::Plus)
                            .tooltip(t!("ai_add_model").to_string())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.add_model(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("ai-model-refresh")
                            .small()
                            .outline()
                            .label(t!("ai_refresh_models").to_string())
                            .disabled(snapshot.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.error = service()
                                    .and_then(|s| {
                                        s.list_models(this.provider(cx), this.key(cx))
                                            .map_err(|e| e.to_string())
                                    })
                                    .err();
                                cx.notify();
                            })),
                    ),
            );
        if self.discovery.is_some() {
            content = content
                .child(discovered_model_select(&self.discovered, window).disabled(snapshot.busy));
        }
        let mut chips = v_flex().w_full().min_w_0().gap_2();
        for model in &self.draft.models {
            let id = model.id.clone();
            let selected = self.draft.default_model == model.id;
            let remove = id.clone();
            let tools_id = id.clone();
            chips = chips.child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .py_1()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(model.id.clone()),
                    )
                    .child(
                        Button::new(format!("ai-select-model-{id}"))
                            .small()
                            .outline()
                            .when(selected, |b| b.primary())
                            .label(
                                t!(if selected {
                                    "ai_default"
                                } else {
                                    "ai_set_default"
                                })
                                .to_string(),
                            )
                            .disabled(snapshot.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.default_model = id.clone();
                                cx.notify();
                            })),
                    )
                    .child(
                        Checkbox::new(format!("ai-model-tools-{tools_id}"))
                            .label(t!("ai_tools_short").to_string())
                            .checked(model.supports_tools)
                            .disabled(snapshot.busy)
                            .on_click(cx.listener(move |this, on, _, cx| {
                                if let Some(m) =
                                    this.draft.models.iter_mut().find(|m| m.id == tools_id)
                                {
                                    m.supports_tools = *on;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(format!("ai-remove-model-{remove}"))
                            .ghost()
                            .xsmall()
                            .icon(IconName::Close)
                            .tooltip(t!("ai_remove_model").to_string())
                            .disabled(snapshot.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.draft.models.retain(|m| m.id != remove);
                                if this.draft.default_model == remove {
                                    this.draft.default_model = this
                                        .draft
                                        .models
                                        .first()
                                        .map(|m| m.id.clone())
                                        .unwrap_or_default();
                                }
                                cx.notify();
                            })),
                    ),
            );
        }
        content = content.child(chips);
        content = content
            .child(note(t!("ai_model_capability_notice").to_string(), cx))
            .child(self.checkbox(
                "ai-stream-usage",
                "audit_stream_usage",
                self.draft.stream_usage,
                cx,
                |this, on, _, _| this.draft.stream_usage = on,
            ))
            .child(self.checkbox(
                "ai-provider-enabled",
                "ai_provider_enabled",
                self.draft.enabled,
                cx,
                |this, on, _, _| {
                    this.draft.enabled = on;
                    if !on {
                        this.default = false;
                    }
                },
            ))
            .child(self.checkbox(
                "ai-provider-default",
                "ai_provider_default",
                self.default,
                cx,
                |this, on, _, _| {
                    this.default = on;
                    if on {
                        this.draft.enabled = true;
                    }
                },
            ))
            .child(self.checkbox(
                "ai-no-key",
                "ai_allow_no_key",
                self.draft.allow_no_key,
                cx,
                |this, on, _, _| this.draft.allow_no_key = on,
            ))
            .child(self.checkbox(
                "ai-http",
                "ai_allow_http",
                self.draft.allow_http,
                cx,
                |this, on, window, cx| {
                    if on {
                        let panel = cx.entity();
                        ConfirmationDialog::new(
                            t!("ai_allow_http").to_string(),
                            t!("ai_http_notice").to_string(),
                        )
                        .on_ok(move |_, _, cx| {
                            panel.update(cx, |this, cx| {
                                this.draft.allow_http = true;
                                cx.notify();
                            });
                            true
                        })
                        .open(window, cx);
                    } else {
                        this.draft.allow_http = false;
                    }
                },
            ))
            .child(note(t!("ai_key_privacy").to_string(), cx))
            .child(note(t!("ai_test_notice").to_string(), cx));
        if snapshot.temporary {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(t!("ai_temporary").to_string()),
            );
        }
        if snapshot.temporary_keys.contains(&self.draft.id) {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().warning)
                    .child(t!("ai_key_temporary").to_string()),
            );
        }
        if let Some(error) = snapshot.storage_error.as_ref().or(self.error.as_ref()) {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error_text(error)),
            );
        }
        if let Some(notice) = snapshot.notice.as_ref() {
            content = content.child(note(error_text(notice), cx));
        }
        let existing = snapshot
            .settings
            .providers
            .iter()
            .any(|p| p.id == self.draft.id);
        let permissions = self.permissions.clone();
        content.child(
            h_flex()
                .flex_wrap()
                .gap_2()
                .pt_3()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    Button::new("ai-delete-provider")
                        .small()
                        .outline()
                        .danger()
                        .label(t!("delete").to_string())
                        .disabled(snapshot.busy || !existing)
                        .on_click(cx.listener(|this, _, window, cx| {
                            let id = this.draft.id.clone();
                            let panel = cx.entity();
                            ConfirmationDialog::new(
                                t!("ai_delete_provider").to_string(),
                                t!("ai_delete_provider_notice").to_string(),
                            )
                            .danger(true)
                            .on_ok(move |_, window, cx| {
                                let ok =
                                    service().is_ok_and(|s| s.remove_provider(id.clone()).is_ok());
                                if ok {
                                    panel.update(cx, |this, cx| {
                                        this.select(Provider::default(), window, cx)
                                    });
                                }
                                ok
                            })
                            .open(window, cx);
                        })),
                )
                .child(
                    Button::new("ai-test-provider")
                        .small()
                        .outline()
                        .label(t!("ai_test_connection").to_string())
                        .disabled(snapshot.busy)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.error = service()
                                .and_then(|s| {
                                    s.test_provider(this.provider(cx), this.key(cx))
                                        .map_err(|e| e.to_string())
                                })
                                .err();
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("ai-save-provider")
                        .small()
                        .primary()
                        .label(t!("ai_save_provider").to_string())
                        .disabled(snapshot.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
                )
                .child(
                    Button::new("ai-open-permissions")
                        .small()
                        .ghost()
                        .label(t!("ai_manage_permissions").to_string())
                        .on_click(move |_, window, cx| permissions(window, cx)),
                ),
        )
    }
}
fn note(text: String, cx: &gpui::App) -> gpui::Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .whitespace_normal()
        .child(text)
}
fn field(label: &str, input: gpui::AnyElement, cx: &gpui::App) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(note(t!(label).to_string(), cx))
        .child(input)
}

fn discovered_model_select(
    state: &Entity<SelectState<SearchableVec<String>>>,
    window: &Window,
) -> Select<SearchableVec<String>> {
    Select::new(state)
        .w_full()
        .small()
        .placeholder(t!("ai_select_discovered_model").to_string())
        .search_placeholder(t!("ai_model_search").to_string())
        .menu_max_h(
            (window.viewport_size().height * 0.4)
                .min(px(280.))
                .max(px(120.)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{InteractiveElement as _, TestAppContext, VisualTestContext};
    use gpui_component::Root;
    use std::cell::Cell;

    #[derive(Default)]
    struct FormHost {
        form: Option<Entity<ProviderForm>>,
    }
    impl Render for FormHost {
        fn render(
            &mut self,
            window: &mut Window,
            cx: &mut Context<Self>,
        ) -> impl gpui::IntoElement {
            let form = window.use_keyed_state("ai-provider-form", cx, |window, cx| {
                ProviderForm::new(Rc::new(|_, _| {}), window, cx)
            });
            self.form = Some(form.clone());
            div().size_full().child(form)
        }
    }

    #[gpui::test]
    fn inline_provider_form_notifications_settle_and_leave_other_windows_usable(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let window = cx.open_window(gpui::size(px(980.), px(700.)), |window, cx| {
            let host = cx.new(|_| FormHost::default());
            Root::new(host, window, cx)
        });
        let root = window.root(cx).unwrap();
        let host = root.read_with(cx, |root, _| {
            root.view().clone().downcast::<FormHost>().unwrap()
        });
        let notifications = Rc::new(Cell::new(0));
        let cycle = Rc::new(Cell::new(false));
        let count = notifications.clone();
        let detected = cycle.clone();
        let guard = cx.update(|cx| {
            cx.observe(&host, move |host, cx| {
                count.set(count.get() + 1);
                if count.get() >= 20 {
                    detected.set(true);
                    // Break a regression's cycle so the test fails instead of hanging the suite.
                    if let Some(form) = host.read(cx).form.clone() {
                        form.update(cx, |form, _| form._subscriptions.clear());
                    }
                }
            })
        });
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(980.), px(700.)),
            |_, _| root.into_any_element(),
        );
        visual.run_until_parked();
        host.read_with(&visual, |host, _| host.form.clone().unwrap())
            .update_in(&mut visual, |form, window, cx| {
                form.name.update(cx, |input, cx| input.focus(window, cx));
            });
        visual.simulate_input("fixture-provider");
        visual.run_until_parked();
        assert!(
            !cycle.get(),
            "inline Provider form and its keyed-state owner repeatedly notify each other"
        );
        assert!(notifications.get() > 0);
        let secondary = cx.add_window(|window, cx| {
            let state = cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["fixture".to_owned()]),
                    None,
                    window,
                    cx,
                )
            });
            let picker = cx.new(|_| Picker { state });
            Root::new(picker, window, cx)
        });
        assert!(secondary.root(cx).is_ok());
        drop(guard);
    }

    struct Picker {
        state: Entity<SelectState<SearchableVec<String>>>,
    }
    impl Render for Picker {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
            div().size_full().p_3().child(
                div()
                    .debug_selector(|| "model-picker".into())
                    .child(discovered_model_select(&self.state, window)),
            )
        }
    }

    #[gpui::test]
    fn large_model_list_searches_and_selects_a_late_model(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let handle = cx.open_window(gpui::size(px(500.), px(320.)), |window, cx| {
            let mut models = (0..180)
                .map(|i| format!("model-{i:03}"))
                .collect::<Vec<_>>();
            models.push("needle-final-model".into());
            let state = cx.new(|cx| {
                SelectState::new(SearchableVec::new(models), None, window, cx).searchable(true)
            });
            let picker = cx.new(|_| Picker { state });
            Root::new(picker, window, cx)
        });
        let root = handle.root(cx).unwrap();
        let mut visual = VisualTestContext::from_window(handle.into(), cx);
        visual.draw(
            gpui::point(px(0.), px(0.)),
            gpui::size(px(500.), px(320.)),
            |_, _| root.into_any_element(),
        );
        let bounds = visual.debug_bounds("model-picker").unwrap();
        visual.simulate_click(bounds.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        visual.simulate_input("needle-final");
        visual.run_until_parked();
        visual.simulate_keystrokes("down enter");
        visual.run_until_parked();
        let selected = visual.update(|window, cx| {
            let picker = window
                .root::<Root>()
                .unwrap()
                .unwrap()
                .read(cx)
                .view()
                .clone();
            picker
                .downcast::<Picker>()
                .unwrap()
                .read(cx)
                .state
                .read(cx)
                .selected_value()
                .cloned()
        });
        assert_eq!(selected.as_deref(), Some("needle-final-model"));
    }
}
