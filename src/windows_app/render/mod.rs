mod ai_assistant;
mod code_pane;
mod completion;
mod editor;
mod hover;
mod panels;
mod primitives;
mod terminal;
mod welcome;

pub(in crate::windows_app) use primitives::{safe_slice_prefix, safe_slice_range};
pub(in crate::windows_app) use welcome::WelcomeAction;
