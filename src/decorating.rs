/// Types related to decoration of individual pager lines with, for example, line numbers.
///
/// Implement `LineDecorator` for custom decoration, use `NoDecorator` if you do not want
/// decoration at all or `LineNumberDecorator` for plain old line numbers.
use unsegen::base::basic_types::*;
use unsegen::base::{Cursor, Window};
use unsegen::widget::{text_width, ColDemand, Demand};

use super::PagerLine;

/// Interface for anything that is able to decorate lines, i.e., to draw something next to the left
/// of a pager line, given some information about the line.
pub trait LineDecorator {
    /// The type of line that can be decorated using this implementation.
    type Line: PagerLine;

    /// Define how much (horizontal) space is required to draw the decoration of the given lines.
    ///
    /// As the width of the decorator column is fixed for all lines, the implementer should choose
    /// the maximum demand of all lines.
    fn horizontal_space_demand<'a, 'b: 'a>(
        &'a self,
        lines: impl DoubleEndedIterator<Item = (LineIndex, &'b Self::Line)> + 'b,
    ) -> ColDemand
    where
        Self::Line: 'b;

    /// Decorate the given `line` by drawing to `window.
    ///
    /// Information of how to decorate the line can retrieved from the line itself, its index,
    /// and/or the index of the currently active line of the pager.
    ///
    /// Note that window is at least one row in height and in fact is so in most cases, but an
    /// implementer cannot rely on that. It is also not guaranteed that the window is as wide as
    /// specified in the last call to `horizontal_space_demand`.
    fn decorate(
        &self,
        line: &Self::Line,
        line_to_decorate_index: LineIndex,
        active_line_index: LineIndex,
        window: Window,
    );
}

/// Do not draw line decoration.
///
/// This is the default for newly created `PagerContent`. Use `with_decorator` to specify another
/// `LineDecorator`.
pub struct NoDecorator<L> {
    _dummy: ::std::marker::PhantomData<L>,
}

impl<L> Default for NoDecorator<L> {
    fn default() -> Self {
        NoDecorator {
            _dummy: Default::default(),
        }
    }
}

impl<L: PagerLine> LineDecorator for NoDecorator<L> {
    type Line = L;
    fn horizontal_space_demand<'a, 'b: 'a>(
        &'a self,
        _: impl DoubleEndedIterator<Item = (LineIndex, &'b Self::Line)> + 'b,
    ) -> ColDemand
    where
        Self::Line: 'b,
    {
        Demand::exact(0)
    }
    fn decorate(&self, _: &L, _: LineIndex, _: LineIndex, _: Window) {}
}

/// Draw line numbers next to every line.
///
/// Add to `PagerContent` using `with_decorator`.
pub struct LineNumberDecorator<L> {
    _dummy: ::std::marker::PhantomData<L>,
}

impl<L> Default for LineNumberDecorator<L> {
    fn default() -> Self {
        LineNumberDecorator {
            _dummy: Default::default(),
        }
    }
}

impl<L: PagerLine> LineDecorator for LineNumberDecorator<L> {
    type Line = L;
    fn horizontal_space_demand<'a, 'b: 'a>(
        &'a self,
        lines: impl DoubleEndedIterator<Item = (LineIndex, &'b Self::Line)> + 'b,
    ) -> ColDemand
    where
        Self::Line: 'b,
    {
        let max_space = lines
            .last()
            .map(|(i, _)| text_width(format!(" {} ", i).as_str()))
            .unwrap_or_else(|| Width::new(0).unwrap());
        Demand::exact(max_space)
    }
    fn decorate(&self, _: &L, line_to_decorate_index: LineIndex, _: LineIndex, mut window: Window) {
        let width = (window.get_width() - 2).positive_or_zero();
        let line_number = LineNumber::from(line_to_decorate_index);
        let mut cursor = Cursor::new(&mut window).position(ColIndex::new(0), RowIndex::new(0));

        use std::fmt::Write;
        write!(cursor, " {:width$} ", line_number, width = width.into()).unwrap();
    }
}
