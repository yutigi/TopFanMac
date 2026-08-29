//! Thermal sensors via IOHIDEventSystem.
//!
//! STATUS: verified working on this M3 Max, unprivileged -- 46 temperature
//! services present, 45 returning live values. This is the read path the
//! governor runs on, and it needs no root, which is why `topfan status` can be
//! unprivileged even though mode changes are not.
//!
//! These are private APIs. They have been stable across many macOS releases but
//! carry no compatibility promise; `Thermals::open` failing is a normal
//! condition to handle, not an invariant to assert.

use crate::ffi::*;
use std::ffi::{c_void, CString};

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub name: String,
    pub celsius: f32,
}

impl Reading {
    /// `tdie*` sensors sit on the die itself and are what fan control should
    /// track. `tdev*` read much cooler (device/board) and would badly
    /// under-drive the fans if mistaken for die temperature -- on this machine
    /// they differ by about 30 C at idle.
    pub fn is_die(&self) -> bool {
        self.name.contains("tdie")
    }
}

pub struct Thermals {
    client: *mut IOHIDEventSystemClient,
    services: CFArrayRef,
}

// SAFETY: the client is only ever touched through &self methods on the thread
// that owns the Thermals value; we never hand out the raw pointer.
unsafe impl Send for Thermals {}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).expect("no interior NUL");
    // SAFETY: valid NUL-terminated UTF-8.
    unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8) }
}

impl Thermals {
    pub fn open() -> Option<Self> {
        // SAFETY: creating the client with the default allocator and matching
        // everything; both calls are null-checked.
        unsafe {
            let client = IOHIDEventSystemClientCreate(std::ptr::null());
            if client.is_null() {
                return None;
            }
            IOHIDEventSystemClientSetMatching(client, std::ptr::null());
            let services = IOHIDEventSystemClientCopyServices(client);
            if services.is_null() {
                return None;
            }
            Some(Thermals { client, services })
        }
    }

    /// Every temperature sensor that answered this poll.
    pub fn read_all(&self) -> Vec<Reading> {
        let mut out = Vec::new();
        // SAFETY: iterating a CFArray by index within its count; each Copy call
        // returns +1 refs that we release.
        unsafe {
            let page_key = cfstr("PrimaryUsagePage");
            let usage_key = cfstr("PrimaryUsage");
            let product_key = cfstr("Product");
            let count = CFArrayGetCount(self.services);
            for i in 0..count {
                let svc = CFArrayGetValueAtIndex(self.services, i) as *mut IOHIDServiceClient;
                if svc.is_null() {
                    continue;
                }
                if read_i32(svc, page_key) != Some(USAGE_PAGE_APPLE_VENDOR)
                    || read_i32(svc, usage_key) != Some(USAGE_TEMPERATURE)
                {
                    continue;
                }
                let event = IOHIDServiceClientCopyEvent(svc, IOHID_EVENT_TYPE_TEMPERATURE, 0, 0);
                if event.is_null() {
                    continue;
                }
                let celsius =
                    IOHIDEventGetFloatValue(event, iohid_event_field(IOHID_EVENT_TYPE_TEMPERATURE))
                        as f32;
                CFRelease(event as CFTypeRef);
                let name = read_string(svc, product_key).unwrap_or_else(|| "(unnamed)".into());
                out.push(Reading { name, celsius });
            }
            CFRelease(page_key);
            CFRelease(usage_key);
            CFRelease(product_key);
        }
        out
    }

    /// The hottest die sensor -- the governor's input signal.
    ///
    /// Falls back to the hottest sensor of any kind if no die sensor answered,
    /// so a naming change upstream degrades the signal instead of blinding the
    /// controller.
    pub fn hottest_die(&self) -> Option<f32> {
        let all = self.read_all();
        let hottest_die = all
            .iter()
            .filter(|r| r.is_die())
            .map(|r| r.celsius)
            .fold(f32::NEG_INFINITY, f32::max);
        if hottest_die.is_finite() {
            return Some(hottest_die);
        }
        let any = all
            .iter()
            .map(|r| r.celsius)
            .fold(f32::NEG_INFINITY, f32::max);
        any.is_finite().then_some(any)
    }
}

impl Drop for Thermals {
    fn drop(&mut self) {
        // SAFETY: both came from Copy/Create calls we own.
        unsafe {
            if !self.services.is_null() {
                CFRelease(self.services);
            }
            if !self.client.is_null() {
                CFRelease(self.client as CFTypeRef);
            }
        }
    }
}

unsafe fn read_i32(svc: *mut IOHIDServiceClient, key: CFStringRef) -> Option<i32> {
    let prop = IOHIDServiceClientCopyProperty(svc, key);
    if prop.is_null() {
        return None;
    }
    let mut v: i32 = 0;
    let ok = CFNumberGetValue(prop, K_CF_NUMBER_INT_TYPE, &mut v as *mut _ as *mut c_void);
    CFRelease(prop);
    ok.then_some(v)
}

unsafe fn read_string(svc: *mut IOHIDServiceClient, key: CFStringRef) -> Option<String> {
    let prop = IOHIDServiceClientCopyProperty(svc, key);
    if prop.is_null() {
        return None;
    }
    let mut buf = [0i8; 128];
    let ok = CFStringGetCString(prop, buf.as_mut_ptr(), 128, K_CF_STRING_ENCODING_UTF8);
    CFRelease(prop);
    if !ok {
        return None;
    }
    let bytes: Vec<u8> = buf
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn die_sensors_are_distinguished_from_device_sensors() {
        let die = Reading {
            name: "PMU tdie8".into(),
            celsius: 63.0,
        };
        let dev = Reading {
            name: "PMU tdev8".into(),
            celsius: 30.0,
        };
        assert!(die.is_die());
        assert!(!dev.is_die(), "tdev must not be mistaken for tdie");
    }
}
