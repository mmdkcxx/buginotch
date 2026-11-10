// SPDX-License-Identifier: GPL-3.0-or-later
//! `Canvas` implemented on Direct2D + DirectWrite. Coordinates are DIPs.

use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use windows_numerics::{Matrix3x2, Vector2};
use windows::{
    Win32::Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
    core::*,
};

/// The bundled display face (Anton), loaded from memory into its own DirectWrite factory.
pub struct DisplayFont {
    factory: IDWriteFactory,
    collection: IDWriteFontCollection,
}

static ANTON: &[u8] = include_bytes!("../assets/Anton-Regular.ttf");

impl DisplayFont {
    pub fn load() -> Option<DisplayFont> {
        unsafe {
            let f5: IDWriteFactory5 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED).ok()?;
            let loader = f5.CreateInMemoryFontFileLoader().ok()?;
            f5.RegisterFontFileLoader(&loader).ok()?;
            let file = loader.CreateInMemoryFontFileReference(&f5, ANTON.as_ptr() as *const _, ANTON.len() as u32, None).ok()?;
            let builder = f5.CreateFontSetBuilder().ok()?;
            builder.AddFontFile(&file).ok()?;
            let set = builder.CreateFontSet().ok()?;
            let collection: IDWriteFontCollection1 = f5.CreateFontCollectionFromFontSet(&set).ok()?;
            Some(DisplayFont { factory: f5.cast().ok()?, collection: collection.cast().ok()? })
        }
    }

    fn layout(&self, text: &str, size: f32, tracking: f32, w: f32, h: f32, align: Align) -> Option<IDWriteTextLayout> {
        unsafe {
            let f = self.factory.CreateTextFormat(w!("Anton"), &self.collection, DWRITE_FONT_WEIGHT_REGULAR, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us")).ok()?;
            let a = match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            };
            let _ = f.SetTextAlignment(a);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let wide: Vec<u16> = text.encode_utf16().collect();
            let layout = self.factory.CreateTextLayout(&wide, &f, w, h).ok()?;
            if tracking != 0.0 {
                if let Ok(l1) = layout.cast::<IDWriteTextLayout1>() {
                    let _ = l1.SetCharacterSpacing(0.0, tracking, 0.0, DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 });
                }
            }
            Some(layout)
        }
    }
}

pub struct D2dCanvas<'a> {
    ctx: &'a ID2D1DeviceContext,
    dw: &'a IDWriteFactory,
    brush: &'a ID2D1SolidColorBrush,
    images: &'a RefCell<HashMap<ImageId, ID2D1Bitmap1>>,
    next_image: &'a Cell<u32>,
    family: &'a HSTRING,
    display: Option<&'a DisplayFont>,
    glass: bool,
    clips: Vec<bool>, // true = layer, false = axis-aligned clip
    shifts: Vec<Matrix3x2>,
}

fn col(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a: c.3 as f32 / 255.0 }
}

/// Glass remap for shapes: a dark, nearly grey colour (the tracks and chips drawn for a black
/// background) becomes white with a see-through amount that follows its brightness.
fn glass_fill(c: Color) -> Color {
    let (mx, mn) = (c.0.max(c.1).max(c.2), c.0.min(c.1).min(c.2));
    if mx == 0 || mx > 90 || mx - mn > 24 {
        return c;
    }
    let alpha = (0.10 + mx as f32 / 90.0 * 0.12) * 255.0 * (c.3 as f32 / 255.0);
    Color(255, 255, 255, alpha as u8)
}

/// Glass remap for text: mid grey (the dim secondary text) is lifted toward white.
fn glass_text(c: Color) -> Color {
    let (mx, mn) = (c.0.max(c.1).max(c.2), c.0.min(c.1).min(c.2));
    if !(85..=200).contains(&mx) || mx - mn > 24 {
        return c;
    }
    let alpha = if mx < 115 { 150.0 } else { 185.0 };
    Color(244, 245, 247, (alpha * c.3 as f32 / 255.0) as u8)
}

