//! An `unsegen` widget for viewing files with additional features.
//!
//! # Examples:
//! ```no_run
//! extern crate unsegen;
//!
//! use std::io::{stdin, stdout};
//! use unsegen::base::Terminal;
//! use unsegen::input::{Input, Key, ScrollBehavior};
//! use unsegen::widget::{RenderingHints, Widget};
//!
//! use unsegen_pager::{Pager, PagerContent, SyntaxSet, SyntectHighlighter, ThemeSet};
//!
//! fn main() {
//!     let stdout = stdout();
//!     let stdin = stdin();
//!     let stdin = stdin.lock();
//!
//!     let file = "path/to/some/file";
//!
//!     let syntax_set = SyntaxSet::load_defaults_nonewlines();
//!     let syntax = syntax_set
//!         .find_syntax_for_file(&file)
//!         .unwrap()
//!         .unwrap_or(syntax_set.find_syntax_plain_text());
//!
//!     let theme_set = ThemeSet::load_defaults();
//!     let theme = &theme_set.themes["base16-ocean.dark"];
//!
//!     let highlighter = SyntectHighlighter::new(syntax, theme);
//!     let mut pager = Pager::new();
//!     pager.load(
//!         PagerContent::from_file(&file)
//!             .unwrap()
//!             .with_highlighter(&highlighter),
//!     );
//!
//!     let mut term = Terminal::new(stdout.lock()).unwrap();
//!
//!     for input in Input::read_all(stdin) {
//!         let input = input.unwrap();
//!         input.chain(
//!             ScrollBehavior::new(&mut pager)
//!                 .forwards_on(Key::Down)
//!                 .forwards_on(Key::Char('j'))
//!                 .backwards_on(Key::Up)
//!                 .backwards_on(Key::Char('k'))
//!                 .to_beginning_on(Key::Home)
//!                 .to_end_on(Key::End),
//!         );
//!         // Put more application logic here...
//!
//!         {
//!             let win = term.create_root_window();
//!             pager.as_widget().draw(win, RenderingHints::default());
//!         }
//!         term.present();
//!     }
//! }
//! ```

extern crate syntect;
extern crate unsegen;

mod decorating;
mod highlighting;

pub use decorating::*;
pub use highlighting::*;

pub use syntect::highlighting::{Theme, ThemeSet};
pub use syntect::parsing::{SyntaxDefinition, SyntaxSet};

use unsegen::base::{
    basic_types::*, BoolModifyMode, Cursor, GraphemeCluster, StyleModifier, Window, WrappingMode,
};
use unsegen::input::{OperationResult, Scrollable};
use unsegen::widget::{layout_linearly, Demand, Demand2D, RenderingHints, Widget};

use std::cmp::{max, min};
use std::ops::{Bound, RangeBounds};

/// Main `Widget`, may (or may not) store content, but defines static types for content and
/// decoration.
///
/// Use `load` to actually fill the widget with content.
///
/// In addition to the `PagerContent`, it has a concept of an 'active line' that can be updated via
/// user interaction (using the `Scrollable` implementation) and is always displayed when drawn to
/// a window.
pub struct Pager<L, D = NoDecorator<L>>
where
    L: PagerLine,
    D: LineDecorator,
{
    content: Option<PagerContent<L, D>>,
    current_line: LineIndex,
}

impl<L, D> Default for Pager<L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    fn default() -> Self {
        Pager {
            content: None,
            current_line: LineIndex::new(0),
        }
    }
}

