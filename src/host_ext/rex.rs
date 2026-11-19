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
        // The reply is flat: top-level block_ids (verified live).
        Ok(v.get("block_ids")
            .and_then(|b| b.as_array())
            .and_then(|b| b.first())
            .and_then(|b| b.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// `rex split -s S -b B --split=right|below --ratio PCT --json` →
    /// the new block's id. `--ratio` is an INTEGER PERCENT of the split
    /// the new block gets (verified live — a fraction is a flag parse
    /// error); nav's ratio is a 0–1 float. The new block comes back as
    /// the top-level `block_ids[0]`.
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
        // 0–1 float → integer percent, defaulting to an even split.
        let pct = ((ratio * 100.0).round() as i64).clamp(1, 99);
        let out = self_rex(
            self,
            &[
                "split",
                "-s",
                &block_session(self, pane_id),
                "-b",
                pane_id,
                "--split",
                split,
                "--ratio",
                &pct.to_string(),
                "--json",
            ],
        )?;
        let v: serde_json::Value =
            serde_json::from_str(&out).map_err(|e| format!("rex split: bad JSON: {e}"))?;
        Ok(v.get("block_ids")
            .and_then(|b| b.as_array())
            .and_then(|b| b.first())
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
                &block_session(self, pane_id),
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
                &[
                    "send-key",
                    "-s",
                    &block_session(self, pane_id),
                    "-b",
                    pane_id,
                    key,
                ],
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
// same contract as HerdrHost::raw_request). Block-scoped calls must
// use the BLOCK's session (muxhost v0.3.4's block_session — the
// topology snapshot knows it; foreign -s is "block not found");
// `rex new` is the one call that doesn't take -s at all.
fn self_rex(host: &RexHost, args: &[&str]) -> Result<String, String> {
    host.run(args, None)
}

fn block_session(host: &RexHost, block_id: &str) -> String {
    host.block_session(block_id)
        .unwrap_or_else(|| host.session().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use muxhost::rex::{RexHost, RexRunner};
    use muxhost::SourceRef;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    type Call = (Vec<String>, Option<String>);

    /// Records every call, answers from a queue (mirrors muxhost's test
    /// runner — that one is cfg(test)-private). The state is behind Rc
    /// so the clone RexHost::new takes shares it.
    #[derive(Clone, Default)]
    struct FakeRunner {
        calls: Rc<RefCell<Vec<Call>>>,
        replies: Rc<RefCell<VecDeque<Result<String, String>>>>,
    }

    impl FakeRunner {
        fn reply(self, r: Result<&str, &str>) -> Self {
            self.replies
                .borrow_mut()
                .push_back(r.map(str::to_string).map_err(str::to_string));
            self
        }
    }

    impl RexRunner for FakeRunner {
        fn run(&self, args: &[&str], stdin: Option<&str>) -> Result<String, String> {
            self.calls.borrow_mut().push((
                args.iter().map(|a| a.to_string()).collect(),
                stdin.map(str::to_string),
            ));
            self.replies
                .borrow_mut()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    fn host(runner: &FakeRunner) -> RexHost {
        RexHost::new(
            Box::new(runner.clone()),
            "session:s1".to_string(),
            SourceRef {
                pane_id: String::new(),
                tab_id: String::new(),
            },
        )
    }

    /// The recorded `rex new --json` shape (verified live): top-level
    /// session_id + initial_windows[].window_id/block_ids.
    const NEW_JSON: &str = r#"{"session_id":"session:abc","initial_windows":[{"window_id":"window:w1","block_ids":["block:b1"]}]}"#;
    const SPLIT_JSON: &str =
        r#"{"anchor_id":"block:b1","block_ids":["block:b2"],"split_ids":[],"revision":3}"#;

    #[test]
    fn create_session_parses_rex_new_shape() {
        let runner = FakeRunner::default().reply(Ok(NEW_JSON));
        let h = host(&runner);
        let created = h.create_session("/tmp/x", "work").unwrap();
        assert_eq!(created.session_id, "session:abc");
        assert_eq!(created.first_window_id, "window:w1");
        assert_eq!(created.root_pane_id.as_deref(), Some("block:b1"));
        assert_eq!(
            runner.calls.borrow()[0].0,
            vec!["new", "work", "-c", "/tmp/x", "--json"]
        );
    }

    #[test]
    fn split_pane_converts_ratio_to_integer_percent() {
        // The first call is the anchor block's session lookup (empty
        // topology → falls back to the host's own session).
        let runner = FakeRunner::default().reply(Ok("{}")).reply(Ok(SPLIT_JSON));
        let h = host(&runner);
        let block = h.split_pane("block:b1", "right", 0.5, "/tmp/x").unwrap();
        assert_eq!(block, "block:b2");
        assert_eq!(
            runner.calls.borrow()[1].0,
            vec![
                "split",
                "-s",
                "session:s1",
                "-b",
                "block:b1",
                "--split",
                "right",
                "--ratio",
                "50", // 0.5 → integer percent (a fraction is a parse error)
                "--json",
            ]
        );
        // herdr's "down" maps to Rex's "below"; the layout ratio 60 → 60.
        let runner = FakeRunner::default().reply(Ok("{}")).reply(Ok(SPLIT_JSON));
        let h = host(&runner);
        h.split_pane("block:b1", "down", 0.6, "/tmp/x").unwrap();
        let args = &runner.calls.borrow()[1].0;
        assert_eq!(args[6], "below");
        assert_eq!(args[8], "60");
    }
}
