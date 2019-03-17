/// Types related to syntax highlighting when draing contents of `Pager`s.
///
/// `Highlighter` defines the main trait any highlighting engine must implement.
/// `SyntectHighlighter` is the only included highlighter and should be sufficient for most
/// usecases.
use unsegen::base::{Color, LineIndex, StyleModifier, TextFormatModifier};

use super::PagerLine;
use syntect::highlighting;
use syntect::parsing::{ParseState, ScopeStack, SyntaxDefinition};

use syntect::highlighting::Theme;

/// Interface for anything that highlights the content of Pagers.
///
/// `SyntectHighlighter` is an exemplary implementation that can should be sufficient for most
/// usecases.
pub trait Highlighter {
    /// Compute highlighting information for the given range of lines.
    fn highlight<'a, L: Iterator<Item = &'a PagerLine>>(&self, lines: L) -> HighlightInfo;
}

/// Result of a highlighting operation (i.e., a call to Highlighter::highlight).
pub struct HighlightInfo {
    /// A map of changes per line.  The outer vec corresponds to lines. The entries of the inner
    /// vec specify that at the specified column index, the given modifier should be applied.
    pub style_changes: Vec<Vec<(usize, StyleModifier)>>,
    /// Style that will be applied if no other style has been specified.
    pub default_style: StyleModifier,
    no_change: Vec<(usize, StyleModifier)>,
}

impl HighlightInfo {
    /// Empty highlighting result that will not apply any style changes.
    pub fn none() -> Self {
        HighlightInfo {
            style_changes: Vec::new(),
            default_style: StyleModifier::new(),
            no_change: Vec::new(),
        }
    }

    /// Get any style changes for the specified line.
    pub fn get_info_for_line<L: Into<LineIndex>>(&self, l: L) -> &Vec<(usize, StyleModifier)> {
        self.style_changes
            .get(l.into().raw_value())
            .unwrap_or(&self.no_change)
    }

    /// Return the default style, i.e., the style that will be applied to text if no modifications
    /// are present.
    pub fn default_style(&self) -> StyleModifier {
        self.default_style
    }
}

/// A `Highlighter` using the `syntect` library as a backend.
pub struct SyntectHighlighter<'a> {
    base_state: ParseState,
    theme: &'a Theme,
}

impl<'a> SyntectHighlighter<'a> {
    /// Create a `SyntectHighlighter` using the specified `SyntaxDefinition` (e.g., what
    /// programming language to assume) and the theme.
    ///
    /// The theme reference has to be alive as long as the highlighter is active.
    pub fn new(syntax: &SyntaxDefinition, theme: &'a highlighting::Theme) -> Self {
        SyntectHighlighter {
            base_state: ParseState::new(syntax),
            theme,
        }
    }
}

impl<'a> Highlighter for SyntectHighlighter<'a> {
    fn highlight<'b, L: Iterator<Item = &'b PagerLine>>(&self, lines: L) -> HighlightInfo {
        let mut info = HighlightInfo::none();

        let highlighter = highlighting::Highlighter::new(self.theme);
        let mut hstate = highlighting::HighlightState::new(&highlighter, ScopeStack::new());
        let mut parse_state = self.base_state.clone();

        for line in lines {
            let line_content = line.get_content();
            let mut current_pos = 0;
            let mut this_line_changes = Vec::new();

            let ops = parse_state.parse_line(line.get_content());
            for (style, fragment) in highlighting::HighlightIterator::new(
                &mut hstate,
                &ops[..],
                line_content,
                &highlighter,
            ) {
                this_line_changes.push((current_pos, to_unsegen_style_modifier(&style)));
                current_pos += fragment.len();
            }
            info.style_changes.push(this_line_changes);
        }
        info.default_style = to_unsegen_style_modifier(&highlighter.get_default());
        info
    }
}

fn to_unsegen_color(color: highlighting::Color) -> Color {
    Color::Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}
fn to_unsegen_text_format(style: highlighting::FontStyle) -> TextFormatModifier {
    TextFormatModifier::new()
        .bold(style.contains(highlighting::FontStyle::BOLD))
        .italic(style.contains(highlighting::FontStyle::ITALIC))
        .underline(style.contains(highlighting::FontStyle::UNDERLINE))
}
fn to_unsegen_style_modifier(style: &highlighting::Style) -> StyleModifier {
    StyleModifier::new()
        .fg_color(to_unsegen_color(style.foreground))
        .bg_color(to_unsegen_color(style.background))
        .format(to_unsegen_text_format(style.font_style))
}
