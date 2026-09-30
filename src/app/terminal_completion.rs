use crate::session::config::QuickCommandCategory;

const MIN_QUERY_CHARS: usize = 2;
const MAX_CANDIDATES: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCompletionCandidate {
    pub(crate) command: String,
    pub(crate) label: String,
    pub(crate) matched_prefix_bytes: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalCompletionState {
    input: String,
    input_uncertain: bool,
    candidates: Vec<TerminalCompletionCandidate>,
    selected: Option<usize>,
}

impl TerminalCompletionState {
    pub(crate) fn candidates(&self) -> &[TerminalCompletionCandidate] {
        &self.candidates
    }

    pub(crate) fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    pub(crate) fn is_visible(&self) -> bool {
        !self.candidates.is_empty()
    }

    pub(crate) fn push_text(&mut self, text: &str, categories: &[QuickCommandCategory]) {
        if self.input_uncertain {
            return;
        }
        self.input.push_str(text);
        self.refresh(categories);
    }

    pub(crate) fn backspace(&mut self, categories: &[QuickCommandCategory]) {
        if self.input_uncertain {
            return;
        }
        self.input.pop();
        self.refresh(categories);
    }

    pub(crate) fn move_selection(&mut self, offset: isize) {
        let count = self.candidates.len();
        if count == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match self.selected {
            Some(selected) => (selected as isize + offset).rem_euclid(count as isize) as usize,
            None if offset < 0 => count - 1,
            None => 0,
        });
    }

    pub(crate) fn select(&mut self, index: usize) {
        if index < self.candidates.len() {
            self.selected = Some(index);
        }
    }

    pub(crate) fn accept_selected_or_first(&mut self) -> Option<String> {
        if self.selected.is_none() && !self.candidates.is_empty() {
            self.selected = Some(0);
        }
        self.accept_selected()
    }

    pub(crate) fn accept_selected(&mut self) -> Option<String> {
        let candidate = self.candidates.get(self.selected?)?;
        let suffix = candidate.command[candidate.matched_prefix_bytes..].to_string();
        self.input.push_str(&suffix);
        self.candidates.clear();
        self.selected = None;
        Some(suffix)
    }

    pub(crate) fn dismiss(&mut self) {
        self.candidates.clear();
        self.selected = None;
    }

    pub(crate) fn clear(&mut self) {
        self.input.clear();
        self.input_uncertain = false;
        self.dismiss();
    }

    pub(crate) fn invalidate(&mut self) {
        self.clear();
        self.input_uncertain = true;
    }

    pub(crate) fn clear_line_prefix(&mut self) {
        // Ctrl+U only clears before the cursor. After an unknown cursor move,
        // an unseen suffix may remain, so it cannot establish an empty line.
        if !self.input_uncertain {
            self.clear();
        }
    }

    /// Only complete saved commands count; arbitrary input is never persisted.
    pub(crate) fn submit(&mut self, categories: &[QuickCommandCategory]) -> Vec<String> {
        if self.input_uncertain {
            self.clear();
            return Vec::new();
        }
        let input = self.input.trim();
        let ids = categories
            .iter()
            .flat_map(|category| &category.commands)
            .filter(|command| !input.is_empty() && command.command.trim() == input)
            .filter(|command| !contains_parameter_placeholder(&command.command))
            .map(|command| command.id.clone())
            .collect();
        self.clear();
        ids
    }

    fn refresh(&mut self, categories: &[QuickCommandCategory]) {
        self.candidates = matching_candidates(&self.input, categories);
        self.selected = None;
    }
}

fn matching_candidates(
    query: &str,
    categories: &[QuickCommandCategory],
) -> Vec<TerminalCompletionCandidate> {
    if query.chars().count() < MIN_QUERY_CHARS || query.chars().any(char::is_control) {
        return Vec::new();
    }

    let mut matches = categories
        .iter()
        .flat_map(|category| category.commands.iter())
        .filter(|command| !contains_parameter_placeholder(&command.command))
        .filter(|command| matched_prefix_bytes(&command.command, query).is_some())
        .collect::<Vec<_>>();
    // Stable sorting preserves configured order when frequencies are equal.
    matches.sort_by_key(|command| std::cmp::Reverse(command.usage.total()));
    matches
        .into_iter()
        .take(MAX_CANDIDATES)
        .filter_map(|command| {
            let matched_prefix_bytes = matched_prefix_bytes(&command.command, query)?;
            Some(TerminalCompletionCandidate {
                command: command.command.clone(),
                label: command.name.clone(),
                matched_prefix_bytes,
            })
        })
        .collect()
}

