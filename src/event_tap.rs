use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::config::Config;
use crate::debounce::{DebounceAction, Debouncer, DropReason, MouseButton};
use crate::logger::{init as init_logger, log_debug, log_error, log_info, log_trace, log_warn};

// macOS CoreGraphics / CoreFoundation FFI types
pub type CGEventTapProxy = *mut c_void;
pub type CGEventRef = *mut c_void;
pub type CFMachPortRef = *mut c_void;
pub type CFRunLoopSourceRef = *mut c_void;
pub type CFRunLoopRef = *mut c_void;
pub type CFStringRef = *const c_void;
pub type CFAllocatorRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFBooleanRef = *const c_void;

pub type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

// CoreGraphics Event Types
pub const CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
pub const CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
pub const CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
pub const CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
pub const CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;
pub const CG_EVENT_OTHER_MOUSE_UP: u32 = 26;

pub const CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFFFFFE;
pub const CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFFFFFF;

pub const CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;

// CGEventTap locations and options
pub const CG_SESSION_EVENT_TAP: u32 = 1;
pub const CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
pub const CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;

    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;

    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFRelease(cf: *const c_void);

    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetTimestamp(event: CGEventRef) -> u64;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;

    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;

    static kCFRunLoopCommonModes: CFStringRef;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    static kCFBooleanTrue: CFBooleanRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;

    fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        val_callbacks: *const c_void,
    ) -> CFDictionaryRef;
}

// POSIX Signal handling
type SigHandler = extern "C" fn(i32);
unsafe extern "C" {
    fn signal(sig: i32, handler: SigHandler) -> usize;
}

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

static ACTIVE_RUN_LOOP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static TERMINATING: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_termination_signal(_sig: i32) {
    if !TERMINATING.swap(true, Ordering::SeqCst) {
        log_info("Received termination signal. Shutting down cleanly...");
        let rl = ACTIVE_RUN_LOOP.load(Ordering::SeqCst);
        if !rl.is_null() {
            unsafe {
                CFRunLoopStop(rl as CFRunLoopRef);
            }
        }
    }
}

pub struct EventTapContext {
    pub debouncer: Debouncer,
    pub tap_port: CFMachPortRef,
}

unsafe extern "C" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let ctx = unsafe { &mut *(user_info as *mut EventTapContext) };

    // Re-enable the event tap if disabled by system timeout or user input
    if event_type == CG_EVENT_TAP_DISABLED_BY_TIMEOUT
        || event_type == CG_EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        log_debug("Event tap was temporarily disabled by macOS, re-enabling...");
        if !ctx.tap_port.is_null() {
            unsafe {
                CGEventTapEnable(ctx.tap_port, true);
            }
        }
        return event;
    }

    if event.is_null() {
        return event;
    }

    let (button, is_down) = match event_type {
        CG_EVENT_LEFT_MOUSE_DOWN => (MouseButton::LEFT, true),
        CG_EVENT_LEFT_MOUSE_UP => (MouseButton::LEFT, false),
        CG_EVENT_RIGHT_MOUSE_DOWN => (MouseButton::RIGHT, true),
        CG_EVENT_RIGHT_MOUSE_UP => (MouseButton::RIGHT, false),
        CG_EVENT_OTHER_MOUSE_DOWN => {
            let num = unsafe { CGEventGetIntegerValueField(event, CG_MOUSE_EVENT_BUTTON_NUMBER) };
            (MouseButton::new(num.max(0) as usize), true)
        }
        CG_EVENT_OTHER_MOUSE_UP => {
            let num = unsafe { CGEventGetIntegerValueField(event, CG_MOUSE_EVENT_BUTTON_NUMBER) };
            (MouseButton::new(num.max(0) as usize), false)
        }
        _ => return event,
    };

    let timestamp_ns = unsafe { CGEventGetTimestamp(event) };

    let action = if is_down {
        ctx.debouncer.process_down(button, timestamp_ns)
    } else {
        ctx.debouncer.process_up(button, timestamp_ns)
    };

    let name = button.name();
    let btn_idx = button.index();
    let state_str = if is_down { "DOWN" } else { "UP  " };

    match action {
        DebounceAction::Pass => {
            log_trace(&format!(
                "Mouse {state_str}: {name} (btn {btn_idx}) at {timestamp_ns}ns -> Accepted"
            ));
            event
        }
        DebounceAction::Drop { reason } => {
            let reason_desc = match reason {
                DropReason::DownTooQuickAfterDown(ms) => {
                    format!("spurious press {ms}ms after previous press")
                }
                DropReason::DownTooQuickAfterUp(ms) => {
                    format!("spurious release bounce {ms}ms after release")
                }
                DropReason::DuplicateDownWhileHeld => "duplicate press while held".to_string(),
                DropReason::PairedBounceUp => "paired bounce release".to_string(),
            };

            log_debug(&format!(
                "[DEBOUNCED] {name} button (btn {btn_idx}): {reason_desc}"
            ));

            log_trace(&format!(
                "Mouse {state_str}: {name} (btn {btn_idx}) at {timestamp_ns}ns -> Dropped ({reason_desc})"
            ));

            // Return null to drop the event from reaching other apps
            ptr::null_mut()
        }
    }
}

