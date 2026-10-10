// Go to Symbol: `@` in Quick Open (Ctrl+P) lists the active file's
// functions, types, fields and so on; Enter jumps to one. The language
// server is asked when `@` is typed, once per version of the file.

use super::*;
use lightline::lsp::{Command as LspCommand, Symbol};

pub(super) struct FileSymbols {
    // The document's `id` and LSP version the list is for.
    document: u64,
    version: i32,
    symbols: Vec<Symbol>,
    // Why there's no list, when the server couldn't give one.
    error: Option<String>,
}

pub(super) struct SymbolsRequest {
    language: LspLanguage,
    id: u64,
    uri: String,
    version: i32,
    document: u64,
}

impl App {
    /// Asks for the active file's symbols when Quick Open is in `@` mode
    /// and the list isn't for this version of the file yet.
    pub(super) fn ensure_symbols(&mut self, hwnd: HWND) {
        if !self.quick_open || !self.quick_query.starts_with('@') {
            return;
        }
        if self.welcome || self.tab().read_only() || Tab::lsp_language(self.doc()).is_none() {
            return;
        }
        if !self.tab().lsp_opened {
            self.ensure_lsp(hwnd);
        }
        let tab = self.tab();
        let document = tab.document.id();
        let version = tab.lsp_version;
        let current =
            |symbols: &FileSymbols| symbols.document == document && symbols.version == version;
        if self.file_symbols.as_ref().is_some_and(current)
            || self
                .symbols_request
                .as_ref()
                .is_some_and(|request| request.document == document && request.version == version)
        {
            return;
        }
        let (Some(language), Some(path), true) = (
            tab.lsp_language,
            tab.document.path.as_deref(),
            tab.lsp_opened,
        ) else {
            return;
        };
        let uri = lsp::file_uri(path);
        self.request_id += 1;
        let id = self.request_id;
        if self.tab_lsp(self.active).is_some_and(|client| {
            client.send(LspCommand::DocumentSymbols {
                id,
                uri: uri.clone(),
                version,
            })
        }) {
            self.symbols_request = Some(SymbolsRequest {
                language,
                id,
                uri,
                version,
                document,
            });
        }
    }

    pub(super) fn finish_symbols(
        &mut self,
        language: LspLanguage,
        id: u64,
        uri: &str,
        version: i32,
        result: Result<Vec<Symbol>, String>,
    ) {
        let Some(request) = self.symbols_request.take_if(|request| {
            request.language == language
                && request.id == id
                && request.uri == uri
                && request.version == version
        }) else {
            return;
        };
        let (symbols, error) = match result {
            Ok(symbols) => (symbols, None),
            Err(reason) => (Vec::new(), Some(reason)),
        };
        self.file_symbols = Some(FileSymbols {
            document: request.document,
            version,
            symbols,
            error,
        });
        self.quick_select(self.quick_selected);
    }

    // The active file's symbols, whichever version they're for: an older
    // list shows while a new one loads.
    fn active_symbols(&self) -> Option<&FileSymbols> {
        self.file_symbols
            .as_ref()
            .filter(|symbols| symbols.document == self.doc().id())
    }

    /// The symbols matching what's typed after `@`, in file order.
    pub(super) fn quick_symbols(&self) -> Vec<&Symbol> {
        let query = self
            .quick_query
            .trim_start_matches('@')
            .trim()
            .to_lowercase();
        self.active_symbols()
            .map(|file| {
                file.symbols
                    .iter()
                    .filter(|symbol| symbol.name.to_lowercase().contains(&query))
                    .take(500)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// What the `@` list shows when it's empty.
    pub(super) fn symbols_empty_message(&self) -> String {
        if Tab::lsp_language(self.doc()).is_none() {
            return "Symbols need a language server, and this file type has none".into();
        }
        match self.active_symbols() {
            _ if self.symbols_request.is_some() => "Loading symbols...".into(),
            Some(FileSymbols {
                error: Some(reason),
                ..
            }) => format!("No symbols: {reason}"),
            Some(_) => "No matching symbols".into(),
            None => "Language server not ready yet".into(),
        }
    }

    pub(super) fn jump_to_symbol(&mut self, hwnd: HWND, index: usize) {
        let Some(position) = self
            .quick_symbols()
            .get(index)
            .map(|symbol| symbol.position)
        else {
            return;
        };
        let doc = self.doc();
        let line = (position.line as usize).min(doc.line_count().saturating_sub(1));
        let byte = lsp::utf16_to_byte(doc.line(line), position.character);
        self.move_cursor(Pos { line, byte }, false);
        self.keep_cursor_visible(hwnd);
    }
}

/// "new  ·  method in impl Point".
pub(super) fn symbol_label(symbol: &Symbol) -> String {
    let kind = kind_name(symbol.kind);
    match (kind, symbol.container.as_str()) {
        ("", "") => symbol.name.clone(),
        ("", container) => format!("{}  ·  in {container}", symbol.name),
        (kind, "") => format!("{}  ·  {kind}", symbol.name),
        (kind, container) => format!("{}  ·  {kind} in {container}", symbol.name),
    }
}

// LSP SymbolKind, as a word.
fn kind_name(kind: u8) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        22 => "enum member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type parameter",
        // Object, and kinds for values: rust-analyzer marks `impl` blocks
        // as objects, whose name says what they are.
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightline::lsp::Position;

    #[test]
    fn labels_say_what_and_where() {
        let symbol = |name: &str, kind, container: &str| Symbol {
            name: name.into(),
            kind,
            container: container.into(),
            position: Position {
                line: 0,
                character: 0,
            },
        };
        assert_eq!(symbol_label(&symbol("main", 12, "")), "main  ·  function");
        assert_eq!(
            symbol_label(&symbol("new", 6, "impl Point")),
            "new  ·  method in impl Point"
        );
        assert_eq!(symbol_label(&symbol("impl Point", 19, "")), "impl Point");
        assert_eq!(symbol_label(&symbol("x", 99, "Point")), "x  ·  in Point");
    }
}
