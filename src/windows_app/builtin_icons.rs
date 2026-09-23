use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum BuiltinIcon {
    Folder,
    FolderOpen,
    FolderSrc,
    File,
    Rust,
    Python,
    Markdown,
    Toml,
    Json,
    Yaml,
    Git,
    Html,
    Css,
    JavaScript,
    TypeScript,
    Image,
    Script,
    Lock,
    Document,
}

impl BuiltinIcon {
    pub(super) fn resolve_for_path(path: &Path, is_dir: bool, expanded: bool) -> Self {
        if is_dir {
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            return match dir_name.as_str() {
                "src" | "source" => BuiltinIcon::FolderSrc,
                _ if expanded => BuiltinIcon::FolderOpen,
                _ => BuiltinIcon::Folder,
            };
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if name.starts_with(".git") || name == ".gitignore" || name == ".gitmodules" {
            return BuiltinIcon::Git;
        }
        if name == "cargo.lock" || name.ends_with(".lock") {
            return BuiltinIcon::Lock;
        }
        if name == "cargo.toml" {
            return BuiltinIcon::Toml;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "rs" => BuiltinIcon::Rust,
            "py" | "pyw" => BuiltinIcon::Python,
            "md" | "markdown" => BuiltinIcon::Markdown,
            "toml" => BuiltinIcon::Toml,
            "json" => BuiltinIcon::Json,
            "yaml" | "yml" => BuiltinIcon::Yaml,
            "html" | "htm" => BuiltinIcon::Html,
            "css" | "scss" | "sass" | "less" => BuiltinIcon::Css,
            "js" | "mjs" | "cjs" | "jsx" => BuiltinIcon::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => BuiltinIcon::TypeScript,
            "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico" | "webp" | "bmp" => BuiltinIcon::Image,
            "ps1" | "bat" | "cmd" | "sh" | "bash" | "zsh" => BuiltinIcon::Script,
            "lock" => BuiltinIcon::Lock,
            "txt" | "log" | "ini" | "cfg" => BuiltinIcon::Document,
            _ => BuiltinIcon::File,
        }
    }

    pub(super) fn svg_str(self) -> &'static str {
        match self {
            BuiltinIcon::Folder => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#3B82F6" d="M2 6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6z"/><path fill="#60A5FA" d="M2 9a1 1 0 0 1 1-1h18a1 1 0 0 1 1 1v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9z"/></svg>"##
            }
            BuiltinIcon::FolderOpen => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#2563EB" d="M2 6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6z"/><path fill="#1E3A8A" d="M3 8h18v4H3z"/><path fill="#93C5FD" d="M2.5 11h19l-2.5 9h-17z"/></svg>"##
            }
            BuiltinIcon::FolderSrc => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#2563EB" d="M2 6a2 2 0 0 1 2-2h5l2 2h9a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6z"/><path fill="#60A5FA" d="M2 9a1 1 0 0 1 1-1h18a1 1 0 0 1 1 1v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9z"/><path fill="#FFFFFF" d="M10 12l-2.5 2.5L10 17l-1 1-3.5-3.5L9 11zm4 0l2.5 2.5L14 17l1 1 3.5-3.5L15 11z"/></svg>"##
            }
            BuiltinIcon::Rust => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g fill="#DE692D"><path d="M12 4a8 8 0 1 0 8 8 8 8 0 0 0-8-8zm0 13.5a5.5 5.5 0 1 1 5.5-5.5 5.5 5.5 0 0 1-5.5 5.5z"/><circle cx="12" cy="12" r="2.5"/><path d="M11 2h2v3h-2zm0 17h2v3h-2zm8.5-8.5v2h3v-2zm-17 0v2h3v-2zm12.5-6.5l1.8-1.8 1.4 1.4-1.8 1.8zm-11.8 11.8l1.8-1.8 1.4 1.4-1.8 1.8zm11.8 0l-1.8-1.8 1.4-1.4 1.8 1.8zm-11.8-11.8l1.8 1.8-1.4 1.4-1.8-1.8z"/></g></svg>"##
            }
            BuiltinIcon::Python => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#3776AB" d="M11.9 2c-3.1 0-2.9 1.3-2.9 1.3l.03 1.4h3v.4H6.3S3.5 4.8 3.5 8c0 3.1 1.7 3 1.7 3h1.1V9.5c0-1.4 1.2-2.5 2.6-2.5h4.1c1.2 0 2.2-1 2.2-2.2V3.4S15.4 2 11.9 2zm-1.8 1.3a.7.7 0 1 1 0 1.4.7.7 0 0 1 0-1.4z"/><path fill="#FFD43B" d="M12.1 22c3.1 0 2.9-1.3 2.9-1.3l-.03-1.4h-3v-.4h5.7s2.8.3 2.8-2.9c0-3.1-1.7-3-1.7-3h-1.1v1.5c0 1.4-1.2 2.5-2.6 2.5H11c-1.2 0-2.2 1-2.2 2.2v1.4s-.2 1.4 3.3 1.4zm1.8-1.3a.7.7 0 1 1 0-1.4.7.7 0 0 1 0 1.4z"/></svg>"##
            }
            BuiltinIcon::Markdown => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="22" height="15" x="1" y="4.5" rx="2.5" fill="none" stroke="#4191C3" stroke-width="2"/><path fill="#4191C3" d="M4 16V8h2.5l2 2.5 2-2.5H13v8h-2v-4.5l-1.5 2-1.5-2V16H4zm13-4V8h2v4h2l-3 4-3-4h2z"/></svg>"##
            }
            BuiltinIcon::Toml => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#CD9137" d="M5 3a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V8.5L15.5 3H5zm9 1.5L19.5 10H14V4.5zM6.5 12h11v2h-11v-2zm0 4h7v2h-7v-2z"/></svg>"##
            }
            BuiltinIcon::Json => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#F59E0B" d="M9 4c-1.5 0-2.5 1-2.5 2.5V10c0 1-.8 1.5-1.5 1.5v1c.7 0 1.5.5 1.5 1.5v3.5C6.5 19 7.5 20 9 20h1.5v-2H9c-.5 0-1-.3-1-1v-3.5c0-1-.8-1.5-1.5-1.5.7 0 1.5-.5 1.5-1.5V7c0-.7.5-1 1-1h1.5V4H9zm6 0c1.5 0 2.5 1 2.5 2.5V10c0 1 .8 1.5 1.5 1.5v1c-.7 0-1.5.5-1.5 1.5v3.5c0 1.5-1 2.5-2.5 2.5H13.5v-2H15c.5 0 1-.3 1-1v-3.5c0-1 .8-1.5 1.5-1.5-.7 0-1.5-.5-1.5-1.5V7c0-.7-.5-1-1-1h-1.5V4H15z"/></svg>"##
            }
            BuiltinIcon::Yaml => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E11D48" d="M5 3a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V8.5L15.5 3H5zm9 1.5L19.5 10H14V4.5zM7.5 11l2.5 4.5V19h2v-3.5l2.5-4.5h-2.2l-1.3 2.7-1.3-2.7H7.5z"/></svg>"##
            }
            BuiltinIcon::Git => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#F05032" d="M21.6 10.7l-8.3-8.3a2.5 2.5 0 0 0-3.5 0L8.6 3.6l3 3a2.2 2.2 0 0 1 2.8 2.8l2.9 2.9a2.2 2.2 0 1 1-1.3 1.3l-2.7-2.7v4.6a2.2 2.2 0 1 1-1.8 0v-4.8a2.2 2.2 0 0 1-1.2-2.9l-3-3-4.9 4.9a2.5 2.5 0 0 0 0 3.5l8.3 8.3a2.5 2.5 0 0 0 3.5 0l9.4-9.4a2.5 2.5 0 0 0 0-3.5z"/></svg>"##
            }
            BuiltinIcon::Html => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#E44D26" d="M4 3l1.6 17.5 6.4 1.8 6.4-1.8L20 3H4zm13.3 4.2l-.3 3.5H9.6l.2 2.2h6.9l-.5 5.5-4.2 1.2-4.2-1.2-.3-3.2h2.2l.1 1.6 2.2.6 2.2-.6.2-2.4H7.2L6.6 7.2h10.7z"/></svg>"##
            }
            BuiltinIcon::Css => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#264DE4" d="M4 3l1.6 17.5 6.4 1.8 6.4-1.8L20 3H4zm13.3 4.2l-.3 3.5H9.6l.2 2.2h6.9l-.5 5.5-4.2 1.2-4.2-1.2-.3-3.2h2.2l.1 1.6 2.2.6 2.2-.6.2-2.4H7.2L6.6 7.2h10.7z"/></svg>"##
            }
            BuiltinIcon::JavaScript => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="20" height="20" x="2" y="2" rx="3" fill="#F7DF1E"/><path fill="#000000" d="M8.5 17.5c-1 0-1.8-.5-2.2-1.3l1.4-.9c.2.4.5.7.9.7.4 0 .7-.2.7-.8v-5h1.7v5.1c0 1.5-1 2.2-2.5 2.2zm7.1 0c-1.7 0-2.8-.9-3.2-2l1.5-.9c.3.6.8 1.1 1.7 1.1.7 0 1.2-.3 1.2-.8 0-.6-.5-.8-1.3-1.2l-.6-.3c-1.3-.5-2.1-1.3-2.1-2.5 0-1.5 1.2-2.4 2.7-2.4 1.3 0 2.2.5 2.8 1.6l-1.4.9c-.3-.5-.7-.8-1.4-.8-.6 0-1 .3-1 .7 0 .5.4.7 1.1 1l.6.3c1.5.6 2.3 1.3 2.3 2.6 0 1.7-1.3 2.7-3 2.7z"/></svg>"##
            }
            BuiltinIcon::TypeScript => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="20" height="20" x="2" y="2" rx="3" fill="#3178C6"/><path fill="#FFFFFF" d="M6 10.5h6v1.8H9.8V18H7.9v-5.7H6v-1.8zm9.6 7.7c-1.7 0-2.8-.9-3.2-2l1.5-.9c.3.6.8 1.1 1.7 1.1.7 0 1.2-.3 1.2-.8 0-.6-.5-.8-1.3-1.2l-.6-.3c-1.3-.5-2.1-1.3-2.1-2.5 0-1.5 1.2-2.4 2.7-2.4 1.3 0 2.2.5 2.8 1.6l-1.4.9c-.3-.5-.7-.8-1.4-.8-.6 0-1 .3-1 .7 0 .5.4.7 1.1 1l.6.3c1.5.6 2.3 1.3 2.3 2.6 0 1.7-1.3 2.7-3 2.7z"/></svg>"##
            }
            BuiltinIcon::Image => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="18" height="16" x="3" y="4" rx="2" fill="none" stroke="#A855F7" stroke-width="2"/><circle cx="8" cy="9" r="1.5" fill="#A855F7"/><path fill="#A855F7" d="M5 18l5-6 4 4 3-3 2 3v2H5z"/></svg>"##
            }
            BuiltinIcon::Script => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect width="20" height="16" x="2" y="4" rx="2.5" fill="#0F172A" stroke="#10B981" stroke-width="1.8"/><path fill="none" stroke="#10B981" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M6 9l3 3-3 3m5 0h4"/></svg>"##
            }
            BuiltinIcon::Lock => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#94A3B8" d="M17 9V7a5 5 0 0 0-10 0v2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-9a2 2 0 0 0-2-2h-2zm-8-2a3 3 0 0 1 6 0v2H9V7zm3 8.5a1.5 1.5 0 1 1 0-3 1.5 1.5 0 0 1 0 3z"/></svg>"##
            }
            BuiltinIcon::Document => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#64748B" d="M5 3a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V8.5L15.5 3H5zm9 1.5L19.5 10H14V4.5zM6.5 12h11v1.8h-11V12zm0 3.5h11v1.8h-11v-1.8z"/></svg>"##
            }
            BuiltinIcon::File => {
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="#475569" d="M5 3a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V8.5L15.5 3H5zm9 1.5L19.5 10H14V4.5z"/></svg>"##
            }
        }
    }
}
