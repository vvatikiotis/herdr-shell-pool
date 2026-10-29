use super::*;

#[test]
fn new_workspace_moves_a_warm_shell_to_the_selected_directory() {
    let temporary = tempfile::TempDir::new().unwrap();
    let directory = temporary.path().join("project's directory");
    std::fs::create_dir(&directory).unwrap();
    let (out, log, err) = scenario_arguments(
        &[
            "new-workspace",
            "--cwd",
            directory.to_str().unwrap(),
            "--label",
            "project",
        ],
        vec![
            snapshot(state()),
            processes("warm", &["zsh"]),
            step(
                "pane.move",
                json!({"pane_id":"warm","focus":false,"destination":{"type":"new_workspace","label":"project","tab_label":"project"}}),
                json!({"move_result":{"changed":true,"pane":{"pane_id":"new-pane","workspace_id":"new-workspace","tab_id":"new-tab"}}}),
            ),
            step(
                "pane.wait_for_output",
                json!({"pane_id":"new-pane","source":"recent","timeout_ms":3000}),
                json!({}),
            ),
            step("pane.focus", json!({"pane_id":"new-pane"}), json!({})),
        ],
        true,
        &[("HERDR_ACTIVE_PANE_ID", ""), ("HERDR_PANE_ID", "")],
        false,
    );
    assert!(out.is_empty());
    assert!(err.is_empty());
    assert!(log.contains("HERDR_PANE_ID=new-pane HERDR_ACTIVE_PANE_ID=new-pane HERDR_WORKSPACE_ID=new-workspace HERDR_TAB_ID=new-tab"));
    assert!(log.contains(&format!(
        "cd -- '{}/project'\\''s directory'",
        temporary.path().display()
    )));
}

#[test]
fn new_workspace_rejects_an_unusable_directory_before_creating_anything() {
    let temporary = tempfile::TempDir::new().unwrap();
    let missing = temporary.path().join("missing");
    let (_, _, error) = scenario_arguments(
        &[
            "new-workspace",
            "--cwd",
            missing.to_str().unwrap(),
            "--label",
            "project",
        ],
        vec![],
        false,
        &[],
        false,
    );
    assert!(error.contains("cannot enter workspace directory"));
}

#[test]
fn socket_resolution_supports_home_xdg_and_named_sessions() {
    for mode in ["home", "xdg", "session"] {
        let (out, _, _) = scenario_with(
            "status",
            vec![snapshot(json!({"workspaces":[],"panes":[]}))],
            true,
            &[("HERDR_TEST_SOCKET_MODE", mode)],
            false,
        );
        assert_eq!(out, "0/2 ready\n", "{mode}");
    }
}

#[test]
fn explicit_socket_overrides_config_and_session() {
    scenario_with(
        "status",
        vec![snapshot(json!({}))],
        true,
        &[
            ("XDG_CONFIG_HOME", "/nonexistent"),
            ("HERDR_SESSION", "unrelated"),
        ],
        false,
    );
}

#[test]
fn focus_error_is_not_silently_accepted() {
    let s = state();
    scenario(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            setup(),
            step("pane.focus", json!({}), json!([])),
        ],
        false,
    );
}

