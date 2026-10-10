use std::{
    collections::{HashMap, HashSet},
    fmt,
    ops::Range,
    sync::OnceLock,
};

use alacritty_terminal::term::cell::{Cell, Flags};
use gpui::{Hsla, Rgba};
use regex::{Regex, RegexBuilder};

use crate::session::highlight_rules::{
    HighlightEntityKind, HighlightMatchKind, HighlightRule, HighlightRuleCategory,
    HighlightRuleStyle, HighlightTarget,
};
use crate::terminal::RenderCell;

const MAX_ENABLED_RULES: usize = 128;
const MAX_PATTERN_CHARACTERS: usize = 512;
const MAX_MATCHES_PER_RULE_PER_LINE: usize = 512;
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HighlightRuleError {
    EmptyPattern,
    PatternTooLong { max: usize },
    InvalidRegex { detail: String },
    EmptyMatch,
    InvalidCaptureGroup { group: usize, available: usize },
    InvalidColor { value: String },
}

impl fmt::Display for HighlightRuleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => formatter.write_str("pattern cannot be empty"),
            Self::PatternTooLong { max } => {
                write!(formatter, "pattern exceeds the {max}-character limit")
            }
            Self::InvalidRegex { detail } => {
                write!(formatter, "invalid regular expression: {detail}")
            }
            Self::EmptyMatch => formatter.write_str("pattern must not match an empty string"),
            Self::InvalidCaptureGroup { group, available } => write!(
                formatter,
                "capture group {group} does not exist; pattern defines {available} groups"
            ),
            Self::InvalidColor { value } => write!(
                formatter,
                "invalid color `{value}`; expected #RRGGBB or #RRGGBBAA"
            ),
        }
    }
}

/// Runtime-only style attached to a terminal cell by a highlight rule.
///
/// Every field is optional/additive so terminal ANSI styling remains the base
/// layer. Search and selection backgrounds are resolved later by the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct HighlightCellStyle {
    pub(crate) foreground: Option<Hsla>,
    pub(crate) background: Option<Hsla>,
    pub(crate) bold: bool,
    pub(crate) underline: bool,
}

pub(crate) type HighlightMap = HashMap<(i32, i32), HighlightCellStyle>;

#[derive(Debug)]
struct CompiledRule {
    matcher: Regex,
    whole_word: bool,
    target: HighlightTarget,
    capture_group: Option<usize>,
    style: HighlightCellStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HighlightMatchExplanation {
    pub(crate) rule_name: String,
    pub(crate) category: HighlightRuleCategory,
    pub(crate) entity: Option<HighlightEntityKind>,
    pub(crate) range: Range<usize>,
    pub(crate) matched_text: String,
}

/// Regexes compiled for one exact rule fingerprint.
///
/// Invalid rules are intentionally omitted. The settings preview reports their
/// error directly, while terminal rendering remains resilient to malformed
/// configuration loaded from an older or manually edited file.
#[derive(Debug)]
pub(crate) struct CompiledRuleSet {
    fingerprint: u64,
    rules: Vec<CompiledRule>,
}

impl CompiledRuleSet {
    pub(crate) fn compile(fingerprint: u64, rules: &[HighlightRule]) -> Self {
        let rules = rules
            .iter()
            .filter(|rule| rule.enabled)
            .take(MAX_ENABLED_RULES)
            .filter_map(|rule| compile_rule(rule).ok())
            .collect();
        Self { fingerprint, rules }
    }

    pub(crate) fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.rules.len()
    }
}

fn compile_rule(rule: &HighlightRule) -> Result<CompiledRule, HighlightRuleError> {
    if rule.pattern.is_empty() {
        return Err(HighlightRuleError::EmptyPattern);
    }
    if rule.pattern.chars().count() > MAX_PATTERN_CHARACTERS {
        return Err(HighlightRuleError::PatternTooLong {
            max: MAX_PATTERN_CHARACTERS,
        });
    }

    let pattern = match rule.match_kind {
        HighlightMatchKind::Literal => regex::escape(&rule.pattern),
        HighlightMatchKind::Regex => rule.pattern.clone(),
    };
    let matcher = RegexBuilder::new(&pattern)
        .case_insensitive(!rule.case_sensitive)
        .unicode(true)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_SIZE_LIMIT)
        .build()
        .map_err(|error| HighlightRuleError::InvalidRegex {
            detail: error.to_string(),
        })?;
    if let Some(group) = rule.capture_group
        && group >= matcher.captures_len()
    {
        return Err(HighlightRuleError::InvalidCaptureGroup {
            group,
            available: matcher.captures_len().saturating_sub(1),
        });
    }
    if ["", "a", " ", "\n", "错"].into_iter().any(|sample| {
        matcher
            .find(sample)
            .is_some_and(|matched| matched.is_empty())
    }) {
        return Err(HighlightRuleError::EmptyMatch);
    }
    let style = runtime_style(&rule.style)?;

    Ok(CompiledRule {
        matcher,
        whole_word: rule.whole_word,
        target: rule.target,
        capture_group: rule.capture_group,
        style,
    })
}

/// Convert a persisted rule style into renderer colors.
pub(crate) fn runtime_style(
    style: &HighlightRuleStyle,
) -> Result<HighlightCellStyle, HighlightRuleError> {
    Ok(HighlightCellStyle {
        foreground: style
            .foreground
            .as_deref()
            .map(parse_hex_color)
            .transpose()?,
        background: style
            .background
            .as_deref()
            .map(parse_hex_color)
            .transpose()?,
        bold: style.bold,
        underline: style.underline,
    })
}

