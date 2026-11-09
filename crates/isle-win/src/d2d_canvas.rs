// SPDX-License-Identifier: GPL-3.0-or-later
//! `Canvas` implemented on Direct2D + DirectWrite. Coordinates are DIPs.

use isle_logic::canvas::{Align, Canvas, Color, ImageId, Rect};
use std::collections::HashMap;
use windows_numerics::{Matrix3x2, Vector2};
use windows::{
    Win32::Graphics::{
        Direct2D::{Common::*, *},
        DirectWrite::*,
    },
    core::*,
};

pub struct D2dCanvas<'a> {
    ctx: &'a ID2D1DeviceContext,
    dw: &'a IDWriteFactory,
    brush: &'a ID2D1SolidColorBrush,
    images: &'a HashMap<ImageId, ID2D1Bitmap1>,
    family: &'a HSTRING,
    clips: Vec<bool>, // true = layer, false = axis-aligned clip
}

fn col(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0 as f32 / 255.0, g: c.1 as f32 / 255.0, b: c.2 as f32 / 255.0, a: c.3 as f32 / 255.0 }
}

fn rf(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

impl<'a> D2dCanvas<'a> {
    pub fn new(
        ctx: &'a ID2D1DeviceContext,
        dw: &'a IDWriteFactory,
        brush: &'a ID2D1SolidColorBrush,
        images: &'a HashMap<ImageId, ID2D1Bitmap1>,
        family: &'a HSTRING,
    ) -> Self {
        Self { ctx, dw, brush, images, family, clips: Vec::new() }
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
    fn fill_round_rect(&mut self, r: Rect, radius: f32, c: Color) {
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rf(r), radiusX: radius, radiusY: radius }, self.brush);
        }
    }

    fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r }, self.brush);
        }
    }

    fn stroke_arc(&mut self, cx: f32, cy: f32, r: f32, start_deg: f32, sweep_deg: f32, width: f32, c: Color) {
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
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.brush.SetColor(&col(c));
            self.ctx.DrawText(&wide, &fmt, &rf(r), self.brush, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
        }
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

    fn draw_image(&mut self, id: ImageId, r: Rect, radius: f32) {
        let Some(bitmap) = self.images.get(&id) else { return };
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
                opacity: 1.0,
                transform: Matrix3x2 { M11: r.w / size.width, M12: 0.0, M21: 0.0, M22: r.h / size.height, M31: r.x, M32: r.y },
            };
            if let Ok(b) = self.ctx.CreateBitmapBrush(bitmap, Some(&props), Some(&brush_props)) {
                self.ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rf(r), radiusX: radius, radiusY: radius }, &b);
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
