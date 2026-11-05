//! An image drawn at a [`Zoom`]: like `image` with `ContentFit::Contain` at fit, bigger
//! and panned when zoomed, clipped to its bounds. iced's `image` can scale only from its
//! centre, so it can't pan.

use iced::advanced::image::{self, FilterMethod, Renderer as _};
use iced::advanced::widget::{Tree, Widget};
use iced::advanced::{Layout, layout, mouse, renderer};
use iced::{Element, Length, Radians, Rectangle, Renderer, Size};
use photo_cruiser_core::{Rect, Zoom};

/// Beyond this many view units per image pixel, pixels show as squares, not a blur.
const SHARP_PIXELS: f32 = 3.0;

pub struct ZoomImage {
    handle: image::Handle,
    zoom: Zoom,
    opacity: f32,
}

pub fn zoom_image(handle: image::Handle, zoom: Zoom, opacity: f32) -> ZoomImage {
    ZoomImage { handle, zoom, opacity }
}

impl<Message, Theme> Widget<Message, Theme, Renderer> for ZoomImage {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(&mut self, _tree: &mut Tree, _renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::Node::new(limits.max())
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let Some(px) = renderer.measure_image(&self.handle) else { return };
        let b = layout.bounds();
        let img = photo_cruiser_core::Size::new(px.width as f32, px.height as f32);
        let r = self.zoom.rect(Rect::new(b.x, b.y, b.width, b.height), img);
        let filter_method =
            if r.w / img.width >= SHARP_PIXELS { FilterMethod::Nearest } else { FilterMethod::Linear };
        let image = image::Image {
            handle: self.handle.clone(),
            filter_method,
            rotation: Radians(0.0),
            border_radius: Default::default(),
            opacity: self.opacity,
            snap: true,
        };
        renderer.draw_image(image, Rectangle::new(iced::Point::new(r.x, r.y), Size::new(r.w, r.h)), b);
    }
}

impl<'a, Message: 'a, Theme: 'a> From<ZoomImage> for Element<'a, Message, Theme, Renderer> {
    fn from(z: ZoomImage) -> Self {
        Element::new(z)
    }
}