fn rf(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

impl<'a> D2dCanvas<'a> {
    pub fn new(
        ctx: &'a ID2D1DeviceContext,
        dw: &'a IDWriteFactory,
        brush: &'a ID2D1SolidColorBrush,
        images: &'a RefCell<HashMap<ImageId, ID2D1Bitmap1>>,
        next_image: &'a Cell<u32>,
        family: &'a HSTRING,
        display: Option<&'a DisplayFont>,
    ) -> Self {
        Self { ctx, dw, brush, images, next_image, family, display, glass: false, clips: Vec::new(), shifts: Vec::new() }
    }

    fn format(&self, size: f32, weight: u16, align: Align) -> Option<IDWriteTextFormat> {
        unsafe {
            let f = self
                .dw
                .CreateTextFormat(self.family, None, DWRITE_FONT_WEIGHT(weight as i32), DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
                .ok()?;
            let a = match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            };
            let _ = f.SetTextAlignment(a);
            let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            Some(f)
        }
    }

    fn rounded_geometry(&self, r: Rect, radius: f32) -> Option<ID2D1Geometry> {
        unsafe {
            let factory = self.ctx.GetFactory().ok()?;
            let g = factory
                .CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rf(r), radiusX: radius, radiusY: radius })
                .ok()?;
            g.cast().ok()
        }
    }
}