#[test]
fn worker_retirement_only_signals_verified_older_builds() {
    use sha2::{Digest, Sha256};
    for case in [
        "older-build",
        "same-build",
        "wrong-birth",
        "wrong-token",
        "wrong-role",
        "newer",
    ] {
        let dir = tempfile::Builder::new()
            .prefix("hp-worker")
            .tempdir_in("/tmp")
            .unwrap();
        let script = dir.path().join("worker.sh");
        std::fs::write(&script, "while :; do sleep 1; done\n").unwrap();
        let mut sentinel = Command::new("/bin/sh")
            .arg(&script)
            .args(["fill-worker", "older"])
            .spawn()
            .unwrap();
        let output = Command::new("ps")
            .args([
                "-p",
                &sentinel.id().to_string(),
                "-o",
                "lstart=",
                "-o",
                "command=",
            ])
            .output()
            .unwrap();
        let identity = String::from_utf8(output.stdout).unwrap();
        let identity = identity.trim();
        assert!(identity.len() >= 26);
        let generation = if case == "same-build" {
            format!("{:x}", Sha256::digest(std::fs::read(executable()).unwrap()))
        } else {
            "older-build".to_owned()
        };
        let record = json!({
            "role": if case == "wrong-role" { "new-tab" } else { "fill" },
            "pid":sentinel.id(), "token":"older", "generation":generation,
            "started": if case == "wrong-birth" { "wrong" } else { &identity[..24] },
            "created": if case == "newer" { i64::MAX } else { 1 },
            "executable": if case == "wrong-token" { "/nonexistent".to_owned() } else { format!("/bin/sh {}", script.display()) }
        }).to_string();
        scenario_with(
            "fill",
            vec![],
            true,
            &[("HERDR_TEST_RECORD", &record)],
            false,
        );
        if case == "older-build" {
            let deadline = Instant::now() + Duration::from_secs(2);
            while sentinel.try_wait().unwrap().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            let retired = sentinel.try_wait().unwrap().is_some();
            if !retired {
                let _ = sentinel.kill();
            }
            sentinel.wait().unwrap();
            assert!(retired, "verified old worker must be retired");
        } else {
            let running = sentinel.try_wait().unwrap().is_none();
            let _ = sentinel.kill();
            sentinel.wait().unwrap();
            assert!(running, "{case} must not be signalled");
        }
    }
}

#[test]
fn malformed_result_is_an_error_not_an_empty_session() {
    scenario(
        "status",
        vec![step("session.snapshot", json!({}), json!([]))],
        false,
    );
}

#[test]
fn malformed_api_error_message_is_rejected() {
    scenario(
        "status",
        vec![Step {
            method: "session.snapshot",
            params: json!({}),
            response: json!({"error":{"message":42}}),
        }],
        false,
    );
}

fn moved(action: &str) -> Step {
    let destination = match action {
        "new-tab" => json!({"type":"new_tab","workspace_id":"work"}),
        "split-left" => {
            json!({"type":"tab","tab_id":"tab","target_pane_id":"source","split":"right","ratio":0.61})
        }
        _ => json!({"type":"tab","tab_id":"tab","target_pane_id":"source","split":"down"}),
    };
    step(
        "pane.move",
        json!({"pane_id":"warm","focus":false,"destination":destination}),
        json!({"move_result":{"changed":true,"pane":{"pane_id":"warm","tab_id":"tab"}}}),
    )
}
fn setup() -> Step {
    step(
        "pane.wait_for_output",
        json!({"pane_id":"warm","source":"recent","timeout_ms":3000}),
        json!({}),
    )
}
fn swap(changed: bool) -> Step {
    step(
        "pane.swap",
        json!({"source_pane_id":"source","target_pane_id":"warm"}),
        json!({"swap":{"changed":changed}}),
    )
}
fn api_error(method: &'static str, message: &str) -> Step {
    Step {
        method,
        params: json!({}),
        response: json!({"error":{"message":message}}),
    }
}

