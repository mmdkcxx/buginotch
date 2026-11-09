// SPDX-License-Identifier: GPL-3.0-or-later
//! DirectComposition + Direct2D/DirectWrite rendering of the notch into a
//! per-pixel-alpha swap chain. Frames are drawn only on request.

use crate::d2d_canvas::D2dCanvas;
use isle_logic::canvas::{Canvas, ImageId};
use std::collections::HashMap;
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::*,
            Direct3D11::*,
            DirectComposition::*,
            DirectWrite::*,
            Dxgi::{Common::*, *},
        },
    },
    core::*,
};

pub struct Renderer {
    d2d_ctx: ID2D1DeviceContext,
    swap: IDXGISwapChain1,
    dw: IDWriteFactory,
    brush: ID2D1SolidColorBrush,
    images: HashMap<ImageId, ID2D1Bitmap1>,
    next_image: u32,
    font_family: HSTRING,
    dpi: f32,
    _dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    _visual: IDCompositionVisual,
}

impl Renderer {
    pub fn new(hwnd: HWND, w_px: u32, h_px: u32, dpi: f32) -> Result<Renderer> {
        unsafe {
            let mut d3d: Option<ID3D11Device> = None;
            let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
            let hw = D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None);
            if hw.is_err() {
                D3D11CreateDevice(None, D3D_DRIVER_TYPE_WARP, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None)?;
            }
            let d3d = d3d.ok_or_else(|| Error::from(E_FAIL))?;
            let dxgi: IDXGIDevice = d3d.cast()?;

            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let d2d_dev = d2d_factory.CreateDevice(&dxgi)?;
            let d2d_ctx = d2d_dev.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            d2d_ctx.SetDpi(dpi, dpi);

            let adapter = dxgi.GetAdapter()?;
            let factory: IDXGIFactory2 = adapter.GetParent()?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: w_px.max(1),
                Height: h_px.max(1),
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                ..Default::default()
            };
            let swap = factory.CreateSwapChainForComposition(&d3d, &desc, None)?;

            let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
            let target = dcomp.CreateTargetForHwnd(hwnd, true)?;
            let visual = dcomp.CreateVisual()?;
            visual.SetContent(&swap)?;
            target.SetRoot(&visual)?;
            dcomp.Commit()?;

            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let font_family = pick_font(&dw);
            let brush = d2d_ctx.CreateSolidColorBrush(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, None)?;

            Ok(Renderer {
                d2d_ctx,
                swap,
                dw,
                brush,
                images: HashMap::new(),
                next_image: 1,
                font_family,
                dpi,
                _dcomp: dcomp,
                _target: target,
                _visual: visual,
            })
        }
    }

    pub fn resize(&mut self, w_px: u32, h_px: u32, dpi: f32) -> Result<()> {
        unsafe {
            self.d2d_ctx.SetTarget(None);
            self.swap.ResizeBuffers(0, w_px.max(1), h_px.max(1), DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
            self.d2d_ctx.SetDpi(dpi, dpi);
            self.dpi = dpi;
        }
        Ok(())
    }

    /// Draws one frame: clears to transparent, runs `f`, presents.
    pub fn frame<F: FnOnce(&mut dyn Canvas)>(&mut self, f: F) -> Result<()> {
        unsafe {
            let surface: IDXGISurface = self.swap.GetBuffer(0)?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: self.dpi,
                dpiY: self.dpi,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                ..Default::default()
            };
            let bitmap = self.d2d_ctx.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
            self.d2d_ctx.SetTarget(&bitmap);
            self.d2d_ctx.BeginDraw();
            self.d2d_ctx.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));
            {
                let mut canvas = D2dCanvas::new(&self.d2d_ctx, &self.dw, &self.brush, &self.images, &self.font_family);
                f(&mut canvas);
            }
            let end = self.d2d_ctx.EndDraw(None, None);
            self.d2d_ctx.SetTarget(None);
            end?;
            let _ = self.swap.Present(1, DXGI_PRESENT(0));
        }
        Ok(())
    }

    /// Registers RGBA8 pixels (straight alpha) as an image usable with `Canvas::draw_image`.
    pub fn register_image(&mut self, rgba: &[u8], w: u32, h: u32) -> Result<ImageId> {
        let mut bgra = Vec::with_capacity(rgba.len());
        for px in rgba.chunks_exact(4) {
            let a = px[3] as u32;
            let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
            bgra.extend_from_slice(&[m(px[2]), m(px[1]), m(px[0]), px[3]]);
        }
        let props = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let bitmap = unsafe { self.d2d_ctx.CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(bgra.as_ptr() as *const _), w * 4, &props)? };
        let id = ImageId(self.next_image);
        self.next_image += 1;
        self.images.insert(id, bitmap);
        Ok(id)
    }
}

/// "Segoe UI Variable Text" if installed (Windows 11), else "Segoe UI".
fn pick_font(dw: &IDWriteFactory) -> HSTRING {
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dw.GetSystemFontCollection(&mut coll, false).is_ok() {
            if let Some(coll) = coll {
                let mut index = 0u32;
                let mut exists = BOOL(0);
                if coll.FindFamilyName(w!("Segoe UI Variable Text"), &mut index, &mut exists).is_ok() && exists.as_bool() {
                    return HSTRING::from("Segoe UI Variable Text");
                }
            }
        }
    }
    HSTRING::from("Segoe UI")
}
