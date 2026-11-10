//! Strong backdrop blur through Windows.UI.Composition: the backdrop brush samples what is behind
//! the window, then a Direct2D Gaussian blur and a saturation effect are applied, like CSS
//! `backdrop-filter: blur(32px) saturate(170%)`. Based on the approach in ALTaleX531/Win32Acrylic.
//! The effect graph needs a hand-written `IGraphicsEffect` (Win2D is not usable in desktop apps).

use std::cell::RefCell;
use windows::core::{implement, Interface, GUID, HSTRING, PCWSTR};
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{CompositionEffectSourceParameter, Compositor, SpriteVisual};
use windows::Win32::Foundation::{E_NOTIMPL, E_INVALIDARG, HWND};
use windows::Win32::Graphics::Direct2D::{CLSID_D2D1GaussianBlur, CLSID_D2D1Saturation};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING};
use windows::Win32::System::WinRT::{CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT};
use windows_numerics::Vector2;

enum Prop {
    Float(f32),
    Uint(u32),
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct Effect {
    id: GUID,
    name: RefCell<HSTRING>,
    props: Vec<Prop>,
    sources: Vec<IGraphicsEffectSource>,
}

impl IGraphicsEffectSource_Impl for Effect_Impl {}

impl IGraphicsEffect_Impl for Effect_Impl {
    fn Name(&self) -> windows::core::Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }
    fn SetName(&self, name: &HSTRING) -> windows::core::Result<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for Effect_Impl {
    fn GetEffectId(&self) -> windows::core::Result<GUID> {
        Ok(self.id)
    }
    fn GetNamedPropertyMapping(&self, _name: &PCWSTR, _index: *mut u32, _mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING) -> windows::core::Result<()> {
        Err(E_NOTIMPL.into())
    }
    fn GetPropertyCount(&self) -> windows::core::Result<u32> {
        Ok(self.props.len() as u32)
    }
    fn GetProperty(&self, index: u32) -> windows::core::Result<IPropertyValue> {
        match self.props.get(index as usize) {
            Some(Prop::Float(v)) => PropertyValue::CreateSingle(*v)?.cast(),
            Some(Prop::Uint(v)) => PropertyValue::CreateUInt32(*v)?.cast(),
            None => Err(E_INVALIDARG.into()),
        }
    }
    fn GetSource(&self, index: u32) -> windows::core::Result<IGraphicsEffectSource> {
        self.sources.get(index as usize).cloned().ok_or_else(|| E_INVALIDARG.into())
    }
    fn GetSourceCount(&self) -> windows::core::Result<u32> {
        Ok(self.sources.len() as u32)
    }
}

fn effect(id: GUID, props: Vec<Prop>, source: IGraphicsEffectSource) -> IGraphicsEffect {
    Effect { id, name: RefCell::new(HSTRING::new()), props, sources: vec![source] }.into()
}

/// What keeps the composition alive for one window.
pub struct Backdrop {
    _controller: Option<DispatcherQueueController>,
    _compositor: Compositor,
    _target: DesktopWindowTarget,
    _visual: SpriteVisual,
}

/// Puts a blurred, saturated copy of what is behind `hwnd` into it. `blur` is the Gaussian
/// standard deviation in pixels (CSS blur(32px) is 32) and `saturation` 1.0 is unchanged.
/// `hwnd` must have been created with WS_EX_NOREDIRECTIONBITMAP.
pub fn attach(hwnd: HWND, blur: f32, saturation: f32) -> windows::core::Result<Backdrop> {
    unsafe {
        // One dispatcher queue per thread: a second glass window on this thread reuses it.
        let controller = if windows::System::DispatcherQueue::GetForCurrentThread().is_ok() {
            None
        } else {
            Some(CreateDispatcherQueueController(DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            })?)
        };
        let compositor = Compositor::new().map_err(|e| step(1, e))?;
        let interop: ICompositorDesktopInterop = compositor.cast().map_err(|e| step(2, e))?;
        let target = interop.CreateDesktopWindowTarget(hwnd, true).map_err(|e| step(3, e))?;

        let backdrop = compositor.CreateBackdropBrush().map_err(|e| step(4, e))?;
        let source = CompositionEffectSourceParameter::Create(&HSTRING::from("Backdrop")).map_err(|e| step(5, e))?;
        // D2D1_GAUSSIANBLUR props: stddev, optimization (1 = balanced), border mode (1 = hard).
        let blurred = effect(CLSID_D2D1GaussianBlur, vec![Prop::Float(blur), Prop::Uint(1), Prop::Uint(1)], source.cast().map_err(|e| step(20, e))?);
        let saturated = effect(CLSID_D2D1Saturation, vec![Prop::Float(saturation)], blurred.cast().map_err(|e| step(21, e))?);
        let factory = compositor.CreateEffectFactory(&saturated).map_err(|e| step(6, e))?;
        let brush = factory.CreateBrush().map_err(|e| step(7, e))?;
        brush.SetSourceParameter(&HSTRING::from("Backdrop"), &backdrop).map_err(|e| step(8, e))?;

        let visual = compositor.CreateSpriteVisual().map_err(|e| step(9, e))?;
        visual.SetRelativeSizeAdjustment(Vector2 { X: 1.0, Y: 1.0 }).map_err(|e| step(10, e))?;
        visual.SetBrush(&brush).map_err(|e| step(11, e))?;
        target.SetRoot(&visual).map_err(|e| step(12, e))?;
        Ok(Backdrop { _controller: controller, _compositor: compositor, _target: target, _visual: visual })
    }
}

fn step(n: u32, e: windows::core::Error) -> windows::core::Error {
    crate::window::glass_log(&format!("backdrop step {n} failed: {e}"));
    e
}
