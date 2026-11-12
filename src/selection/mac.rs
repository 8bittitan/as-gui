//! The macOS capture: the general pasteboard through AppKit, ⌘C through a posted CGEvent, and
//! the Accessibility trust that posting needs.

use std::ffi::c_void;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};

use super::{Access, Capture, Desktop, SelectionSource, copy_and_restore};

type CFTypeRef = *const c_void;

#[repr(C)]
struct Opaque {
    _private: [u8; 0],
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn CGEventSourceCreate(state: i32) -> CFTypeRef;
    fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: u16, down: bool) -> CFTypeRef;
    fn CGEventSetFlags(event: CFTypeRef, flags: u64);
    fn CGEventPost(tap: u32, event: CFTypeRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: Opaque;
    static kCFTypeDictionaryValueCallBacks: Opaque;
    fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const Opaque,
        value_callbacks: *const Opaque,
    ) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
}

const HID_SYSTEM_STATE: i32 = 1;
const HID_EVENT_TAP: u32 = 0;
const KEY_C: u16 = 8;
const FLAG_COMMAND: u64 = 1 << 20;

/// The pasteboard types password managers and similar apps mark a secret copy with
/// (nspasteboard.org).
const SECRET_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

/// Every item on the pasteboard, each as its types and their bytes.
type Items = Vec<Vec<(String, Vec<u8>)>>;

pub struct MacDesktop;

impl Desktop for MacDesktop {
    type Saved = Items;

    fn trusted(&self) -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    fn change_count(&self) -> i64 {
        unsafe { NSPasteboard::generalPasteboard().changeCount() as i64 }
    }

    fn text(&self) -> Option<String> {
        unsafe {
            NSPasteboard::generalPasteboard()
                .stringForType(NSPasteboardTypeString)
                .map(|s| s.to_string())
        }
    }

    fn concealed(&self) -> bool {
        unsafe {
            let Some(items) = NSPasteboard::generalPasteboard().pasteboardItems() else {
                return false;
            };
            items.iter().any(|item| {
                item.types()
                    .iter()
                    .any(|kind| SECRET_TYPES.contains(&kind.to_string().as_str()))
            })
        }
    }

    fn save(&self) -> Items {
        unsafe {
            let Some(items) = NSPasteboard::generalPasteboard().pasteboardItems() else {
                return Vec::new();
            };
            items
                .iter()
                .map(|item| {
                    item.types()
                        .iter()
                        .filter_map(|kind| {
                            let data = item.dataForType(kind)?;
                            Some((kind.to_string(), data.bytes().to_vec()))
                        })
                        .collect()
                })
                .collect()
        }
    }

    fn restore(&self, saved: Items) {
        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            if saved.is_empty() {
                return;
            }
            let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = saved
                .iter()
                .map(|kinds| {
                    let item = NSPasteboardItem::new();
                    for (kind, bytes) in kinds {
                        item.setData_forType(&NSData::with_bytes(bytes), &NSString::from_str(kind));
                    }
                    ProtocolObject::from_retained(item)
                })
                .collect();
            pasteboard.writeObjects(&NSArray::from_vec(items));
        }
    }

    fn send_copy(&self) -> bool {
        unsafe {
            let source = CGEventSourceCreate(HID_SYSTEM_STATE);
            let mut posted = false;
            for down in [true, false] {
                let event = CGEventCreateKeyboardEvent(source, KEY_C, down);
                if event.is_null() {
                    break;
                }
                // The chord's own modifiers are still held; ⌘ alone makes it a plain copy.
                CGEventSetFlags(event, FLAG_COMMAND);
                CGEventPost(HID_EVENT_TAP, event);
                CFRelease(event);
                posted = true;
            }
            if !source.is_null() {
                CFRelease(source);
            }
            posted
        }
    }

    fn wait(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

impl SelectionSource for MacDesktop {
    fn capture(&self) -> Option<Capture> {
        copy_and_restore(self)
    }

    fn access(&self) -> Access {
        if self.trusted() {
            Access::Granted
        } else {
            Access::NotGranted
        }
    }

    /// Shows the system's Accessibility prompt, which lists as-gui in System Settings.
    fn request_access(&self) {
        prompt_for_access();
    }
}

fn prompt_for_access() {
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        );
        AXIsProcessTrustedWithOptions(options);
        if !options.is_null() {
            CFRelease(options);
        }
    }
}