impl<L, D> Pager<L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    /// Create an empty pager, with no current content.
    pub fn new() -> Self {
        Pager {
            content: None,
            current_line: LineIndex::new(0),
        }
    }

    /// Load (and potentially overwrite previous) content to display in the pager.
    ///
    /// If possible, the current line position will be preserved.
    pub fn load(&mut self, content: PagerContent<L, D>) {
        self.content = Some(content);

        // Go back to last available line
        let current_line = self.current_line;
        if !self.line_exists(current_line) {
            let _ = self.scroll_to_end();
        }
    }

    /// Clear the current content.
    ///
    /// On subsequent `draw` calls, nothing will be written to the window.
    pub fn clear_content(&mut self) {
        self.content = None;
    }

    /// Get a reference to the current content, if available.
    pub fn content(&self) -> Option<&PagerContent<L, D>> {
        self.content.as_ref()
    }

    /// Get a mutable reference to the current content, if available.
    ///
    /// Note that `PagerContent` does not allow mutable access to the stored lines, so it is
    /// required to use `load` to update the contents. A pager is not a text editor.
    pub fn content_mut(&mut self) -> Option<&mut PagerContent<L, D>> {
        self.content.as_mut()
    }

    fn line_exists<I: Into<LineIndex>>(&mut self, line: I) -> bool {
        let line: LineIndex = line.into();
        if let Some(ref mut content) = self.content {
            line.raw_value() < content.storage.len()
        } else {
            false
        }
    }

    /// Go to the specified line, if present.
    ///
    /// If there is no such line, an error is returned.
    pub fn go_to_line<I: Into<LineIndex>>(&mut self, line: I) -> Result<(), PagerError> {
        let line: LineIndex = line.into();
        if self.line_exists(line) {
            self.current_line = line;
            Ok(())
        } else {
            Err(PagerError::NoLineWithIndex(line))
        }
    }

    /// Go to first line that matches the given predicate.
    ///
    /// If there is no such line, an error is returned.
    pub fn go_to_line_if<F: Fn(LineIndex, &L) -> bool>(
        &mut self,
        predicate: F,
    ) -> Result<(), PagerError> {
        let line = if let Some(ref mut content) = self.content {
            content
                .view(LineIndex::new(0)..)
                .find(|&(index, ref line)| predicate(index, line))
                .map(|(index, _)| index)
                .ok_or(PagerError::NoLineWithPredicate)
        } else {
            Err(PagerError::NoContent)
        };
        line.and_then(|index| self.go_to_line(index))
    }

    /// Get the index of the currently active line.
    pub fn current_line_index(&self) -> LineIndex {
        self.current_line
    }

    /// Get a reference to the currently active line.
    pub fn current_line(&self) -> Option<&L> {
        if let Some(ref content) = self.content {
            content.storage.get(self.current_line_index().raw_value())
        } else {
            None
        }
    }

    pub fn as_widget<'a>(&'a self) -> impl Widget + 'a {
        PagerWidget { inner: self }
    }
}

struct PagerWidget<'a, L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    inner: &'a Pager<L, D>,
}

