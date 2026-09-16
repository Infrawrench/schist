//! AI sidebar state; harnesses and bridge live in schist-app-ai.
pub use schist_app_ai::*;

/// Everything the workspace holds for the sidebar.
pub struct AiState {
    pub backend: Backend,
    /// The prompt being typed. Its `active` is what says the box has
    /// the keyboard.
    pub input: schist_ui::LineEdit,
    pub transcript: Vec<AiEntry>,
    /// A turn is in flight.
    pub running: bool,
    pub shared: AiShared,
    pub conversation: Option<Conversation>,
    /// The backend's id for this conversation, once it has said.
    pub session: Option<String>,
    /// The published tool list, built from the app's registry on first
    /// use. Also serves `tools/list` for the workers.
    pub catalog: Option<schist_mcp::Catalog>,
    /// The model picker popup.
    pub model_menu: bool,
    /// Which harness's list the picker's rail is showing.
    pub menu_backend: Backend,
    /// The picker's search buffer.
    pub model_search: String,
    /// Live model catalogs, fetched from each installed CLI once per run
    /// ([`models::fetch`]); `None` until they arrive.
    pub models_claude: Option<Vec<ModelEntry>>,
    pub models_codex: Option<Vec<ModelEntry>>,
    pub fetching_claude: bool,
    pub fetching_codex: bool,
    /// Whether the drain ticker task is live (only ever one at a time).
    pub ticker: bool,
    /// Loopback endpoint for harnesses that spawn their MCP servers as
    /// processes (Codex); started on first use.
    pub endpoint: Option<endpoint::Endpoint>,
    pub scroll: gpui::ScrollHandle,
    /// (claude, codex) CLIs found on PATH, probed once at startup.
    pub available: (bool, bool),
    /// Whether the live conversation was started under the gallery's
    /// prompt (`Some(true)`) or the editor's. A send from the other
    /// room restarts the conversation under the right one — resumed,
    /// so the transcript and the harness's memory carry over.
    pub conversation_gallery: Option<bool>,
}

impl AiState {
    pub fn new(backend: Backend) -> AiState {
        AiState {
            backend,
            input: schist_ui::LineEdit::multiline(),
            transcript: Vec::new(),
            running: false,
            shared: AiShared::default(),
            conversation: None,
            session: None,
            catalog: None,
            model_menu: false,
            menu_backend: backend,
            model_search: String::new(),
            models_claude: None,
            models_codex: None,
            fetching_claude: false,
            fetching_codex: false,
            ticker: false,
            endpoint: None,
            scroll: gpui::ScrollHandle::new(),
            available: (Backend::Claude.available(), Backend::Codex.available()),
            conversation_gallery: None,
        }
    }
}