impl Canvas for D2dCanvas<'_> {
    fn set_glass(&mut self, on: bool) {
        self.glass = on;
    }

    fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Color) {
        let c = if self.glass { glass_fill(c) } else { c };
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rf(r), radiusX: radius, radiusY: radius }, self.brush);
        }
    }

    fn fill_polygon(&mut self, points: &[(f32, f32)], c: Color) {
        if points.len() < 3 {
            return;
        }
        unsafe {
            let Ok(factory) = self.ctx.GetFactory() else { return };
            let Ok(path) = factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            sink.BeginFigure(Vector2 { X: points[0].0, Y: points[0].1 }, D2D1_FIGURE_BEGIN_FILLED);
            for p in &points[1..] {
                sink.AddLine(Vector2 { X: p.0, Y: p.1 });
            }
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            if sink.Close().is_err() {
                return;
            }
            self.brush.SetColor(&col(c));
            self.ctx.FillGeometry(&path, self.brush, None);
        }
    }

    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        let c = if self.glass { glass_fill(c) } else { c };
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.brush);
        }
    }

    fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, width: f32, c: Color) {
        let c = if self.glass { glass_fill(c) } else { c };
        unsafe {
            let Ok(factory) = self.ctx.GetFactory() else { return };
            let Ok(path) = factory.CreatePathGeometry() else { return };
            let Ok(sink) = path.Open() else { return };
            let pt = |deg: f32| {
                let rad = deg.to_radians();
                Vector2 { X: cx + r * rad.cos(), Y: cy + r * rad.sin() }
            };
            let sweep = sweep_deg.clamp(-359.99, 359.99);
            if sweep.abs() < 0.01 {
                return;
            }
            sink.BeginFigure(pt(start_deg), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: pt(start_deg + sweep),
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: if sweep > 0.0 { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                arcSize: if sweep.abs() > 180.0 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_err() {
                return;
            }
            self.brush.SetColor(&col(c));
            let style = factory
                .CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() }, None)
                .ok();
            self.ctx.DrawGeometry(&path, self.brush, width, style.as_ref());
        }
    }

    fn draw_text(&mut self, text: &str, r: Rect, size: f32, weight: u16, c: Color, align: Align) {
        let Some(fmt) = self.format(size, weight, align) else { return };
        let c = if self.glass { glass_text(c) } else { c };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.DrawText(&wide, &fmt, &rf(r), self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    fn draw_display(&mut self, text: &str, r: Rect, size: f32, tracking: f32, c: Color, align: Align) {
        let Some(font) = self.display else { return self.draw_text(text, r, size, 800, c, align) };
        let Some(layout) = font.layout(text, size, tracking, r.w, r.h, align) else { return };
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.DrawTextLayout(windows_numerics::Vector2 { X: r.x, Y: r.y }, &layout, self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
    }

    fn measure_display(&mut self, text: &str, size: f32, tracking: f32) -> f32 {
        let Some(font) = self.display else { return self.measure_text(text, size, 800) };
        let Some(layout) = font.layout(text, size, tracking, 10_000.0, 1_000.0, Align::Left) else { return 0.0 };
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { if layout.GetMetrics(&mut m).is_ok() { m.width } else { 0.0 } }
    }

    fn measure_text(&mut self, text: &str, size: f32, weight: u16) -> f32 {
        let Some(fmt) = self.format(size, weight, Align::Left) else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.dw.CreateTextLayout(&wide, &fmt, 10_000.0, 1_000.0) else { return 0.0 };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() { m.widthIncludingTrailingWhitespace } else { 0.0 }
        }
    }

    fn load_image(&mut self, rgba: &[u8], w: u32, h: u32) -> ImageId {
        let mut bgra = Vec::with_capacity(rgba.len());
        for px in rgba.chunks_exact(4) {
            let a = px[3] as u32;
            let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            bgra.extend_from_slice(&[m(px[2]), m(px[1]), m(px[0]), px[3]]);
        }
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let id = ImageId(self.next_image.get());
        self.next_image.set(id.0 + 1);
        unsafe {
            if let Ok(bitmap) = self.ctx.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(bgra.as_ptr() as *const _), w * 4, &props) {
                let mut images = self.images.borrow_mut();
                images.insert(id, bitmap);
                // Only the newest few covers are ever drawn; free the rest.
                images.retain(|k, _| k.0 + 4 > id.0);
            }
        }
        id
    }

    fn draw_image(&mut self, id: ImageId, r: Rect, radius: f32) {
        self.draw_image_alpha(id, r, radius, 1.0);
    }

    fn draw_image_alpha(&mut self, id: ImageId, r: Rect, radius: f32, alpha: f32) {
        let images = self.images.borrow();
        let Some(bitmap) = images.get(&id) else { return };
        unsafe {
            let size = bitmap.GetSize();
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let props = D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_INTERPOLATION_MODE_LINEAR,
            };
            let brush_props = D2D1_BRUSH_PROPERTIES {
                opacity: alpha.clamp(0.0, 1.0),
                transform: Matrix3x2 { M11: r.w / size.width, M12: 0.0, M21: 0.0, M22: r.h / size.height, M31: r.x, M32: r.y },
            };
            if let Ok(b) = self.ctx.CreateBitmapBrush(bitmap, Some(&props), Some(&brush_props)) {
                self.ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rf(r), radiusX: radius, radiusY: radius }, &b);
            }
        }
    }

    fn push_shift(&mut self, dx: f32, alpha: f32) {
        unsafe {
            let mut old = Matrix3x2::identity();
            self.ctx.GetTransform(&mut old);
            let mut moved = old;
            moved.M31 += dx;
            self.ctx.SetTransform(&moved);
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: D2D_RECT_F { left: -1e9, top: -1e9, right: 1e9, bottom: 1e9 },
                geometricMask: std::mem::ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: alpha.clamp(0.0, 1.0),
                opacityBrush: std::mem::ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
            self.shifts.push(old);
        }
    }

    fn pop_shift(&mut self) {
        unsafe {
            if let Some(old) = self.shifts.pop() {
                self.ctx.PopLayer();
                self.ctx.SetTransform(&old);
            }
        }
    }

    fn push_clip(&mut self, r: Rect, radius: f32) {
        unsafe {
            if radius > 0.0 {
                if let Some(g) = self.rounded_geometry(r, radius) {
                    let params = D2D1_LAYER_PARAMETERS1 {
                        contentBounds: D2D_RECT_F { left: -1e9, top: -1e9, right: 1e9, bottom: 1e9 },
                        geometricMask: std::mem::ManuallyDrop::new(Some(g)),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: Matrix3x2::identity(),
                        opacity: 1.0,
                        opacityBrush: std::mem::ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                    };
                    self.ctx.PushLayer(&params, None);
                    // Release the geometry reference we leaked into the struct.
                    drop(std::mem::ManuallyDrop::into_inner(params.geometricMask));
                    self.clips.push(true);
                    return;
                }
            }
            self.ctx.PushAxisAlignedClip(&rf(r), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.clips.push(false);
        }
    }

    fn pop_clip(&mut self) {
        unsafe {
            match self.clips.pop() {
                Some(true) => self.ctx.PopLayer(),
                Some(false) => self.ctx.PopAxisAlignedClip(),
                None => {}
            }
        }
    }
}
