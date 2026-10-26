use cam_app::mcp::{self, Session};
use serde_json::{json, Value};
use std::io::{Cursor, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DRAWING: &str = "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n10\n20\n10\n11\n70\n21\n10\n0\nLINE\n10\n70\n20\n10\n11\n70\n21\n50\n0\nLINE\n10\n70\n20\n50\n11\n10\n21\n50\n0\nLINE\n10\n10\n20\n50\n11\n10\n21\n10\n0\nENDSEC\n0\nEOF\n";

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn initialization() -> Vec<Value> {
    vec![
        request(
            1,
            "initialize",
            json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
        ),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    ]
}
fn transcript(messages: Vec<Value>, session: &mut Session) -> Vec<Value> {
    let input = messages
        .into_iter()
        .map(|m| format!("{m}\n"))
        .collect::<String>();
    let mut output = Vec::new();
    mcp::serve(
        Cursor::new(input),
        &mut output,
        mcp::tools(),
        |name, args| session.call(name, args),
    )
    .unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}

#[test]
fn protocol_initialization_tools_errors_and_notification_isolation() {
    let mut session = Session::default();
    let mut messages = vec![request(0, "tools/list", json!({}))];
    messages.extend(initialization());
    messages.extend([
        request(2, "tools/list", json!({})),
        request(3, "tools/call", json!({"name":"add_setup","arguments":{}})),
        request(
            4,
            "tools/call",
            json!({"name":"activate_setup","arguments":{"id":999}}),
        ),
        request(5, "tools/call", json!({"name":"does_not_exist"})),
        json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"add_setup","arguments":{}}}),
        request(6, "tools/call", json!({"name":"get_state","arguments":{}})),
        request(7, "ping", json!({})),
    ]);
    let response = transcript(messages, &mut session);
    assert_eq!(response.len(), 8);
    assert_eq!(response[0]["error"]["code"], -32002);
    assert_eq!(response[1]["result"]["protocolVersion"], "2025-11-25");
    assert!(response[2]["result"]["tools"].as_array().unwrap().len() > 90);
    assert_eq!(response[3]["result"]["isError"], false);
    assert_eq!(response[4]["result"]["isError"], true);
    assert_eq!(response[5]["error"]["code"], -32602);
    assert_eq!(
        response[6]["result"]["structuredContent"]["result"]["document"]["setups"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(response[7]["result"], json!({}));
}

#[test]
fn malformed_input_recovers_and_catalog_arguments_are_enforced() {
    let mut output = Vec::new();
    let mut input = String::from("not JSON\n[]\n");
    for message in initialization() {
        input.push_str(&format!("{message}\n"));
    }
    input.push_str(&format!(
        "{}\n",
        request(
            2,
            "tools/call",
            json!({"name":"update_setup","arguments":{"patch":{"unknown":1}}})
        )
    ));
    let mut session = Session::default();
    let before = session.controller.document().clone();
    mcp::serve(
        Cursor::new(input),
        &mut output,
        mcp::tools(),
        |name, args| session.call(name, args),
    )
    .unwrap();
    let responses = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["error"]["code"], -32600);
    assert_eq!(responses.last().unwrap()["result"]["isError"], true);
    assert_eq!(*session.controller.document(), before);
    for (name, args) in [
        ("get_state", json!({"unexpected":true})),
        ("add_setup", json!([])),
        ("activate_setup", json!({})),
        ("activate_setup", json!({"id":-1})),
        ("set_post_kind", json!({"post":"Mach3"})),
        (
            "set_pending_geometry",
            json!({"loops":[{"region":999,"part":"Outer"}],"start":[1]}),
        ),
    ] {
        assert!(session.call(name, args).is_err(), "{name}");
    }
    let tools = mcp::tools();
    let mut names = std::collections::BTreeSet::new();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(names.insert(name.to_owned()), "duplicate tool {name}");
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        if !tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            let error = session.call(name, json!({})).unwrap_err();
            assert!(error.contains("missing argument"), "{name}: {error}");
        }
    }
}

