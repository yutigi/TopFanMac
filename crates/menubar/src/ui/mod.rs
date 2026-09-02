//! The AppKit application (T010, T011, T014).
//!
//! Presentation only, per research D8: every value shown comes from a
//! `SurfaceState` derived by `state.rs`; every action funnels through
//! `actions.rs`'s table. The `MainThreadMarker` is acquired here, once.

pub mod dashboard;
pub mod menu;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{BufRead as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSMenu, NSMenuDelegate,
    NSMenuItem, NSStatusBar, NSStatusItem,
};
use objc2_foundation::{ns_string, NSString, NSTimer};

use crate::actions::{self, Action};
use crate::client;
use crate::delegate::{self, Outcome};
use crate::render_title;
use crate::state::{self, SurfaceState};

/// -1.0 == the system's variable-length status item constant.
const VARIABLE_LENGTH: f64 = -1.0;

/// Events from background threads and UI actions, drained on the main
/// thread. Nothing here mutates fan state directly -- delegation via the
/// `topfan` CLI is the only write path (FR-003).
pub(crate) enum Event {
    Mode(Action),
    OpenDashboard,
    Quit,
    Delegated(Outcome, &'static str),
}

#[derive(Default)]
pub(crate) struct Shared {
    events: Mutex<VecDeque<Event>>,
    notice: Mutex<Option<String>>,
}

impl Shared {
    pub(crate) fn push(&self, ev: Event) {
        self.events
            .lock()
            .expect("events lock poisoned")
            .push_back(ev);
    }

    fn notice(&self) -> Option<String> {
        self.notice.lock().expect("notice lock").clone()
    }
}

/// App state carried as the root object's ivars. Mutable presentation state
/// sits in `RefCell`s -- every access runs on the main thread (research D7).
/// The menu is built after the root object exists (it targets the root), so
/// it enters the ivars through a `RefCell`.
struct App {
    shared: Arc<Shared>,
    item: Retained<NSStatusItem>,
    menu: RefCell<Option<menu::Menu>>,
    last_state: RefCell<Option<SurfaceState>>,
    poll_timer: RefCell<Option<Retained<NSTimer>>>,
    service_timer: RefCell<Option<Retained<NSTimer>>>,
    /// Created on first open and kept: the page lives across close/reopen,
    /// so re-showing is instant and no object dangles.
    dashboard: RefCell<Option<dashboard::Dashboard>>,
}

define_class!(
    /// The single root object: app delegate, menu delegate, timer target,
    /// and menu action target -- all main-thread only.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = App]
    #[name = "TopFanRoot"]
    struct Root;

    unsafe impl NSObjectProtocol for Root {}

    // Closing the dashboard window must not end the app (FR-004, FR-008):
    // the menu-bar item owns the app's lifecycle.
    unsafe impl NSApplicationDelegate for Root {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn application_should_terminate_after_last_window_closed(&self, _app: &NSApplication) -> bool {
            false
        }
    }

    unsafe impl NSMenuDelegate for Root {
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, _menu: Option<&NSMenu>) {
            // Re-render from the last poll; the 2 s cadence bounds how stale
            // the checkmark can be, and re-polling from a menu callback
            // would block the UI (FR-005).
            self.render_menu();
        }
    }

    impl Root {
        #[unsafe(method(menuAction:))]
        fn menu_action(&self, sender: Option<&NSMenuItem>) {
            let Some(item) = sender else { return };
            let index = usize::try_from(item.tag()).unwrap_or(usize::MAX);
            let Some(action) = actions::MENU.get(index) else { return };
            match *action {
                Action::OpenDashboard => self.ivars().shared.push(Event::OpenDashboard),
                Action::Quit => self.ivars().shared.push(Event::Quit),
                mode => self.ivars().shared.push(Event::Mode(mode)),
            }
            // Honest: nothing is ticked or confirmed here; the next poll settles it.
        }

        #[unsafe(method(pollTick:))]
        fn poll_tick(&self, _timer: Option<&NSTimer>) {
            self.poll_and_render();
        }

        #[unsafe(method(serviceTick:))]
        fn service_tick(&self, _timer: Option<&NSTimer>) {
            self.drain_events();
        }
    }
);

// The root object's behaviour, outside the ObjC ABI surface.
impl Root {
    fn new(
        mtm: MainThreadMarker,
        shared: Arc<Shared>,
        item: Retained<NSStatusItem>,
    ) -> Retained<Self> {
        unsafe {
            let this = Self::alloc(mtm).set_ivars(App {
                shared,
                item,
                menu: RefCell::new(None),
                last_state: RefCell::new(None),
                poll_timer: RefCell::new(None),
                service_timer: RefCell::new(None),
                dashboard: RefCell::new(None),
            });
            msg_send![super(this), init]
        }
    }

