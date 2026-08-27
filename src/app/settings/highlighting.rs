use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    Anchor, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px, rems,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    dialog::Dialog,
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    switch::Switch,
    v_flex,
};
use rust_i18n::t;
use uuid::Uuid;

use crate::{
    TinyShell,
    app::{DialogKind, runtime_state::DialogToken},
    session::{
        config::HighlightRule,
        highlight_rules::{HighlightMatchKind, HighlightRuleStyle, HighlightTarget},
    },
    terminal::highlight::{
        HighlightCellStyle, HighlightRuleError, preview_match_ranges, runtime_style,
    },
};

const MAX_HIGHLIGHT_RULES: usize = 128;
const MAX_RULE_NAME_CHARS: usize = 64;
const MAX_RULE_PATTERN_CHARS: usize = 512;
const MAX_PREVIEW_CHARS: usize = 8_192;

fn highlight_dialog_dimensions(
    viewport_width: Pixels,
    viewport_height: Pixels,
) -> (Pixels, Pixels) {
    (
        px(800.).min((viewport_width - px(32.)).max(px(0.))),
        px(620.).min((viewport_height - px(32.)).max(px(0.))),
    )
}

fn highlight_dialog_uses_stacked_layout(viewport_width: Pixels) -> bool {
    viewport_width < px(680.)
}

const DEFAULT_HIGHLIGHT_PREVIEW: &str = "2026-08-27T14:05:09+08:00 INFO Connected to 10.0.0.8:22\n\
WARN latency above threshold\n\
ERROR authentication failed (HTTP 401)\n\
SUCCESS deploy completed; service HEALTHY\n\
GET https://example.com/runbook owner=ops@example.com\n\
peer [2001:db8::42] adapter 00:1A:2B:3C:4D:5E\n\
request=550e8400-e29b-41d4-a716-446655440000 path=/var/log/app.log\n\
backup completed\n\
token refreshed\n\
thread payload ready";

#[cfg(test)]
fn preview_contains_boundary_regressions(preview: &str) -> bool {
    ["backup", "token", "thread", "payload"]
        .iter()
        .all(|sample| preview.contains(sample))
}

fn new_rule() -> HighlightRule {
    HighlightRule {
        id: format!("custom-{}", Uuid::new_v4()),
        name: String::new(),
        enabled: true,
        pattern: String::new(),
        match_kind: HighlightMatchKind::Literal,
        case_sensitive: false,
        whole_word: true,
        target: HighlightTarget::Match,
        style: HighlightRuleStyle {
            foreground: Some("#6CB4EE".to_string()),
            background: None,
            bold: false,
            underline: false,
        },
    }
}

fn built_in_name_key(rule: &HighlightRule) -> Option<&'static str> {
    match (rule.id.as_str(), rule.name.as_str()) {
        ("builtin-critical", "Critical") => Some("highlight_builtin_critical"),
        ("builtin-error", "Error") => Some("highlight_builtin_error"),
        ("builtin-warning", "Warning") => Some("highlight_builtin_warning"),
        ("builtin-success", "Success") => Some("highlight_builtin_success"),
        ("builtin-info", "Info") => Some("highlight_builtin_info"),
        ("builtin-debug", "Debug") => Some("highlight_builtin_debug"),
        ("builtin-http-errors", "HTTP errors") => Some("highlight_builtin_http_errors"),
        ("builtin-http-method", "HTTP method") => Some("highlight_builtin_http_method"),
        ("builtin-url", "Web URL") => Some("highlight_builtin_url"),
        ("builtin-email", "Email address") => Some("highlight_builtin_email"),
        ("builtin-ipv4", "IPv4 address") => Some("highlight_builtin_ipv4"),
        ("builtin-mac", "MAC address") => Some("highlight_builtin_mac"),
        ("builtin-ipv6", "IPv6 address") => Some("highlight_builtin_ipv6"),
        ("builtin-uuid", "UUID") => Some("highlight_builtin_uuid"),
        ("builtin-timestamp", "ISO timestamp") => Some("highlight_builtin_timestamp"),
        ("builtin-file-path", "File path") => Some("highlight_builtin_file_path"),
        _ => None,
    }
}

fn display_rule_name(rule: &HighlightRule) -> String {
    built_in_name_key(rule)
        .map(|key| t!(key).to_string())
        .unwrap_or_else(|| rule.name.clone())
}

fn highlight_rule_has_conflict(
    baseline: Option<&HighlightRule>,
    current: Option<&HighlightRule>,
) -> bool {
    baseline != current
}

fn clear_highlight_rules_dialog_window<T: PartialEq>(tracked: &mut Option<T>, closing: &T) -> bool {
    if tracked.as_ref() != Some(closing) {
        return false;
    }
    *tracked = None;
    true
}

fn localized_rule_error(error: HighlightRuleError) -> String {
    match error {
        HighlightRuleError::EmptyPattern => t!("highlight_rule_error_empty_pattern").to_string(),
        HighlightRuleError::PatternTooLong { max } => {
            t!("highlight_rule_error_pattern_too_long", count = max).to_string()
        }
        HighlightRuleError::InvalidRegex { detail } => {
            t!("highlight_rule_error_invalid_regex", error = detail).to_string()
        }
        HighlightRuleError::EmptyMatch => t!("highlight_rule_error_empty_match").to_string(),
        HighlightRuleError::InvalidColor { value } => {
            t!("highlight_rule_error_invalid_color", value = value).to_string()
        }
    }
}

