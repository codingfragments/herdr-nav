//! `TemplateHost` + `HostExt` for [`HerdrHost`] — today's socket calls
//! (rex-migration.md R1), moved verbatim from `source.rs`/`main.rs`/
//! `capture.rs` behind nav-local traits. Not in muxhost: the plan
//! keeps template/capture glue and the plugin surface host-local so
//! the Rex implementations (R3/R5) are added here, next to their
//! consumers, without muxhost releases in the loop.

use muxhost::herdr::HerdrHost;

use super::{CreatedSession, HostExt, TemplateHost};

impl TemplateHost for HerdrHost {
    /// `workspace.create {cwd, label}` → the new workspace's id, its
    /// initial tab's id (response `tab.tab_id`, else
    /// `workspace.active_tab_id`) and the root pane's id
    /// (`root_pane.pane_id`).
    fn create_session(&self, cwd: &str, label: &str) -> Result<CreatedSession, String> {
        let resp = self
            .raw_request(
                "workspace.create",
                serde_json::json!({"cwd": cwd, "label": label}),
            )
            .map_err(|e| format!("workspace.create failed: {e}"))?;
        let str_at = |keys: &[&str]| {
            let mut v = &resp;
            for k in keys {
                v = v.get(k)?;
            }
            v.as_str().map(str::to_string)
        };
        Ok(CreatedSession {
            session_id: str_at(&["workspace", "workspace_id"]).unwrap_or_default(),
            first_window_id: str_at(&["tab", "tab_id"])
                .or_else(|| str_at(&["workspace", "active_tab_id"]))
                .unwrap_or_default(),
            root_pane_id: str_at(&["root_pane", "pane_id"]),
        })
    }

    /// `tab.create {workspace_id, cwd, label?}` → the new tab's root
    /// pane id (`root_pane.pane_id`).
    fn create_window(
        &self,
        session_id: &str,
        cwd: &str,
        label: Option<&str>,
    ) -> Result<String, String> {
        let mut params = serde_json::json!({"workspace_id": session_id, "cwd": cwd});
        if let Some(label) = label {
            params["label"] = serde_json::json!(label);
        }
        let r = self
            .raw_request("tab.create", params)
            .map_err(|e| format!("tab.create failed: {e}"))?;
        Ok(r.get("root_pane")
            .and_then(|v| v.get("pane_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// `pane.split {pane_id, direction, ratio, cwd}` → the new pane's
    /// id (`pane.pane_id`), empty when the response carries none.
    fn split_pane(
        &self,
        pane_id: &str,
        direction: &str,
        ratio: f64,
        cwd: &str,
    ) -> Result<String, String> {
        let r = self.raw_request(
            "pane.split",
            serde_json::json!({"pane_id": pane_id, "direction": direction, "ratio": ratio, "cwd": cwd}),
        )?;
        Ok(r.get("pane")
            .and_then(|v| v.get("pane_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// `layout.export {tab_id}` → the raw response object.
    fn export_layout(&self, window_id: &str) -> Result<serde_json::Value, String> {
        self.raw_request("layout.export", serde_json::json!({"tab_id": window_id}))
            .map_err(|e| format!("layout.export failed: {e}"))
    }

    /// `pane.process_info {pane_id}` → the raw response object.
    fn process_info(&self, pane_id: &str) -> Result<serde_json::Value, String> {
        self.raw_request("pane.process_info", serde_json::json!({"pane_id": pane_id}))
            .map_err(|e| format!("pane.process_info failed: {e}"))
    }
}

impl HostExt for HerdrHost {
    /// `plugin.list` → the `plugins` array (empty on a missing array —
    /// herdr's schema marks it optional).
    fn list_plugins(&self) -> Result<Vec<serde_json::Value>, String> {
        let r = self
            .raw_request("plugin.list", serde_json::json!({}))
            .map_err(|e| format!("plugin.list failed: {e}"))?;
        Ok(r.get("plugins")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    }

    /// `plugin.action.invoke {plugin, action_id}`.
    fn invoke_plugin_action(&self, plugin_id: &str, action_id: &str) -> Result<(), String> {
        self.raw_request(
            "plugin.action.invoke",
            serde_json::json!({"plugin": plugin_id, "action_id": action_id}),
        )
        .map(|_| ())
    }

    /// `pane.get {pane_id}` → `pane.cwd` (None when the pane has none).
    fn pane_cwd(&self, pane_id: &str) -> Result<Option<String>, String> {
        let r = self
            .raw_request("pane.get", serde_json::json!({"pane_id": pane_id}))
            .map_err(|e| format!("pane.get failed: {e}"))?;
        Ok(r.get("pane")
            .and_then(|p| p.get("cwd"))
            .and_then(|v| v.as_str())
            .map(str::to_string))
    }

    /// `pane.send_keys {pane_id, keys}`.
    fn send_keys(&self, pane_id: &str, keys: &[&str]) -> Result<(), String> {
        self.raw_request(
            "pane.send_keys",
            serde_json::json!({"pane_id": pane_id, "keys": keys}),
        )
        .map(|_| ())
    }

    /// `agent.send_keys {target, keys}` — the target is the agent's
    /// pane id.
    fn agent_send_keys(&self, target: &str, keys: &[&str]) -> Result<(), String> {
        self.raw_request(
            "agent.send_keys",
            serde_json::json!({"target": target, "keys": keys}),
        )
        .map(|_| ())
    }

    /// `pane.release_agent {pane_id}`.
    fn release_agent(&self, pane_id: &str) -> Result<(), String> {
        self.raw_request(
            "pane.release_agent",
            serde_json::json!({"pane_id": pane_id}),
        )
        .map(|_| ())
    }

    /// `notification.show {title, message, duration_ms}`.
    fn notify(&self, message: &str) -> Result<(), String> {
        self.raw_request(
            "notification.show",
            serde_json::json!({"title": "herdr-nav", "message": message, "duration_ms": 1500}),
        )
        .map(|_| ())
    }
}
