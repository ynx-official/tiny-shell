use super::*;
use gpui::{InteractiveElement as _, TestAppContext, VisualTestContext};
use gpui_component::Root;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct FormHost {
    form: Option<Entity<ProviderForm>>,
}
impl Render for FormHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
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
            // Supply the loaded empty settings without installing a process-global service.
            form.refresh(
                &Snapshot {
                    ready: true,
                    ..Snapshot::default()
                },
                window,
                cx,
            );
            form.name.update(cx, |input, cx| input.focus(window, cx));
        });
    visual.run_until_parked();
    visual.simulate_input("fixture-provider");
    visual.run_until_parked();
    assert_eq!(
        host.read_with(&visual, |host, _| host.form.clone().unwrap())
            .read_with(&visual, |form, cx| form.name.read(cx).value().to_string()),
        "fixture-provider"
    );
    assert!(
        !cycle.get(),
        "inline Provider form and its keyed-state owner repeatedly notify each other"
    );
    assert!(notifications.get() > 0);
    let secondary = cx.add_window(|window, cx| {
        let state = cx.new(|cx| {
            SelectState::new(
                DiscoveredModels::new(vec![DiscoveredModel::new("fixture".to_owned(), false)]),
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
    state: Entity<SelectState<DiscoveredModels>>,
}
impl Render for Picker {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
        div().size_full().p_3().child(
            div()
                .debug_selector(|| "model-picker".into())
                .child(discovered_model_select(&self.state, false, window)),
        )
    }
}

#[gpui::test]
fn large_model_list_searches_and_selects_a_late_model(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let handle = cx.open_window(gpui::size(px(500.), px(320.)), |window, cx| {
        let mut models = (0..180)
            .map(|i| DiscoveredModel::new(format!("model-{i:03}"), false))
            .collect::<Vec<_>>();
        models.push(DiscoveredModel::new("needle-final-model".into(), false));
        let state = cx.new(|cx| {
            SelectState::new(DiscoveredModels::new(models), None, window, cx).searchable(true)
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
    assert!(
        bounds.size.height <= px(32.),
        "the picker must not fill its parent height"
    );
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

struct ModelSection {
    form: Entity<ProviderForm>,
}

impl Render for ModelSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let models = self
            .form
            .update(cx, |form, cx| form.render_models(false, window, cx));
        v_flex().size_full().p_3().child(models)
    }
}

#[gpui::test]
fn model_section_keeps_picker_compact_and_long_names_above_the_help(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let handle = cx.open_window(gpui::size(px(520.), px(640.)), |window, cx| {
        let form = cx.new(|cx| ProviderForm::new(Rc::new(|_, _| {}), window, cx));
        form.update(cx, |form, _| {
            form.draft.models = [
                "gpt-6-astra".to_owned(),
                "provider/very-long-model-name-with-a-release-date-and-context-window-2026-10-22-extended".to_owned(),
                "gpt-6-sol".to_owned(),
                "gpt-6-luna".to_owned(),
            ]
            .into_iter()
            .map(|id| Model { id, supports_tools: false })
            .collect();
            form.discovery = Some((form.draft.id.clone(), form.draft.base_url.clone(), vec![]));
        });
        let host = cx.new(|_| ModelSection { form });
        Root::new(host, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(
        gpui::point(px(0.), px(0.)),
        gpui::size(px(520.), px(640.)),
        |_, _| root.into_any_element(),
    );
    let picker = visual.debug_bounds("ai-discovered-picker").unwrap();
    let header = visual.debug_bounds("ai-model-header").unwrap();
    assert!(picker.size.height <= px(32.));
    assert!(header.top() >= picker.bottom());
    assert!(
        header.top() - picker.bottom() <= px(16.),
        "unexpected blank below picker"
    );
    let mut bottom = header.bottom();
    for (row_id, label_id, tools_id) in [
        ("ai-model-row-0", "ai-model-name-0", "ai-model-tools-cell-0"),
        ("ai-model-row-1", "ai-model-name-1", "ai-model-tools-cell-1"),
        ("ai-model-row-2", "ai-model-name-2", "ai-model-tools-cell-2"),
        ("ai-model-row-3", "ai-model-name-3", "ai-model-tools-cell-3"),
    ] {
        let row = visual.debug_bounds(row_id).unwrap();
        let label = visual.debug_bounds(label_id).unwrap();
        let tools = visual.debug_bounds(tools_id).unwrap();
        assert!(row.top() >= bottom);
        assert!(label.bottom() <= row.bottom());
        assert!(label.right() <= tools.left());
        bottom = row.bottom();
    }
    let short = visual.debug_bounds("ai-model-name-0").unwrap();
    let long = visual.debug_bounds("ai-model-name-1").unwrap();
    assert!(
        long.size.height > short.size.height,
        "long IDs should wrap instead of truncating"
    );
    let help = visual.debug_bounds("ai-model-help").unwrap();
    assert!(help.top() >= bottom, "help must not cover the last model");
    assert!(help.bottom() <= px(640.));
}

#[gpui::test]
fn discovered_model_addition_resets_picker_without_changing_default(cx: &mut TestAppContext) {
    use gpui_component::select::SelectItem as _;
    cx.update(gpui_component::init);
    let window = cx.add_window(|window, cx| {
        let form = cx.new(|cx| ProviderForm::new(Rc::new(|_, _| {}), window, cx));
        form.update(cx, |form, cx| {
            form.draft.models = vec![Model {
                id: "existing-model".into(),
                supports_tools: true,
            }];
            form.draft.default_model = "existing-model".into();
            form.selection = Some(ModelSelection {
                provider_id: form.draft.id.clone(),
                model_id: "existing-model".into(),
            });
            form.refresh(
                &Snapshot {
                    settings: Settings {
                        providers: vec![form.draft.clone()],
                        default_provider: Some(form.draft.id.clone()),
                        ..Settings::default()
                    },
                    ready: true,
                    discovered_models: Some((
                        form.draft.id.clone(),
                        form.draft.base_url.clone(),
                        vec!["existing-model".into(), "discovered-model".into()],
                    )),
                    ..Snapshot::default()
                },
                window,
                cx,
            );
            assert!(form.discovered_choices[0].disabled());
            assert!(!form.discovered_choices[1].disabled());
        });
        let host = cx.new(|_| ModelSection { form });
        Root::new(host, window, cx)
    });
    let root = window.root(cx).unwrap();
    let form = root.read_with(cx, |root, cx| {
        root.view()
            .clone()
            .downcast::<ModelSection>()
            .unwrap()
            .read(cx)
            .form
            .clone()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    let initial_selection = form.read_with(&visual, |form, _| form.selection.clone());
    for _ in 0..2 {
        form.update_in(&mut visual, |form, window, cx| {
            form.discovered.update(cx, |state, cx| {
                state.set_selected_value(&"discovered-model".to_owned(), window, cx);
                cx.emit(SelectEvent::<DiscoveredModels>::Confirm(Some(
                    "discovered-model".to_owned(),
                )));
            });
        });
        visual.run_until_parked();
        form.read_with(&visual, |form, cx| {
            assert_eq!(
                form.draft
                    .models
                    .iter()
                    .filter(|m| m.id == "discovered-model")
                    .count(),
                1
            );
            assert!(form.discovered.read(cx).selected_value().is_none());
            assert_eq!(form.selection, initial_selection);
            assert!(form.model.read(cx).value().is_empty());
            assert!(form.discovered_choices[1].disabled());
        });
    }
    form.update_in(&mut visual, |form, _, cx| {
        form.remove_model("discovered-model", cx);
        assert!(!form.discovered_choices[1].disabled());
        assert_eq!(form.selection, initial_selection);
    });
}

#[gpui::test]
fn saving_provider_without_default_opens_picker_and_cancel_keeps_edits(cx: &mut TestAppContext) {
    use gpui_component::WindowExt as _;
    cx.update(gpui_component::init);
    let window = cx.add_window(|window, cx| {
        let form = cx.new(|cx| ProviderForm::new(Rc::new(|_, _| {}), window, cx));
        Root::new(form, window, cx)
    });
    let root = window.root(cx).unwrap();
    let form = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<ProviderForm>().unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    form.update_in(&mut visual, |form, window, cx| {
        let provider = Provider {
            name: "Unsaved provider".into(),
            base_url: "https://api.example.com/v1".into(),
            models: vec![Model {
                id: "added-only".into(),
                supports_tools: false,
            }],
            ..Provider::default()
        };
        form.select(provider.clone(), window, cx);
        form.selection = None;
        form.request_save(window, cx);
        assert!(window.has_active_dialog(cx));
        assert!(form.pending.is_none());
        window.close_dialog(cx);
        assert_eq!(form.provider(cx).models[0].id, "added-only");
        assert_eq!(form.provider(cx).name, provider.name);
        assert!(form.selection.is_none());
    });
}

struct DefaultSection {
    form: Entity<ProviderForm>,
}

impl Render for DefaultSection {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let selection = self
            .form
            .update(cx, |form, cx| form.render_default(false, window, cx));
        v_flex().size_full().p_3().child(selection)
    }
}

#[gpui::test]
fn default_pair_has_compact_triggers_and_a_wrapping_full_summary(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let size = gpui::size(px(520.), px(480.));
    let handle = cx.open_window(size, |window, cx| {
        let form = cx.new(|cx| ProviderForm::new(Rc::new(|_, _| {}), window, cx));
        form.update(cx, |form, cx| {
            let model_id = "provider/really-long-model-identifier-with-version-context-window-and-release-date-2026-10-22";
            let provider = Provider {
                name: "Long provider name with a team and deployment region".into(),
                models: vec![Model { id: model_id.into(), supports_tools: false }],
                ..Provider::default()
            };
            form.selection = Some(ModelSelection { provider_id: provider.id.clone(), model_id: model_id.into() });
            form.select(provider, window, cx);
            form.refresh_selectors(&Settings::default(), window, cx);
        });
        let host = cx.new(|_| DefaultSection { form });
        Root::new(host, window, cx)
    });
    let root = handle.root(cx).unwrap();
    let mut visual = VisualTestContext::from_window(handle.into(), cx);
    visual.draw(gpui::point(px(0.), px(0.)), size, |_, _| {
        root.into_any_element()
    });
    let provider = visual.debug_bounds("ai-default-provider-picker").unwrap();
    let model = visual.debug_bounds("ai-default-model-picker").unwrap();
    let summary = visual.debug_bounds("ai-selected-model-summary").unwrap();
    assert!(provider.size.height <= px(32.));
    assert!(model.size.height <= px(32.));
    assert!(summary.top() >= provider.bottom().max(model.bottom()));
    assert!(
        summary.size.height > px(32.),
        "the complete pair must wrap over multiple lines"
    );
    assert!(summary.right() <= size.width);
    assert!(summary.bottom() <= size.height);
}

#[gpui::test]
fn global_selectors_keep_editing_independent_and_clear_models_when_provider_changes(
    cx: &mut TestAppContext,
) {
    cx.update(gpui_component::init);
    let window = cx.add_window(|window, cx| {
        let form = cx.new(|cx| ProviderForm::new(Rc::new(|_, _| {}), window, cx));
        Root::new(form, window, cx)
    });
    let root = window.root(cx).unwrap();
    let form = root.read_with(cx, |root, _| {
        root.view().clone().downcast::<ProviderForm>().unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    form.update_in(&mut visual, |form, window, cx| {
        let first = Provider {
            name: "Same name".into(),
            base_url: "https://api.example.com/v1".into(),
            models: vec![Model {
                id: "shared-model".into(),
                supports_tools: false,
            }],
            default_model: "shared-model".into(),
            ..Provider::default()
        };
        let mut second = first.clone();
        second.id = uuid::Uuid::new_v4().to_string();
        second.models.push(Model {
            id: "second-only".into(),
            supports_tools: true,
        });
        let mut disabled = first.clone();
        disabled.id = uuid::Uuid::new_v4().to_string();
        disabled.enabled = false;
        let settings = Settings {
            providers: vec![first.clone(), second.clone(), disabled],
            default_provider: Some(first.id.clone()),
            ..Settings::default()
        };
        form.refresh(
            &Snapshot {
                settings: settings.clone(),
                ready: true,
                ..Snapshot::default()
            },
            window,
            cx,
        );
        form.select(second.clone(), window, cx);
        form.refresh_selectors(&settings, window, cx);
        assert_eq!(form.selection, settings.default_selection());
        assert_eq!(
            form.default_provider.read(cx).selected_value(),
            Some(&first.id)
        );
        assert_eq!(form.provider_items.len(), 2);

        form.choose_default_provider(&second.id);
        form.refresh_selectors(&settings, window, cx);
        assert_eq!(
            form.default_provider.read(cx).selected_value(),
            Some(&second.id)
        );
        assert!(form.default_model.read(cx).selected_value().is_none());
        assert_eq!(form.model_items, vec!["shared-model", "second-only"]);
        form.selection = Some(ModelSelection {
            provider_id: second.id.clone(),
            model_id: "second-only".into(),
        });
        form.refresh_selectors(&settings, window, cx);
        assert_eq!(
            form.default_model
                .read(cx)
                .selected_value()
                .map(String::as_str),
            Some("second-only")
        );
        form.remove_model("second-only", cx);
        form.refresh_selectors(&settings, window, cx);
        assert!(form.default_model.read(cx).selected_value().is_none());
        assert!(form.provider_dirty(cx));
    });
}
