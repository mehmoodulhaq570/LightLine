# LightLine Architecture
> **Maintenance Note:** This is a living document. If you submit a Pull Request that introduces new top-level folders, core systems, or architectural changes, please update the Mermaid diagram and directory structure below to keep it accurate.

LightLine is a native Windows IDE built in Rust. It interacts directly with the Win32 API for UI rendering and window management, while delegating language intelligence and external tool execution to isolated subsystems.

## System Flow

The following diagram illustrates how the core subsystems interact with the operating system, external services, and the user:

```mermaid
flowchart TD

subgraph group_ui["Native editor UI"]
  node_main["Windows entry<br/>[main.rs]"]
  node_window["Window loop<br/>[window.rs]"]
  node_app["App state<br/>[app.rs]"]
  node_render["Editor rendering<br/>[editor.rs]"]
  node_panels["Panels and review<br/>[panels.rs]"]
end

subgraph group_editing["Editing services"]
  node_document["Document model<br/>[document.rs]"]
  node_syntax["Syntax highlighting<br/>[syntax.rs]"]
  node_lsp["Language support<br/>[lsp.rs]"]
  node_formatter["Formatting<br/>[formatter.rs]"]
  node_settings["User settings<br/>[settings.rs]"]
end

subgraph group_project["Workspace services"]
  node_workspace["Workspace navigation<br/>[workspace.rs]"]
  node_workflow["Project workflows<br/>[workflow.rs]"]
  node_session["Session restore<br/>[session.rs]"]
  node_git["Source control UI<br/>[git.rs]"]
end

subgraph group_runtime["Run and debug"]
  node_terminal["Terminal service<br/>[mod.rs]"]
  node_terminal_platform["Windows ConPTY<br/>[platform.rs]"]
  node_debug["DAP debugger<br/>[debug.rs]"]
  node_debug_ui["Debugger UI<br/>[debugger.rs]"]
end

subgraph group_extensions["Extensions and appearance"]
  node_extensions["Extension management<br/>[mod.rs]"]
  node_registry["Zed registry<br/>[zed_registry.rs]"]
  node_installer["Extension installer<br/>[installer.rs]"]
  node_theme["Theme integration<br/>[theme.rs]"]
end

node_user(("Editor user"))
node_os(("Windows"))
node_files[("Project files")]
node_language_servers["Language servers"]
node_formatter_tool["Prettier"]
node_zed_service["Zed registry service"]
node_git_service["Git tools"]
node_debug_adapter["lldb-dap"]
node_shell["Shell or program"]

node_user -->|"interacts"| node_window
node_main -->|"starts"| node_window
node_window -->|"dispatches events"| node_app
node_app -->|"renders state"| node_render
node_app -->|"renders panels"| node_panels
node_app -->|"edits documents"| node_document
node_app -->|"updates highlighting"| node_syntax
node_app -->|"requests language features"| node_lsp
node_app -->|"requests formatting"| node_formatter
node_app -->|"loads settings"| node_settings
node_app -->|"manages workspace"| node_workspace
node_app -->|"requests project operations"| node_workflow
node_app -->|"restores session"| node_session
node_app -->|"dispatches Git actions"| node_git
node_app -->|"manages sessions"| node_terminal
node_app -->|"dispatches debug actions"| node_debug_ui
node_app -->|"manages extensions"| node_extensions
node_app -->|"applies themes"| node_theme
node_document -->|"reads and writes"| node_files
node_workflow -->|"searches and persists"| node_files
node_session -->|"saves and loads"| node_workflow
node_git -->|"requests Git operations"| node_workflow
node_workflow -->|"invokes"| node_git_service
node_lsp -->|"communicates"| node_language_servers
node_formatter -.->|"invokes"| node_formatter_tool
node_extensions -->|"queries registry"| node_registry
node_registry -->|"fetches listings"| node_zed_service
node_extensions -->|"installs extensions"| node_installer
node_installer -->|"retrieves extension sources"| node_zed_service
node_terminal -->|"starts sessions"| node_terminal_platform
node_terminal_platform -->|"launches and exchanges I/O"| node_shell
node_debug_ui -->|"sends debug commands"| node_debug
node_debug -->|"speaks DAP"| node_debug_adapter
node_window -->|"uses Win32"| node_os

classDef toneNeutral fill:#f8fafc,stroke:#334155,stroke-width:1.5px,color:#0f172a
classDef toneBlue fill:#dbeafe,stroke:#2563eb,stroke-width:1.5px,color:#172554
classDef toneAmber fill:#fef3c7,stroke:#d97706,stroke-width:1.5px,color:#78350f
classDef toneMint fill:#dcfce7,stroke:#16a34a,stroke-width:1.5px,color:#14532d
classDef toneRose fill:#ffe4e6,stroke:#e11d48,stroke-width:1.5px,color:#881337
classDef toneIndigo fill:#e0e7ff,stroke:#4f46e5,stroke-width:1.5px,color:#312e81
classDef toneTeal fill:#ccfbf1,stroke:#0f766e,stroke-width:1.5px,color:#134e4a
class node_main,node_window,node_app,node_render,node_panels,node_user toneBlue
class node_document,node_syntax,node_lsp,node_formatter,node_settings,node_files toneAmber
class node_workspace,node_workflow,node_session,node_git,node_language_servers toneMint
class node_terminal,node_terminal_platform,node_debug,node_debug_ui toneRose
class node_extensions,node_registry,node_installer,node_theme,node_os,node_formatter_tool,node_zed_service,node_git_service,node_debug_adapter,node_shell toneIndigo

# Directory Structure

* **`src/windows_app/`**: Contains the native Windows GUI rendering engine, raw mouse/keyboard input handling, window event loops, and UI view layouts.

* **`src/terminal/`**: Houses the backend logic for Windows ConPTY integration, managing shell sessions and standard I/O passing.

* **`src/extensions/`**: Manages the plugin architecture, theme application, and API integration with the Zed registry.

* **`src/` (Root)**: Contains core application state, document object models (`document.rs`), Debug Adapter Protocol (DAP) logic, and Language Server Protocol (LSP) integrations.
