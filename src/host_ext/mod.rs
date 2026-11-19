//! nav's host layer on top of [`muxhost`].
//!
//! [`NavHost`] bundles the muxhost trait groups the switcher core
//! needs (reads, topology, navigate) so `dyn NavHost` is testable with
//! muxhost's `FakeHost`. [`TemplateHost`] and [`HostExt`] are
//! nav-local glue the plan (rex-migration.md, Phase R1) keeps out of
//! muxhost: template building + capture, and the herdr-only plugin /
//! pane-extras surface. [`NavFull`] is what the binary threads
//! everywhere — today only `HerdrHost` implements it; Rex lands in
//! R3 (topology/navigate in muxhost) and R5 (templates/capture).
//!
//! [`NoHost`] preserves the dev path: with no multiplexer detected,
//! every call fails with the same message and the popup still opens
//! with "unavailable" groups (spec §11).

mod herdr;

use muxhost::navigate::HostNavigate;
use muxhost::topology::HostTopology;
use muxhost::{Host, SourceRef};

// AgentStatus is re-exported for the provider tests (cfg(test) only in a
// bin crate, which rustc flags as unused otherwise).
#[allow(unused_imports)]
pub use muxhost::topology::{AgentEntry, AgentStatus, PaneEntry, SessionInfo, WindowInfo};

/// Everything the switcher core needs from any host. Blanket-implied
/// by muxhost's own trait groups, so `muxhost::FakeHost` is a `NavHost`
/// and tree-building tests run against it.
pub trait NavHost: Host + HostTopology + HostNavigate {}
impl<T: Host + HostTopology + HostNavigate> NavHost for T {}

/// A `workspace.create` result, mapped host-neutrally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedSession {
    pub session_id: String,
    /// The window (herdr: tab) `create_session` made, if any — the
    /// template builder renames it to the template's first-tab name.
    pub first_window_id: String,
    /// The root pane the workspace starts with (None when the host
    /// reports none — the build then degrades to no splits).
    pub root_pane_id: Option<String>,
}

/// Host-local glue for template building and capture (rex-migration.md
/// R1: deliberately NOT in muxhost — Rex's implementation lands in R5,
/// behind this same trait).
pub trait TemplateHost: Host {
    /// Create a session at `cwd` with `label` (herdr: `workspace.create`).
    fn create_session(&self, cwd: &str, label: &str) -> Result<CreatedSession, String>;

    /// Create a window in `session_id` at `cwd`, optionally labelled
    /// (herdr: `tab.create`). Returns the window's root pane id.
    fn create_window(
        &self,
        session_id: &str,
        cwd: &str,
        label: Option<&str>,
    ) -> Result<String, String>;

    /// Split `pane_id` in `direction` ("left"/"right"/"up"/"down") at
    /// `ratio` (0–1), starting the new pane at `cwd` (herdr:
    /// `pane.split`). Returns the new pane's id (empty when the host
    /// response carries none — the build then keeps splitting `pane_id`).
    fn split_pane(
        &self,
        pane_id: &str,
        direction: &str,
        ratio: f64,
        cwd: &str,
    ) -> Result<String, String>;

    /// The layout tree of `window_id` (herdr: `layout.export`), as the
    /// raw response object — the capture module's pure parsers own the
    /// shape (R5 may type it once Rex's shape is known).
    fn export_layout(&self, window_id: &str) -> Result<serde_json::Value, String>;

    /// Foreground-process info for `pane_id` (herdr:
    /// `pane.process_info`), raw response object — capture's
    /// best-effort command heuristic owns the parsing.
    fn process_info(&self, pane_id: &str) -> Result<serde_json::Value, String>;
}

/// The herdr-only surface muxhost does not model. Gate the Plugins
/// group on `caps().plugins` (Rex hides it); the rest is checked at
/// the call site with a clear error when a host lacks it.
pub trait HostExt: Host {
    /// Every registered plugin, raw entries (id/name/version/enabled/
    /// actions) — herdr: `plugin.list`. Empty is a valid answer.
    fn list_plugins(&self) -> Result<Vec<serde_json::Value>, String>;

