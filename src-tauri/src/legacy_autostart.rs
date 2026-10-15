//! Removes Windows "Run at login" entries written by older builds.
//!
//! The autostart plugin registers `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! under the product name ("butterlog"). Earlier builds registered under the bundle
//! identifier instead, leaving entries that point at an exe path that no longer
//! exists. Explorer stops processing the Run key when it reaches them, so the
//! current entry (and anything after it) never launches.

const LEGACY_NAMES: &[&str] = &["net.flyingbutter.butterlog", "net.flyvoyager.butterlog"];

#[cfg(windows)]
pub fn cleanup() {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for path in [
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run",
    ] {
        let Ok(key) = hkcu.open_subkey_with_flags(path, KEY_SET_VALUE) else { continue };
        for name in LEGACY_NAMES {
            // Missing values are the normal case; ignore NotFound.
            let _ = key.delete_value(name);
        }
    }
}

#[cfg(not(windows))]
pub fn cleanup() {
    let _ = LEGACY_NAMES;
}
