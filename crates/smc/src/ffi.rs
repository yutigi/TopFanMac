//! Raw IOKit / IOHIDEventSystem declarations.
//!
//! This is the ONLY module in the workspace that declares foreign functions.
//! Everything here is `unsafe`; every caller outside this crate gets a safe
//! `Result`-returning wrapper. See `crate::smc` and `crate::hid`.
//!
//! We declare the externs by hand rather than depending on `io-kit-sys` or
//! `objc2-io-kit`: the C ABI for these is stable, and hand-declaring keeps the
//! menu-bar crate free to pick its own objc2 generation without ABI drift
//! between two IOKit binding crates in one binary.

use std::ffi::c_void;
use std::os::raw::c_char;

pub type IoObject = u32;
pub type KernReturn = i32;
pub const KERN_SUCCESS: KernReturn = 0;

/// `kIOMainPortDefault` is NULL/0 on every macOS that ships Apple Silicon.
pub const K_IO_MAIN_PORT_DEFAULT: u32 = 0;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    pub fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    pub fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> IoObject;
    pub fn IOServiceOpen(
        service: IoObject,
        owning_task: u32,
        type_: u32,
        connect: *mut IoObject,
    ) -> KernReturn;
    pub fn IOServiceClose(connect: IoObject) -> KernReturn;
    pub fn IOObjectRelease(object: IoObject) -> KernReturn;
    pub fn IOConnectCallStructMethod(
        connection: IoObject,
        selector: u32,
        input_struct: *const c_void,
        input_struct_cnt: usize,
        output_struct: *mut c_void,
        output_struct_cnt: *mut usize,
    ) -> KernReturn;
}

extern "C" {
    /// `mach_task_self()` is a macro over this global in C.
    pub static mach_task_self_: u32;
}

// --- IOHIDEventSystem (private, but the only route to Apple Silicon sensors) ---

pub enum IOHIDEventSystemClient {}
pub enum IOHIDServiceClient {}
pub enum IOHIDEvent {}

pub type CFAllocatorRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFTypeRef = *const c_void;
pub type CFIndex = isize;

/// `kIOHIDEventTypeTemperature`. The float field is `type << 16`.
pub const IOHID_EVENT_TYPE_TEMPERATURE: i64 = 15;
pub const fn iohid_event_field(event_type: i64) -> i32 {
    (event_type << 16) as i32
}

/// AppleVendor usage page carrying thermal sensors, and its temperature usage.
pub const USAGE_PAGE_APPLE_VENDOR: i32 = 0xff00;
pub const USAGE_TEMPERATURE: i32 = 5;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    pub fn IOHIDEventSystemClientCreate(allocator: CFAllocatorRef) -> *mut IOHIDEventSystemClient;
    pub fn IOHIDEventSystemClientSetMatching(
        client: *mut IOHIDEventSystemClient,
        matching: CFDictionaryRef,
    ) -> i32;
    pub fn IOHIDEventSystemClientCopyServices(client: *mut IOHIDEventSystemClient) -> CFArrayRef;
    pub fn IOHIDServiceClientCopyProperty(
        service: *mut IOHIDServiceClient,
        key: CFStringRef,
    ) -> CFTypeRef;
    pub fn IOHIDServiceClientCopyEvent(
        service: *mut IOHIDServiceClient,
        event_type: i64,
        options: i32,
        timeout: i64,
    ) -> *mut IOHIDEvent;
    pub fn IOHIDEventGetFloatValue(event: *mut IOHIDEvent, field: i32) -> f64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> *const c_void;
    pub fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    pub fn CFStringGetCString(
        string: CFStringRef,
        buffer: *mut c_char,
        buffer_size: CFIndex,
        encoding: u32,
    ) -> bool;
    pub fn CFNumberGetValue(number: CFTypeRef, the_type: i32, value_ptr: *mut c_void) -> bool;
}

pub const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
pub const K_CF_NUMBER_INT_TYPE: i32 = 9;