    /// One poll tick: outcome -> SurfaceState -> both surfaces (FR-005).
    fn poll_and_render(&self) {
        let st = state::derive(client::poll());
        *self.ivars().last_state.borrow_mut() = Some(st.clone());
        self.render_all(&st);
    }

    /// The one render path both surfaces share (status item title, T011).
    /// `render_title` verbatim for Live and ReadOnly; the distinct
    /// no-numbers presentation otherwise (FR-001, FR-005;
    /// contracts/surfaces.md Unavailable row).
    fn render_all(&self, st: &SurfaceState) {
        let ivars = self.ivars();
        let title = match st {
            SurfaceState::Live(s) | SurfaceState::ReadOnly(s) => render_title(s),
            SurfaceState::Unavailable => "--  --".to_string(),
        };
        // Timer/menu callbacks only ever run on the main run loop.
        let mtm = MainThreadMarker::new().expect("render_all runs on the main thread");
        if let Some(button) = ivars.item.button(mtm) {
            button.setTitle(&NSString::from_str(&title));
        }
        // The dashboard mirrors the same state on the same cadence while it
        // is open (FR-005): one render per poll, nothing else.
        if let Some(dash) = ivars.dashboard.borrow().as_ref() {
            dash.render(st);
        }
        self.render_menu();
    }

    /// Menu state is a pure render of (SurfaceState, notice): the checkmark
    /// follows the polled mode, disabled rows follow `can_control`, and the
    /// hint line follows the degradation contract (T012, T021).
    fn render_menu(&self) {
        let ivars = self.ivars();
        let st = self.ivars().last_state.borrow().clone();
        if let Some(menu) = ivars.menu.borrow().as_ref() {
            if let Some(st) = st.as_ref() {
                menu.update(st, ivars.shared.notice().as_deref());
            }
        }
    }

    fn drain_events(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let ivars = self.ivars();
        let events: Vec<Event> = {
            let mut q = ivars.shared.events.lock().expect("events lock");
            q.drain(..).collect()
        };
        for ev in events {
            match ev {
                Event::OpenDashboard => self.open_dashboard(),
                Event::Quit => self.quit(mtm),
                Event::Mode(action) => self.perform_mode(action),
                Event::Delegated(outcome, verb) => {
                    if let Some(hint) = delegate::hint_for(outcome, verb) {
                        // One short honest hint; state unchanged (FR-005:
                        // the next poll shows the truth, not this outcome).
                        *ivars.shared.notice.lock().expect("notice lock") = Some(hint);
                        self.render_menu();
                    }
                }
            }
        }
    }

    /// Mode change through the single sanctioned path: discover `topfan`,
    /// then the system admin prompt runs the CLI (FR-003, research D4).
    fn perform_mode(&self, action: Action) {
        let Some(verb) = action.verb() else { return };
        let ivars = self.ivars();

        // The surfaces only enable actions in Live, but guard here too so a
        // click racing a degradation can never prompt uselessly.
        if !self.controllable_now() {
            return;
        }
        let Some(topfan) = delegate::find_topfan(&delegate::default_topfan_candidates()) else {
            // No prompt: prompting a command that cannot succeed would be
            // dishonest (Constitution VI).
            ivars
                .shared
                .push(Event::Delegated(Outcome::TopfanMissing, verb));
            return;
        };
        let shared = Arc::clone(&ivars.shared);
        delegate::spawn_delegation(topfan, verb, move |outcome| {
            shared.push(Event::Delegated(outcome, verb));
        });
    }

    /// The guard above: one quick unprivileged read, so a click handler
    /// does not trust a state that may be up to 2 s stale.
    fn controllable_now(&self) -> bool {
        matches!(state::derive(client::poll()), SurfaceState::Live(_))
    }

    /// Dashboard open path (T019): menu item, double-click, direct launch,
    /// or the single-instance forward all land here. An already-open window is
    /// just raised; a *closed* one is rebuilt rather than re-shown, so the page
    /// reloads and the sparkline history restarts per viewing (SC-003,
    /// quickstart scenario 8) instead of carrying over the previous session's.
    fn open_dashboard(&self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let ivars = self.ivars();
        let mut slot = ivars.dashboard.borrow_mut();
        let closed = slot.as_ref().is_some_and(|d| !d.is_open());
        if closed {
            // Dropping the old `Dashboard` releases its window and web view.
            *slot = None;
        }
        if slot.is_none() {
            // The freshest state there is as of this very tick: the last
            // poll's result. A fresh window renders it immediately (SC-003).
            let st = ivars
                .last_state
                .borrow()
                .clone()
                .unwrap_or(state::SurfaceState::Unavailable);
            *slot = Some(dashboard::Dashboard::new(
                mtm,
                Arc::clone(&ivars.shared),
                &st,
            ));
        }
        if let Some(dash) = slot.as_ref() {
            dash.show();
        }
    }

    /// Quit ends the UI process only; the daemon and fans are untouched
    /// (FR-010, Constitution I).
    fn quit(&self, mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        app.terminate(None);
    }
}