fn parse_hex_color(value: &str) -> Result<Hsla, HighlightRuleError> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if !matches!(hex.len(), 6 | 8) {
        return Err(HighlightRuleError::InvalidColor {
            value: value.to_string(),
        });
    }
    let parsed = u32::from_str_radix(hex, 16).map_err(|_| HighlightRuleError::InvalidColor {
        value: value.to_string(),
    })?;
    let (r, g, b, a) = if hex.len() == 8 {
        (
            (parsed >> 24) as u8,
            (parsed >> 16) as u8,
            (parsed >> 8) as u8,
            parsed as u8,
        )
    } else {
        (
            (parsed >> 16) as u8,
            (parsed >> 8) as u8,
            parsed as u8,
            u8::MAX,
        )
    };
    Ok(Hsla::from(Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }))
}

/// Match a single rule for the settings live preview.
pub(crate) fn preview_match_ranges(
    text: &str,
    rule: &HighlightRule,
) -> Result<Vec<Range<usize>>, HighlightRuleError> {
    if !rule.enabled {
        return Ok(Vec::new());
    }
    let compiled = compile_rule(rule)?;
    let mut result = Vec::new();
    let mut line_offset = 0;
    for preview_line in text.split('\n') {
        // A pasted CRLF sample represents the same logical lines as terminal
        // output, which does not retain the carriage return cell.
        let line = preview_line.strip_suffix('\r').unwrap_or(preview_line);
        let matches = matching_ranges(line, &compiled);
        if compiled.target == HighlightTarget::Line {
            if !matches.is_empty() {
                result.push(line_offset..line_offset + line.len());
            }
        } else {
            result.extend(
                matches
                    .into_iter()
                    .map(|range| line_offset + range.start..line_offset + range.end),
            );
        }
        line_offset += preview_line.len().saturating_add(1);
    }
    Ok(result)
}