#[test]
fn warm_left_split_preserves_ratio_and_focus() {
    let s = state();
    scenario(
        "split-left",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["bash"]),
            moved("split-left"),
            swap(true),
            setup(),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
    );
}
#[test]
fn warm_down_split_uses_default_ratio() {
    let s = state();
    scenario(
        "split-down",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["sh"]),
            moved("split-down"),
            setup(),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
    );
}
#[test]
fn missing_pool_opens_cold_tab_without_shell_setup() {
    let s = json!({"workspaces":[],"panes":[state()["panes"][0].clone()]});
    let (_, log, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            step(
                "tab.create",
                json!({"workspace_id":"work","cwd":"/tmp/a b","focus":true}),
                json!({}),
            ),
        ],
        true,
        &[],
        false,
    );
    assert!(log.is_empty());
    assert!(err.is_empty(), "existing recovery is silent");
}
#[test]
fn busy_lock_opens_cold_left_split_and_swaps_it() {
    let (_, log) = {
        let (out, log, err) = scenario_with(
            "split-left",
            vec![
                snapshot(state()),
                step(
                    "pane.split",
                    json!({"target_pane_id":"source","direction":"right","ratio":0.61,"cwd":"/tmp/a b","focus":true}),
                    json!({"pane":{"pane_id":"warm"}}),
                ),
                swap(true),
                step("pane.focus", json!({"pane_id":"warm"}), json!({})),
            ],
            true,
            &[],
            true,
        );
        assert!(err.is_empty());
        (out, log)
    };
    assert!(log.is_empty());
}
#[test]
fn unchanged_move_recovers_with_cold_down_split() {
    let s = state();
    scenario(
        "split-down",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            step(
                "pane.move",
                json!({}),
                json!({"move_result":{"changed":false}}),
            ),
            step(
                "pane.split",
                json!({"direction":"down","target_pane_id":"source","cwd":"/tmp/a b","focus":true}),
                json!({"pane_id":"cold"}),
            ),
            step("pane.focus", json!({"pane_id":"cold"}), json!({})),
        ],
        true,
    );
}
#[test]
fn selection_skips_agents_busy_and_unknown_shells() {
    let mut s = state();
    s["panes"].as_array_mut().unwrap().splice(
        1..1,
        [
            json!({"pane_id":"agent","workspace_id":"pool","agent":"worker"}),
            json!({"pane_id":"busy","workspace_id":"pool"}),
            json!({"pane_id":"unknown","workspace_id":"pool"}),
        ],
    );
    scenario(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("busy", &["zsh", "vim"]),
            api_error("pane.process_info", "unavailable"),
            processes("warm", &["/bin/-ZSH"]),
            moved("new-tab"),
            setup(),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
    );
}
#[test]
fn directory_falls_back_to_recorded_cwd_and_quotes_apostrophes() {
    let mut s = state();
    s["panes"][0]["foreground_cwd"] = json!("");
    s["panes"][0]["cwd"] = json!("/tmp/it's here");
    let (_, log) = scenario(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            setup(),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
    );
    assert!(log.contains("cd -- '/tmp/it'\\''s here'"));
}
#[test]
fn source_identifier_and_directory_fall_back() {
    let mut s = state();
    s["panes"][0]["foreground_cwd"] = json!("");
    s["panes"][0]["cwd"] = json!("");
    let (_, log, _) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            setup(),
            step("pane.focus", json!({"pane_id":"warm"}), json!({})),
        ],
        true,
        &[("HERDR_ACTIVE_PANE_ID", ""), ("HERDR_PANE_ID", "source")],
        false,
    );
    assert!(log.contains("cd -- /tmp/herdr-test-home"));
}
#[test]
fn missing_source_identifier_fails_before_api_access() {
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![],
        false,
        &[("HERDR_ACTIVE_PANE_ID", "")],
        false,
    );
    assert!(err.contains("Herdr did not provide HERDR_ACTIVE_PANE_ID"));
}
#[test]
fn absent_source_fails_without_creating_destination() {
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![snapshot(json!({"panes":[]}))],
        false,
        &[],
        false,
    );
    assert!(err.contains("active pane not found: source"));
}
#[test]
fn move_api_error_does_not_create_cold_destination() {
    let s = state();
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            api_error("pane.move", "move refused"),
        ],
        false,
        &[],
        false,
    );
    assert!(err.contains("Herdr API pane.move: move refused"));
}
#[test]
fn failed_left_swap_returns_error_without_shell_setup() {
    let s = state();
    let (_, log, err) = scenario_with(
        "split-left",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("split-left"),
            swap(false),
        ],
        false,
        &[],
        false,
    );
    assert!(log.is_empty());
    assert!(err.contains("warm shell moved, but Herdr could not place it on the left"));
}
#[test]
fn shell_command_failure_does_not_focus_or_recover() {
    let s = state();
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
        ],
        false,
        &[("HERDR_TEST_EXIT", "7")],
        false,
    );
    assert!(err.contains("exit status 7"));
}
#[test]
fn history_advancement_failure_does_not_focus_or_recover() {
    let s = state();
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            setup(),
        ],
        false,
        &[("HERDR_TEST_FAIL_RUN", "2")],
        false,
    );
    assert!(err.contains("history advancement exit status 7"));
}

