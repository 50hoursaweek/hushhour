//! Frosted glass: the page is translucent and, where the compositor supports it,
//! the desktop behind the window is blurred and frosted.
//!
//! Blur behind a window is the compositor's job on Wayland. KDE Plasma's KWin
//! offers the `org_kde_kwin_blur` and `org_kde_kwin_contrast` protocols, which
//! are bound here on the window's own `wl_surface`. Compositors without them
//! (COSMIC, GNOME) get the translucent page only.
//!
//! # Safety invariant
//!
//! The display and surface belong to winit; this module borrows them. Using them is
//! sound because of the following, which `main.rs` upholds:
//!
//! 1. Requests are sent from exactly two places, both on the event-loop thread while
//!    the window is alive by construction. [`Frost::attach`] runs inside a `window::run`
//!    closure, to which iced lends the live `&dyn Window` for the duration of the call.
//!    [`Frost::apply`] and [`Frost::release`] run inside `Inkpad::update`.
//! 2. The window outlives every `update` call and every `window::run` closure: iced
//!    destroys its windows only after the event loop has stopped, and the loop stops
//!    only once an `iced::exit()` task has been processed.
//! 3. `iced::exit()` is issued from one place, `Inkpad::quit`, which first calls
//!    [`Frost::release`] and drops the handle. Every path that stops the loop goes
//!    through it: Quit (clean, after "Don't save", or after a save), the compositor's
//!    close request (`exit_on_close_request(false)` routes it there), and the
//!    `--soundtest` and `--selftest` diagnostic runs. The self-test's failure path is
//!    the one deliberate exception: it calls `process::exit(1)`, which terminates the
//!    process without unwinding or running any destructor, so neither winit's nor this
//!    module's cleanup runs and no ordering question arises.
//! 4. Messages can still be delivered after `quit` has run and before the loop has
//!    actually stopped. A `FrostAttached` arriving then is released and dropped inside
//!    that `update`, so its release requests, like every other request, go out while
//!    the display is still connected. Nothing is sent once the loop has stopped.
//! 5. The guest event queue is wrapped in `ManuallyDrop` and never destroyed. It holds a
//!    `Connection` clone of a backend built from the foreign display, so leaking it keeps
//!    that backend alive for the rest of the process. Retained: the `wl_event_queue`, the
//!    backend's bookkeeping, and the client-side proxies it knows about (the registry,
//!    the bound blur and contrast managers, and any blur or contrast object not yet
//!    released). Avoided: wayland-backend's `ConnectionState::drop`, which would destroy
//!    those proxies and the queue, and must not run after winit has disconnected the
//!    display. The backend does not own the display, so `wl_display_disconnect` is never
//!    at stake. The blur and contrast objects themselves are released by protocol in
//!    `release`, so the compositor side is cleaned up before exit; the managers and the
//!    registry live until the connection closes with the process.
//!
//! This is a runtime invariant, documented and checked by inspection and by the
//! headless quit tests in `main.rs`; it is not a type-level guarantee.

use std::fmt;
use std::sync::{Arc, Mutex};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols_plasma::blur::client::{org_kde_kwin_blur::OrgKdeKwinBlur, org_kde_kwin_blur_manager::OrgKdeKwinBlurManager};
use wayland_protocols_plasma::contrast::client::{
    org_kde_kwin_contrast::OrgKdeKwinContrast, org_kde_kwin_contrast_manager::OrgKdeKwinContrastManager,
};

/// What the user asked for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub enabled: bool,
    /// 0 = opaque page, 1 = fully see-through.
    pub transparency: f32,
    /// 0 = clear glass, 1 = heavily frosted.
    pub thickness: f32,
    /// The page colour, used to tint the frost.
    pub tint: [u8; 3],
}

/// Handle to the compositor-side effects for the main window.
#[derive(Clone)]
pub struct Frost(Arc<Mutex<Inner>>);

