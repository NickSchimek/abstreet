pub mod autocomplete;
pub mod button;
pub mod checkbox;
pub mod compare_times;
pub mod containers;
pub mod dropdown;
pub mod fan_chart;
pub mod filler;
pub mod just_draw;
pub mod line_plot;
pub mod menu;
pub mod persistent_split;
pub mod scatter_plot;
pub mod slider;
pub mod spinner;
pub mod text_box;

use crate::{EventCtx, GfxCtx, ScreenDims, ScreenPt};

/// Create a new widget by implementing this trait. You can instantiate your widget by calling
/// `Widget::new(Box::new(instance of your new widget))`, which gives you the usual style options.
pub trait WidgetImpl: downcast_rs::Downcast {
    /// What width and height does the widget occupy? If this changes, be sure to set
    /// `redo_layout` to true in `event`.
    fn get_dims(&self) -> ScreenDims;
    /// Your widget's top left corner should be here. Handle mouse events and draw appropriately.
    fn set_pos(&mut self, top_left: ScreenPt);
    /// Your chance to react to an event. Any side effects outside of this widget are communicated
    /// through the output.
    fn event(&mut self, ctx: &mut EventCtx, output: &mut WidgetOutput);
    /// Draw the widget. Be sure to draw relative to the top-left specified by `set_pos`.
    fn draw(&self, g: &mut GfxCtx);
    /// If a new Composite is being created to replace an older one, all widgets have the chance to
    /// preserve state from the previous version.
    fn can_restore(&self) -> bool {
        false
    }
    /// Restore state from the previous version of this widget, with the same ID. Implementors must
    /// downcast.
    fn restore(&mut self, _: &mut EventCtx, _prev: &Box<dyn WidgetImpl>) {
        unreachable!()
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// An action was done
    Clicked(String),
    /// A dropdown, checkbox, spinner, etc changed values. Usually this triggers a refresh of
    /// everything, so not useful to plumb along what changed.
    Changed,
    /// Nothing happened
    Nothing,
}

pub struct WidgetOutput {
    /// This widget changed dimensions, so recalculate layout.
    pub redo_layout: bool,
    /// This widget produced an Outcome, and event handling should immediately stop. Most widgets
    /// shouldn't set this.
    pub outcome: Outcome,
}

impl WidgetOutput {
    pub fn new() -> WidgetOutput {
        WidgetOutput {
            redo_layout: false,
            outcome: Outcome::Nothing,
        }
    }
}

downcast_rs::impl_downcast!(WidgetImpl);
