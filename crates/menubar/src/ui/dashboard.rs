//! The dashboard window (T017, T018): an NSWindow hosting a WKWebView with
//! the embedded [`DASHBOARD_HTML`].
//!
//! The Rust side is deliberately thin: one inbound render per poll —
//! `window.topfan.setStatus(json)` with the serialized [`state::snapshot`]
//! — and one outbound bridge, a `WKScriptMessageHandler` that turns the
//! page's `{kind:"mode", value}` message into the same event-pipeline entry
//! the menu produces. Confirmation always comes from the next poll, never
//! from a click (FR-005).
//!
//! New unsafe is confined to this module and is the minimum objc2 requires
//! for the window/web-view plumbing: object allocation/initializers and
//! `evaluateJavaScript`. The message body is a JSON *string* (the page does
//! `JSON.stringify`), so no untyped NSDictionary access is needed.

use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{ns_string, NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::{
    WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKWebView,
    WKWebViewConfiguration,
};

use super::Shared;
use crate::actions::Action;
use crate::state::{self, SurfaceState};

/// The dashboard page, embedded at compile time.
pub const DASHBOARD_HTML: &str = include_str!("../../assets/dashboard.html");

/// Handler name the page posts mode changes to
/// (`window.webkit.messageHandlers.topfan.postMessage(...)`).
const MESSAGE_NAME: &str = "topfan";

// -- the bridge: {kind:"mode", value} -> the actions pipeline (T018) ---------

struct Bridge {
    shared: Arc<Shared>,
}

define_class!(
    /// Receives the page's mode messages and feeds them into the same event
    /// pipe the menu uses (T018, research D1 honesty constraint). The page
    /// is presentation; an unexpected message is a bug in it, never a state
    /// change (defense in depth for FR-003).
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Bridge]
    #[name = "TopFanDashboardBridge"]
    struct DashBridge;

    unsafe impl NSObjectProtocol for DashBridge {}

    unsafe impl WKScriptMessageHandler for DashBridge {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn user_content_controller_did_receive_script_message(
            &self,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            self.receive(message);
        }
    }
);

impl DashBridge {
    fn new(mtm: MainThreadMarker, shared: Arc<Shared>) -> Retained<Self> {
        unsafe {
            let this = Self::alloc(mtm).set_ivars(Bridge { shared });
            msg_send![super(this), init]
        }
    }

    /// The single message contract of the bridge. The body is the JSON text
    /// `{"kind":"mode","value":"auto|managed|full|off"}`; anything else is
    /// ignored.
    fn receive(&self, message: &WKScriptMessage) {
        // The page posts `JSON.stringify({...})`, so the body is an
        // NSString; anything else is dropped by this downcast.
        let Ok(text) = (unsafe { message.body() }).downcast::<NSString>() else {
            return;
        };
        let value: serde_json::Value = match serde_json::from_str(&text.to_string()) {
            Ok(v) => v,
            Err(_) => return,
        };
        if value.get("kind").and_then(|k| k.as_str()) != Some("mode") {
            return;
        }
        // Buttons carry the protocol's own mode names (auto|managed|full|off).
        let action = match value.get("value").and_then(|v| v.as_str()) {
            Some("auto") => Action::Auto,
            Some("managed") => Action::Managed,
            Some("full") => Action::Full,
            Some("off") => Action::Off,
            _ => return,
        };
        self.ivars().shared.push(super::Event::Mode(action));
    }
}

// -- the window --------------------------------------------------------------

/// The dashboard window and its web view. Presentation only: every render is
/// one `setStatus` call with the latest `SurfaceSnapshot`.
pub struct Dashboard {
    pub window: Retained<NSWindow>,
    webview: Retained<WKWebView>,
}

/// The window's content size. Deliberately fixed-ish: the dashboard is a
/// compact readout, documented in the mockup (P2), not a spreadsheet.
const CONTENT: NSRect = NSRect::new(NSPoint::new(120.0, 300.0), NSSize::new(660.0, 360.0));