pub(crate) fn explain_matches(
    text: &str,
    rules: &[HighlightRule],
) -> Vec<HighlightMatchExplanation> {
    let mut explanations = Vec::new();
    for rule in rules.iter().filter(|rule| rule.enabled) {
        let Ok(compiled) = compile_rule(rule) else {
            continue;
        };
        for range in matching_ranges(text, &compiled) {
            explanations.push(HighlightMatchExplanation {
                rule_name: rule.name.clone(),
                category: rule.category,
                entity: rule.entity,
                matched_text: text[range.clone()].to_string(),
                range,
            });
        }
    }
    explanations.sort_by_key(|explanation| explanation.range.start);
    explanations
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StructuredLogFormat {
    Json,
    Logfmt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StructuredLogField {
    pub(crate) format: StructuredLogFormat,
    pub(crate) key: String,
    pub(crate) value: String,
    pub(crate) value_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FileLocation {
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) column: Option<usize>,
    pub(crate) range: Range<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalLineAnalysis {
    pub(crate) structured_fields: Vec<StructuredLogField>,
    pub(crate) file_locations: Vec<FileLocation>,
    pub(crate) is_stack_frame: bool,
}

pub(crate) fn analyze_terminal_line(text: &str) -> TerminalLineAnalysis {
    TerminalLineAnalysis {
        structured_fields: parse_structured_log_fields(text),
        file_locations: extract_file_locations(text),
        is_stack_frame: is_stack_frame(text),
    }
}

fn parse_structured_log_fields(text: &str) -> Vec<StructuredLogField> {
    static JSON_FIELD: OnceLock<Regex> = OnceLock::new();
    static LOGFMT_FIELD: OnceLock<Regex> = OnceLock::new();
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return JSON_FIELD
            .get_or_init(|| {
                Regex::new(
                    r#""([^"\\]+)"\s*:\s*("(?:\\.|[^"\\])*"|-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?|true|false|null)"#,
                )
                .expect("static JSON field regex must compile")
            })
            .captures_iter(text)
            .filter_map(|captures| {
                let key = captures.get(1)?.as_str().to_string();
                let raw_value = captures.get(2)?;
                let value = if raw_value.as_str().starts_with('"') {
                    serde_json::from_str::<String>(raw_value.as_str()).ok()?
                } else {
                    raw_value.as_str().to_string()
                };
                let range = if raw_value.as_str().starts_with('"') {
                    raw_value.start() + 1..raw_value.end() - 1
                } else {
                    raw_value.range()
                };
                Some(StructuredLogField {
                    format: StructuredLogFormat::Json,
                    key,
                    value,
                    value_range: range,
                })
            })
            .collect();
    }

    LOGFMT_FIELD
        .get_or_init(|| {
            Regex::new(r#"(?:^|\s)([A-Za-z_][A-Za-z0-9_.-]*)=("(?:\\.|[^"\\])*"|[^\s]+)"#)
                .expect("static logfmt field regex must compile")
        })
        .captures_iter(text)
        .filter_map(|captures| {
            let key = captures.get(1)?.as_str().to_string();
            let raw_value = captures.get(2)?;
            let (value, range) = if raw_value.as_str().starts_with('"') {
                (
                    raw_value.as_str()[1..raw_value.as_str().len() - 1].to_string(),
                    raw_value.start() + 1..raw_value.end() - 1,
                )
            } else {
                (raw_value.as_str().to_string(), raw_value.range())
            };
            Some(StructuredLogField {
                format: StructuredLogFormat::Logfmt,
                key,
                value,
                value_range: range,
            })
        })
        .collect()
}

fn extract_file_locations(text: &str) -> Vec<FileLocation> {
    static LOCATION: OnceLock<Regex> = OnceLock::new();
    static PYTHON_LOCATION: OnceLock<Regex> = OnceLock::new();
    let mut locations = LOCATION
        .get_or_init(|| {
            Regex::new(r#"((?:[A-Za-z]:\\|/|\./|\.\./|~/)[^\s:()]+):(\d+)(?::(\d+))?"#)
                .expect("static file location regex must compile")
        })
        .captures_iter(text)
        .filter_map(|captures| {
            let matched = captures.get(0)?;
            Some(FileLocation {
                path: captures.get(1)?.as_str().to_string(),
                line: captures.get(2)?.as_str().parse().ok()?,
                column: captures
                    .get(3)
                    .and_then(|value| value.as_str().parse().ok()),
                range: matched.range(),
            })
        })
        .collect::<Vec<_>>();
    locations.extend(
        PYTHON_LOCATION
            .get_or_init(|| {
                Regex::new(r#"File\s+"([^"]+)",\s+line\s+(\d+)"#)
                    .expect("static Python location regex must compile")
            })
            .captures_iter(text)
            .filter_map(|captures| {
                let matched = captures.get(0)?;
                Some(FileLocation {
                    path: captures.get(1)?.as_str().to_string(),
                    line: captures.get(2)?.as_str().parse().ok()?,
                    column: None,
                    range: matched.range(),
                })
            }),
    );
    locations.sort_by_key(|location| location.range.start);
    locations
}

fn is_stack_frame(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("at ")
        || trimmed.starts_with("File \"")
        || trimmed.starts_with("Caused by:")
        || trimmed.starts_with("Traceback (")
        || trimmed.starts_with("goroutine ")
}

fn matching_ranges(text: &str, rule: &CompiledRule) -> Vec<Range<usize>> {
    rule.matcher
        .captures_iter(text)
        .take(MAX_MATCHES_PER_RULE_PER_LINE)
        .filter_map(|captures| {
            captures
                .get(rule.capture_group.unwrap_or(0))
                .map(|matched| matched.range())
        })
        .filter(|range| !range.is_empty())
        .filter(|range| !rule.whole_word || has_word_boundaries(text, range))
        .collect()
}

fn has_word_boundaries(text: &str, range: &Range<usize>) -> bool {
    let matched = &text[range.clone()];
    let starts_with_word = matched.chars().next().is_some_and(is_word_character);
    let ends_with_word = matched.chars().next_back().is_some_and(is_word_character);
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    (!starts_with_word || before.is_none_or(|character| !is_word_character(character)))
        && (!ends_with_word || after.is_none_or(|character| !is_word_character(character)))
}

fn is_word_character(character: char) -> bool {
    // `regex`'s Unicode `\w` includes alphabetic characters, marks,
    // decimal numbers, connector punctuation, and join controls. Reusing that
    // definition keeps manual whole-word filtering aligned with regex rules.
    static WORD_CHARACTER: OnceLock<Result<Regex, regex::Error>> = OnceLock::new();
    let mut encoded = [0; 4];
    let character = character.encode_utf8(&mut encoded);
    WORD_CHARACTER
        .get_or_init(|| Regex::new(r"^\w$"))
        .as_ref()
        .is_ok_and(|matcher| matcher.is_match(character))
}

/// Compute all highlights for a fresh viewport or a changed rule set.
pub(crate) fn highlight_cells(
    cells: &[RenderCell],
    rows: usize,
    rules: &CompiledRuleSet,
) -> HighlightMap {
    let logical_lines = build_logical_lines(cells, rows);
    let mut result = HighlightMap::new();
    for line in &logical_lines {
        highlight_line(line, rules, &mut result);
    }
    result
}

/// Re-evaluate only logical lines that could have been affected by terminal
/// damage. Building logical-line boundaries is linear in viewport cells, but
/// regex/literal rules no longer rescan every unchanged line on every revision.
pub(crate) fn update_highlights_for_rows(
    cells: &[RenderCell],
    rows: usize,
    rules: &CompiledRuleSet,
    previous: &HighlightMap,
    damaged_rows: &[usize],
) -> HighlightMap {
    if damaged_rows.is_empty() {
        return previous.clone();
    }

    let logical_lines = build_logical_lines(cells, rows);
    let mut nearby_rows = HashSet::with_capacity(damaged_rows.len() * 3);
    for &row in damaged_rows {
        if row >= rows {
            continue;
        }
        nearby_rows.insert(row);
        if row > 0 {
            nearby_rows.insert(row - 1);
        }
        if row + 1 < rows {
            nearby_rows.insert(row + 1);
        }
    }

    let affected = logical_lines
        .iter()
        .filter(|line| line.rows().any(|row| nearby_rows.contains(&row)))
        .collect::<Vec<_>>();
    if affected.is_empty() {
        return previous.clone();
    }

    let affected_rows = affected
        .iter()
        .flat_map(|line| line.rows())
        .collect::<HashSet<_>>();
    let mut result = previous.clone();
    result.retain(|(row, _), _| !affected_rows.contains(&((*row).max(0) as usize)));
    for line in affected {
        highlight_line(line, rules, &mut result);
    }
    result
}

fn highlight_line(line: &LogicalLine<'_>, rules: &CompiledRuleSet, result: &mut HighlightMap) {
    let mut occupied = HashSet::new();

    // Rule order is the explicit priority. A lower-priority overlapping span
    // is discarded as a whole, rather than filling only the remaining suffix
    // and creating split-color tokens such as PASS|WORD or RE|START.
    for rule in &rules.rules {
        let ranges = matching_ranges(&line.text, rule);
        if ranges.is_empty() {
            continue;
        }

        if rule.target == HighlightTarget::Line {
            let coordinates = line.display_cells();
            if coordinates
                .iter()
                .any(|coordinate| occupied.contains(coordinate))
            {
                continue;
            }
            for coordinate in coordinates {
                occupied.insert(coordinate);
                result.insert(coordinate, rule.style);
            }
            continue;
        }

        for range in ranges {
            let coordinates = line.cells_for_range(range);
            if coordinates.is_empty()
                || coordinates
                    .iter()
                    .any(|coordinate| occupied.contains(coordinate))
            {
                continue;
            }
            for coordinate in coordinates {
                occupied.insert(coordinate);
                result.insert(coordinate, rule.style);
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct LogicalLine<'a> {
    pub(crate) text: String,
    pub(crate) byte_to_cell: Vec<(usize, usize)>,
    pub(crate) row_cells: Vec<&'a RenderCell>,
}

impl LogicalLine<'_> {
    fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        let mut previous = None;
        self.row_cells.iter().filter_map(move |cell| {
            let row = cell.row.max(0) as usize;
            if previous == Some(row) {
                None
            } else {
                previous = Some(row);
                Some(row)
            }
        })
    }

    fn display_cells(&self) -> Vec<(i32, i32)> {
        self.row_cells
            .iter()
            .filter(|cell| {
                !cell.cell.flags.intersects(
                    Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER,
                )
            })
            .map(|cell| (cell.row, cell.col))
            .collect()
    }

    fn cells_for_range(&self, range: Range<usize>) -> Vec<(i32, i32)> {
        let mut cells = Vec::with_capacity(range.len());
        for &(row, col) in self.byte_to_cell.get(range).into_iter().flatten() {
            let coordinate = (row as i32, col as i32);
            if cells.last() != Some(&coordinate) {
                cells.push(coordinate);
            }
        }
        cells
    }
}

pub(crate) fn build_logical_lines<'a>(
    cells: &'a [RenderCell],
    rows: usize,
) -> Vec<LogicalLine<'a>> {
    let mut row_cells: Vec<Vec<&RenderCell>> = vec![Vec::new(); rows];
    for cell in cells {
        if cell.row >= 0 && (cell.row as usize) < rows {
            row_cells[cell.row as usize].push(cell);
        }
    }
    for row in &mut row_cells {
        row.sort_by_key(|cell| cell.col);
    }

    let mut logical_lines = Vec::new();
    let mut current: Option<LogicalLine<'a>> = None;
    for (row_index, row) in row_cells.into_iter().enumerate() {
        if row.is_empty() {
            if let Some(line) = current.take() {
                logical_lines.push(line);
            }
            continue;
        }

        let wraps_from_previous = row_index > 0
            && current.as_ref().is_some_and(|line| {
                line.row_cells
                    .last()
                    .is_some_and(|cell| cell.cell.flags.contains(Flags::WRAPLINE))
            });
        if !wraps_from_previous && let Some(line) = current.take() {
            logical_lines.push(line);
        }

        let mut line = current.take().unwrap_or_else(|| LogicalLine {
            text: String::new(),
            byte_to_cell: Vec::new(),
            row_cells: Vec::new(),
        });
        let text_cell_count = physical_row_text_cell_count(row.iter().copied());
        for (index, cell) in row.into_iter().enumerate() {
            if index < text_cell_count {
                append_render_cell_text(&mut line, cell);
            }
            line.row_cells.push(cell);
        }
        current = Some(line);
    }
    if let Some(line) = current {
        logical_lines.push(line);
    }
    logical_lines
}

fn physical_row_text_cell_count<'a>(
    cells: impl DoubleEndedIterator<Item = &'a RenderCell> + ExactSizeIterator,
) -> usize {
    let total = cells.len();
    let default_cell = Cell::default();
    let unused_padding = cells
        .rev()
        .take_while(|cell| cell.cell == default_cell)
        .count();
    total - unused_padding
}

fn append_render_cell_text(line: &mut LogicalLine<'_>, cell: &RenderCell) {
    let hidden_or_spacer = cell
        .cell
        .flags
        .intersects(Flags::HIDDEN | Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    if hidden_or_spacer {
        return;
    }

    push_mapped_character(
        &mut line.text,
        &mut line.byte_to_cell,
        cell.cell.c,
        cell.row as usize,
        cell.col as usize,
    );
    if let Some(combining) = cell.cell.zerowidth() {
        for &character in combining {
            push_mapped_character(
                &mut line.text,
                &mut line.byte_to_cell,
                character,
                cell.row as usize,
                cell.col as usize,
            );
        }
    }
}

fn push_mapped_character(
    text: &mut String,
    byte_to_cell: &mut Vec<(usize, usize)>,
    character: char,
    row: usize,
    col: usize,
) {
    text.push(character);
    byte_to_cell.resize(text.len(), (row, col));
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalEntity {
    pub(crate) kind: HighlightEntityKind,
    pub(crate) value: String,
    pub(crate) cells: Vec<(usize, usize)>,
}

#[derive(Debug)]
struct SemanticMatcher {
    kind: HighlightEntityKind,
    matcher: Regex,
    whole_word: bool,
}

pub(crate) fn find_entity_at_cell(
    cells: &[RenderCell],
    rows: usize,
    row: usize,
    col: usize,
) -> Option<TerminalEntity> {
    static MATCHERS: OnceLock<Vec<SemanticMatcher>> = OnceLock::new();
    let matchers = MATCHERS.get_or_init(|| {
        crate::session::highlight_rules::default_highlight_rules()
            .into_iter()
            .filter_map(|rule| {
                let kind = rule.entity?;
                let compiled = compile_rule(&rule).ok()?;
                Some(SemanticMatcher {
                    kind,
                    matcher: compiled.matcher,
                    whole_word: compiled.whole_word,
                })
            })
            .collect()
    });
    let line = logical_line_at_row(cells, rows, row)?;
    for semantic in matchers {
        for matched in semantic.matcher.find_iter(&line.text) {
            let range = matched.range();
            if range.is_empty() || (semantic.whole_word && !has_word_boundaries(&line.text, &range))
            {
                continue;
            }
            let mut entity_cells = Vec::with_capacity(range.len());
            for &coordinate in line.byte_to_cell.get(range.clone())? {
                if entity_cells.last() != Some(&coordinate) {
                    entity_cells.push(coordinate);
                }
            }
            if entity_cells.binary_search(&(row, col)).is_ok() {
                return Some(TerminalEntity {
                    kind: semantic.kind,
                    value: line.text[range].to_string(),
                    cells: entity_cells,
                });
            }
        }
    }
    None
}

/// Build only the wrapped logical line containing `target_row`. Entity hover runs
/// for mouse movement and must not rebuild/sort the entire viewport.
fn logical_line_at_row<'a>(
    cells: &'a [RenderCell],
    rows: usize,
    target_row: usize,
) -> Option<LogicalLine<'a>> {
    if rows == 0 || target_row >= rows || cells.len() % rows != 0 {
        return None;
    }
    let columns = cells.len() / rows;
    if columns == 0 {
        return None;
    }

    let wraps_to_next = |row: usize| {
        cells
            .get((row + 1).saturating_mul(columns).saturating_sub(1))
            .is_some_and(|cell| cell.cell.flags.contains(Flags::WRAPLINE))
    };
    let mut first = target_row;
    while first > 0 && wraps_to_next(first - 1) {
        first -= 1;
    }
    let mut last = target_row;
    while last + 1 < rows && wraps_to_next(last) {
        last += 1;
    }

    let mut line = LogicalLine {
        text: String::new(),
        byte_to_cell: Vec::new(),
        row_cells: Vec::with_capacity((last - first + 1) * columns),
    };
    for row in first..=last {
        let physical_row = &cells[row * columns..(row + 1) * columns];
        let text_cell_count = physical_row_text_cell_count(physical_row.iter());
        for (index, cell) in physical_row.iter().enumerate() {
            if index < text_cell_count {
                append_render_cell_text(&mut line, cell);
            }
            line.row_cells.push(cell);
        }
    }
    Some(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::highlight_rules::default_highlight_rules;

    fn rule(
        pattern: &str,
        match_kind: HighlightMatchKind,
        case_sensitive: bool,
        whole_word: bool,
        target: HighlightTarget,
        foreground: &str,
    ) -> HighlightRule {
        HighlightRule {
            id: pattern.to_string(),
            name: pattern.to_string(),
            enabled: true,
            pattern: pattern.to_string(),
            match_kind,
            case_sensitive,
            whole_word,
            target,
            capture_group: None,
            pack: crate::session::highlight_rules::HighlightRulePack::Custom,
            category: crate::session::highlight_rules::HighlightRuleCategory::General,
            scope: crate::session::highlight_rules::HighlightRuleScope::Global,
            entity: None,
            style: HighlightRuleStyle {
                foreground: Some(foreground.to_string()),
                background: None,
                bold: false,
                underline: false,
            },
        }
    }

    fn row_cells(row: i32, text: &str) -> Vec<RenderCell> {
        text.chars()
            .enumerate()
            .map(|(col, character)| RenderCell {
                row,
                col: col as i32,
                cell: Cell {
                    c: character,
                    ..Cell::default()
                },
            })
            .collect()
    }

    #[test]
    fn capture_group_limits_highlight_and_explanation_to_semantic_value() {
        let mut json_level = rule(
            r#""level"\s*:\s*"(error)""#,
            HighlightMatchKind::Regex,
            false,
            false,
            HighlightTarget::Match,
            "#E85D68",
        );
        json_level.id = "json-level-error".to_string();
        json_level.name = "JSON error level".to_string();
        json_level.capture_group = Some(1);
        json_level.category = HighlightRuleCategory::StructuredLog;

        assert_eq!(
            preview_match_ranges(r#"{"level":"error","message":"ok"}"#, &json_level).unwrap(),
            vec![10..15]
        );
        let explanation = explain_matches(r#"{"level":"error","message":"ok"}"#, &[json_level]);
        assert_eq!(explanation[0].matched_text, "error");
        assert_eq!(
            explanation[0].category,
            HighlightRuleCategory::StructuredLog
        );
    }

    #[test]
    fn invalid_capture_group_is_reported() {
        let mut invalid = rule(
            "(error)",
            HighlightMatchKind::Regex,
            false,
            false,
            HighlightTarget::Match,
            "#E85D68",
        );
        invalid.capture_group = Some(2);
        assert!(matches!(
            preview_match_ranges("error", &invalid),
            Err(HighlightRuleError::InvalidCaptureGroup {
                group: 2,
                available: 1
            })
        ));
    }

    #[test]
    fn semantic_entity_hit_testing_covers_url_email_ip_and_file_path() {
        let samples = [
            ("https://example.com/runbook", HighlightEntityKind::Url, 10),
            ("ops@example.com", HighlightEntityKind::Email, 4),
            ("10.20.30.40", HighlightEntityKind::IpAddress, 5),
            ("/var/log/app.log", HighlightEntityKind::FilePath, 7),
        ];
        for (text, expected_kind, col) in samples {
            let cells = row_cells(0, text);
            let entity = find_entity_at_cell(&cells, 1, 0, col).unwrap();
            assert_eq!(entity.kind, expected_kind);
            assert_eq!(entity.value, text);
        }
    }

    #[test]
    fn line_analysis_parses_json_logfmt_and_cross_platform_stack_locations() {
        let json = analyze_terminal_line(r#"{"level":"error","code":503,"retry":true}"#);
        assert_eq!(json.structured_fields.len(), 3);
        assert_eq!(json.structured_fields[0].key, "level");
        assert_eq!(json.structured_fields[0].value, "error");
        assert_eq!(json.structured_fields[1].value, "503");

        let logfmt = analyze_terminal_line(r#"level=warn message="slow request" latency_ms=420"#);
        assert_eq!(logfmt.structured_fields.len(), 3);
        assert_eq!(logfmt.structured_fields[1].value, "slow request");

        let rust = analyze_terminal_line("  at C:\\src\\main.rs:42:7");
        assert!(rust.is_stack_frame);
        assert_eq!(rust.file_locations[0].path, "C:\\src\\main.rs");
        assert_eq!(rust.file_locations[0].line, 42);
        assert_eq!(rust.file_locations[0].column, Some(7));

        let python = analyze_terminal_line(r#"  File "/srv/app.py", line 19, in run"#);
        assert!(python.is_stack_frame);
        assert_eq!(python.file_locations[0].path, "/srv/app.py");
        assert_eq!(python.file_locations[0].line, 19);
    }

    #[test]
    fn literal_matching_honors_case_and_unicode_word_boundaries() {
        let insensitive = rule(
            "错误",
            HighlightMatchKind::Literal,
            false,
            true,
            HighlightTarget::Match,
            "#FF0000",
        );
        let case_sensitive = rule(
            "Error",
            HighlightMatchKind::Literal,
            true,
            true,
            HighlightTarget::Match,
            "#FF0000",
        );

        assert_eq!(
            preview_match_ranges("发生 错误 完成", &insensitive).unwrap(),
            vec![7..13]
        );
        assert_eq!(
            preview_match_ranges("error Error", &case_sensitive).unwrap(),
            vec![6..11]
        );
    }

    #[test]
    fn preview_matches_each_physical_line_with_global_utf8_offsets() {
        let anchored = rule(
            "^WARN",
            HighlightMatchKind::Regex,
            true,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let across_newline = rule(
            "错误\\nWARN",
            HighlightMatchKind::Regex,
            true,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let preview = "INFO 错误\nWARN ready";
        let second_line_start = preview.find("WARN").unwrap();

        assert_eq!(
            preview_match_ranges(preview, &anchored).unwrap(),
            vec![second_line_start..second_line_start + 4]
        );
        assert!(
            preview_match_ranges(preview, &across_newline)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn whole_word_treats_combining_marks_and_connector_punctuation_as_word_characters() {
        let whole_word = rule(
            "ERROR",
            HighlightMatchKind::Literal,
            true,
            true,
            HighlightTarget::Match,
            "#FF0000",
        );
        let preview = "ERROR\u{0301}suffix ERROR\u{203f}suffix ERROR";
        let standalone_start = preview.rfind("ERROR").unwrap();

        assert_eq!(
            preview_match_ranges(preview, &whole_word).unwrap(),
            vec![standalone_start..standalone_start + 5]
        );
    }

    #[test]
    fn invalid_regex_is_reported_in_preview_and_skipped_at_runtime() {
        let invalid = rule(
            "(",
            HighlightMatchKind::Regex,
            false,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );

        assert!(matches!(
            preview_match_ranges("anything", &invalid),
            Err(HighlightRuleError::InvalidRegex { detail }) if !detail.is_empty()
        ));
        assert_eq!(CompiledRuleSet::compile(1, &[invalid]).len(), 0);
    }

    #[test]
    fn matcher_enforces_resource_limits_for_manually_edited_configuration() {
        let empty_pattern = rule(
            "",
            HighlightMatchKind::Literal,
            false,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let too_long = rule(
            &"x".repeat(MAX_PATTERN_CHARACTERS + 1),
            HighlightMatchKind::Literal,
            false,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let empty_match = rule(
            "x*",
            HighlightMatchKind::Regex,
            false,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let one_character = rule(
            "x",
            HighlightMatchKind::Literal,
            false,
            false,
            HighlightTarget::Match,
            "#FF0000",
        );
        let many_rules = (0..MAX_ENABLED_RULES + 10)
            .map(|index| {
                rule(
                    &format!("word{index}"),
                    HighlightMatchKind::Literal,
                    false,
                    false,
                    HighlightTarget::Match,
                    "#FF0000",
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            preview_match_ranges("x", &empty_pattern),
            Err(HighlightRuleError::EmptyPattern)
        );
        assert_eq!(
            preview_match_ranges("x", &too_long),
            Err(HighlightRuleError::PatternTooLong {
                max: MAX_PATTERN_CHARACTERS
            })
        );
        assert_eq!(
            preview_match_ranges("x", &empty_match),
            Err(HighlightRuleError::EmptyMatch)
        );
        assert_eq!(
            preview_match_ranges(
                &"x".repeat(MAX_MATCHES_PER_RULE_PER_LINE + 20),
                &one_character
            )
            .unwrap()
            .len(),
            MAX_MATCHES_PER_RULE_PER_LINE
        );
        assert_eq!(
            CompiledRuleSet::compile(1, &many_rules).len(),
            MAX_ENABLED_RULES
        );
    }

    #[test]
    fn whole_word_rules_do_not_match_noisy_substrings() {
        let rules = [
            rule(
                "UP",
                HighlightMatchKind::Literal,
                false,
                true,
                HighlightTarget::Match,
                "#FF0000",
            ),
            rule(
                "READ",
                HighlightMatchKind::Literal,
                false,
                true,
                HighlightTarget::Match,
                "#00FF00",
            ),
            rule(
                "LOAD",
                HighlightMatchKind::Literal,
                false,
                true,
                HighlightTarget::Match,
                "#0000FF",
            ),
        ];
        let compiled = CompiledRuleSet::compile(1, &rules);
        let cells = row_cells(0, "backup thread payload");

        assert!(highlight_cells(&cells, 1, &compiled).is_empty());
    }

    #[test]
    fn recommended_rules_ignore_ambiguous_tokens_and_highlight_the_complete_url() {
        let rules = default_highlight_rules();
        let compiled = CompiledRuleSet::compile(2, &rules);
        let text = "PASSWORD TOKEN RESTART https://example.test/path).";
        let url_start = text.find("https://").unwrap();
        let url_end = text.find(")").unwrap();
        let cells = row_cells(0, text);
        let highlights = highlight_cells(&cells, 1, &compiled);

        assert!((0..url_start).all(|col| !highlights.contains_key(&(0, col as i32))));
        assert!((url_start..url_end).all(|col| highlights.contains_key(&(0, col as i32))));
        assert!((url_end..text.len()).all(|col| !highlights.contains_key(&(0, col as i32))));
    }

    #[test]
    fn recommended_rules_cover_operational_states_and_validate_common_identifiers() {
        let rules = default_highlight_rules();
        let find = |id: &str| rules.iter().find(|rule| rule.id == id).unwrap();

        let error = find("builtin-error");
        assert_eq!(
            preview_match_ranges("connection refused; service inactive", error).unwrap(),
            vec![11..18, 28..36]
        );

        let ipv4 = find("builtin-ipv4");
        assert_eq!(
            preview_match_ranges("peer 192.168.10.42:22", ipv4).unwrap(),
            vec![5..18]
        );
        assert!(
            preview_match_ranges("invalid 999.168.10.42", ipv4)
                .unwrap()
                .is_empty()
        );

        let http = find("builtin-http-errors");
        assert_eq!(
            preview_match_ranges("HTTP/1.1 503 unavailable", http).unwrap(),
            vec![0..12]
        );
        assert!(
            preview_match_ranges("process 503 exited", http)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn specialized_rule_corpus_covers_tools_without_matching_near_misses() {
        let rules = default_highlight_rules();
        let get = |id: &str| rules.iter().find(|rule| rule.id == id).unwrap();

        assert_eq!(
            preview_match_ranges(
                r#"{"severity":"error","message":"safe"}"#,
                get("builtin-structured-error")
            )
            .unwrap(),
            vec![13..18]
        );
        assert!(
            preview_match_ranges("severity_reason=error", get("builtin-structured-error"))
                .unwrap()
                .is_empty()
        );
        assert!(
            !preview_match_ranges("<<<<<<< HEAD", get("builtin-git-conflict"))
                .unwrap()
                .is_empty()
        );
        assert!(
            preview_match_ranges("======", get("builtin-git-conflict"))
                .unwrap()
                .is_empty()
        );
        assert!(
            !preview_match_ranges(
                "pod status=CrashLoopBackOff",
                get("builtin-container-failure")
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            !preview_match_ranges(
                "scanner found CVE-2026-12345",
                get("builtin-security-advisory")
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            preview_match_ranges("CVE-26-123", get("builtin-security-advisory"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            preview_match_ranges("at /srv/src/main.rs:42:7", get("builtin-source-location"))
                .unwrap(),
            vec![3..19]
        );
    }

    #[test]
    fn recommended_rules_cover_common_protocols_addresses_ids_times_and_paths() {
        let rules = default_highlight_rules();
        let find = |id: &str| rules.iter().find(|rule| rule.id == id).unwrap();
        let matched_text = |id: &str, text: &str| {
            preview_match_ranges(text, find(id))
                .unwrap()
                .into_iter()
                .map(|range| text[range].to_string())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            matched_text("builtin-http-method", "GET /health then PATCH /api/v1"),
            ["GET", "PATCH"]
        );
        assert_eq!(
            matched_text("builtin-email", "owner ops@example.com notified"),
            ["ops@example.com"]
        );
        assert_eq!(
            matched_text("builtin-mac", "adapter 00:1A:2B:3C:4D:5E ready"),
            ["00:1A:2B:3C:4D:5E"]
        );
        assert_eq!(
            matched_text("builtin-ipv6", "peer [2001:db8::42]:443 connected"),
            ["2001:db8::42"]
        );
        assert_eq!(
            matched_text(
                "builtin-uuid",
                "request 550e8400-e29b-41d4-a716-446655440000 failed"
            ),
            ["550e8400-e29b-41d4-a716-446655440000"]
        );
        assert_eq!(
            matched_text(
                "builtin-timestamp",
                "2026-08-27T14:05:09.123+08:00 service started"
            ),
            ["2026-08-27T14:05:09.123+08:00"]
        );
        assert_eq!(
            matched_text(
                "builtin-file-path",
                r#"read /var/log/app.log and C:\work\tiny-shell\Cargo.toml"#
            ),
            ["/var/log/app.log", r#"C:\work\tiny-shell\Cargo.toml"#]
        );
    }

    #[test]
    fn lower_priority_overlap_is_dropped_as_a_span_not_split_by_cell() {
        let rules = [
            rule(
                "PASS",
                HighlightMatchKind::Literal,
                false,
                false,
                HighlightTarget::Match,
                "#00FF00",
            ),
            rule(
                "PASSWORD",
                HighlightMatchKind::Literal,
                false,
                false,
                HighlightTarget::Match,
                "#FF0000",
            ),
        ];
        let compiled = CompiledRuleSet::compile(1, &rules);
        let cells = row_cells(0, "PASSWORD");
        let highlights = highlight_cells(&cells, 1, &compiled);

        assert_eq!(highlights.len(), 4);
        assert!((0..4).all(|col| highlights.contains_key(&(0, col))));
        assert!((4..8).all(|col| !highlights.contains_key(&(0, col))));
    }

    #[test]
    fn line_target_covers_wrapped_unicode_cells() {
        let mut first_row = row_cells(0, "错误");
        first_row
            .last_mut()
            .unwrap()
            .cell
            .flags
            .insert(Flags::WRAPLINE);
        let mut cells = first_row;
        cells.extend(row_cells(1, " detail"));
        let line_rule = rule(
            "错误",
            HighlightMatchKind::Literal,
            false,
            true,
            HighlightTarget::Line,
            "#FF0000",
        );
        let compiled = CompiledRuleSet::compile(7, &[line_rule]);

        let highlights = highlight_cells(&cells, 2, &compiled);

        assert_eq!(highlights.len(), cells.len());
        assert!(highlights.contains_key(&(0, 0)));
        assert!(highlights.contains_key(&(1, 6)));
    }

    #[test]
    fn end_anchor_ignores_default_terminal_padding_but_line_target_keeps_it() {
        let columns = 12;
        let mut cells = row_cells(0, "ERROR");
        cells.extend((cells.len()..columns).map(|col| RenderCell {
            row: 0,
            col: col as i32,
            cell: Cell::default(),
        }));
        let anchored_line = rule(
            "ERROR$",
            HighlightMatchKind::Regex,
            true,
            false,
            HighlightTarget::Line,
            "#FF0000",
        );
        let compiled = CompiledRuleSet::compile(71, &[anchored_line]);

        let logical_lines = build_logical_lines(&cells, 1);
        let highlights = highlight_cells(&cells, 1, &compiled);

        assert_eq!(logical_lines[0].text, "ERROR");
        assert_eq!(logical_lines[0].row_cells.len(), columns);
        assert_eq!(highlights.len(), columns);
        assert!((0..columns).all(|col| highlights.contains_key(&(0, col as i32))));
    }

    #[test]
    fn wide_characters_map_to_the_rendered_cell_not_the_spacer() {
        let wide = Cell {
            c: '错',
            flags: Flags::WIDE_CHAR,
            ..Cell::default()
        };
        let spacer = Cell {
            flags: Flags::WIDE_CHAR_SPACER,
            ..Cell::default()
        };
        let cells = vec![
            RenderCell {
                row: 0,
                col: 0,
                cell: wide,
            },
            RenderCell {
                row: 0,
                col: 1,
                cell: spacer,
            },
        ];
        let wide_rule = rule(
            "错",
            HighlightMatchKind::Literal,
            false,
            true,
            HighlightTarget::Match,
            "#FF0000",
        );
        let compiled = CompiledRuleSet::compile(8, &[wide_rule]);

        let highlights = highlight_cells(&cells, 1, &compiled);

        assert_eq!(highlights.len(), 1);
        assert!(highlights.contains_key(&(0, 0)));
        assert!(!highlights.contains_key(&(0, 1)));
    }

    #[test]
    fn incremental_update_replaces_only_damaged_logical_lines() {
        let red = rule(
            "ERROR",
            HighlightMatchKind::Literal,
            false,
            true,
            HighlightTarget::Match,
            "#FF0000",
        );
        let compiled = CompiledRuleSet::compile(9, &[red]);
        let mut cells = row_cells(0, "ERROR ok");
        cells.extend(row_cells(1, "ERROR ok"));
        let previous = highlight_cells(&cells, 2, &compiled);
        for cell in cells.iter_mut().filter(|cell| cell.row == 1) {
            cell.cell.c = 'x';
        }

        let updated = update_highlights_for_rows(&cells, 2, &compiled, &previous, &[1]);

        assert!((0..5).all(|col| updated.contains_key(&(0, col))));
        assert!((0..5).all(|col| !updated.contains_key(&(1, col))));
    }

    #[test]
    fn entity_hover_detection_is_independent_from_keyword_rules() {
        let cells = row_cells(0, "https://example.test/path");

        let entity = find_entity_at_cell(&cells, 1, 0, 10).unwrap();

        assert_eq!(entity.value, "https://example.test/path");
        assert_eq!(entity.cells.first(), Some(&(0, 0)));
        assert_eq!(entity.cells.last(), Some(&(0, 24)));
    }

    #[test]
    fn style_parser_supports_alpha_and_all_runtime_attributes() {
        let style = HighlightRuleStyle {
            foreground: Some("#112233".to_string()),
            background: Some("#44556680".to_string()),
            bold: true,
            underline: true,
        };

        let runtime = runtime_style(&style).unwrap();

        assert!(runtime.foreground.is_some());
        assert_eq!(runtime.background.unwrap().a, 128.0 / 255.0);
        assert!(runtime.bold);
        assert!(runtime.underline);

        assert_eq!(
            runtime_style(&HighlightRuleStyle {
                foreground: Some("not-a-color".to_string()),
                ..HighlightRuleStyle::default()
            }),
            Err(HighlightRuleError::InvalidColor {
                value: "not-a-color".to_string()
            })
        );
    }
}