#[test]
fn output_wait_failure_does_not_focus_or_recover() {
    let s = state();
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            api_error("pane.wait_for_output", "not ready"),
        ],
        false,
        &[],
        false,
    );
    assert!(err.contains("not ready"));
}
#[test]
fn focus_failure_returns_error_without_recovery() {
    let s = state();
    let (_, _, err) = scenario_with(
        "new-tab",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            moved("new-tab"),
            setup(),
            api_error("pane.focus", "focus failed"),
        ],
        false,
        &[],
        false,
    );
    assert!(err.contains("focus failed"));
}
#[test]
fn fill_does_not_add_shells_when_two_are_eligible() {
    let s = json!({"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[{"pane_id":"a","workspace_id":"pool"},{"pane_id":"b","workspace_id":"pool"}]});
    let (_, log) = scenario(
        "fill",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("a", &["zsh"]),
            processes("b", &["bash"]),
        ],
        true,
    );
    assert!(log.is_empty());
}
#[test]
fn fill_adds_a_shell_without_focus_when_pool_is_short() {
    let mut s = state();
    let mut grown = s.clone();
    grown["panes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"pane_id":"second","workspace_id":"pool","tab_id":"second-tab"}));
    s["panes"]
        .as_array_mut()
        .unwrap()
        .retain(|p| p["pane_id"] != "source");
    let (_, log) = scenario(
        "fill",
        vec![
            snapshot(s.clone()),
            snapshot(s),
            processes("warm", &["zsh"]),
            step(
                "tab.create",
                json!({"workspace_id":"pool","cwd":"/tmp/herdr-test-home","focus":false}),
                json!({"tab":{"tab_id":"second-tab"}}),
            ),
            snapshot(grown.clone()),
            step(
                "pane.wait_for_output",
                json!({"pane_id":"second"}),
                json!({}),
            ),
            snapshot(grown),
            processes("warm", &["zsh"]),
            processes("second", &["zsh"]),
        ],
        true,
    );
    assert!(log.contains("second\n"));
}
#[test]
fn fill_creates_pool_workspace_and_initializes_its_first_shell() {
    let empty = json!({"workspaces":[],"panes":[]});
    let one = json!({"workspaces":[{"workspace_id":"pool","label":"__herdr_shell_pool__"}],"panes":[{"pane_id":"first","workspace_id":"pool","tab_id":"first-tab"}]});
    let mut two = one.clone();
    two["panes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"pane_id":"second","workspace_id":"pool","tab_id":"second-tab"}));
    let (_, log) = scenario(
        "fill",
        vec![
            snapshot(empty),
            step(
                "workspace.create",
                json!({"cwd":"/tmp/herdr-test-home","label":"__herdr_shell_pool__","focus":false}),
                json!({}),
            ),
            snapshot(one.clone()),
            snapshot(one.clone()),
            step(
                "pane.wait_for_output",
                json!({"pane_id":"first"}),
                json!({}),
            ),
            snapshot(one.clone()),
            snapshot(one),
            processes("first", &["zsh"]),
            step(
                "tab.create",
                json!({"focus":false}),
                json!({"tab":{"tab_id":"second-tab"}}),
            ),
            snapshot(two.clone()),
            step(
                "pane.wait_for_output",
                json!({"pane_id":"second"}),
                json!({}),
            ),
            snapshot(two),
            processes("first", &["zsh"]),
            processes("second", &["zsh"]),
        ],
        true,
    );
    assert!(log.contains("first\n"));
    assert!(log.contains("second\n"));
}
#[test]
fn fill_exits_successfully_when_pool_lock_is_busy() {
    scenario_with("fill", vec![], true, &[], true);
}