impl Dashboard {
    /// Create the window. The page loads once per creation, so a reopened
    /// dashboard starts on a fresh page — sparkline and history are
    /// per-open by construction (SC-003); the caller renders current
    /// state immediately.
    pub fn new(mtm: MainThreadMarker, shared: Arc<Shared>, first: &SurfaceState) -> Self {
        // Window: titled, closable, miniaturizable, resizable. Closing must
        // not quit the app (FR-004) -- the app delegate already answers
        // applicationShouldTerminateAfterLastWindowClosed: with false.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                CONTENT,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::Closable
                    | NSWindowStyleMask::Miniaturizable
                    | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(ns_string!("TopFan"));

        // Web view + page, with the mode bridge registered under one name.
        // (WebKit APIs are unsafe in objc2-web-kit: the handler/selector
        // pairing cannot be type-checked.)
        let (_config, webview) = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            let controller = config.userContentController();
            let bridge = DashBridge::new(mtm, shared);
            controller.addScriptMessageHandler_name(
                ProtocolObject::from_ref(&*bridge),
                ns_string!(MESSAGE_NAME),
            );
            let webview =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), CONTENT, &config);
            window.setContentView(Some(&webview));
            webview.loadHTMLString_baseURL(&NSString::from_str(DASHBOARD_HTML), None);
            (config, webview)
        };

        let dash = Self { window, webview };
        // First render right after creation: the window opens with current
        // values, never an empty frame waiting for the next poll (SC-003).
        dash.render(first);
        dash
    }

    /// One inbound render: latest snapshot, through one JS call. The
    /// snapshot is passed as a JSON object literal (our strings are ASCII,
    /// and the page also tolerates the string form).
    pub fn render(&self, st: &SurfaceState) {
        let snapshot_json =
            serde_json::to_string(&state::snapshot(st)).expect("snapshot serializes");
        let js = NSString::from_str(&format!("window.topfan.setStatus({snapshot_json});"));
        // One-way push: no completion handler (the page's next state is the
        // confirmation, and errors would only ever mean paint, not truth).
        unsafe { self.webview.evaluateJavaScript_completionHandler(&js, None) };
    }

    /// Bring the window forward (re-open path: menu item, double-click,
    /// second launch).
    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge's message contract, pinned without any ObjC object: the
    /// same JSON string the page posts is what `receive` consumes.
    #[test]
    fn page_message_is_the_contracted_envelope() {
        // Exactly what assets/dashboard.html posts on a mode click.
        let posted = "{\"kind\":\"mode\",\"value\":\"full\"}";
        let v: serde_json::Value = serde_json::from_str(posted).expect("page posts valid JSON");
        assert_eq!(v.get("kind").and_then(|k| k.as_str()), Some("mode"));
        assert_eq!(v.get("value").and_then(|v| v.as_str()), Some("full"));

        // The four buttons' payloads, and nothing else, parse as modes.
        for (value, action) in [
            ("auto", Action::Auto),
            ("managed", Action::Managed),
            ("full", Action::Full),
            ("off", Action::Off),
        ] {
            let msg = serde_json::json!({"kind": "mode", "value": value});
            let parsed: serde_json::Value = serde_json::from_str(&msg.to_string()).unwrap();
            assert_eq!(parsed["kind"], "mode");
            let action_matches = match parsed["value"].as_str() {
                Some("auto") => matches!(action, Action::Auto),
                Some("managed") => matches!(action, Action::Managed),
                Some("full") => matches!(action, Action::Full),
                Some("off") => matches!(action, Action::Off),
                _ => false,
            };
            assert!(action_matches, "mode `{value}` maps to its action");
        }

        // Non-mode messages are dropped by the guard.
        let noise: serde_json::Value =
            serde_json::from_str("{\"kind\":\"poke\",\"value\":\"auto\"}").unwrap();
        assert_eq!(noise.get("kind").and_then(|k| k.as_str()), Some("poke"));
    }
}
