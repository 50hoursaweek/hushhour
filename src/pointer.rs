//! Hides the mouse pointer while the user types. The whole window is wrapped in
//! [`HideWhileTyping`]: everything passes through to the content, except that the window
//! asks for no pointer while `hidden` is set, and the first mouse movement, click or
//! scroll publishes `on_mouse` so the app can show it again.
//!
//! A wrapper rather than a [`Stack`](iced::widget::Stack) layer on top: a layer that claims
//! the pointer would make the stack hand the layers below a levitating cursor, so a click
//! made without moving the mouse first would reach nothing.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Operation, Tree, Widget};
use iced::advanced::{Clipboard, Shell, overlay};
use iced::{Element, Event, Length, Rectangle, Size, Vector, mouse};

pub struct HideWhileTyping<'a, Message, Theme, Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    hidden: bool,
    on_mouse: Message,
}

impl<'a, Message, Theme, Renderer> HideWhileTyping<'a, Message, Theme, Renderer> {
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>, hidden: bool, on_mouse: Message) -> Self {
        Self { content: content.into(), hidden, on_mouse }
    }
}

impl<Message: Clone, Theme, Renderer: renderer::Renderer> Widget<Message, Theme, Renderer>
    for HideWhileTyping<'_, Message, Theme, Renderer>
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.content.as_widget_mut().layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.content.as_widget_mut().operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if self.hidden
            && matches!(
                event,
                Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::ButtonPressed(_) | mouse::Event::WheelScrolled { .. })
            )
        {
            shell.publish(self.on_mouse.clone());
        }
        self.content
            .as_widget_mut()
            .update(&mut tree.children[0], event, layout, cursor, renderer, clipboard, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.hidden {
            mouse::Interaction::Hidden
        } else {
            self.content.as_widget().mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(&tree.children[0], renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(&mut tree.children[0], layout, renderer, viewport, translation)
    }
}

impl<'a, Message: Clone + 'a, Theme: 'a, Renderer: renderer::Renderer + 'a> From<HideWhileTyping<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(widget: HideWhileTyping<'a, Message, Theme, Renderer>) -> Self {
        Element::new(widget)
    }
}