#[derive(Debug)]
struct DraftEvaluation {
    style: HighlightCellStyle,
    ranges: Vec<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewSegment {
    text: String,
    matched: bool,
}

fn split_preview_lines(text: &str, ranges: &[Range<usize>]) -> Vec<Vec<PreviewSegment>> {
    let mut result = Vec::new();
    let mut line_start = 0;

    for line in text.split('\n') {
        let line_end = line_start + line.len();
        let mut cursor = line_start;
        let mut segments = Vec::new();

        for range in ranges {
            let start = range.start.max(line_start).min(line_end);
            let end = range.end.max(line_start).min(line_end);
            if start >= end || end <= cursor {
                continue;
            }
            if start > cursor {
                segments.push(PreviewSegment {
                    text: text[cursor..start].to_string(),
                    matched: false,
                });
            }
            let matched_start = start.max(cursor);
            segments.push(PreviewSegment {
                text: text[matched_start..end].to_string(),
                matched: true,
            });
            cursor = end;
        }
        if cursor < line_end {
            segments.push(PreviewSegment {
                text: text[cursor..line_end].to_string(),
                matched: false,
            });
        }
        if segments.is_empty() {
            segments.push(PreviewSegment {
                text: if line.is_empty() { " " } else { line }.to_string(),
                matched: false,
            });
        }
        result.push(segments);
        line_start = line_end.saturating_add(1);
    }

    result
}

pub(crate) struct HighlightRulesManager {
    owner: Entity<TinyShell>,
    dialog_token: Rc<Cell<Option<DialogToken>>>,
    selected_id: Option<String>,
    draft_id: String,
    baseline_rule: HighlightRule,
    baseline_persisted: bool,
    enabled: bool,
    match_kind: HighlightMatchKind,
    case_sensitive: bool,
    whole_word: bool,
    target: HighlightTarget,
    bold: bool,
    underline: bool,
    reset_armed: bool,
    delete_armed: bool,
    saved: bool,
    unsaved_warning: bool,
    name_input: Entity<InputState>,
    pattern_input: Entity<InputState>,
    foreground_input: Entity<InputState>,
    background_input: Entity<InputState>,
    preview_input: Entity<InputState>,
    _owner_subscription: gpui::Subscription,
    _input_subscriptions: Vec<gpui::Subscription>,
}

impl HighlightRulesManager {
    fn new(
        owner: Entity<TinyShell>,
        dialog_token: Rc<Cell<Option<DialogToken>>>,
        initial_rule: HighlightRule,
        selected_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("highlight_rule_name_placeholder").to_string())
                .default_value(initial_rule.name.clone())
        });
        let pattern_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("highlight_rule_pattern_placeholder").to_string())
                .default_value(initial_rule.pattern.clone())
        });
        let foreground_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial_rule.style.foreground.as_deref().unwrap_or_default())
        });
        let background_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial_rule.style.background.as_deref().unwrap_or_default())
        });
        let preview_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .default_value(DEFAULT_HIGHLIGHT_PREVIEW)
        });

        let owner_subscription = cx.observe(&owner, |_, _, cx| cx.notify());
        let mut input_subscriptions = [
            name_input.clone(),
            pattern_input.clone(),
            foreground_input.clone(),
            background_input.clone(),
        ]
        .into_iter()
        .map(|input| {
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.saved = false;
                    this.reset_armed = false;
                    this.delete_armed = false;
                    cx.notify();
                }
            })
        })
        .collect::<Vec<_>>();
        input_subscriptions.push(cx.subscribe_in(
            &preview_input,
            window,
            |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.reset_armed = false;
                    this.delete_armed = false;
                }
                cx.notify();
            },
        ));
        let baseline_persisted = selected_id.is_some();

        Self {
            owner,
            dialog_token,
            selected_id,
            draft_id: initial_rule.id.clone(),
            baseline_rule: initial_rule.clone(),
            baseline_persisted,
            enabled: initial_rule.enabled,
            match_kind: initial_rule.match_kind,
            case_sensitive: initial_rule.case_sensitive,
            whole_word: initial_rule.whole_word,
            target: initial_rule.target,
            bold: initial_rule.style.bold,
            underline: initial_rule.style.underline,
            reset_armed: false,
            delete_armed: false,
            saved: false,
            unsaved_warning: false,
            name_input,
            pattern_input,
            foreground_input,
            background_input,
            preview_input,
            _owner_subscription: owner_subscription,
            _input_subscriptions: input_subscriptions,
        }
    }

    fn set_input(
        input: &Entity<InputState>,
        value: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.into();
        input.update(cx, |input, cx| input.set_value(value, window, cx));
    }

    fn load_rule(
        &mut self,
        rule: HighlightRule,
        selected: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        Self::set_input(&self.name_input, rule.name.clone(), window, cx);
        Self::set_input(&self.pattern_input, rule.pattern.clone(), window, cx);
        Self::set_input(
            &self.foreground_input,
            rule.style.foreground.clone().unwrap_or_default(),
            window,
            cx,
        );
        Self::set_input(
            &self.background_input,
            rule.style.background.clone().unwrap_or_default(),
            window,
            cx,
        );
        self.selected_id = selected.then(|| rule.id.clone());
        self.draft_id = rule.id.clone();
        self.enabled = rule.enabled;
        self.match_kind = rule.match_kind;
        self.case_sensitive = rule.case_sensitive;
        self.whole_word = rule.whole_word;
        self.target = rule.target;
        self.bold = rule.style.bold;
        self.underline = rule.style.underline;
        self.baseline_rule = rule;
        self.baseline_persisted = selected;
        self.reset_armed = false;
        self.delete_armed = false;
        self.saved = false;
        self.unsaved_warning = false;
        cx.notify();
    }

    fn select_rule(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_id.as_deref() == Some(id) {
            if self.has_external_conflict(cx)
                && self.allow_draft_replacement(cx)
                && let Some(current) = self.current_persisted_rule(cx)
            {
                self.load_rule(current, true, window, cx);
            }
            return;
        }
        if !self.allow_draft_replacement(cx) {
            return;
        }
        let rule = self
            .owner
            .read(cx)
            .config
            .highlight_rules()
            .iter()
            .find(|rule| rule.id == id)
            .cloned();
        if let Some(rule) = rule {
            self.load_rule(rule, true, window, cx);
        }
    }

    fn start_new_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.owner.read(cx).config.highlight_rules().len() >= MAX_HIGHLIGHT_RULES {
            return;
        }
        if !self.allow_draft_replacement(cx) {
            return;
        }
        self.load_rule(new_rule(), false, window, cx);
    }

    fn draft_snapshot(&self, cx: &App) -> HighlightRule {
        let foreground = self.foreground_input.read(cx).value().trim().to_string();
        let background = self.background_input.read(cx).value().trim().to_string();

        HighlightRule {
            id: self.draft_id.clone(),
            name: self.name_input.read(cx).value().trim().to_string(),
            enabled: self.enabled,
            pattern: self.pattern_input.read(cx).value().to_string(),
            match_kind: self.match_kind,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            target: self.target,
            style: HighlightRuleStyle {
                foreground: (!foreground.is_empty()).then_some(foreground),
                background: (!background.is_empty()).then_some(background),
                bold: self.bold,
                underline: self.underline,
            },
        }
    }

    fn draft_is_dirty(&self, cx: &App) -> bool {
        self.draft_snapshot(cx) != self.baseline_rule
    }

    fn current_persisted_rule(&self, cx: &App) -> Option<HighlightRule> {
        self.owner
            .read(cx)
            .config
            .highlight_rules()
            .iter()
            .find(|rule| rule.id == self.draft_id)
            .cloned()
    }

    fn has_external_conflict(&self, cx: &App) -> bool {
        let current = self.current_persisted_rule(cx);
        let baseline = self.baseline_persisted.then_some(&self.baseline_rule);
        highlight_rule_has_conflict(baseline, current.as_ref())
    }

    fn allow_draft_replacement(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.draft_is_dirty(cx) {
            return true;
        }
        self.unsaved_warning = true;
        self.saved = false;
        cx.notify();
        false
    }

    fn draft_rule(&self, cx: &App) -> Result<HighlightRule, String> {
        let rule = self.draft_snapshot(cx);
        if rule.name.is_empty() {
            return Err(t!("highlight_rule_validation_name").to_string());
        }
        if rule.name.chars().count() > MAX_RULE_NAME_CHARS {
            return Err(t!(
                "highlight_rule_validation_name_long",
                count = MAX_RULE_NAME_CHARS
            )
            .to_string());
        }

        if rule.pattern.is_empty() {
            return Err(t!("highlight_rule_validation_pattern").to_string());
        }
        if rule.pattern.chars().count() > MAX_RULE_PATTERN_CHARS {
            return Err(t!(
                "highlight_rule_validation_pattern_long",
                count = MAX_RULE_PATTERN_CHARS
            )
            .to_string());
        }

        if rule.style.foreground.is_none()
            && rule.style.background.is_none()
            && !rule.style.bold
            && !rule.style.underline
        {
            return Err(t!("highlight_rule_validation_style").to_string());
        }

        Ok(rule)
    }

    fn evaluate_draft(&self, cx: &App) -> Result<DraftEvaluation, String> {
        let rule = self.draft_rule(cx)?;
        let style = runtime_style(&rule.style).map_err(localized_rule_error)?;
        let preview = self.preview_input.read(cx).value().to_string();
        if preview.chars().count() > MAX_PREVIEW_CHARS {
            return Err(t!(
                "highlight_rule_validation_preview_long",
                count = MAX_PREVIEW_CHARS
            )
            .to_string());
        }
        let mut preview_rule = rule;
        preview_rule.enabled = true;
        let ranges = preview_match_ranges(&preview, &preview_rule).map_err(localized_rule_error)?;
        Ok(DraftEvaluation { style, ranges })
    }

    fn save_rule(&mut self, cx: &mut Context<Self>) {
        if self.evaluate_draft(cx).is_err() {
            return;
        }
        if self.has_external_conflict(cx) {
            self.unsaved_warning = true;
            self.saved = false;
            cx.notify();
            return;
        }
        let Ok(rule) = self.draft_rule(cx) else {
            return;
        };
        let rules = self.owner.read(cx).config.highlight_rules();
        let updates_existing = rules.iter().any(|existing| existing.id == rule.id);
        if !updates_existing && rules.len() >= MAX_HIGHLIGHT_RULES {
            return;
        }
        let id = rule.id.clone();
        let baseline_rule = rule.clone();
        self.owner.update(cx, |this, cx| {
            this.config.upsert_highlight_rule(rule);
            this.mark_config_preferences_dirty();
            cx.notify();
        });
        self.selected_id = Some(id);
        self.baseline_rule = baseline_rule;
        self.baseline_persisted = true;
        self.saved = true;
        self.unsaved_warning = false;
        self.delete_armed = false;
        self.reset_armed = false;
        cx.notify();
    }

    fn set_master_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.owner.update(cx, |this, cx| {
            this.config.set_keyword_highlight(enabled);
            this.mark_config_preferences_dirty();
            cx.notify();
        });
        self.reset_armed = false;
        self.delete_armed = false;
        cx.notify();
    }

    fn set_rule_enabled(&mut self, id: &str, enabled: bool, cx: &mut Context<Self>) {
        self.owner.update(cx, |this, cx| {
            let mut rules = this.config.highlight_rules().to_vec();
            if let Some(rule) = rules.iter_mut().find(|rule| rule.id == id) {
                rule.enabled = enabled;
                this.config.replace_highlight_rules(rules);
                this.mark_config_preferences_dirty();
                cx.notify();
            }
        });
        if self.draft_id == id {
            self.enabled = enabled;
            self.baseline_rule.enabled = enabled;
        }
        self.reset_armed = false;
        self.delete_armed = false;
        cx.notify();
    }

    fn move_selected(&mut self, offset: isize, cx: &mut Context<Self>) {
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.owner.update(cx, |this, cx| {
            let Some(index) = this
                .config
                .highlight_rules()
                .iter()
                .position(|rule| rule.id == id)
            else {
                return;
            };
            let target = index.saturating_add_signed(offset);
            if this.config.move_highlight_rule(&id, target) {
                this.mark_config_preferences_dirty();
                cx.notify();
            }
        });
        self.reset_armed = false;
        self.delete_armed = false;
        cx.notify();
    }

    fn toggle_delete_confirmation(&mut self, cx: &mut Context<Self>) {
        if self.selected_id.is_none() {
            return;
        }
        if !self.allow_draft_replacement(cx) {
            return;
        }
        self.delete_armed = !self.delete_armed;
        self.reset_armed = false;
        cx.notify();
    }

    fn confirm_delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.delete_armed || !self.allow_draft_replacement(cx) {
            return;
        }
        let Some(id) = self.selected_id.clone() else {
            return;
        };
        self.delete_armed = false;

        let (next_index, remaining) = self.owner.update(cx, |this, cx| {
            let index = this
                .config
                .highlight_rules()
                .iter()
                .position(|rule| rule.id == id)
                .unwrap_or(0);
            if this.config.remove_highlight_rule(&id) {
                this.mark_config_preferences_dirty();
                cx.notify();
            }
            (index, this.config.highlight_rules().to_vec())
        });
        if let Some(rule) = remaining
            .get(next_index.min(remaining.len().saturating_sub(1)))
            .cloned()
        {
            self.load_rule(rule, true, window, cx);
        } else {
            self.load_rule(new_rule(), false, window, cx);
        }
    }

    fn toggle_restore_confirmation(&mut self, cx: &mut Context<Self>) {
        if !self.allow_draft_replacement(cx) {
            return;
        }
        self.reset_armed = !self.reset_armed;
        self.delete_armed = false;
        cx.notify();
    }

    fn confirm_restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.reset_armed || !self.allow_draft_replacement(cx) {
            return;
        }
        self.reset_armed = false;

        let rules = self.owner.update(cx, |this, cx| {
            this.config.reset_highlight_rules();
            this.mark_config_preferences_dirty();
            cx.notify();
            this.config.highlight_rules().to_vec()
        });
        if let Some(rule) = rules.first().cloned() {
            self.load_rule(rule, true, window, cx);
        }
    }

    fn set_match_kind(&mut self, match_kind: HighlightMatchKind, cx: &mut Context<Self>) {
        self.match_kind = match_kind;
        self.saved = false;
        self.reset_armed = false;
        self.delete_armed = false;
        cx.notify();
    }

    fn set_target(&mut self, target: HighlightTarget, cx: &mut Context<Self>) {
        self.target = target;
        self.saved = false;
        self.reset_armed = false;
        self.delete_armed = false;
        cx.notify();
    }

    fn discard_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.baseline_persisted {
            if let Some(current) = self.current_persisted_rule(cx) {
                self.load_rule(current, true, window, cx);
            } else {
                self.load_rule(new_rule(), false, window, cx);
            }
            return;
        }
        let baseline = self.baseline_rule.clone();
        let persisted = self.baseline_persisted;
        self.load_rule(baseline, persisted, window, cx);
    }

    fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.allow_draft_replacement(cx) {
            return;
        }
        let Some(token) = self.dialog_token.get() else {
            return;
        };
        let dialog_window = window.window_handle();
        self.owner.update(cx, |this, cx| {
            if this.dismiss_modal_dialog(token, window, cx) {
                clear_highlight_rules_dialog_window(
                    &mut this.highlight_rules_dialog_window,
                    &dialog_window,
                );
            }
            cx.notify();
        });
    }
}

