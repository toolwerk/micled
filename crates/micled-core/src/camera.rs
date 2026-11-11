//! Ist gerade eine Kamera in Benutzung?
//!
//! CoreMediaIO meldet pro Kamera „läuft irgendwo“ (`gone`). Welches Programm
//! sie nutzt, verrät macOS nicht. Wie beim Mikrofon braucht das keine
//! Kamera-Berechtigung, weil kein Bild gelesen wird.

/// `true`, wenn mindestens eine Kamera von irgendeinem Programm genutzt wird.
pub fn in_use() -> Result<bool, String> {
    imp::in_use()
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::null;

    type ObjectID = u32;
    type OSStatus = i32;

    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }

    const fn fourcc(s: &[u8; 4]) -> u32 {
        ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32
    }

    const SYSTEM_OBJECT: ObjectID = 1;
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");
    const ELEMENT_MAIN: u32 = 0;
    const HARDWARE_DEVICES: u32 = fourcc(b"dev#");
    const DEVICE_IS_RUNNING_SOMEWHERE: u32 = fourcc(b"gone");

    #[link(name = "CoreMediaIO", kind = "framework")]
    extern "C" {
        fn CMIOObjectGetPropertyDataSize(
            id: ObjectID,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> OSStatus;
        fn CMIOObjectGetPropertyData(
            id: ObjectID,
            addr: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            data_size: u32,
            data_used: *mut u32,
            out: *mut c_void,
        ) -> OSStatus;
    }

    fn addr(selector: u32) -> Address {
        Address { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
    }

    pub fn in_use() -> Result<bool, String> {
        let list = addr(HARDWARE_DEVICES);
        let mut size = 0u32;
        let st = unsafe { CMIOObjectGetPropertyDataSize(SYSTEM_OBJECT, &list, 0, null(), &mut size) };
        if st != 0 {
            return Err(crate::tr!("Kameraliste nicht lesbar (OSStatus {st})", "Camera list unreadable (OSStatus {st})", st = st));
        }
        let mut ids = vec![0 as ObjectID; size as usize / size_of::<ObjectID>()];
        let mut used = 0u32;
        let st = unsafe {
            CMIOObjectGetPropertyData(SYSTEM_OBJECT, &list, 0, null(), size, &mut used, ids.as_mut_ptr() as *mut c_void)
        };
        if st != 0 {
            return Err(crate::tr!("Kameraliste nicht lesbar (OSStatus {st})", "Camera list unreadable (OSStatus {st})", st = st));
        }
        ids.truncate(used as usize / size_of::<ObjectID>());

        let running = addr(DEVICE_IS_RUNNING_SOMEWHERE);
        Ok(ids.into_iter().any(|id| {
            let mut on = 0u32;
            let mut used = 0u32;
            let st = unsafe {
                CMIOObjectGetPropertyData(
                    id,
                    &running,
                    0,
                    null(),
                    size_of::<u32>() as u32,
                    &mut used,
                    &mut on as *mut u32 as *mut c_void,
                )
            };
            st == 0 && on != 0
        }))
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn in_use() -> Result<bool, String> {
        Ok(false)
    }
}