struct Inner {
    conn: Connection,
    /// Kept alive so objects created with `qh` stay valid; never dispatched (no events
    /// matter) and never destroyed: see the safety invariant in the module docs.
    _queue: std::mem::ManuallyDrop<EventQueue<State>>,
    qh: QueueHandle<State>,
    surface: WlSurface,
    blur_manager: Option<OrgKdeKwinBlurManager>,
    contrast_manager: Option<OrgKdeKwinContrastManager>,
    blur: Option<OrgKdeKwinBlur>,
    contrast: Option<OrgKdeKwinContrast>,
}

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore OrgKdeKwinBlurManager);
delegate_noop!(State: ignore OrgKdeKwinBlur);
delegate_noop!(State: ignore OrgKdeKwinContrastManager);
delegate_noop!(State: ignore OrgKdeKwinContrast);

impl Frost {
    /// Bind to the window's Wayland surface. `None` when not on Wayland.
    pub fn attach(window: &dyn iced::window::Window) -> Option<Self> {
        let RawWindowHandle::Wayland(wh) = window.window_handle().ok()?.as_raw() else { return None };
        let RawDisplayHandle::Wayland(dh) = window.display_handle().ok()?.as_raw() else { return None };
        // SAFETY: winit owns this display and surface and keeps them alive for the
        // window's lifetime; the backend created here does not take ownership.
        let (conn, surface) = unsafe {
            let backend = Backend::from_foreign_display(dh.display.as_ptr().cast());
            let conn = Connection::from_backend(backend);
            let id = ObjectId::from_ptr(WlSurface::interface(), wh.surface.as_ptr().cast()).ok()?;
            let surface = WlSurface::from_id(&conn, id).ok()?;
            (conn, surface)
        };
        let (globals, queue) = registry_queue_init::<State>(&conn).ok()?;
        let qh = queue.handle();
        let blur_manager = globals.bind(&qh, 1..=1, ()).ok();
        let contrast_manager = globals.bind(&qh, 1..=2, ()).ok();
        Some(Self(Arc::new(Mutex::new(Inner {
            conn,
            _queue: std::mem::ManuallyDrop::new(queue),
            qh,
            surface,
            blur_manager,
            contrast_manager,
            blur: None,
            contrast: None,
        }))))
    }

    /// Release the compositor-side objects; call before the window goes away.
    pub fn release(&self) {
        let Ok(mut inner) = self.0.lock() else { return };
        if let Some(blur) = inner.blur.take() {
            blur.release();
        }
        if let Some(contrast) = inner.contrast.take() {
            contrast.release();
        }
        let _ = inner.conn.flush();
    }

    /// Whether this compositor can blur behind the window.
    pub fn blur_supported(&self) -> bool {
        self.0.lock().map(|i| i.blur_manager.is_some()).unwrap_or(false)
    }

    /// Request (or release) the effects to match `settings`. Takes effect on the next frame.
    pub fn apply(&self, settings: Settings) {
        let Ok(mut inner) = self.0.lock() else { return };
        let inner = &mut *inner;
        let want = settings.enabled && settings.thickness > 0.0;
        if !want {
            if let Some(blur) = inner.blur.take() {
                blur.release();
            }
            if let Some(contrast) = inner.contrast.take() {
                contrast.release();
            }
            let _ = inner.conn.flush();
            return;
        }
        if inner.blur.is_none()
            && let Some(manager) = &inner.blur_manager
        {
            let blur = manager.create(&inner.surface, &inner.qh, ());
            blur.set_region(None);
            blur.commit();
            inner.blur = Some(blur);
        }
        if let Some(manager) = &inner.contrast_manager {
            let contrast = inner.contrast.get_or_insert_with(|| manager.create(&inner.surface, &inner.qh, ()));
            let t = settings.thickness as f64;
            // Thicker glass washes colour out and lifts the backdrop a little.
            contrast.set_region(None);
            contrast.set_contrast(1.0 + 0.25 * t);
            contrast.set_intensity(1.0 + 0.15 * t);
            contrast.set_saturation(1.0 - 0.6 * t);
            if contrast.version() >= 2 {
                let [r, g, b] = settings.tint;
                let alpha = (40.0 + 150.0 * t) as i32;
                contrast.set_frost(r as i32, g as i32, b as i32, alpha);
            }
            contrast.commit();
        }
        let _ = inner.conn.flush();
    }
}

impl fmt::Debug for Frost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Frost")
    }
}
