//! `TemplateHost` + `HostExt` for [`RexHost`] — the Rex side of nav's
//! host-local glue (rex-migration.md R3). Template building beyond the
//! dir-open default (splits, commands, capture) lands in R5; here
//! `create_session` is the one `rex new` the dir-open path needs, and
//! the window/split calls cover what R5 will build on. The plugin
//! surface errors: Rex has no plugin system (`caps.plugins` gates the
//! group off), and there is no agent-release action
//! (`caps.agent_release` hides `^x`).

use muxhost::rex::RexHost;

use super::{CreatedSession, HostExt, TemplateHost};

impl TemplateHost for RexHost {
    /// `rex new LABEL -c DIR --json` → the created session, its first
    /// window and first block.
    fn create_session(&self, cwd: &str, label: &str) -> Result<CreatedSession, String> {
        let out = self_rex(self, &["new", label, "-c", cwd, "--json"])?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex new: bad JSON: {e}"))?;
        let first_window = v
            .get("initial_windows")
            .and_then(|w| w.as_array())
            .and_then(|w| w.first())
            .cloned()
            .unwrap_or_default();
        let str_at = |val: &serde_json::Value, keys: &[&str]| {
            let mut x = val;
            for k in keys {
                x = x.get(k)?;
            }
            x.as_str().map(str::to_string)
        };
        Ok(CreatedSession {
            session_id: str_at(&v, &["session_id"]).unwrap_or_default(),
            first_window_id: str_at(&first_window, &["window_id"]).unwrap_or_default(),
            root_pane_id: first_window
                .get("block_ids")
                .and_then(|b| b.as_array())
                .and_then(|b| b.first())
                .and_then(|b| b.as_str())
                .map(str::to_string),
        })
    }

    /// `rex window new -s S LABEL --json` → the new window's root block.
    fn create_window(
        &self,
        session_id: &str,
        _cwd: &str,
        label: Option<&str>,
    ) -> Result<String, String> {
        let mut args: Vec<&str> = vec!["window", "new", "-s", session_id];
        if let Some(label) = label {
            args.push(label);
        }
        args.push("--json");
        let out = self_rex(self, &args)?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex window new: bad JSON: {e}"))?;
        Ok(v.get("window")
            .and_then(|w| w.get("block_ids"))
            .and_then(|b| b.as_array())
            .and_then(|b| b.first())
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// `rex split -s S -b B --split=right|below --ratio N --json` → the
    /// new block's id.
    fn split_pane(
        &self,
        pane_id: &str,
        direction: &str,
        ratio: f64,
        _cwd: &str,
    ) -> Result<String, String> {
        // nav's directions are herdr's split verbs; map to Rex's.
        let split = match direction {
            "down" => "below",
            _ => "right",
        };
        let out = self_rex(
            self,
            &[
                "split",
                "-s",
                &self_session(self),
                "-b",
                pane_id,
                "--split",
                split,
                "--ratio",
                &format!("{ratio}"),
                "--json",
            ],
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex split: bad JSON: {e}"))?;
        Ok(v.get("block")
            .and_then(|b| b.get("block_id"))
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// Layout export for capture — R5 (typed against Rex's layout
    /// trees then).
    fn export_layout(&self, _window_id: &str) -> Result<serde_json::Value, String> {
        Err("layout export on Rex lands with templates + capture (R5)".to_string())
    }

    /// Foreground-process info for capture — R5.
    fn process_info(&self, _pane_id: &str) -> Result<serde_json::Value, String> {
        Err("process info on Rex lands with templates + capture (R5)".to_string())
    }
}

impl HostExt for RexHost {
    /// Rex has no plugin system — `caps.plugins` is false and the
    /// Plugins group stays hidden.
    fn list_plugins(&self) -> Result<Vec<serde_json::Value>, String> {
        Err("Rex has no plugin system".to_string())
    }

    fn invoke_plugin_action(&self, _plugin_id: &str, _action_id: &str) -> Result<(), String> {
        Err("Rex has no plugin system".to_string())
    }

    /// A block's working directory (`block call … pwd`; a file URL —
    /// keep the path).
    fn pane_cwd(&self, pane_id: &str) -> Result<Option<String>, String> {
        let out = self_rex(
            self,
            &[
                "block",
                "call",
                "-s",
                &self_session(self),
                "-b",
                pane_id,
                "pwd",
            ],
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex pwd: bad JSON: {e}"))?;
        let p = v
            .get("pwd")
            .and_then(|p| p.as_str())
            .ok_or_else(|| "rex pwd: no \"pwd\" field".to_string())?;
        let path = if let Some(rest) = p.strip_prefix("file://") {
            match rest.find('/') {
                Some(i) => &rest[i..],
                None => "",
            }
        } else {
            p
        };
        Ok((!path.is_empty()).then(|| path.to_string()))
    }

    /// `rex send-key -s S -b B ctrl+c` (nav's only use: interrupting
    /// the foreground process).
    fn send_keys(&self, pane_id: &str, keys: &[&str]) -> Result<(), String> {
        for key in keys {
            self_rex(
                self,
                &["send-key", "-s", &self_session(self), "-b", pane_id, key],
            )?;
        }
        Ok(())
    }

    /// Same as `send_keys` — Rex addresses agents by their block.
    fn agent_send_keys(&self, target: &str, keys: &[&str]) -> Result<(), String> {
        self.send_keys(target, keys)
    }

    /// No agent-release action on Rex — `^x` is hidden
    /// (`caps.agent_release` is false).
    fn release_agent(&self, _pane_id: &str) -> Result<(), String> {
        Err("Rex has no agent release".to_string())
    }

    /// No host toast surface; the toast was dropped anyway.
    fn notify(&self, _message: &str) -> Result<(), String> {
        Err("Rex has no notification surface".to_string())
    }
}

// RexHost's `run` is the raw-CLI escape hatch (muxhost v0.3.1, the
// same contract as HerdrHost::raw_request); `session` is the scope
// every -s call uses. `rex new` is the one call that doesn't take -s
// (it makes the session).
fn self_rex(host: &RexHost, args: &[&str]) -> Result<String, String> {
    host.run(args, None)
}

fn self_session(host: &RexHost) -> String {
    host.session().to_string()
}
