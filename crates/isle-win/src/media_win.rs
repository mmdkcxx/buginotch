// SPDX-License-Identifier: GPL-3.0-or-later
//! System media sessions (SMTC): what is playing, and transport commands.
//! A worker thread owns the subscriptions; changes are posted to the UI thread.

use crate::system::epoch_ms;
use crate::window::WM_APP_MEDIA;
use isle_logic::media::{CoverImage, MediaInfo, friendly_app};
use std::sync::{Arc, Mutex};
use windows::{
    Foundation::TypedEventHandler,
    Media::Control::*,
    Storage::Streams::DataReader,
    Win32::{
        Foundation::*,
        System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize},
        UI::WindowsAndMessaging::PostMessageW,
    },
};

type Manager = GlobalSystemMediaTransportControlsSessionManager;
type Session = GlobalSystemMediaTransportControlsSession;

struct Subs {
    session: Session,
    tokens: [i64; 3],
}

struct Inner {
    manager: Manager,
    hwnd: isize,
    state: Mutex<Option<MediaInfo>>,
    session: Mutex<Option<Session>>,
    subs: Mutex<Option<Subs>>,
}

pub struct MediaWatcher {
    inner: Arc<Mutex<Option<Arc<Inner>>>>,
}

impl MediaWatcher {
    pub fn spawn(hwnd: HWND) -> MediaWatcher {
        let slot: Arc<Mutex<Option<Arc<Inner>>>> = Arc::new(Mutex::new(None));
        let slot2 = Arc::clone(&slot);
        let hwnd = hwnd.0 as isize;
        std::thread::spawn(move || {
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let Ok(manager) = Manager::RequestAsync().and_then(|op| op.join()) else {
                crate::log("SMTC: cannot create session manager");
                return;
            };
            let inner = Arc::new(Inner { manager, hwnd, state: Mutex::new(None), session: Mutex::new(None), subs: Mutex::new(None) });
            *slot2.lock().unwrap() = Some(Arc::clone(&inner));
            let a = Arc::clone(&inner);
            let _ = inner.manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
                attach(&a);
                Ok(())
            }));
            attach(&inner);
            loop {
                std::thread::park();
            }
        });
        MediaWatcher { inner: slot }
    }

    fn inner(&self) -> Option<Arc<Inner>> {
        self.inner.lock().ok()?.clone()
    }

    pub fn latest(&self) -> Option<MediaInfo> {
        self.inner()?.state.lock().ok()?.clone()
    }

    fn with_session(&self, f: impl FnOnce(&Session)) {
        if let Some(inner) = self.inner() {
            if let Some(s) = inner.session.lock().ok().and_then(|g| g.clone()) {
                f(&s);
            }
        }
    }

    pub fn play_pause(&self) {
        self.with_session(|s| {
            let _ = s.TryTogglePlayPauseAsync();
        });
    }

    pub fn next(&self) {
        self.with_session(|s| {
            let _ = s.TrySkipNextAsync();
        });
    }

    pub fn previous(&self) {
        self.with_session(|s| {
            let _ = s.TrySkipPreviousAsync();
        });
    }

    pub fn seek(&self, ms: i64) {
        self.with_session(|s| {
            let _ = s.TryChangePlaybackPositionAsync(ms.max(0) * 10_000);
        });
    }
}

fn notify(inner: &Inner) {
    unsafe {
        let _ = PostMessageW(Some(HWND(inner.hwnd as *mut _)), WM_APP_MEDIA, WPARAM(0), LPARAM(0));
    }
}

/// (Re)subscribes to the current session and reads it in full.
fn attach(inner: &Arc<Inner>) {
    if let Some(old) = inner.subs.lock().unwrap().take() {
        let _ = old.session.RemoveMediaPropertiesChanged(old.tokens[0]);
        let _ = old.session.RemovePlaybackInfoChanged(old.tokens[1]);
        let _ = old.session.RemoveTimelinePropertiesChanged(old.tokens[2]);
    }
    let current = inner.manager.GetCurrentSession().ok();
    *inner.session.lock().unwrap() = current.clone();
    if let Some(s) = current {
        let (a, b, c) = (Arc::clone(inner), Arc::clone(inner), Arc::clone(inner));
        let t0 = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            refresh(&a, true);
            Ok(())
        }));
        let t1 = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            refresh(&b, false);
            Ok(())
        }));
        let t2 = s.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
            refresh(&c, false);
            Ok(())
        }));
        if let (Ok(t0), Ok(t1), Ok(t2)) = (t0, t1, t2) {
            *inner.subs.lock().unwrap() = Some(Subs { session: s, tokens: [t0, t1, t2] });
        }
    }
    refresh(inner, true);
}

/// `full` re-reads title/artist/cover; otherwise only playback state and position.
fn refresh(inner: &Arc<Inner>, full: bool) {
    let session = inner.session.lock().unwrap().clone();
    let new_state = session.and_then(|s| read(&s, if full { None } else { inner.state.lock().unwrap().clone() }));
    *inner.state.lock().unwrap() = new_state;
    notify(inner);
}

fn read(s: &Session, previous: Option<MediaInfo>) -> Option<MediaInfo> {
    let playback = s.GetPlaybackInfo().ok()?;
    let playing = playback.PlaybackStatus().ok()? == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
    let timeline = s.GetTimelineProperties().ok()?;
    let ticks = |t: windows::Foundation::TimeSpan| t.Duration / 10_000;
    let position_ms = timeline.Position().map(ticks).unwrap_or(0);
    let duration_ms = (timeline.EndTime().map(ticks).unwrap_or(0) - timeline.StartTime().map(ticks).unwrap_or(0)).max(0);
    let updated_at_ms = timeline
        .LastUpdatedTime()
        .map(|t| t.UniversalTime / 10_000 - 11_644_473_600_000)
        .ok()
        .filter(|&t| t > 0)
        .unwrap_or_else(epoch_ms);

    let (title, artist, album, cover) = match previous {
        Some(p) => (p.title, p.artist, p.album, p.cover),
        None => {
            let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
            let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
            let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
            let album = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
            (title, artist, album, read_cover(&props))
        }
    };
    let app = friendly_app(&s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default());
    Some(MediaInfo { title, artist, album, app, playing, position_ms, duration_ms, updated_at_ms, cover })
}

fn read_cover(props: &GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<CoverImage> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 || size > 8_000_000 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf).ok()?;
    let img = image::load_from_memory(&buf).ok()?.thumbnail(192, 192).to_rgba8();
    Some(CoverImage { width: img.width(), height: img.height(), rgba: img.into_raw() })
}