fn render_preview_result(
    text: &str,
    ranges: &[Range<usize>],
    style: HighlightCellStyle,
    cx: &App,
) -> gpui::Div {
    v_flex()
        .w_full()
        .gap_1()
        .children(
            split_preview_lines(text, ranges)
                .into_iter()
                .map(|segments| {
                    h_flex()
                        .min_h(px(20.))
                        .items_center()
                        .whitespace_nowrap()
                        .children(segments.into_iter().map(|segment| {
                            div()
                                .when(segment.matched, |this| {
                                    this.when_some(style.foreground, |this, color| {
                                        this.text_color(color)
                                    })
                                    .when_some(style.background, |this, color| this.bg(color))
                                    .when(style.bold, |this| this.font_weight(FontWeight::BOLD))
                                    .when(style.underline, |this| this.underline())
                                })
                                .child(segment.text)
                        }))
                }),
        )
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(rems(0.78))
}

impl Render for HighlightRulesManager {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        let manager = cx.entity();
        let stacked_layout = highlight_dialog_uses_stacked_layout(window.viewport_size().width);
        let rules = self.owner.read(cx).config.highlight_rules().to_vec();
        let master_enabled = self.owner.read(cx).config.keyword_highlight();
        let enabled_count = rules.iter().filter(|rule| rule.enabled).count();
        let selected_index = self
            .selected_id
            .as_deref()
            .and_then(|selected| rules.iter().position(|rule| rule.id.as_str() == selected));
        let can_move_up = selected_index.is_some_and(|index| index > 0);
        let can_move_down = selected_index.is_some_and(|index| index + 1 < rules.len());
        let selected_exists = selected_index.is_some();
        let at_rule_limit = rules.len() >= MAX_HIGHLIGHT_RULES;
        let draft_dirty = self.draft_is_dirty(cx);
        let external_conflict = self.has_external_conflict(cx);