pub fn check_accessibility(prompt_if_missing: bool) -> bool {
    unsafe {
        if !prompt_if_missing {
            return AXIsProcessTrusted();
        }

        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );

        let trusted = AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
        trusted
    }
}

pub fn run_event_tap(config: Config) -> Result<(), String> {
    // 0. Initialize logger
    init_logger(
        config.log_level,
        config.use_syslog,
        config.log_file.as_deref(),
    );

    // Register clean signal handling early so waiting or startup can be interrupted gracefully
    unsafe {
        signal(SIGINT, handle_termination_signal);
        signal(SIGTERM, handle_termination_signal);
    }

    log_info(&format!(
        "MacDebounce v{} starting (debounce_ms: {}, buttons: {}, log_level: {:?}, syslog: {})",
        env!("CARGO_PKG_VERSION"),
        config.debounce_ms,
        config.format_buttons_summary(),
        config.log_level,
        config.use_syslog,
    ));

    // 1. Check Accessibility permissions.
    // If not granted, trigger the system prompt once, then wait for the user to grant permission
    // in System Settings without rapidly exiting or spamming system dialogs.
    if !check_accessibility(false) {
        log_warn("Accessibility permission is not granted yet.");
        log_warn(
            "Requesting Accessibility permission (System Settings -> Privacy & Security -> Accessibility)...",
        );
        // Trigger the system prompt dialog once
        check_accessibility(true);

        log_warn("Waiting for Accessibility permission to be granted in System Settings...");
        log_warn(
            "MacDebounce will automatically resume once permission is enabled (no restart required).",
        );

        while !check_accessibility(false) {
            if TERMINATING.load(Ordering::SeqCst) {
                log_info(
                    "Termination signal received while waiting for Accessibility permission. Exiting.",
                );
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(1000));
        }

        log_info("Accessibility permission granted! Proceeding with startup...");
    }

    let selection = config.selection;
    let event_mask = selection.cg_event_mask();

    let context = Box::new(EventTapContext {
        debouncer: Debouncer::new(config.debounce_ms, selection),
        tap_port: ptr::null_mut(),
    });

    let context_ptr = Box::into_raw(context);

    // 2. Create the event tap (retry briefly in case macOS TCC propagation has a slight delay)
    let mut tap = ptr::null_mut();
    for attempt in 0..5 {
        tap = unsafe {
            CGEventTapCreate(
                CG_SESSION_EVENT_TAP,
                CG_HEAD_INSERT_EVENT_TAP,
                CG_EVENT_TAP_OPTION_DEFAULT,
                event_mask,
                event_tap_callback,
                context_ptr as *mut c_void,
            )
        };
        if !tap.is_null() {
            break;
        }
        if TERMINATING.load(Ordering::SeqCst) {
            unsafe {
                drop(Box::from_raw(context_ptr));
            }
            return Ok(());
        }
        if attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    if tap.is_null() {
        unsafe {
            drop(Box::from_raw(context_ptr));
        }
        log_error("Failed to create event tap.");
        return Err(
            "Failed to create event tap. Make sure Accessibility permission is granted \
             in System Settings -> Privacy & Security -> Accessibility."
                .to_string(),
        );
    }

    unsafe {
        (*context_ptr).tap_port = tap;
    }

    // 3. Create run loop source and attach to current run loop
    let run_loop_source = unsafe { CFMachPortCreateRunLoopSource(ptr::null(), tap, 0) };
    if run_loop_source.is_null() {
        unsafe {
            CFRelease(tap);
            drop(Box::from_raw(context_ptr));
        }
        log_error("Failed to create CFRunLoopSource for event tap.");
        return Err("Failed to create CFRunLoopSource for event tap.".to_string());
    }

    let run_loop = unsafe { CFRunLoopGetCurrent() };
    ACTIVE_RUN_LOOP.store(run_loop as *mut c_void, Ordering::SeqCst);

    unsafe {
        CFRunLoopAddSource(run_loop, run_loop_source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
    }

    log_info("MacDebounce is active (0.0% idle CPU). Listening for mouse events...");

    // 4. Run loop blocks until CFRunLoopStop is called
    unsafe {
        CFRunLoopRun();
    }

    // Cleanup
    ACTIVE_RUN_LOOP.store(ptr::null_mut(), Ordering::SeqCst);
    unsafe {
        CGEventTapEnable(tap, false);
        CFRelease(run_loop_source);
        CFRelease(tap);
        drop(Box::from_raw(context_ptr));
    }

    log_info("MacDebounce exited cleanly.");
    Ok(())
}
