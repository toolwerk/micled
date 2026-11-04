//! Wer nutzt gerade das Mikrofon?
//!
//! macOS 14+ führt in CoreAudio pro Prozess ein Audio-Objekt mit der Eigenschaft
//! „IsRunningInput“. Das ist genauer als der Gerätestatus „läuft irgendwo“, der
//! bei Headsets auch bei reiner Wiedergabe anschlägt. Es braucht keine
//! Mikrofon-Berechtigung, weil kein Ton gelesen wird.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicUser {
    pub pid: i32,
    /// Bundle-ID, bei Kommandozeilen-Programmen leer
    pub bundle_id: String,
    /// Prozessname als Rückfall für die Anzeige
    pub name: String,
}

/// Liefert alle Prozesse (außer dem eigenen), die gerade vom Mikrofon lesen.
pub fn users() -> Result<Vec<MicUser>, String> {
    imp::users()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::MicUser;
    use std::ffi::{c_char, c_void, CStr};
    use std::mem::size_of;
    use std::ptr::null;

    type AudioObjectID = u32;
    type OSStatus = i32;
    type CFStringRef = *const c_void;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32
    }

    const SYSTEM_OBJECT: AudioObjectID = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const PROCESS_OBJECT_LIST: u32 = fourcc(b"prs#");
    const PROCESS_PID: u32 = fourcc(b"ppid");
    const PROCESS_BUNDLE_ID: u32 = fourcc(b"pbid");
    const PROCESS_IS_RUNNING_INPUT: u32 = fourcc(b"piri");
    const UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: AudioObjectID,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> OSStatus;
        fn AudioObjectGetPropertyData(
            id: AudioObjectID,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out: *mut c_void,
        ) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFRelease(obj: *const c_void);
    }

    extern "C" {
        fn proc_name(pid: i32, buf: *mut c_void, size: u32) -> i32;
    }

    fn addr(selector: u32) -> Address {
        Address { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
    }

    fn get<T: Copy + Default>(id: AudioObjectID, selector: u32) -> Option<T> {
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        let st = unsafe {
            AudioObjectGetPropertyData(id, &addr(selector), 0, null(), &mut size, &mut value as *mut T as *mut c_void)
        };
        (st == 0).then_some(value)
    }

    fn bundle_id(id: AudioObjectID) -> String {
        let mut cf: CFStringRef = null();
        let mut size = size_of::<CFStringRef>() as u32;
        let st = unsafe {
            AudioObjectGetPropertyData(id, &addr(PROCESS_BUNDLE_ID), 0, null(), &mut size, &mut cf as *mut _ as *mut c_void)
        };
        if st != 0 || cf.is_null() {
            return String::new();
        }
        let mut buf = [0 as c_char; 512];
        let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), buf.len() as isize, UTF8) };
        unsafe { CFRelease(cf) };
        if ok == 0 {
            return String::new();
        }
        unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned()
    }

    fn process_name(pid: i32) -> String {
        let mut buf = [0u8; 256];
        let n = unsafe { proc_name(pid, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) };
        if n <= 0 {
            return format!("PID {pid}");
        }
        String::from_utf8_lossy(&buf[..n as usize]).into_owned()
    }

    pub fn users() -> Result<Vec<MicUser>, String> {
        let list = addr(PROCESS_OBJECT_LIST);
        let mut size = 0u32;
        let st = unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &list, 0, null(), &mut size) };
        if st != 0 {
            return Err(format!("CoreAudio-Prozessliste nicht lesbar (OSStatus {st}, macOS 14+ nötig)"));
        }
        let count = size as usize / size_of::<AudioObjectID>();
        let mut ids = vec![0 as AudioObjectID; count];
        let st = unsafe {
            AudioObjectGetPropertyData(SYSTEM_OBJECT, &list, 0, null(), &mut size, ids.as_mut_ptr() as *mut c_void)
        };
        if st != 0 {
            return Err(format!("CoreAudio-Prozessliste nicht lesbar (OSStatus {st})"));
        }
        ids.truncate(size as usize / size_of::<AudioObjectID>());

        let own = std::process::id() as i32;
        let mut users = Vec::new();
        for id in ids {
            if get::<u32>(id, PROCESS_IS_RUNNING_INPUT).unwrap_or(0) == 0 {
                continue;
            }
            let pid = get::<i32>(id, PROCESS_PID).unwrap_or(-1);
            if pid == own {
                continue;
            }
            users.push(MicUser { pid, bundle_id: bundle_id(id), name: process_name(pid) });
        }
        Ok(users)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::MicUser;

    pub fn users() -> Result<Vec<MicUser>, String> {
        Err("Mikrofon-Erkennung gibt es nur unter macOS".into())
    }
}