        let evaluation = self.evaluate_draft(cx);
        let can_apply =
            evaluation.is_ok() && !external_conflict && (selected_exists || !at_rule_limit);
        let (preview_style, preview_ranges, validation_error) = match evaluation {
            Ok(evaluation) => (evaluation.style, evaluation.ranges, None),
            Err(error) => (HighlightCellStyle::default(), Vec::new(), Some(error)),
        };
        let preview_text = self.preview_input.read(cx).value().to_string();

        let restore_label = if self.reset_armed {
            t!("cancel").to_string()
        } else {
            t!("highlight_rules_restore").to_string()
        };
        let restore_button = Button::new("restore-highlight-rules")
            .small()
            .secondary()
            .label(restore_label)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_restore_confirmation(cx);
            }));
        let restore_confirm_button = self.reset_armed.then(|| {
            Button::new("confirm-restore-highlight-rules")
                .small()
                .warning()
                .label(t!("highlight_rules_restore_confirm_action").to_string())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.confirm_restore_defaults(window, cx);
                }))
        });

        let list_hint = if at_rule_limit {
            t!("highlight_rules_limit", count = MAX_HIGHLIGHT_RULES).to_string()
        } else {
            t!("highlight_rules_priority_hint").to_string()
        };
        let rule_list =
            v_flex()
                .flex_none()
                .min_h(px(0.))
                .border_color(cx.theme().border)
                .when(stacked_layout, |this| {
                    this.w_full().h(px(176.)).border_b_1()
                })
                .when(!stacked_layout, |this| this.w(px(248.)).border_r_1())
                .child(
                    v_flex()
                        .flex_none()
                        .gap_2()
                        .p_3()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            Button::new("new-highlight-rule")
                                .small()
                                .primary()
                                .icon(IconName::Plus)
                                .label(t!("highlight_rules_add").to_string())
                                .disabled(at_rule_limit)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_new_rule(window, cx);
                                })),
                        )
                        .child(restore_button)
                        .children(restore_confirm_button)
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(list_hint),
                        ),
                )
                .child(
                    v_flex()
                        .id("highlight-rules-list")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scrollbar()
                        .when(rules.is_empty(), |this| {
                            this.child(
                                div()
                                    .p_3()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("highlight_rules_empty").to_string()),
                            )
                        })
                        .children(rules.clone().into_iter().enumerate().map(|(index, rule)| {
                            let row_id = rule.id.clone();
                            let edit_id = rule.id.clone();
                            let toggle_id = rule.id.clone();
                            let selected = self.selected_id.as_deref() == Some(rule.id.as_str());
                            let swatch = runtime_style(&rule.style)
                                .ok()
                                .and_then(|style| style.foreground.or(style.background))
                                .unwrap_or(cx.theme().muted_foreground);

                            h_flex()
                                .id(SharedString::from(format!("highlight-rule-row-{index}")))
                                .min_h(px(48.))
                                .px_2()
                                .py_1()
                                .gap_2()
                                .items_center()
                                .cursor_pointer()
                                .border_b_1()
                                .border_color(cx.theme().border.opacity(0.55))
                                .when(selected, |this| this.bg(cx.theme().selection.opacity(0.72)))
                                .hover(|this| this.bg(cx.theme().secondary.opacity(0.65)))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_rule(&row_id, window, cx);
                                }))
                                .child(div().size(px(8.)).flex_none().rounded_full().bg(swatch))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::MEDIUM)
                                                .truncate()
                                                .child(display_rule_name(&rule)),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .text_color(cx.theme().muted_foreground)
                                                .truncate()
                                                .child(rule.pattern.clone()),
                                        ),
                                )
                                .child(
                                    Button::new(SharedString::from(format!(
                                        "highlight-rule-edit-{index}"
                                    )))
                                    .small()
                                    .ghost()
                                    .icon(IconName::Settings)
                                    .tooltip(t!("edit").to_string())
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.select_rule(&edit_id, window, cx);
                                    })),
                                )
                                .child(
                                    Switch::new(SharedString::from(format!(
                                        "highlight-rule-toggle-{index}"
                                    )))
                                    .small()
                                    .checked(rule.enabled)
                                    .on_click(cx.listener(move |this, checked, _, cx| {
                                        cx.stop_propagation();
                                        this.set_rule_enabled(&toggle_id, *checked, cx);
                                    })),
                                )
                        })),
                );

        let current_match_kind = self.match_kind;
        let match_kind_label = match current_match_kind {
            HighlightMatchKind::Literal => t!("highlight_rule_match_literal").to_string(),
            HighlightMatchKind::Regex => t!("highlight_rule_match_regex").to_string(),
        };
        let match_manager = manager.clone();
        let match_kind_button = Button::new("highlight-match-kind")
            .small()
            .secondary()
            .w_full()
            .icon(IconName::ChevronsUpDown)
            .label(match_kind_label)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, _| {
                let literal_manager = match_manager.clone();
                let regex_manager = match_manager.clone();
                menu.min_w(190.)
                    .item(
                        PopupMenuItem::new(t!("highlight_rule_match_literal").to_string())
                            .checked(current_match_kind == HighlightMatchKind::Literal)
                            .on_click(window.listener_for(&literal_manager, |this, _, _, cx| {
                                this.set_match_kind(HighlightMatchKind::Literal, cx);
                            })),
                    )
                    .item(
                        PopupMenuItem::new(t!("highlight_rule_match_regex").to_string())
                            .checked(current_match_kind == HighlightMatchKind::Regex)
                            .on_click(window.listener_for(&regex_manager, |this, _, _, cx| {
                                this.set_match_kind(HighlightMatchKind::Regex, cx);
                            })),
                    )
            });

        let current_target = self.target;
        let target_label = match current_target {
            HighlightTarget::Match => t!("highlight_rule_target_match").to_string(),
            HighlightTarget::Line => t!("highlight_rule_target_line").to_string(),
        };
        let target_manager = manager.clone();
        let target_button = Button::new("highlight-target")
            .small()
            .secondary()
            .w_full()
            .icon(IconName::ChevronsUpDown)
            .label(target_label)
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, window, _| {
                let match_manager = target_manager.clone();
                let line_manager = target_manager.clone();
                menu.min_w(190.)
                    .item(
                        PopupMenuItem::new(t!("highlight_rule_target_match").to_string())
                            .checked(current_target == HighlightTarget::Match)
                            .on_click(window.listener_for(&match_manager, |this, _, _, cx| {
                                this.set_target(HighlightTarget::Match, cx);
                            })),
                    )
                    .item(
                        PopupMenuItem::new(t!("highlight_rule_target_line").to_string())
                            .checked(current_target == HighlightTarget::Line)
                            .on_click(window.listener_for(&line_manager, |this, _, _, cx| {
                                this.set_target(HighlightTarget::Line, cx);
                            })),
                    )
            });

        let identity_fields = v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().child(t!("highlight_rule_name").to_string()))
                    .child(Input::new(&self.name_input).small().w_full()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .child(t!("highlight_rule_pattern").to_string()),
                    )
                    .child(Input::new(&self.pattern_input).small().w_full())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("highlight_rule_pattern_hint").to_string()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("highlight_rule_match_kind").to_string()),
                            )
                            .child(match_kind_button),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("highlight_rule_target").to_string()),
                            )
                            .child(target_button),
                    ),
            );

        let behavior_toggles = h_flex()
            .items_center()
            .flex_wrap()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Switch::new("highlight-rule-enabled")
                            .small()
                            .checked(self.enabled)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.enabled = *checked;
                                this.saved = false;
                                this.reset_armed = false;
                                this.delete_armed = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(t!("highlight_rule_enabled").to_string()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Switch::new("highlight-rule-case")
                            .small()
                            .checked(self.case_sensitive)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.case_sensitive = *checked;
                                this.saved = false;
                                this.reset_armed = false;
                                this.delete_armed = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(t!("highlight_rule_case_sensitive").to_string()),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Switch::new("highlight-rule-whole-word")
                            .small()
                            .checked(self.whole_word)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.whole_word = *checked;
                                this.saved = false;
                                this.reset_armed = false;
                                this.delete_armed = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(t!("highlight_rule_whole_word").to_string()),
                    ),
            );

        let style_fields = v_flex()
            .gap_3()
            .child(
                div()
                    .pt_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("highlight_rule_style").to_string()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("highlight_rule_foreground").to_string()),
                            )
                            .child(Input::new(&self.foreground_input).small().w_full()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(160.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("highlight_rule_background").to_string()),
                            )
                            .child(Input::new(&self.background_input).small().w_full()),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .flex_wrap()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("highlight_rule_color_hint").to_string()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Switch::new("highlight-rule-bold")
                                    .small()
                                    .checked(self.bold)
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        this.bold = *checked;
                                        this.saved = false;
                                        this.reset_armed = false;
                                        this.delete_armed = false;
                                        cx.notify();
                                    })),
                            )
                            .child(div().text_sm().child(t!("highlight_rule_bold").to_string())),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Switch::new("highlight-rule-underline")
                                    .small()
                                    .checked(self.underline)
                                    .on_click(cx.listener(|this, checked, _, cx| {
                                        this.underline = *checked;
                                        this.saved = false;
                                        this.reset_armed = false;
                                        this.delete_armed = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("highlight_rule_underline").to_string()),
                            ),
                    ),
            );

        let match_status = if preview_ranges.is_empty() {
            t!("highlight_rule_preview_no_match").to_string()
        } else {
            t!(
                "highlight_rule_preview_matches",
                count = preview_ranges.len()
            )
            .to_string()
        };
        let preview = v_flex()
            .gap_3()
            .child(
                div()
                    .pt_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("highlight_rule_preview").to_string()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .child(t!("highlight_rule_preview_sample").to_string()),
                    )
                    .child(Input::new(&self.preview_input).h(px(92.)).w_full()),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .child(t!("highlight_rule_preview_result").to_string()),
                            )
                            .when(validation_error.is_none(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(if preview_ranges.is_empty() {
                                            cx.theme().muted_foreground
                                        } else {
                                            cx.theme().success
                                        })
                                        .child(match_status),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_h(px(108.))
                            .p_2()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().muted.opacity(0.22))
                            .overflow_hidden()
                            .child(render_preview_result(
                                &preview_text,
                                &preview_ranges,
                                preview_style,
                                cx,
                            )),
                    ),
            )
            .when_some(validation_error.clone(), |this, error| {
                this.child(div().text_xs().text_color(cx.theme().danger).child(error))
            });

        let delete_label = if self.delete_armed {
            t!("cancel").to_string()
        } else {
            t!("highlight_rule_delete").to_string()
        };
        let delete_button = Button::new("delete-highlight-rule")
            .small()
            .secondary()
            .icon(IconName::Delete)
            .label(delete_label)
            .disabled(!selected_exists)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_delete_confirmation(cx);
            }));
        let delete_confirm_button = self.delete_armed.then(|| {
            Button::new("confirm-delete-highlight-rule")
                .small()
                .danger()
                .label(t!("confirm_delete").to_string())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.confirm_delete_selected(window, cx);
                }))
        });

        let actions = h_flex()
            .justify_end()
            .flex_wrap()
            .gap_2()
            .pt_1()
            .child(
                Button::new("highlight-rule-up")
                    .small()
                    .secondary()
                    .icon(IconName::ChevronUp)
                    .label(t!("highlight_rule_move_up").to_string())
                    .disabled(!can_move_up)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.move_selected(-1, cx);
                    })),
            )
            .child(
                Button::new("highlight-rule-down")
                    .small()
                    .secondary()
                    .icon(IconName::ChevronDown)
                    .label(t!("highlight_rule_move_down").to_string())
                    .disabled(!can_move_down)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.move_selected(1, cx);
                    })),
            )
            .child(delete_button)
            .children(delete_confirm_button)
            .child(
                Button::new("apply-highlight-rule")
                    .small()
                    .primary()
                    .icon(IconName::Check)
                    .label(t!("highlight_rule_apply").to_string())
                    .disabled(!can_apply)
                    .on_click(cx.listener(|this, _, _, cx| this.save_rule(cx))),
            );

        let editor_title = if let Some(index) = selected_index {
            format!(
                "{}  {}/{}",
                t!("highlight_rule_editor"),
                index + 1,
                rules.len()
            )
        } else {
            t!("highlight_rule_new").to_string()
        };
        let editor = v_flex()
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .flex_wrap()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(editor_title),
                    )
                    .when(self.saved, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().success)
                                .child(t!("highlight_rule_saved").to_string()),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("highlight-rule-editor-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .p_3()
                    .gap_3()
                    .child(identity_fields)
                    .child(behavior_toggles)
                    .child(style_fields)
                    .child(preview)
                    .child(actions),
            );

        let footer = h_flex()
            .flex_none()
            .items_center()
            .flex_wrap()
            .gap_2()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w(px(180.))
                    .text_xs()
                    .text_color(
                        if external_conflict || self.unsaved_warning && draft_dirty {
                            cx.theme().warning
                        } else {
                            cx.theme().muted_foreground
                        },
                    )
                    .child(if external_conflict {
                        t!("highlight_rule_external_conflict").to_string()
                    } else if self.unsaved_warning && draft_dirty {
                        t!("highlight_rule_unsaved_warning").to_string()
                    } else if draft_dirty {
                        t!("highlight_rule_unsaved_status").to_string()
                    } else {
                        t!("highlight_rule_changes_applied").to_string()
                    }),
            )
            .child(
                Button::new("discard-highlight-rule-draft")
                    .small()
                    .secondary()
                    .label(if external_conflict {
                        t!("highlight_rule_reload").to_string()
                    } else {
                        t!("highlight_rule_discard").to_string()
                    })
                    .disabled(!draft_dirty && !external_conflict)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.discard_draft(window, cx);
                    })),
            )
            .child(
                Button::new("close-highlight-rules")
                    .small()
                    .primary()
                    .label(t!("highlight_rules_done").to_string())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.request_close(window, cx);
                    })),
            );

        v_flex()
            .size_full()
            .min_h(px(0.))
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("highlight_rules_master").to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("highlight_rules_master_hint").to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                t!("highlight_rules_enabled_count", count = enabled_count)
                                    .to_string(),
                            ),
                    )
                    .child(
                        Switch::new("highlight-rules-master")
                            .small()
                            .checked(master_enabled)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                this.set_master_enabled(*checked, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .items_stretch()
                    .when(stacked_layout, |this| this.flex_col())
                    .when(!stacked_layout, |this| this.flex_row())
                    .child(rule_list)
                    .child(editor),
            )
            .child(footer)
    }
}

