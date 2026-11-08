//! The drawings macOS asks Oxidraft to open: opened with it in Finder
//! (double-clicked, Open With, dropped on its Dock icon), at launch or
//! while it runs.
//!
//! AppKit hands them to the application's delegate, which is winit's, and
//! winit doesn't take them (its `platform::macos` docs say to add a
//! delegate of one's own, which would leave winit without its). So winit's
//! delegate is given AppKit's `application:openURLs:` as the app finishes
//! launching: after winit has set it, before AppKit hands over the drawings
//! a launch was asked to open. This crate holds Oxidraft's only unsafe
//! code; elsewhere than macOS it does nothing.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// The drawings asked for and not yet taken.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
/// What wakes the app when they come.
static WAKE: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Starts taking the drawings macOS asks the app to open. Called on the
/// main thread before the event loop runs, or a launch's are missed.
pub fn listen() {
    #[cfg(target_os = "macos")]
    appkit::listen();
}

/// What to call when drawings come, to wake the app (the first one given).
pub fn wake_with(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKE.set(Box::new(wake));
}

/// The drawings asked for since last time, in the order asked.
pub fn opened() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Drawings asked for: kept for `opened`, and the app woken.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn arrived(paths: impl IntoIterator<Item = PathBuf>) {
    OPENED.lock().unwrap_or_else(|e| e.into_inner()).extend(paths);
    if let Some(wake) = WAKE.get() {
        wake();
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification};
    use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSURL};

    /// `application:openURLs:`'s types: returns nothing; self, its
    /// selector, the application, the URLs.
    const OPEN_URLS_TYPES: &std::ffi::CStr = c"v@:@@";

    type OpenUrls = extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>);

    pub(super) fn listen() {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let block = RcBlock::new(|_: NonNull<NSNotification>| give_delegate_open_urls());
        // SAFETY: AppKit's own notification, from any object, its block
        // run as it's posted (on the main thread); the block is 'static.
        #[allow(unsafe_code)]
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSApplicationWillFinishLaunchingNotification),
                None,
                None,
                &block,
            )
        };
        // Kept for the app's life.
        std::mem::forget(observer);
    }

    /// The application's delegate (winit's) given `application:openURLs:`.
    fn give_delegate_open_urls() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let Some(delegate) = app.delegate() else { return };
        let object: &AnyObject = (*delegate).as_ref();
        let class: &AnyClass = object.class();
        // SAFETY: a function pointer for another with the same calling
        // convention, called by the runtime with the arguments its types
        // (OPEN_URLS_TYPES) say.
        #[allow(unsafe_code)]
        let imp = unsafe { std::mem::transmute::<OpenUrls, Imp>(open_urls) };
        // SAFETY: a live class; a method it hasn't (class_addMethod adds
        // none in place of one it has), whose types match the function's.
        #[allow(unsafe_code)]
        let added = unsafe {
            objc2::ffi::class_addMethod(
                class as *const AnyClass as *mut AnyClass,
                sel!(application:openURLs:),
                imp,
                OPEN_URLS_TYPES.as_ptr(),
            )
        };
        if added.as_bool() {
            // AppKit notes what its delegate answers to as it's set.
            app.setDelegate(Some(&delegate));
        }
    }

    /// `-[NSApplicationDelegate application:openURLs:]`: the files among
    /// them, to open.
    extern "C-unwind" fn open_urls(_this: *mut AnyObject, _cmd: Sel, _app: *mut AnyObject, urls: *mut NSArray<NSURL>) {
        // SAFETY: AppKit passes an NSArray of NSURLs (or nil), alive for
        // the call.
        #[allow(unsafe_code)]
        let Some(urls) = (unsafe { urls.as_ref() }) else { return };
        super::arrived(urls.iter().filter_map(|u| u.to_file_path()));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn drawings_asked_for_are_taken_once_and_wake_the_app() {
        static WOKEN: AtomicUsize = AtomicUsize::new(0);
        wake_with(|| {
            WOKEN.fetch_add(1, Ordering::SeqCst);
        });
        arrived([PathBuf::from("/a/plan.dwg"), PathBuf::from("/a/site.dxf")]);
        arrived([PathBuf::from("/b/detail.dxf")]);
        assert_eq!(opened(), ["/a/plan.dwg", "/a/site.dxf", "/b/detail.dxf"].map(PathBuf::from));
        assert!(opened().is_empty(), "taken once");
        assert_eq!(WOKEN.load(Ordering::SeqCst), 2);
    }
}
