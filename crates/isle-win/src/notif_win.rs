// SPDX-License-Identifier: GPL-3.0-or-later
//! Reads Windows toast notifications (UserNotificationListener) on a worker thread.
//! Windows' change events need an app package, so the list is simply checked every few seconds.

use isle_logic::notifs::Notif;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::NotificationKinds;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::HSTRING;

const EVERY: std::time::Duration = std::time::Duration::from_secs(3);
/// 100 ns ticks between 1601-01-01 and 1970-01-01, in ms.
const EPOCH_DIFF_MS: i64 = 11_644_473_600_000;

/// The newest list the worker read, taken by the module.
static LATEST: Mutex<Option<Vec<Notif>>> = Mutex::new(None);

/// An app's logo as RGBA pixels.
pub struct Icon {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Logos found so far, by app name. Apps whose logo cannot be read are simply missing.
static ICONS: Mutex<Option<HashMap<String, Arc<Icon>>>> = Mutex::new(None);

pub fn icon(app: &str) -> Option<Arc<Icon>> {
    ICONS.lock().ok()?.as_ref()?.get(app).cloned()
}

pub fn take() -> Option<Vec<Notif>> {
    LATEST.lock().ok()?.take()
}

pub fn spawn() {
    std::thread::spawn(|| {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        let Ok(listener) = UserNotificationListener::Current() else {
            crate::log("notifications: no listener");
            return;
        };
        let allowed = listener.RequestAccessAsync().and_then(|op| op.join()).is_ok_and(|s| s == UserNotificationListenerAccessStatus::Allowed);
        if !allowed {
            crate::log("notifications: access not allowed");
            return;
        }
        crate::log("notifications: access allowed");
        let mut tried: HashSet<String> = HashSet::new();
        loop {
            if let Ok(list) = read(&listener, &mut tried) {
                if let Ok(mut slot) = LATEST.lock() {
                    *slot = Some(list);
                }
            }
            std::thread::sleep(EVERY);
        }
    });
}

fn read(l: &UserNotificationListener, tried: &mut HashSet<String>) -> windows::core::Result<Vec<Notif>> {
    let list = l.GetNotificationsAsync(NotificationKinds::Toast)?.join()?;
    let mut out = Vec::new();
    for n in list {
        let mut one = || -> windows::core::Result<Notif> {
            let app = n.AppInfo()?.DisplayInfo()?.DisplayName()?.to_string();
            let binding = n.Notification()?.Visual()?.GetBinding(&HSTRING::from("ToastGeneric"))?;
            let mut texts = Vec::new();
            for t in binding.GetTextElements()? {
                texts.push(t.Text()?.to_string());
            }
            let mut texts = texts.into_iter();
            let title = texts.next().unwrap_or_default();
            let body = texts.collect::<Vec<_>>().join(" ");
            if tried.insert(app.clone()) {
                if let Some(icon) = read_logo(&n) {
                    if let Ok(mut m) = ICONS.lock() {
                        m.get_or_insert_with(HashMap::new).insert(app.clone(), Arc::new(icon));
                    }
                }
            }
            let at_ms = n.CreationTime()?.UniversalTime / 10_000 - EPOCH_DIFF_MS;
            Ok(Notif { id: n.Id()?, app, title, body, at_ms })
        };
        if let Ok(item) = one() {
            if !(item.title.is_empty() && item.body.is_empty()) {
                out.push(item);
            }
        }
    }
    Ok(out)
}

fn read_logo(n: &windows::UI::Notifications::UserNotification) -> Option<Icon> {
    use windows::Foundation::Size;
    use windows::Storage::Streams::DataReader;
    let logo = n.AppInfo().ok()?.DisplayInfo().ok()?.GetLogo(Size { Width: 64.0, Height: 64.0 }).ok()?;
    let stream = logo.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 4_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let img = image::load_from_memory(&buf).ok()?.thumbnail(64, 64).to_rgba8();
    Some(Icon { w: img.width(), h: img.height(), rgba: img.into_raw() })
}
