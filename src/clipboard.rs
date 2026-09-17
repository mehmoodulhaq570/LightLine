#![cfg(windows)]

use std::io;
use std::mem::size_of;
use std::ptr;
use windows_sys::Win32::Foundation::{GlobalFree, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

struct OpenClipboardGuard;

impl Drop for OpenClipboardGuard {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn open(hwnd: HWND) -> io::Result<OpenClipboardGuard> {
    if unsafe { OpenClipboard(hwnd) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(OpenClipboardGuard)
    }
}

pub fn copy(hwnd: HWND, text: &str) -> io::Result<()> {
    let utf16: Vec<u16> = text
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let bytes = utf16.len() * size_of::<u16>();
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if memory.is_null() {
        return Err(io::Error::last_os_error());
    }
    let pointer = unsafe { GlobalLock(memory) as *mut u16 };
    if pointer.is_null() {
        unsafe { GlobalFree(memory) };
        return Err(io::Error::last_os_error());
    }
    unsafe {
        ptr::copy_nonoverlapping(utf16.as_ptr(), pointer, utf16.len());
        GlobalUnlock(memory);
    }
    let result = (|| -> io::Result<()> {
        let _guard = open(hwnd)?;
        if unsafe { EmptyClipboard() } == 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { SetClipboardData(CF_UNICODETEXT as u32, memory) }.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe { GlobalFree(memory) };
    }
    result
}

pub fn paste(hwnd: HWND) -> io::Result<Option<String>> {
    let _guard = open(hwnd)?;
    if unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT as u32) } == 0 {
        return Ok(None);
    }
    let memory = unsafe { GetClipboardData(CF_UNICODETEXT as u32) };
    if memory.is_null() {
        return Err(io::Error::last_os_error());
    }
    let units = unsafe { GlobalSize(memory) } / size_of::<u16>();
    if units == 0 {
        return Err(io::Error::last_os_error());
    }
    let pointer = unsafe { GlobalLock(memory) as *const u16 };
    if pointer.is_null() {
        return Err(io::Error::last_os_error());
    }
    let text = unsafe {
        let buffer = std::slice::from_raw_parts(pointer, units);
        let length = buffer.iter().position(|unit| *unit == 0).unwrap_or(units);
        String::from_utf16_lossy(&buffer[..length])
    };
    unsafe { GlobalUnlock(memory) };
    Ok(Some(text))
}