    /// Run a plugin's declared action (herdr: `plugin.action.invoke`).
    fn invoke_plugin_action(&self, plugin_id: &str, action_id: &str) -> Result<(), String>;

    /// A pane's working directory (herdr: `pane.get` → `cwd`).
    /// `Ok(None)` when the pane has no cwd.
    fn pane_cwd(&self, pane_id: &str) -> Result<Option<String>, String>;

    /// Send key tokens (e.g. `ctrl+c`) to a pane (herdr:
    /// `pane.send_keys`).
    fn send_keys(&self, pane_id: &str, keys: &[&str]) -> Result<(), String>;

    /// Send key tokens to an agent by its terminal target (herdr:
    /// `agent.send_keys`).
    fn agent_send_keys(&self, target: &str, keys: &[&str]) -> Result<(), String>;

    /// Release the agent running in a pane (herdr: `pane.release_agent`).
    fn release_agent(&self, pane_id: &str) -> Result<(), String>;

    /// Show a one-line toast in the host terminal (herdr:
    /// `notification.show`).
    fn notify(&self, message: &str) -> Result<(), String>;
}

/// The full host surface nav's binary threads everywhere. Implemented
/// by real hosts only (`HerdrHost` today; RexHost in R3/R5) — tests
/// use muxhost's `FakeHost` plus local impls of the two nav traits.
pub trait NavFull: NavHost + TemplateHost + HostExt {}
impl<T: NavHost + TemplateHost + HostExt> NavFull for T {}

/// Detect the host from the environment (launchers set `NAV_HOST`
/// explicitly so a herdr running inside a Rex terminal — or the
/// reverse — is never mistaken for the other host; see muxhost's
/// `detect`). Herdr today; Rex lands in R3. Falls back to [`NoHost`]
/// so the dev path (no multiplexer) still opens the popup with
/// "unavailable" groups, exactly as an empty `HERDR_SOCKET_PATH` did
/// before the migration.
pub fn detect() -> Box<dyn NavFull> {
    let explicit = std::env::var("NAV_HOST").ok();
    let is_herdr = explicit.as_deref() == Some("herdr")
        || (explicit.is_none() && std::env::var_os("HERDR_SOCKET_PATH").is_some());
    if is_herdr {
        let socket_path = std::env::var("HERDR_SOCKET_PATH").unwrap_or_default();
        if socket_path.is_empty() {
            return Box::new(NoHost);
        }
        // from_env() also requires a launch context; nav treats the
        // context as optional (a live socket with no context keeps
        // working — only the focused-pane conveniences degrade), so
        // fall back to with_socket on a context error.
        return match muxhost::herdr::HerdrHost::from_env() {
            Ok(h) => Box::new(h),
            Err(_) => Box::new(muxhost::herdr::HerdrHost::with_socket(socket_path)),
        };
    }
    Box::new(NoHost)
}

// ── NoHost: the dev fallback ─────────────────────────────────────────────────

const NO_HOST: &str =
    "no multiplexer host detected — run inside herdr (HERDR_SOCKET_PATH) or set NAV_HOST";

/// The no-host fallback: every call fails with the same message, so
/// providers degrade to "unavailable" stubs and previews show the
/// reason — the popup still opens (dev path, spec §11).
pub struct NoHost;

impl Host for NoHost {
    fn name(&self) -> &'static str {
        "none"
    }
    fn source(&self) -> &SourceRef {
        static EMPTY: std::sync::OnceLock<SourceRef> = std::sync::OnceLock::new();
        EMPTY.get_or_init(|| SourceRef {
            pane_id: String::new(),
            tab_id: String::new(),
        })
    }
    fn read_pane(
        &self,
        _pane_id: &str,
        _source: muxhost::ReadSource,
        _format: muxhost::ReadFormat,
        _lines: Option<u32>,
    ) -> Result<String, String> {
        Err(NO_HOST.to_string())
    }
    fn list_tab_panes(&self) -> Result<Vec<muxhost::PaneInfo>, String> {
        Err(NO_HOST.to_string())
    }
    fn insert_text(&self, _pane_id: &str, _text: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn scroll_info(&self, _pane_id: &str) -> Result<Option<muxhost::ScrollInfo>, String> {
        Err(NO_HOST.to_string())
    }
    fn caps(&self) -> muxhost::Caps {
        muxhost::Caps::default()
    }
}

