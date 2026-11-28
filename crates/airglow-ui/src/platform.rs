//! Windows platform integration: single-instance mutex + show wake-up event,
//! system-wide global hotkeys (`Ctrl+Alt+Shift+L / P / A`), and native JSON file picker
//! via Win32 `comdlg32` (`GetOpenFileNameW`) with zero extra crate dependencies.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalHotkeyAction {
    ToggleLights,
    CycleProfile,
    OpenWindow,
}

pub enum SingleInstanceResult {
    Primary(SingleInstanceGuard),
    AlreadyRunning,
}

#[cfg(windows)]
mod imp {
    use super::{GlobalHotkeyAction, SingleInstanceResult};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    type Handle = isize;

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const WAIT_OBJECT_0: u32 = 0;
    const ASFW_ANY: u32 = 0xFFFF_FFFF;
    const UOI_NAME: i32 = 2;

    const MOD_ALT: u32 = 0x0001;
    const MOD_CONTROL: u32 = 0x0002;
    const MOD_SHIFT: u32 = 0x0004;
    const MOD_NOREPEAT: u32 = 0x4000;

    const WM_HOTKEY: u32 = 0x0312;
    const WM_QUIT: u32 = 0x0012;

    const HK_TOGGLE_LIGHTS: i32 = 1;
    const HK_CYCLE_PROFILE: i32 = 2;
    const HK_OPEN_WINDOW: i32 = 3;

    const VK_A: u32 = 0x41;
    const VK_L: u32 = 0x4C;
    const VK_P: u32 = 0x50;

    const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
    const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
    const OFN_NOCHANGEDIR: u32 = 0x0000_0008;

    #[repr(C)]
    #[derive(Default)]
    struct Msg {
        hwnd: isize,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        pt_x: i32,
        pt_y: i32,
    }

    #[repr(C)]
    pub(super) struct OpenFileNameW {
        l_struct_size: u32,
        hwnd_owner: isize,
        h_instance: isize,
        lpstr_filter: *const u16,
        lpstr_custom_filter: *mut u16,
        n_max_cust_filter: u32,
        n_filter_index: u32,
        lpstr_file: *mut u16,
        n_max_file: u32,
        lpstr_file_title: *mut u16,
        n_max_file_title: u32,
        lpstr_initial_dir: *const u16,
        lpstr_title: *const u16,
        flags: u32,
        n_file_offset: u16,
        n_file_extension: u16,
        lpstr_def_ext: *const u16,
        l_cust_data: isize,
        lpfn_hook: isize,
        lp_template_name: *const u16,
        pv_reserved: *mut core::ffi::c_void,
        dw_reserved: u32,
        flags_ex: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateMutexW(
            lp_mutex_attributes: *mut core::ffi::c_void,
            b_initial_owner: i32,
            lp_name: *const u16,
        ) -> Handle;
        fn CreateEventW(
            lp_event_attributes: *mut core::ffi::c_void,
            b_manual_reset: i32,
            b_initial_state: i32,
            lp_name: *const u16,
        ) -> Handle;
        fn SetEvent(h_event: Handle) -> i32;
        fn WaitForSingleObject(h_handle: Handle, dw_milliseconds: u32) -> u32;
        fn CloseHandle(h_object: Handle) -> i32;
        fn GetLastError() -> u32;
        fn GetCurrentThreadId() -> u32;
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterHotKey(h_wnd: isize, id: i32, fs_modifiers: u32, vk: u32) -> i32;
        fn UnregisterHotKey(h_wnd: isize, id: i32) -> i32;
        fn PeekMessageW(
            lp_msg: *mut Msg,
            h_wnd: isize,
            w_msg_filter_min: u32,
            w_msg_filter_max: u32,
            w_remove_msg: u32,
        ) -> i32;
        fn GetMessageW(
            lp_msg: *mut Msg,
            h_wnd: isize,
            w_msg_filter_min: u32,
            w_msg_filter_max: u32,
        ) -> i32;
        fn PostThreadMessageW(
            id_thread: u32,
            msg: u32,
            w_param: usize,
            l_param: isize,
        ) -> i32;
        fn AllowSetForegroundWindow(dw_process_id: u32) -> i32;
        fn GetThreadDesktop(dw_thread_id: u32) -> Handle;
        fn GetUserObjectInformationW(
            h_obj: Handle,
            n_index: i32,
            pv_info: *mut core::ffi::c_void,
            n_length: u32,
            lpn_length_needed: *mut u32,
        ) -> i32;
    }

