// SPDX-License-Identifier: GPL-3.0-or-later
//! Lists the active audio outputs that are headphones or headsets, on a worker thread,
//! and posts the connect / disconnect events to the UI thread.

use crate::window::WM_APP_MEDIA;
use isle_logic::headphones::{Change, Endpoint, Tracker, is_personal_audio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::*,
        Media::Audio::*,
        System::Com::*,
        UI::WindowsAndMessaging::PostMessageW,
    },
    core::GUID,
};

/// How often the list of outputs is checked.
const POLL: Duration = Duration::from_secs(2);

/// `PKEY_AudioEndpoint_FormFactor`
const FORM_FACTOR: PROPERTYKEY = PROPERTYKEY { fmtid: GUID::from_u128(0x1da5d803_d492_4edd_8c23_e0c0ffee7f0e), pid: 0 };

#[derive(Clone, Default)]
pub struct HeadphoneWatcher {
    events: Arc<Mutex<Vec<Change>>>,
}

impl HeadphoneWatcher {
    pub fn spawn(hwnd: HWND) -> Self {
        let watcher = Self::default();
        let events = Arc::clone(&watcher.events);
        let hwnd = hwnd.0 as isize;
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            let mut tracker = Tracker::default();
            let mut first = true;
            loop {
                match list() {
                    Ok(current) => {
                        if std::mem::take(&mut first) {
                            crate::log(&format!("headphones: {} personal output(s) at start: {:?}", current.len(), current.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()));
                        }
                        let changes = tracker.update(&current);
                        if !changes.is_empty() {
                            events.lock().unwrap().extend(changes);
                            unsafe {
                                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
                            }
                        }
                    }
                    Err(e) => crate::log(&format!("headphones: cannot list outputs: {e}")),
                }
                std::thread::sleep(POLL);
            }
        });
        watcher
    }

    /// Changes since the last call.
    pub fn take(&self) -> Vec<Change> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

fn list() -> windows::core::Result<Vec<Endpoint>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut out = Vec::new();
        for i in 0..devices.GetCount()? {
            let device = devices.Item(i)?;
            let store = device.OpenPropertyStore(STGM_READ)?;
            let form = store.GetValue(&FORM_FACTOR).ok().and_then(|v| u32::try_from(&v).ok());
            if !is_personal_audio(form) {
                continue;
            }
            let name = store.GetValue(&PKEY_Device_FriendlyName).map(|v| v.to_string()).unwrap_or_default();
            let id = device.GetId()?.to_string()?;
            out.push(Endpoint { id, name });
        }
        Ok(out)
    }
}