impl HostTopology for NoHost {
    fn list_sessions(&self) -> Result<Vec<SessionInfo>, String> {
        Err(NO_HOST.to_string())
    }
    fn list_windows(&self) -> Result<Vec<WindowInfo>, String> {
        Err(NO_HOST.to_string())
    }
    fn list_all_panes(&self) -> Result<Vec<PaneEntry>, String> {
        Err(NO_HOST.to_string())
    }
    fn list_agents(&self) -> Result<Vec<AgentEntry>, String> {
        Err(NO_HOST.to_string())
    }
}

impl HostNavigate for NoHost {
    fn jump(
        &self,
        _jump: muxhost::navigate::Jump,
    ) -> Result<muxhost::navigate::JumpOutcome, String> {
        Err(NO_HOST.to_string())
    }
    fn close_pane(&self, _pane_id: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn close_window(&self, _window_id: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn close_session(&self, _session_id: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn rename_pane(&self, _pane_id: &str, _label: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn rename_window(&self, _window_id: &str, _label: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
}

impl TemplateHost for NoHost {
    fn create_session(&self, _cwd: &str, _label: &str) -> Result<CreatedSession, String> {
        Err(NO_HOST.to_string())
    }
    fn create_window(
        &self,
        _session_id: &str,
        _cwd: &str,
        _label: Option<&str>,
    ) -> Result<String, String> {
        Err(NO_HOST.to_string())
    }
    fn split_pane(
        &self,
        _pane_id: &str,
        _direction: &str,
        _ratio: f64,
        _cwd: &str,
    ) -> Result<String, String> {
        Err(NO_HOST.to_string())
    }
    fn export_layout(&self, _window_id: &str) -> Result<serde_json::Value, String> {
        Err(NO_HOST.to_string())
    }
    fn process_info(&self, _pane_id: &str) -> Result<serde_json::Value, String> {
        Err(NO_HOST.to_string())
    }
}

impl HostExt for NoHost {
    fn list_plugins(&self) -> Result<Vec<serde_json::Value>, String> {
        Err(NO_HOST.to_string())
    }
    fn invoke_plugin_action(&self, _plugin_id: &str, _action_id: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn pane_cwd(&self, _pane_id: &str) -> Result<Option<String>, String> {
        Err(NO_HOST.to_string())
    }
    fn send_keys(&self, _pane_id: &str, _keys: &[&str]) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn agent_send_keys(&self, _target: &str, _keys: &[&str]) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn release_agent(&self, _pane_id: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
    fn notify(&self, _message: &str) -> Result<(), String> {
        Err(NO_HOST.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_host_name_is_none() {
        // The preview pane checks name() == "none" for the old
        // "(no socket)" message — keep that contract pinned.
        let h = NoHost;
        assert_eq!(h.name(), "none");
    }

    #[test]
    fn no_host_errors_every_call() {
        let h = NoHost;
        assert!(h.list_all_panes().is_err());
        assert!(h
            .read_pane(
                "p",
                muxhost::ReadSource::Viewport,
                muxhost::ReadFormat::Plain,
                None
            )
            .is_err());
        assert!(h.list_plugins().is_err());
        assert!(h.create_session("/x", "y").is_err());
    }

    /// muxhost's FakeHost implements the three muxhost trait groups, so
    /// it is a NavHost with zero nav-side code — the contract R1's
    /// "tree-building tests run against a fake host" relies on.
    #[test]
    fn muxhost_fake_host_is_a_nav_host() {
        fn takes_nav_host(_: &dyn NavHost) {}
        takes_nav_host(&muxhost::fake::FakeHost::new("p1", "t1"));
    }
}
