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
        self.input.push_str(text);
        self.refresh(categories);
    }

    pub(crate) fn backspace(&mut self, categories: &[QuickCommandCategory]) {
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
        self.input = candidate.command.clone();
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
        self.dismiss();
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

    categories
        .iter()
        .flat_map(|category| category.commands.iter())
        .filter(|command| !contains_parameter_placeholder(&command.command))
        .filter_map(|command| {
            let matched_prefix_bytes = matched_prefix_bytes(&command.command, query)?;
            Some(TerminalCompletionCandidate {
                command: command.command.clone(),
                label: command.name.clone(),
                matched_prefix_bytes,
            })
        })
        .take(MAX_CANDIDATES)
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
