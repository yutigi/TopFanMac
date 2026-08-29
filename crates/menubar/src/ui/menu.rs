//! Menu construction and per-render updates (T012, T021).
//!
//! Built from the [`crate::actions`] table; every update comes from the
//! polled [`SurfaceState`] -- the checkmark is never set from a click
//! (FR-005). Degraded states are pure renders of the same state (T021).

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem};
use objc2_foundation::NSString;

use crate::actions::{self, Action};
use crate::state::{self, SurfaceState};

/// The status-item menu, top to bottom (FR-002):
/// Auto | Managed | Full | Off | [hint line] | --- | Open Dashboard | Quit
pub struct Menu {
    pub menu: Retained<NSMenu>,
    mode_items: Vec<(Action, Retained<NSMenuItem>)>,
    hint_item: Retained<NSMenuItem>,
    #[allow(dead_code)] // used by tests-facing access later; kept for clarity
    pub open_dashboard_item: Retained<NSMenuItem>,
    #[allow(dead_code)]
    pub quit_item: Retained<NSMenuItem>,
}

/// How menu rows reach the app handlers (target + action). `mod.rs` supplies
/// the root object once; `menu.rs` never names it.
pub struct MenuTargets<'a> {
    pub target: &'a ProtocolObject<dyn NSObjectProtocol>,
    pub sel: Sel,
}

impl Menu {
    pub fn build(targets: &MenuTargets) -> Self {
        let mtm = MainThreadMarker::new().expect("menu built on the main thread");
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("TopFan"));
        // State is driven exclusively by SurfaceState in `update`; AppKit's
        // enable-on-has-target heuristic must not interfere.
        menu.setAutoenablesItems(false);

        let mut mode_items = Vec::new();
        for (index, action) in actions::MENU.into_iter().enumerate() {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(action.label()),
                    None,
                    &NSString::from_str(""),
                )
            };
            item.setTag(index as isize);
            // NSAction target: the object, as plain `&AnyObject` (AppKit
            // retains it). `setTarget`/`setAction` are unsafe because the API
            // cannot verify selector validity.
            unsafe {
                item.setTarget(Some(targets.target.as_ref()));
                item.setAction(Some(targets.sel));
            }
            menu.addItem(&item);
            if action.verb().is_some() {
                mode_items.push((action, item));
            }
        }

        // Hint line below "Off", then a separator above Open Dashboard:
        // [Auto, Managed, Full, Off, hint, sep, Open Dashboard, Quit]
        let hint = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(""),
                None,
                &NSString::from_str(""),
            )
        };
        hint.setEnabled(false);
        hint.setHidden(true);
        menu.insertItem_atIndex(&hint, 4);
        let sep = NSMenuItem::separatorItem(mtm);
        menu.insertItem_atIndex(&sep, 5);

        let items: Vec<_> = menu.itemArray().into_iter().collect();
        let open_item = items[6].clone();
        let quit_item = items[7].clone();

        Self {
            menu,
            mode_items,
            hint_item: hint,
            open_dashboard_item: open_item,
            quit_item,
        }
    }

    /// Re-render from polled state + any delegation notice. Never reads a
    /// click: enabled rows follow `can_control`, the checkmark follows the
    /// polled mode exactly (contracts/surfaces.md rendering table).
    pub fn update(&self, st: &SurfaceState, notice: Option<&str>) {
        let enabled = actions::can_control(st);

        // Checkmark: the state item matching the *polled* mode, or none.
        let checked = match st {
            SurfaceState::Live(s) | SurfaceState::ReadOnly(s) => {
                Some(actions::checked_item(s.mode))
            }
            SurfaceState::Unavailable => None,
        };

        for (action, item) in &self.mode_items {
            item.setState(if checked == Some(*action) {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            item.setEnabled(enabled);
        }

        // Hint line: delegation notice > unavailable hint > read-only reason.
        let hint = match notice {
            Some(n) => Some(n.to_string()),
            None => match st {
                SurfaceState::Live(_) => None,
                SurfaceState::ReadOnly(_) => Some(state::READ_ONLY_REASON.to_string()),
                SurfaceState::Unavailable => Some(state::UNAVAILABLE_HINT.to_string()),
            },
        };
        match hint {
            Some(text) => {
                self.hint_item.setTitle(&NSString::from_str(&text));
                self.hint_item.setHidden(false);
            }
            None => self.hint_item.setHidden(true),
        }
    }
}