impl TinyShell {
    pub(crate) fn open_highlight_rules_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(existing_window) = self.highlight_rules_dialog_window {
            if existing_window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
            self.highlight_rules_dialog_window = None;
        }

        let owner = cx.entity();
        let initial_rule = self
            .config
            .highlight_rules()
            .first()
            .cloned()
            .unwrap_or_else(new_rule);
        let selected_id = self
            .config
            .highlight_rules()
            .first()
            .map(|rule| rule.id.clone());
        let dialog_token = Rc::new(Cell::new(None));
        let manager_token = dialog_token.clone();
        let manager = cx.new(|cx| {
            HighlightRulesManager::new(
                owner.clone(),
                manager_token,
                initial_rule,
                selected_id,
                window,
                cx,
            )
        });
        let dialog_window = window.window_handle();
        self.highlight_rules_dialog_window = Some(dialog_window);
        self.open_modal_dialog(
            DialogKind::HighlightRules,
            window,
            cx,
            move |dialog: Dialog, token, window, _cx| {
                dialog_token.set(Some(token));
                let content_manager = manager.clone();
                let cancel_manager = manager.clone();
                let viewport = window.viewport_size();
                let (dialog_width, dialog_height) =
                    highlight_dialog_dimensions(viewport.width, viewport.height);
                dialog
                    .title(t!("highlight_rules_dialog_title").to_string())
                    .w(dialog_width)
                    .h(dialog_height)
                    .margin_top(px(16.))
                    .close_button(false)
                    .overlay_closable(false)
                    .on_cancel(move |_, window, cx| {
                        cancel_manager.update(cx, |this, cx| {
                            this.request_close(window, cx);
                        });
                        false
                    })
                    .on_ok(|_, _, _| false)
                    .on_close({
                        let owner = owner.clone();
                        move |_, window, cx| {
                            owner.update(cx, |this, cx| {
                                this.modal_dialog_closed(token, window, cx);
                                clear_highlight_rules_dialog_window(
                                    &mut this.highlight_rules_dialog_window,
                                    &dialog_window,
                                );
                                cx.notify();
                            });
                        }
                    })
                    .content(move |content, _window, _cx| content.child(content_manager.clone()))
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::session::config::default_highlight_rules;

    use super::*;

    #[test]
    fn default_preview_exercises_known_substring_regressions() {
        assert!(preview_contains_boundary_regressions(
            DEFAULT_HIGHLIGHT_PREVIEW
        ));
    }

    #[test]
    fn highlight_dialog_fits_the_minimum_settings_window() {
        assert_eq!(
            highlight_dialog_dimensions(px(720.), px(520.)),
            (px(688.), px(488.))
        );
        assert_eq!(
            highlight_dialog_dimensions(px(1_200.), px(900.)),
            (px(800.), px(620.))
        );
        assert_eq!(
            highlight_dialog_dimensions(px(20.), px(20.)),
            (px(0.), px(0.))
        );
        assert!(highlight_dialog_uses_stacked_layout(px(679.)));
        assert!(!highlight_dialog_uses_stacked_layout(px(680.)));
    }

    #[test]
    fn dialog_window_lifecycle_only_clears_the_current_handle() {
        let mut tracked = Some(7_u64);

        assert!(!clear_highlight_rules_dialog_window(&mut tracked, &6));
        assert_eq!(tracked, Some(7));
        assert!(clear_highlight_rules_dialog_window(&mut tracked, &7));
        assert_eq!(tracked, None);
        // The framework on_close callback may run after a synchronous Done/Esc
        // close; cleanup must therefore remain idempotent.
        assert!(!clear_highlight_rules_dialog_window(&mut tracked, &7));

        tracked = Some(8);
        assert!(!clear_highlight_rules_dialog_window(&mut tracked, &7));
        assert_eq!(tracked, Some(8));
    }

    #[test]
    fn preview_segments_preserve_utf8_text_and_match_boundaries() {
        let text = "INFO 连接成功\nbackup completed";
        let start = text.find("连接").unwrap();
        let end = start + "连接".len();
        let matched_range = start..end;

        assert_eq!(
            split_preview_lines(text, std::slice::from_ref(&matched_range)),
            vec![
                vec![
                    PreviewSegment {
                        text: "INFO ".to_string(),
                        matched: false,
                    },
                    PreviewSegment {
                        text: "连接".to_string(),
                        matched: true,
                    },
                    PreviewSegment {
                        text: "成功".to_string(),
                        matched: false,
                    },
                ],
                vec![PreviewSegment {
                    text: "backup completed".to_string(),
                    matched: false,
                }],
            ]
        );
    }

    #[test]
    fn localized_builtin_name_only_applies_until_the_user_renames_it() {
        let rules = default_highlight_rules();
        assert!(rules.iter().all(|rule| built_in_name_key(rule).is_some()));

        let mut rule = rules[0].clone();
        assert_eq!(built_in_name_key(&rule), Some("highlight_builtin_critical"));

        rule.name = "Production crash".to_string();
        assert_eq!(built_in_name_key(&rule), None);
    }

    #[test]
    fn rule_conflicts_detect_external_changes_and_deletions() {
        let baseline = default_highlight_rules().remove(0);
        let mut changed = baseline.clone();
        changed.pattern.push_str("|ABORTED");

        assert!(!highlight_rule_has_conflict(
            Some(&baseline),
            Some(&baseline)
        ));
        assert!(highlight_rule_has_conflict(Some(&baseline), Some(&changed)));
        assert!(highlight_rule_has_conflict(Some(&baseline), None));
        assert!(highlight_rule_has_conflict(None, Some(&changed)));
        assert!(!highlight_rule_has_conflict(None, None));
    }

    #[test]
    fn conservative_success_rule_does_not_match_substrings_in_preview() {
        let rule = default_highlight_rules()
            .into_iter()
            .find(|rule| rule.id == "builtin-success")
            .unwrap();
        let ranges = preview_match_ranges(DEFAULT_HIGHLIGHT_PREVIEW, &rule).unwrap();
        let matches = ranges
            .into_iter()
            .map(|range| &DEFAULT_HIGHLIGHT_PREVIEW[range])
            .collect::<Vec<_>>();

        assert_eq!(
            matches,
            [
                "Connected",
                "SUCCESS",
                "completed",
                "HEALTHY",
                "completed",
                "ready"
            ]
        );
        assert!(!matches.iter().any(|matched| matches!(
            matched.to_ascii_lowercase().as_str(),
            "up" | "ok" | "read" | "load"
        )));
    }
}
