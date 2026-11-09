// SPDX-License-Identifier: GPL-3.0-or-later
//! Files and the Windows shell: thumbnails, dragging files out, copying files to the
//! clipboard, and receiving files dropped on the notch.

use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{
            Com::*,
            Ole::*,
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
        UI::Shell::{Common::*, *},
    },
    core::*,
};

/// Starts OLE on this (UI) thread; needed for drag and drop and the file clipboard.
pub fn init() {
    let _ = unsafe { OleInitialize(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A picture of the file (its thumbnail or icon) as straight-alpha RGBA, `px` square at most.
pub fn thumbnail(path: &str, px: i32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let p = wide(path);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(p.as_ptr()), None).ok()?;
        let hbm = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_BIGGERSIZEOK).ok()?;
        let mut bm = BITMAP::default();
        if GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
            let _ = DeleteObject(hbm.into());
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let hdc = GetDC(None);
        let rows = GetDIBits(hdc, hbm, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, hdc);
        let _ = DeleteObject(hbm.into());
        if rows == 0 {
            return None;
        }
        // BGRA (premultiplied when the image has alpha) -> straight RGBA.
        let has_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
        for p in buf.chunks_exact_mut(4) {
            let a = if has_alpha { p[3] } else { 255 };
            let un = |c: u8| if a == 0 || a == 255 { c } else { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 };
            let (b, g, r) = (un(p[0]), un(p[1]), un(p[2]));
            p[0] = r;
            p[1] = g;
            p[2] = b;
            p[3] = a;
        }
        Some((buf, w as u32, h as u32))
    }
}

/// Makes thumbnails on a background thread, so the shell's message pumping never runs
/// inside a paint.
pub struct ThumbWorker {
    tx: std::sync::mpsc::Sender<String>,
    rx: std::sync::mpsc::Receiver<(String, Option<(Vec<u8>, u32, u32)>)>,
}

impl ThumbWorker {
    pub fn spawn() -> Self {
        let (tx, jobs) = std::sync::mpsc::channel::<String>();
        let (done, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            for path in jobs {
                let picture = thumbnail(&path, 128);
                if done.send((path, picture)).is_err() {
                    break;
                }
            }
        });
        Self { tx, rx }
    }

    pub fn request(&self, path: &str) {
        let _ = self.tx.send(path.to_string());
    }

    pub fn take(&self) -> Vec<(String, Option<(Vec<u8>, u32, u32)>)> {
        self.rx.try_iter().collect()
    }
}

/// A shell data object holding these files (what Explorer itself would hand out). The
/// shell wants one parent folder and the files as children of it, so only files that share
/// the first file's folder are included.
fn files_object(paths: &[String]) -> Option<IDataObject> {
    unsafe {
        let mut fulls: Vec<*mut ITEMIDLIST> = Vec::new();
        for p in paths {
            let w = wide(p);
            let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
            if SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None).is_ok() && !pidl.is_null() {
                fulls.push(pidl);
            }
        }
        let first = *fulls.first()?;
        let parent = ILClone(first);
        ILRemoveLastID(Some(parent));
        let mut children: Vec<*const ITEMIDLIST> = Vec::new();
        for &f in &fulls {
            let up = ILClone(f);
            ILRemoveLastID(Some(up));
            if ILIsEqual(up, parent).as_bool() {
                children.push(ILFindLastID(f) as *const _);
            }
            ILFree(Some(up));
        }
        let obj = SHCreateDataObject::<Option<&IDataObject>, IDataObject>(Some(parent as *const _), Some(&children), None::<&IDataObject>).ok();
        ILFree(Some(parent));
        for f in fulls {
            CoTaskMemFree(Some(f as *const _));
        }
        obj
    }
}

/// Puts the files on the clipboard as files (paste into Explorer, a chat, an email).
pub fn copy_files(paths: &[String]) -> bool {
    match files_object(paths) {
        Some(obj) => unsafe { OleSetClipboard(&obj).is_ok() },
        None => false,
    }
}

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }
    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Runs a drag of these files out of the notch; returns when the mouse button is released.
pub fn drag_files(paths: &[String]) {
    let Some(obj) = files_object(paths) else {
        crate::log("shelf: could not build a data object for the drag");
        return;
    };
    let source: IDropSource = DropSource.into();
    let mut effect = DROPEFFECT_NONE;
    let _ = unsafe { DoDragDrop(&obj, &source, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect) };
}

/// The file paths a data object carries (empty when it holds no files).
fn dropped_paths(obj: &IDataObject) -> Vec<String> {
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    let mut out = Vec::new();
    unsafe {
        let Ok(mut medium) = obj.GetData(&fmt) else { return out };
        let hdrop = HDROP(medium.u.hGlobal.0);
        let n = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
        for i in 0..n {
            let len = DragQueryFileW(hdrop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            out.push(String::from_utf16_lossy(&buf[..len]));
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

fn carries_files(obj: &IDataObject) -> bool {
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    unsafe { obj.QueryGetData(&fmt) == S_OK }
}

/// What the notch does when files are dragged over it.
pub struct DropHandlers {
    pub hover: fn(HWND, bool),
    pub files: fn(HWND, Vec<String>),
}

#[implement(IDropTarget)]
struct DropTarget {
    hwnd: HWND,
    on: DropHandlers,
    /// Set while the dragged data carries files.
    accepting: std::cell::Cell<bool>,
}

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        let ok = data.as_ref().is_some_and(carries_files);
        self.accepting.set(ok);
        unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if ok {
            (self.on.hover)(self.hwnd, true);
        }
        Ok(())
    }
    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        unsafe { *effect = if self.accepting.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        if self.accepting.get() {
            (self.on.hover)(self.hwnd, true);
        }
        Ok(())
    }
    fn DragLeave(&self) -> Result<()> {
        if self.accepting.replace(false) {
            (self.on.hover)(self.hwnd, false);
        }
        Ok(())
    }
    fn Drop(&self, data: Ref<IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
        self.accepting.set(false);
        let paths = data.as_ref().map(dropped_paths).unwrap_or_default();
        unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
        (self.on.files)(self.hwnd, paths);
        Ok(())
    }
}

/// Makes the notch window accept dropped files.
pub fn register_drop_target(hwnd: HWND, on: DropHandlers) {
    let target: IDropTarget = DropTarget { hwnd, on, accepting: std::cell::Cell::new(false) }.into();
    unsafe {
        let _ = RegisterDragDrop(hwnd, &target);
    }
}
