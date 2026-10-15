//! Choosing, applying and remembering the color theme.
//!
//! The choice is the installed theme variant's name, kept as `colorTheme`
//! in settings.json (VS Code keeps `workbench.colorTheme` the same way), so
//! it survives restarts and can also be edited by hand.

use super::*;
use lightline::color_theme::{self as zed_themes, ZedColorTheme};
use lightline::settings::Settings;

pub(super) const DEFAULT_THEME_NAME: &str = "LightLine Midnight";

impl App {
    /// The theme `settings` asks for: the installed variant `colorTheme`
    /// names (and the id of its extension), else LightLine's own theme,
    /// with the `colors` overrides on top.
    pub(super) fn build_theme(settings: &Settings) -> (Theme, Option<String>) {
        let chosen = settings.color_theme.as_deref().and_then(|name| {
            let dir = workflow::extensions_dir()?;
            zed_themes::installed(&dir)
                .into_iter()
                .find(|(_, theme)| theme.name == name)
        });
        let (theme, extension) = match chosen {
            Some((id, zed)) => (Theme::from_zed_color_theme(&zed), Some(id)),
            None => (Theme::default_dark(), None),
        };
        (theme.with_overrides(&settings.colors), extension)
    }

    /// Draws everything with `theme` from now on, including the UI colors
    /// that aren't theme roles (see theme::themed).
    pub(super) fn apply_theme(&mut self, hwnd: HWND, theme: Theme, extension: Option<String>) {
        self.theme = theme;
        self.active_color_theme = extension;
        self.syntax_epoch += 1;
        super::theme::activate(&self.theme);
        // Windows draws the frame's edges light or dark to match.
        let dark: i32 = (!self.theme.is_light()).into();
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                &dark as *const i32 as *const std::ffi::c_void,
                size_of::<i32>() as u32,
            );
        }
        // Previews composite their images and colors against the theme.
        for preview in self.tabs.iter().filter_map(|tab| tab.markdown.as_ref()) {
            preview.relayout();
        }
        unsafe { InvalidateRect(hwnd, null(), 0) };
    }

    /// Switches to the installed variant `name` (None: LightLine's own
    /// theme) and remembers the choice.
    pub(super) fn set_color_theme(&mut self, hwnd: HWND, name: Option<String>) {
        self.settings.color_theme = name.clone();
        let (theme, extension) = Self::build_theme(&self.settings);
        if name.is_some() && extension.is_none() {
            // Uninstalled since it was chosen: fall back rather than
            // remembering a theme that no longer exists.
            self.settings.color_theme = None;
        }
        self.apply_theme(hwnd, theme, extension);
        let label = self.color_theme_name();
        self.status = match self.settings.save() {
            Ok(()) => format!("Color theme: {label}"),
            Err(error) => format!("Color theme: {label} (not saved: {error})"),
        };
    }

    /// The name of the theme in use.
    pub(super) fn color_theme_name(&self) -> String {
        match (&self.settings.color_theme, &self.active_color_theme) {
            (Some(name), Some(_)) => name.clone(),
            _ => DEFAULT_THEME_NAME.to_string(),
        }
    }

    /// Preferences: Color Theme -- a menu of LightLine's own theme and every
    /// installed theme variant, the current one checked.
    pub(super) fn show_color_theme_menu(&mut self, hwnd: HWND, x: i32, y: i32) {
        use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            AppendMenuW, CreatePopupMenu, DestroyMenu, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
            MF_STRING, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu,
        };
        let variants: Vec<(String, ZedColorTheme)> = workflow::extensions_dir()
            .map(|dir| zed_themes::installed(&dir))
            .unwrap_or_default();
        let current = self.color_theme_name();
        let chosen = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            let flags = |name: &str| {
                if name == current {
                    MF_STRING | MF_CHECKED
                } else {
                    MF_STRING
                }
            };
            AppendMenuW(
                menu,
                flags(DEFAULT_THEME_NAME),
                1,
                wide(&format!("{DEFAULT_THEME_NAME} (default)")).as_ptr(),
            );
            if !variants.is_empty() {
                AppendMenuW(menu, MF_SEPARATOR, 0, null());
            }
            for (index, (_, variant)) in variants.iter().enumerate() {
                let label = if variant.dark {
                    variant.name.clone()
                } else {
                    format!("{} (light)", variant.name)
                };
                AppendMenuW(menu, flags(&variant.name), index + 2, wide(&label).as_ptr());
            }
            if variants.is_empty() {
                AppendMenuW(
                    menu,
                    MF_STRING | MF_GRAYED,
                    0,
                    wide("Install color themes from Extensions (Ctrl+Shift+X)").as_ptr(),
                );
            }
            let mut point = POINT { x, y };
            ClientToScreen(hwnd, &mut point);
            let chosen = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_LEFTALIGN,
                point.x,
                point.y,
                0,
                hwnd,
                null(),
            ) as usize;
            DestroyMenu(menu);
            chosen
        };
        match chosen {
            0 => {}
            1 => self.set_color_theme(hwnd, None),
            index => {
                if let Some((_, variant)) = variants.get(index - 2) {
                    self.set_color_theme(hwnd, Some(variant.name.clone()));
                }
            }
        }
    }
}
