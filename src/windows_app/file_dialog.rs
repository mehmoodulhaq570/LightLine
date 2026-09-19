//! Minimal hand-rolled bindings for the modern folder picker (`IFileOpenDialog`).
//!
//! `windows-sys` only exposes flat constants for this COM interface (CLSID,
//! IIDs, option flags), not the vtable itself, so the small slice of it this
//! app actually calls is defined here. Vtable layouts follow the documented
//! COM interface order (IUnknown -> IModalWindow -> IFileDialog ->
//! IFileOpenDialog, and IUnknown -> IShellItem) exactly; getting that order
//! wrong would call the wrong method through the vtable, so nothing here is
//! reordered or trimmed even though only a few methods are ever invoked.
//!
//! Unlike the legacy `SHBrowseForFolder` dialog, this one automatically
//! follows the OS light/dark setting, so no manual theming is needed.

use super::*;
use std::ffi::c_void;
use windows_sys::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows_sys::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, SIGDN_FILESYSPATH,
};
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

const IID_IFILE_OPEN_DIALOG: GUID = GUID::from_u128(0xd57c7288_d4ad_4768_be02_9d969532d960);

#[repr(C)]
struct IUnknownVtbl {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct IModalWindowVtbl {
    base: IUnknownVtbl,
    show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
}

#[repr(C)]
struct IFileDialogVtbl {
    base: IModalWindowVtbl,
    set_file_types: unsafe extern "system" fn(*mut c_void, u32, *const c_void) -> HRESULT,
    set_file_type_index: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    get_file_type_index: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    advise: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut u32) -> HRESULT,
    unadvise: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    set_options: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
    set_default_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    set_folder: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    get_folder: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    get_current_selection: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    set_file_name: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_file_name: unsafe extern "system" fn(*mut c_void, *mut PWSTR) -> HRESULT,
    set_title: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    set_ok_button_label: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    set_file_name_label: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    add_place: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> HRESULT,
    set_default_extension: unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT,
    close: unsafe extern "system" fn(*mut c_void, HRESULT) -> HRESULT,
    set_client_guid: unsafe extern "system" fn(*mut c_void, *const GUID) -> HRESULT,
    clear_client_data: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    set_filter: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IFileOpenDialogVtbl {
    base: IFileDialogVtbl,
    get_results: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    get_selected_items: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IShellItemVtbl {
    base: IUnknownVtbl,
    bind_to_handler: unsafe extern "system" fn(
        *mut c_void,
        *mut c_void,
        *const GUID,
        *const GUID,
        *mut *mut c_void,
    ) -> HRESULT,
    get_parent: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT,
    get_attributes: unsafe extern "system" fn(*mut c_void, u32, *mut u32) -> HRESULT,
    compare: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut i32) -> HRESULT,
}

unsafe fn com_release(obj: *mut c_void) {
    unsafe {
        let vtbl = *(obj as *mut *mut IUnknownVtbl);
        ((*vtbl).release)(obj);
    }
}

/// Shows the modern Windows folder picker (the same dialog File Explorer and
/// most current apps use) and returns the chosen folder, or `None` if the
/// dialog was cancelled, failed to create, or the platform can't supply it.
pub(super) fn pick_folder(owner: HWND, title: &str) -> Option<PathBuf> {
    unsafe {
        let mut dialog: *mut c_void = null_mut();
        let hr = CoCreateInstance(
            &FileOpenDialog,
            null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_IFILE_OPEN_DIALOG,
            &mut dialog,
        );
        if hr < 0 || dialog.is_null() {
            return None;
        }
        let vtbl = *(dialog as *mut *mut IFileOpenDialogVtbl);

        ((*vtbl).base.set_options)(dialog, FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM);
        let title_wide = wide(title);
        ((*vtbl).base.set_title)(dialog, title_wide.as_ptr());

        let hr = ((*vtbl).base.base.show)(dialog, owner);
        if hr < 0 {
            com_release(dialog);
            return None;
        }

        let mut item: *mut c_void = null_mut();
        let hr = ((*vtbl).base.get_result)(dialog, &mut item);
        if hr < 0 || item.is_null() {
            com_release(dialog);
            return None;
        }

        let item_vtbl = *(item as *mut *mut IShellItemVtbl);
        let mut name: PWSTR = null_mut();
        let hr = ((*item_vtbl).get_display_name)(item, SIGDN_FILESYSPATH, &mut name);
        let path = if hr >= 0 && !name.is_null() {
            let len = (0..).take_while(|&i| *name.add(i) != 0).count();
            let slice = std::slice::from_raw_parts(name, len);
            Some(PathBuf::from(String::from_utf16_lossy(slice)))
        } else {
            None
        };
        if !name.is_null() {
            CoTaskMemFree(name.cast());
        }
        com_release(item);
        com_release(dialog);
        path
    }
}