// -- single instance (T014) --------------------------------------------------

mod single_instance {
    use super::*;

    /// Where the lock socket lives (the only user-domain artifact, FR-010).
    pub fn socket_path() -> std::path::PathBuf {
        std::env::temp_dir().join("topfan-ui.lock")
    }

    pub enum Role {
        /// Bound the lock socket; forward later launches to this instance.
        Primary(UnixListener),
        /// Forwarded `open-dashboard` to the primary; caller exits quietly.
        Secondary,
    }

    pub fn acquire() -> Role {
        let path = socket_path();
        match UnixListener::bind(&path) {
            Ok(listener) => return Role::Primary(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {}
            Err(_) => return Role::Secondary,
        }
        if forward(&path) {
            return Role::Secondary;
        }
        // Nothing alive owns the socket (stale) -- unlink and re-bind once.
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(listener) => Role::Primary(listener),
            Err(_) => Role::Secondary, // give up quietly rather than double-run
        }
    }

    /// Second-launch behaviour (FR-008): send one line, wait <= 2 s for an
    /// ack, then the caller exits -- a single instance and a single item
    /// remain.
    fn forward(path: &std::path::Path) -> bool {
        let Ok(stream) = UnixStream::connect(path) else {
            return false;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
        let Ok(mut writer) = stream.try_clone() else {
            return false;
        };
        if writer.write_all(b"{\"cmd\":\"open-dashboard\"}\n").is_err() {
            return false;
        }
        let mut reader = std::io::BufReader::new(stream);
        let mut ack = String::new();
        reader.read_line(&mut ack).is_ok() && ack.contains("ack")
    }

    /// Primary-side acceptor (research D7): forward each peer's
    /// `open-dashboard` to the app via the event queue.
    pub fn spawn_acceptor(listener: UnixListener, shared: Arc<Shared>) {
        std::thread::Builder::new()
            .name("topfan-ui-lock".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
                    let mut line = String::new();
                    let mut reader = std::io::BufReader::new(&stream);
                    if reader.read_line(&mut line).is_err() {
                        continue;
                    }
                    use std::io::Write as _;
                    if stream
                        .try_clone()
                        .and_then(|mut w| w.write_all(b"{\"ack\":true}\n"))
                        .is_ok()
                    {
                        shared.push(Event::OpenDashboard);
                    }
                }
            })
            .expect("spawn lock acceptor");
    }
}

// -- entry point ------------------------------------------------------------

/// Run the app on the main thread. Returns only after Quit.
pub fn run() {
    let mtm = MainThreadMarker::new().expect("menubar GUI must run on the main thread");

    // Same selectors as the declared methods; runtime-created (`sel!` is
    // not const-callable).
    let sel_menu_action = objc2::sel!(menuAction:);
    let sel_poll_tick = objc2::sel!(pollTick:);
    let sel_service_tick = objc2::sel!(serviceTick:);

    // Single instance first (FR-008): a second launch forwards and exits
    // before it ever creates a status item.
    let listener = match single_instance::acquire() {
        single_instance::Role::Secondary => return,
        single_instance::Role::Primary(listener) => listener,
    };

    let shared = Arc::new(Shared::default());
    single_instance::spawn_acceptor(listener, Arc::clone(&shared));

    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon: menu-bar presence only (unbundled binary, research D7).
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Status item (T011): variable length, title from render_title.
    let status_bar = NSStatusBar::systemStatusBar();
    let item = status_bar.statusItemWithLength(VARIABLE_LENGTH);
    if let Some(button) = item.button(mtm) {
        button.setTitle(ns_string!("--  --"));
    }

    // Root object, then the menu built onto it as its target.
    let root = Root::new(mtm, Arc::clone(&shared), item.clone());
    {
        // The target borrow lives only for the build call; each menu item
        // retains the root object under ARC when it stores the target.
        let proto = ProtocolObject::from_ref(&*root);
        let targets = menu::MenuTargets {
            target: proto,
            sel: sel_menu_action,
        };
        let m = menu::Menu::build(&targets);
        // Menu attached to the item: single-click opens it natively; the OS
        // draws it against any background (contracts/surfaces.md, SC-005).
        item.setMenu(Some(&m.menu));
        root.ivars().menu.replace(Some(m));
    }

    // First poll happens now; SC-001's ~2 s bound is the timer's job after.
    root.poll_and_render();

    unsafe {
        let poll_timer = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
            2.0,
            &root,
            sel_poll_tick,
            None,
            true,
        );
        let service_timer =
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                0.25,
                &root,
                sel_service_tick,
                None,
                true,
            );
        *root.ivars().poll_timer.borrow_mut() = Some(poll_timer);
        *root.ivars().service_timer.borrow_mut() = Some(service_timer);
    }

    app.run();
}

// Selectors are created at runtime in `run()`; `sel!` is not const-callable.