    #[link(name = "comdlg32")]
    unsafe extern "system" {
        fn GetOpenFileNameW(lp_ofn: *mut OpenFileNameW) -> i32;
    }

    pub(super) fn to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn decode_wide_null(buf: &[u16]) -> Option<String> {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if len == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len]))
    }

    pub(super) fn single_instance_object_names(desktop_name: Option<&str>) -> (String, String) {
        let trimmed = desktop_name.map(str::trim).unwrap_or("");
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
            return (
                "Local\\AirglowSingleInstanceMutex".to_string(),
                "Local\\AirglowShowWindowEvent".to_string(),
            );
        }
        let sanitized: String = trimmed
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .take(48)
            .collect();
        if sanitized.is_empty() {
            return (
                "Local\\AirglowSingleInstanceMutex".to_string(),
                "Local\\AirglowShowWindowEvent".to_string(),
            );
        }
        (
            format!("Local\\AirglowSingleInstanceMutex_{sanitized}"),
            format!("Local\\AirglowShowWindowEvent_{sanitized}"),
        )
    }

    fn current_desktop_name() -> Option<String> {
        let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if desktop == 0 {
            return None;
        }
        let mut needed: u32 = 0;
        let _ = unsafe {
            GetUserObjectInformationW(desktop, UOI_NAME, std::ptr::null_mut(), 0, &mut needed)
        };
        if needed < 2 {
            return None;
        }
        let mut buf = vec![0u16; (needed as usize).div_ceil(2)];
        let ok = unsafe {
            GetUserObjectInformationW(
                desktop,
                UOI_NAME,
                buf.as_mut_ptr().cast(),
                (buf.len() * 2) as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return None;
        }
        decode_wide_null(&buf)
    }

    pub struct SingleInstanceGuard {
        mutex: Handle,
        show_event: Handle,
    }

    unsafe impl Send for SingleInstanceGuard {}

    impl SingleInstanceGuard {
        pub fn acquire(signal_existing_to_show: bool) -> SingleInstanceResult {
            let (mutex_str, event_str) =
                single_instance_object_names(current_desktop_name().as_deref());
            let mutex_name = to_wide_null(&mutex_str);
            let event_name = to_wide_null(&event_str);

            let mutex = unsafe { CreateMutexW(std::ptr::null_mut(), 0, mutex_name.as_ptr()) };
            if mutex == 0 {
                // If mutex creation unexpectedly failed, allow app to launch cleanly.
                return SingleInstanceResult::Primary(Self {
                    mutex: 0,
                    show_event: 0,
                });
            }
            let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
            let show_event =
                unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, event_name.as_ptr()) };

            if already_exists {
                if signal_existing_to_show && show_event != 0 {
                    unsafe {
                        let _ = AllowSetForegroundWindow(ASFW_ANY);
                        SetEvent(show_event);
                    }
                }
                if show_event != 0 {
                    unsafe {
                        CloseHandle(show_event);
                    }
                }
                unsafe {
                    CloseHandle(mutex);
                }
                return SingleInstanceResult::AlreadyRunning;
            }

            SingleInstanceResult::Primary(Self { mutex, show_event })
        }

        pub fn poll_show_requested(&self) -> bool {
            if self.show_event == 0 {
                return false;
            }
            unsafe { WaitForSingleObject(self.show_event, 0) == WAIT_OBJECT_0 }
        }
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            if self.show_event != 0 {
                unsafe {
                    CloseHandle(self.show_event);
                }
                self.show_event = 0;
            }
            if self.mutex != 0 {
                unsafe {
                    CloseHandle(self.mutex);
                }
                self.mutex = 0;
            }
        }
    }

    pub struct GlobalHotkeyManager {
        rx: Receiver<GlobalHotkeyAction>,
        thread_id: Arc<AtomicU32>,
        stop: Arc<AtomicBool>,
        join: Option<JoinHandle<()>>,
    }

    impl GlobalHotkeyManager {
        pub fn start() -> Self {
            let (tx, rx) = mpsc::channel();
            let thread_id = Arc::new(AtomicU32::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let tid_w = Arc::clone(&thread_id);
            let stop_r = Arc::clone(&stop);

            let join = thread::Builder::new()
                .name("airglow-hotkeys".to_string())
                .spawn(move || {
                    let mut msg = Msg::default();
                    unsafe {
                        PeekMessageW(&mut msg, 0, 0, 0, 0);
                        tid_w.store(GetCurrentThreadId(), Ordering::Release);
                    }
                    if stop_r.load(Ordering::Acquire) {
                        return;
                    }

                    let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
                    unsafe {
                        let _ = RegisterHotKey(0, HK_TOGGLE_LIGHTS, mods, VK_L);
                        let _ = RegisterHotKey(0, HK_CYCLE_PROFILE, mods, VK_P);
                        let _ = RegisterHotKey(0, HK_OPEN_WINDOW, mods, VK_A);
                    }

                    while !stop_r.load(Ordering::Acquire) {
                        let res = unsafe { GetMessageW(&mut msg, 0, 0, 0) };
                        if res <= 0 {
                            break;
                        }
                        if msg.message == WM_HOTKEY {
                            let action = match msg.w_param as i32 {
                                HK_TOGGLE_LIGHTS => Some(GlobalHotkeyAction::ToggleLights),
                                HK_CYCLE_PROFILE => Some(GlobalHotkeyAction::CycleProfile),
                                HK_OPEN_WINDOW => Some(GlobalHotkeyAction::OpenWindow),
                                _ => None,
                            };
                            if let Some(act) = action
                                && tx.send(act).is_err()
                            {
                                break;
                            }
                        }
                    }

                    unsafe {
                        let _ = UnregisterHotKey(0, HK_TOGGLE_LIGHTS);
                        let _ = UnregisterHotKey(0, HK_CYCLE_PROFILE);
                        let _ = UnregisterHotKey(0, HK_OPEN_WINDOW);
                    }
                })
                .ok();

            Self {
                rx,
                thread_id,
                stop,
                join,
            }
        }

        pub fn poll_actions(&self) -> Vec<GlobalHotkeyAction> {
            let mut out = Vec::new();
            while let Ok(act) = self.rx.try_recv() {
                out.push(act);
            }
            out
        }
    }

    impl Drop for GlobalHotkeyManager {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            let mut tid = self.thread_id.load(Ordering::Acquire);
            if tid == 0 && self.join.is_some() {
                for _ in 0..25 {
                    thread::sleep(Duration::from_millis(2));
                    tid = self.thread_id.load(Ordering::Acquire);
                    if tid != 0 {
                        break;
                    }
                }
            }
            if tid != 0 {
                unsafe {
                    let _ = PostThreadMessageW(tid, WM_QUIT, 0, 0);
                }
            }
            if let Some(h) = self.join.take() {
                let _ = h.join();
            }
        }
    }

    pub fn pick_json_backup_file() -> Option<String> {
        let mut filter: Vec<u16> = "Airglow Backup (*.json)\0*.json\0All Files (*.*)\0*.*\0\0"
            .encode_utf16()
            .collect();
        let title = to_wide_null("Select Airglow Rig Backup (.json)");
        let def_ext = to_wide_null("json");
        let mut file_buf = vec![0u16; 4096];

        let mut ofn = OpenFileNameW {
            l_struct_size: std::mem::size_of::<OpenFileNameW>() as u32,
            hwnd_owner: 0,
            h_instance: 0,
            lpstr_filter: filter.as_mut_ptr(),
            lpstr_custom_filter: std::ptr::null_mut(),
            n_max_cust_filter: 0,
            n_filter_index: 1,
            lpstr_file: file_buf.as_mut_ptr(),
            n_max_file: file_buf.len() as u32,
            lpstr_file_title: std::ptr::null_mut(),
            n_max_file_title: 0,
            lpstr_initial_dir: std::ptr::null(),
            lpstr_title: title.as_ptr(),
            flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
            n_file_offset: 0,
            n_file_extension: 0,
            lpstr_def_ext: def_ext.as_ptr(),
            l_cust_data: 0,
            lpfn_hook: 0,
            lp_template_name: std::ptr::null(),
            pv_reserved: std::ptr::null_mut(),
            dw_reserved: 0,
            flags_ex: 0,
        };

        let ok = unsafe { GetOpenFileNameW(&mut ofn) != 0 };
        if ok {
            decode_wide_null(&file_buf)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{GlobalHotkeyAction, SingleInstanceResult};

    pub struct SingleInstanceGuard;

    impl SingleInstanceGuard {
        pub fn acquire(_signal_existing_to_show: bool) -> SingleInstanceResult {
            SingleInstanceResult::Primary(Self)
        }

        pub fn poll_show_requested(&self) -> bool {
            false
        }
    }

    pub struct GlobalHotkeyManager;

    impl GlobalHotkeyManager {
        pub fn start() -> Self {
            Self
        }

        pub fn poll_actions(&self) -> Vec<GlobalHotkeyAction> {
            Vec::new()
        }
    }

    pub fn pick_json_backup_file() -> Option<String> {
        None
    }
}

pub use imp::{GlobalHotkeyManager, SingleInstanceGuard, pick_json_backup_file};

#[cfg(test)]
mod tests {
    #[cfg(all(windows, target_pointer_width = "64"))]
    #[test]
    fn openfilenamew_matches_win64_abi_size() {
        assert_eq!(std::mem::size_of::<super::imp::OpenFileNameW>(), 152);
    }

    #[cfg(windows)]
    #[test]
    fn wide_string_encode_and_decode_roundtrip() {
        let encoded = super::imp::to_wide_null(r"C:\Users\demo\backup.json");
        assert_eq!(encoded.last(), Some(&0));
        let mut buf = encoded;
        buf.extend_from_slice(&[0u16; 16]);
        assert_eq!(
            super::imp::decode_wide_null(&buf).as_deref(),
            Some(r"C:\Users\demo\backup.json")
        );
        assert_eq!(super::imp::decode_wide_null(&[0u16; 8]), None);
    }

    #[cfg(windows)]
    #[test]
    fn single_instance_names_scope_non_default_desktops() {
        for name in [None, Some(""), Some("Default"), Some("default")] {
            let (m, e) = super::imp::single_instance_object_names(name);
            assert_eq!(m, "Local\\AirglowSingleInstanceMutex");
            assert_eq!(e, "Local\\AirglowShowWindowEvent");
        }
        let (m, e) = super::imp::single_instance_object_names(Some("exebox-ABC/123"));
        assert_eq!(m, "Local\\AirglowSingleInstanceMutex_exebox-ABC_123");
        assert_eq!(e, "Local\\AirglowShowWindowEvent_exebox-ABC_123");
        assert!(!m.strip_prefix("Local\\").unwrap().contains('\\'));
        assert!(!e.strip_prefix("Local\\").unwrap().contains('\\'));
    }
}