fn contains_parameter_placeholder(command: &str) -> bool {
    (1..=5).any(|index| command.contains(&format!("[p{index}]")))
}

fn matched_prefix_bytes(command: &str, query: &str) -> Option<usize> {
    let mut command_chars = command.chars();
    let mut matched_bytes = 0;

    for query_char in query.chars() {
        let command_char = command_chars.next()?;
        if !command_char.eq_ignore_ascii_case(&query_char) {
            return None;
        }
        matched_bytes += command_char.len_utf8();
    }

    Some(matched_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{QuickCommand, QuickCommandCategory};

    fn categories(commands: &[(&str, &str, &str)]) -> Vec<QuickCommandCategory> {
        vec![QuickCommandCategory {
            id: "category".into(),
            name: "常用".into(),
            commands: commands
                .iter()
                .enumerate()
                .map(|(index, (name, remark, command))| QuickCommand {
                    id: format!("command-{index}"),
                    name: (*name).into(),
                    remark: (*remark).into(),
                    command: (*command).into(),
                    usage: Default::default(),
                })
                .collect(),
        }]
    }

    #[test]
    fn requires_two_characters_before_matching() {
        let categories = categories(&[("列表", "列出目录", "ls")]);
        let mut state = TerminalCompletionState::default();

        state.push_text("l", &categories);
        assert!(!state.is_visible());

        state.push_text("s", &categories);
        assert_eq!(state.candidates()[0].command, "ls");
    }

    #[test]
    fn frequency_ranks_all_matches_before_limiting_and_preserves_ties() {
        let categories = categories(&[
            ("0", "", "docker a"),
            ("1", "", "docker b"),
            ("2", "", "docker c"),
            ("3", "", "docker d"),
            ("4", "", "docker e"),
            ("5", "", "docker f"),
            ("6", "", "docker g"),
            ("7", "", "docker h"),
        ]);
        let mut categories = categories;
        for (index, count) in [(7, 20), (6, 10), (2, 10)] {
            for _ in 0..count {
                categories[0].commands[index].usage.record("test");
            }
        }
        let candidates = matching_candidates("do", &categories);
        assert_eq!(
            candidates
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            vec!["7", "2", "6", "0", "1", "3"]
        );
    }

    #[test]
    fn matches_ascii_prefix_case_insensitively_and_uses_command_name() {
        let categories = categories(&[("Git 状态", "查看状态", "git status")]);
        let mut state = TerminalCompletionState::default();

        state.push_text("GI", &categories);

        assert_eq!(
            state.candidates(),
            &[TerminalCompletionCandidate {
                command: "git status".into(),
                label: "Git 状态".into(),
                matched_prefix_bytes: 2,
            }]
        );
    }

    #[test]
    fn uses_command_name_and_filters_parameter_templates() {
        let categories = categories(&[
            ("查看日志", "", "journalctl"),
            ("查看服务", "服务详情", "journalctl -u [p1]"),
        ]);
        let mut state = TerminalCompletionState::default();

        state.push_text("jo", &categories);

        assert_eq!(state.candidates().len(), 1);
        assert_eq!(state.candidates()[0].label, "查看日志");
    }

    #[test]
    fn candidates_start_unselected_and_wrap_after_explicit_navigation() {
        let commands = (0..8)
            .map(|index| (format!("命令 {index}"), String::new(), format!("ls{index}")))
            .collect::<Vec<_>>();
        let borrowed = commands
            .iter()
            .map(|(name, remark, command)| (name.as_str(), remark.as_str(), command.as_str()))
            .collect::<Vec<_>>();
        let categories = categories(&borrowed);
        let mut state = TerminalCompletionState::default();

        state.push_text("ls", &categories);
        assert_eq!(state.candidates().len(), MAX_CANDIDATES);
        assert_eq!(state.selected_index(), None);

        state.move_selection(-1);
        assert_eq!(state.selected_index(), Some(MAX_CANDIDATES - 1));
        state.move_selection(1);
        assert_eq!(state.selected_index(), Some(0));
        state.move_selection(1);
        assert_eq!(state.selected_index(), Some(1));
    }

    #[test]
    fn down_selects_first_candidate_from_unselected_state() {
        let categories = categories(&[("列表", "", "ls"), ("查看块设备", "", "lsblk")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("ls", &categories);

        state.move_selection(1);

        assert_eq!(state.selected_index(), Some(0));
    }

    #[test]
    fn accepting_requires_selection_and_returns_only_missing_suffix() {
        let categories = categories(&[("Git 状态", "", "git status")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("git", &categories);

        assert_eq!(state.accept_selected(), None);
        assert!(state.is_visible());

        state.move_selection(1);
        assert_eq!(state.accept_selected().as_deref(), Some(" status"));
        assert_eq!(state.input, "git status");
        assert!(!state.is_visible());
    }

    #[test]
    fn tab_acceptance_falls_back_to_first_candidate() {
        let categories = categories(&[("Docker", "", "docker ps")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("docker", &categories);

        assert_eq!(state.accept_selected_or_first().as_deref(), Some(" ps"));
    }

    #[test]
    fn appended_text_continues_matching_after_initial_chunk() {
        let categories = categories(&[("Docker", "", "docker ps")]);
        let mut state = TerminalCompletionState::default();

        state.push_text("do", &categories);
        state.push_text("cker", &categories);

        assert_eq!(state.candidates()[0].command, "docker ps");
        assert_eq!(state.candidates()[0].matched_prefix_bytes, "docker".len());
        assert_eq!(state.selected_index(), None);
    }

    #[test]
    fn paste_click_type_and_backspace_keep_the_full_prefix() {
        let categories = categories(&[("Docker", "", "docker ps")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("d", &categories);
        state.push_text("ock", &categories);
        state.dismiss(); // Clicking/selecting terminal output only hides the popup.
        state.push_text("er", &categories);
        assert_eq!(state.candidates()[0].matched_prefix_bytes, 6);
        state.backspace(&categories);
        assert_eq!(state.accept_selected_or_first().as_deref(), Some("r ps"));
        assert_eq!(state.submit(&categories), vec!["command-0"]);
        assert!(state.submit(&categories).is_empty());
    }

    #[test]
    fn only_submitted_complete_commands_count_and_acceptance_preserves_actual_case() {
        let categories = categories(&[("Docker", "", "docker ps")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("docker", &categories);
        assert!(state.submit(&categories).is_empty());
        state.push_text("DOCKER", &categories);
        assert_eq!(state.accept_selected_or_first().as_deref(), Some(" ps"));
        assert!(state.submit(&categories).is_empty());
        state.push_text("docker", &categories);
        state.accept_selected_or_first();
        state.clear(); // Ctrl+C cancels without counting.
        assert!(state.submit(&categories).is_empty());
        state.push_text("docker ps", &categories);
        assert_eq!(state.submit(&categories), vec!["command-0"]);
    }

    #[test]
    fn unknown_cursor_or_history_edits_cannot_count_a_later_fragment_as_a_command() {
        let categories = categories(&[("Docker", "", "docker ps")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("echo ", &categories);
        state.invalidate(); // A shell-side edit makes the current line unknown.
        state.clear_line_prefix();
        state.push_text("docker ps", &categories);
        assert!(!state.is_visible());
        assert!(state.submit(&categories).is_empty());
        state.push_text("docker ps", &categories);
        assert_eq!(state.submit(&categories), vec!["command-0"]);
        state.push_text("echo ", &categories);
        state.clear_line_prefix();
        state.push_text("docker ps", &categories);
        assert_eq!(state.submit(&categories), vec!["command-0"]);
    }

    #[test]
    fn builtin_commands_show_short_names_while_typing() {
        let categories = crate::session::quick_commands::builtin_quick_command_categories("zh-CN");
        let mut state = TerminalCompletionState::default();

        state.push_text("ls", &categories);

        assert_eq!(
            state
                .candidates()
                .iter()
                .map(|candidate| candidate.label.as_str())
                .collect::<Vec<_>>(),
            vec![
                "列出目录",
                "文件详情",
                "显示隐藏",
                "友好大小",
                "CPU 信息",
                "端口占用",
            ]
        );
    }

    #[test]
    fn backspace_refreshes_and_clear_resets_state() {
        let categories = categories(&[("列表", "", "ls")]);
        let mut state = TerminalCompletionState::default();
        state.push_text("ls", &categories);
        assert!(state.is_visible());

        state.backspace(&categories);
        assert_eq!(state.input, "l");
        assert!(!state.is_visible());

        state.clear();
        assert!(state.input.is_empty());
    }
}