impl<'a, L, D> Widget for PagerWidget<'a, L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    fn space_demand(&self) -> Demand2D {
        Demand2D {
            width: Demand::at_least(1),
            height: Demand::at_least(1),
        }
    }
    fn draw(&self, window: Window, _: RenderingHints) {
        if let Some(ref content) = self.inner.content {
            let height: Height = window.get_height();
            // The highlighter might need a minimum number of lines to figure out the syntax:
            // TODO: make this configurable?
            let min_highlight_context = 40;
            let num_adjacent_lines_to_load = max(height.into(), min_highlight_context / 2);
            let min_line = self
                .inner
                .current_line
                .checked_sub(num_adjacent_lines_to_load)
                .unwrap_or_else(|| LineIndex::new(0));
            let max_line = self.inner.current_line + num_adjacent_lines_to_load;

            // Split window
            let decorator_demand = content
                .decorator
                .horizontal_space_demand(content.view(min_line..max_line));
            let split_pos = layout_linearly(
                window.get_width(),
                Width::new(0).unwrap(),
                &[decorator_demand, Demand::at_least(1)],
                &[0.0, 1.0],
            )[0];

            let (mut decoration_window, mut content_window) = window
                .split(split_pos.from_origin())
                .expect("valid split pos");

            // Fill background with correct color
            let bg_style = content.highlight_info.default_style();
            content_window.set_default_style(bg_style.apply_to_default());
            content_window.fill(GraphemeCluster::space());

            let mut cursor = Cursor::new(&mut content_window)
                .position(ColIndex::new(0), RowIndex::new(0))
                .wrapping_mode(WrappingMode::Wrap);

            let num_line_wraps_until_current_line = {
                content
                    .view(min_line..self.inner.current_line)
                    .map(|(_, line)| (cursor.num_expected_wraps(line.get_content()) + 1) as i32)
                    .sum::<i32>()
            };
            let num_line_wraps_from_current_line = {
                content
                    .view(self.inner.current_line..max_line)
                    .map(|(_, line)| (cursor.num_expected_wraps(line.get_content()) + 1) as i32)
                    .sum::<i32>()
            };

            let centered_current_line_start_pos: RowIndex = (height / (2 as usize)).from_origin();
            let best_current_line_pos_for_bottom = max(
                centered_current_line_start_pos,
                height.from_origin() - num_line_wraps_from_current_line,
            );
            let required_start_pos = min(
                RowIndex::new(0),
                best_current_line_pos_for_bottom - num_line_wraps_until_current_line,
            );

            cursor.move_to(ColIndex::new(0), required_start_pos);

            for (line_index, line) in content.view(min_line..max_line) {
                let line_content = line.get_content();
                let base_style = if line_index == self.inner.current_line {
                    StyleModifier::new()
                        .invert(BoolModifyMode::Toggle)
                        .bold(true)
                } else {
                    StyleModifier::new()
                };

                let (_, start_y) = cursor.get_position();
                let mut last_change_pos = 0;
                for &(change_pos, style) in content.highlight_info.get_info_for_line(line_index) {
                    cursor.write(&line_content[last_change_pos..change_pos]);

                    cursor.set_style_modifier(style.on_top_of(base_style));
                    last_change_pos = change_pos;
                }
                cursor.write(&line_content[last_change_pos..]);

                cursor.set_style_modifier(base_style);
                cursor.fill_and_wrap_line();
                let (_, end_y) = cursor.get_position();

                let range_start_y = min(max(start_y, RowIndex::new(0)), height.from_origin());
                let range_end_y = min(max(end_y, RowIndex::new(0)), height.from_origin());
                content.decorator.decorate(
                    &line,
                    line_index,
                    self.inner.current_line,
                    decoration_window.create_subwindow(.., range_start_y..range_end_y),
                );
                //decoration_window.create_subwindow(.., range_start_y..range_end_y).fill('X');
            }
        }
    }
}

impl<L, D> Scrollable for Pager<L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    fn scroll_backwards(&mut self) -> OperationResult {
        if self.current_line > LineIndex::new(0) {
            self.current_line -= 1;
            Ok(())
        } else {
            Err(())
        }
    }
    fn scroll_forwards(&mut self) -> OperationResult {
        let new_line = self.current_line + 1;
        self.go_to_line(new_line).map_err(|_| ())
    }
    fn scroll_to_beginning(&mut self) -> OperationResult {
        if self.current_line == LineIndex::new(0) {
            Err(())
        } else {
            self.current_line = LineIndex::new(0);
            Ok(())
        }
    }
    fn scroll_to_end(&mut self) -> OperationResult {
        if let Some(ref content) = self.content {
            if content.storage.is_empty() {
                return Err(());
            }
            let last_line = LineIndex::new(content.storage.len() - 1);
            if self.current_line == last_line {
                Err(())
            } else {
                self.current_line = last_line;
                Ok(())
            }
        } else {
            Err(())
        }
    }
}

/// Anything that represents a single line in a pager. Other than the main content (something
/// string-like) it may also store additional information that can be used by a `Highlighter`.
pub trait PagerLine {
    fn get_content(&self) -> &str;
}

impl PagerLine for String {
    fn get_content(&self) -> &str {
        self.as_str()
    }
}

/// A collection of `PagerLines` including information about the highlighting state and (if
/// present) a `LineDecorator`.
///
/// Use `from_lines` or `from_file` to build an initial content and add highlighter and decorator
/// using `with_highlighter` and `with_decorator`.
pub struct PagerContent<L: PagerLine, D: LineDecorator> {
    storage: Vec<L>,
    highlight_info: HighlightInfo,
    decorator: D,
}