#[test]
fn edits_patterns_setup_transfers_and_shop_data_use_real_application_history() {
    let mut session = Session::default();
    session
        .call("open_dxf", json!({"text":DRAWING,"name":"MCP part"}))
        .unwrap();
    let id = session.controller.document().setup().operations[0].id();
    let initial = session.controller.operation(id).cloned().unwrap();
    let representation = serde_json::to_value(&initial).unwrap();
    let variant = representation.as_object().unwrap().keys().next().unwrap();
    let patch = json!({variant:{"feed":250.}});
    session
        .call("update_operation", json!({"id":id,"patch":patch}))
        .unwrap();
    let edited = session.controller.operation(id).cloned().unwrap();
    assert_ne!(initial, edited);
    session.call("undo", json!({})).unwrap();
    assert_eq!(session.controller.operation(id), Some(&initial));
    session.call("redo", json!({})).unwrap();
    assert_eq!(session.controller.operation(id), Some(&edited));
    let setup_id = session.controller.active_setup_id();
    let pattern = session
        .call(
            "create_pattern",
            json!({"setup_id":setup_id,"members":[id],"name":"Array",
        "kind":{"kind":"rectangular","nx":2,"ny":1,"dx":80.,"dy":0.},"order":"by_instance"}),
        )
        .unwrap();
    session
        .call(
            "set_pattern_suppressed",
            json!({"setup_id":setup_id,"id":pattern,"suppressed":true}),
        )
        .unwrap();
    assert!(session.controller.document().setup().patterns[0].suppressed);
    session
        .call(
            "dissolve_pattern",
            json!({"setup_id":setup_id,"id":pattern}),
        )
        .unwrap();
    let destination = session.call("add_setup", json!({})).unwrap();
    let copied = session
        .call(
            "transfer_operation",
            json!({"id":id,"destination":destination,"copy":true}),
        )
        .unwrap();
    assert_ne!(copied, json!(id));
    assert_eq!(
        session
            .controller
            .setups()
            .iter()
            .map(|s| s.operations.len())
            .sum::<usize>(),
        2
    );
    let material = session
        .call("add_material", json!({"name":"Aluminium"}))
        .unwrap();
    session
        .call(
            "rename_material",
            json!({"id":material,"name":"Aluminium 6082"}),
        )
        .unwrap();
    assert_eq!(session.library.materials[0].name, "Aluminium 6082");
    let tools_before = session.library.tools.len();
    session
        .call("duplicate_library_tool", json!({"index":0}))
        .unwrap();
    assert_eq!(session.library.tools.len(), tools_before + 1);
    assert!(session
        .call("renumber_tool_library", json!({"order":[0,0]}))
        .is_err());
    session
        .call(
            "update_preferences",
            json!({"patch":{"view":{"gizmo_size":1e8}}}),
        )
        .unwrap();
    assert!(session.settings.view.gizmo_size < 1e8);
}

#[test]
fn background_planning_verification_export_and_cancellation() {
    let mut session = Session::default();
    assert!(session.call("export_nc", json!({})).is_err());
    session
        .call("open_dxf", json!({"text":DRAWING,"name":"MCP part"}))
        .unwrap();
    session.call("start_run", json!({})).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        let status = session.call("get_run_status", json!({})).unwrap();
        if status["pending"] == false {
            break status;
        }
        assert!(Instant::now() < deadline, "job timed out");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(status["outcome"]["completion"], "Complete", "{status}");
    let nc = session.call("export_nc", json!({})).unwrap();
    assert!(nc.as_str().unwrap().contains("G"));
    let time = session.call("get_processing_time", json!({})).unwrap();
    assert!(time["total_seconds"].as_f64().unwrap() > 0.);
    session.call("start_run", json!({})).unwrap();
    session.call("cancel_run", json!({})).unwrap();
    assert!(session.call("export_nc", json!({})).is_err());
    assert!(session.controller.outcome().is_none());
}

#[test]
fn console_binary_speaks_only_protocol_and_exits_on_eof() {
    let mut messages = initialization();
    messages.push(request(
        2,
        "tools/call",
        json!({"name":"get_state","arguments":{}}),
    ));
    let mut child = Command::new(env!("CARGO_BIN_EXE_opencamstudio-mcp"))
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for message in messages {
        writeln!(stdin, "{message}").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(response.len(), 2);
    assert_eq!(response[1]["result"]["isError"], false);
    assert!(response[1]["result"]["structuredContent"]["result"]["document"].is_object());
}

#[test]
fn duplicate_cut_export_requires_the_same_explicit_confirmation_as_the_desktop() {
    let mut session = Session::default();
    session
        .call("open_dxf", json!({"text":DRAWING,"name":"Duplicate cuts"}))
        .unwrap();
    let id = session.controller.document().setup().operations[0].id();
    session
        .call("duplicate_operation", json!({"id":id}))
        .unwrap();
    session.controller.run(&cam_toolpath::CancelToken::new());
    let error = session.call("export_nc", json!({})).unwrap_err();
    assert!(error.contains("allow_duplicates"), "{error}");
    let nc = session
        .call("export_nc", json!({"allow_duplicates":true}))
        .unwrap();
    assert!(nc.as_str().unwrap().contains('G'));
}