impl<L: PagerLine> PagerContent<L, NoDecorator<L>> {
    /// Create a simple `PagerContent` from a ordered collection of lines. The lines the `Vec` will
    /// be displayed top to bottom from beginning to end.
    pub fn from_lines(storage: Vec<L>) -> Self {
        PagerContent {
            storage,
            highlight_info: HighlightInfo::none(),
            decorator: NoDecorator::default(),
        }
    }
}

impl PagerContent<String, NoDecorator<String>> {
    /// Try to load lines (as strings) from the given file as the lines of PagerContent.
    pub fn from_file<F: AsRef<::std::path::Path>>(file_path: F) -> ::std::io::Result<Self> {
        use std::io::Read;
        let mut file = ::std::fs::File::open(file_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        Ok(PagerContent {
            storage: contents.lines().map(|s| s.to_owned()).collect::<Vec<_>>(),
            highlight_info: HighlightInfo::none(),
            decorator: NoDecorator::default(),
        })
    }
}

impl<L, D> PagerContent<L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    /// Add a `Highlighter` to `PagerContent` that previously did not have one.
    pub fn with_highlighter<HN: Highlighter>(self, highlighter: &HN) -> PagerContent<L, D> {
        let highlight_info =
            highlighter.highlight(self.storage.iter().map(|l| l as &dyn PagerLine));
        PagerContent {
            storage: self.storage,
            highlight_info,
            decorator: self.decorator,
        }
    }
}

impl<L> PagerContent<L, NoDecorator<L>>
where
    L: PagerLine,
{
    /// Add a `Decorator` to `PagerContent` that previously did not have one.
    pub fn with_decorator<DN: LineDecorator<Line = L>>(self, decorator: DN) -> PagerContent<L, DN> {
        PagerContent {
            storage: self.storage,
            highlight_info: self.highlight_info,
            decorator,
        }
    }
}

impl<L, D> PagerContent<L, D>
where
    L: PagerLine,
    D: LineDecorator<Line = L>,
{
    /// Iterate over a specified range of lines stored.
    ///
    /// The specified range can be larger than what the `PagerContent` currently holds. In that
    /// case the additional indices are simply not part of the returned iterator.
    pub fn view<'a, I: Into<LineIndex> + Clone, R: RangeBounds<I>>(
        &'a self,
        range: R,
    ) -> impl DoubleEndedIterator<Item = (LineIndex, &'a L)> + 'a
    where
        Self: ::std::marker::Sized,
    {
        // Not exactly sure, why this is needed... we only store a reference?!
        let start: LineIndex = match range.start_bound() {
            // Always inclusive
            Bound::Unbounded => LineIndex::new(0),
            Bound::Included(i) => i.clone().into(),
            Bound::Excluded(i) => i.clone().into() + 1,
        };
        let end: LineIndex = match range.end_bound() {
            // Always exclusive
            Bound::Unbounded => LineIndex::new(self.storage.len()),
            Bound::Included(i) => i.clone().into() + 1,
            Bound::Excluded(i) => i.clone().into(),
        };
        let ustart = start.raw_value();
        let uend = self.storage.len().min(end.raw_value());
        let urange = ustart..uend;
        urange
            .clone()
            .zip(self.storage[urange].iter())
            .map(|(i, l)| (LineIndex::new(i), l))
    }

    /// Try to view a specific line with the given index.
    pub fn view_line<I: Into<LineIndex>>(&self, line: I) -> Option<&L> {
        self.storage.get(line.into().raw_value())
    }

    /// Overwrite the current decorator with a compatible one.
    pub fn set_decorator(&mut self, decorator: D) {
        self.decorator = decorator;
    }
}

/// All errors that can occur when operating on a `Pager` or its contents.
#[derive(Debug)]
pub enum PagerError {
    NoLineWithIndex(LineIndex),
    NoLineWithPredicate,
    NoContent,
}
