// End-to-end tests of the MCP server over its stdio transport, driving the
// real binary with raw JSON-RPC — no SDK client needed, which also proves the
// wire format is what a real client will find.

#![cfg(feature = "mcp")]

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use tempfile::TempDir;

struct Client {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
    next_id: i64,
    // the server writes its database in here; dropped on close
    _data_directory: TempDir,
}

impl Client {
    fn spawn(args: &[&str]) -> Self {
        let data_directory = TempDir::new().expect("temp data directory");
        let mut child = Command::new(env!("CARGO_BIN_EXE_oxyromon"))
            .arg("mcp")
            .args(args)
            .env("OXYROMON_DATA_DIRECTORY", data_directory.path())
            .env("OXYROMON_LOG_LEVEL", "off")
            // leak so the drop order is explicit: stdin closes, then we wait
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn oxyromon mcp");
        let out = BufReader::new(child.stdout.take().expect("stdout"));
        let mut client = Client {
            child,
            out,
            next_id: 0,
            _data_directory: data_directory,
        };
        client.initialize();
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            let read = self.out.read_line(&mut line).expect("read a response");
            assert_ne!(read, 0, "the server closed stdout unexpectedly");
            let message: Value = serde_json::from_str(&line).expect("a JSON-RPC message");
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    fn notify(&mut self, method: &str) {
        self.write(json!({"jsonrpc": "2.0", "method": method}));
    }

    fn write(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{message}").expect("write a request");
    }

    fn initialize(&mut self) -> Value {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {"name": "oxyromon-tests", "version": "0"},
                }),
            )
            .get("result")
            .cloned()
            .expect("an initialize result");
        assert_eq!(result["serverInfo"]["name"].as_str(), Some("oxyromon"));
        self.notify("notifications/initialized");
        result
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": tool, "arguments": arguments}))
            .get("result")
            .cloned()
            .expect("a tools/call result")
    }

    fn tool_names(&mut self) -> Vec<String> {
        self.request("tools/list", json!({}))["result"]["tools"]
            .as_array()
            .expect("a tools array")
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect()
    }

    fn close(&mut self) {
        drop(self.child.stdin.take());
        let status = self.child.wait().expect("the server exits on EOF");
        assert!(status.success(), "the server exits cleanly");
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .expect("a text block")
        .to_string()
}

#[test]
fn handshake_exposes_the_full_surface() {
    let mut client = Client::spawn(&[]);
    let tools = client.tool_names();
    for expected in [
        "status",
        "list_systems",
        "get_system",
        "list_games",
        "get_game",
        "search_games",
        "get_settings",
        "downloadable_systems",
        "parse_game_name",
        "import_dats",
        "download_dats",
        "import_roms",
        "sort_roms",
        "check_roms",
        "convert_roms",
        "set_setting",
        "purge_roms",
        "purge_systems",
        "purge_irds",
        "purge_games",
    ] {
        assert!(
            tools.contains(&expected.to_string()),
            "missing tool {expected}"
        );
    }

    let prompts = client.request("prompts/list", json!({}))["result"]["prompts"]
        .as_array()
        .expect("a prompts array")
        .iter()
        .filter_map(|prompt| prompt["name"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert_eq!(prompts, ["audit_system", "onboard_system"]);

    let resources = client.request("resources/list", json!({}))["result"]["resources"]
        .as_array()
        .expect("a resources array")
        .iter()
        .filter_map(|resource| resource["uri"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert_eq!(
        resources,
        [
            "oxyromon://status",
            "oxyromon://systems",
            "oxyromon://settings"
        ]
    );
    client.close();
}

#[test]
fn status_reports_the_empty_library() {
    let mut client = Client::spawn(&[]);
    let result = client.call("status", json!({}));
    assert_ne!(result["isError"].as_bool(), Some(true));
    let status = &result["structuredContent"];
    assert_eq!(status["version"].as_str(), Some(env!("CARGO_PKG_VERSION")));
    assert_eq!(status["read_only"].as_bool(), Some(false));
    assert_eq!(status["systems"].as_i64(), Some(0));
    client.close();
}

#[test]
fn unknown_ids_are_tool_errors_with_pointers() {
    let mut client = Client::spawn(&[]);
    let result = client.call("get_system", json!({"system_id": 1}));
    assert_eq!(result["isError"].as_bool(), Some(true));
    assert!(
        text(&result).contains("list_systems"),
        "the error should point at the tool that has the ids: {}",
        text(&result)
    );
    client.close();
}

#[test]
fn import_dats_then_read_the_catalog_back() {
    let mut client = Client::spawn(&[]);
    let dat =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/Test System (20200721).dat");
    let result = client.call("import_dats", json!({"paths": [dat.to_str().unwrap()]}));
    assert_ne!(result["isError"].as_bool(), Some(true), "{}", text(&result));

    let result = client.call("list_systems", json!({}));
    let systems = result["structuredContent"]["systems"]
        .as_array()
        .expect("systems");
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0]["name"].as_str(), Some("Test System"));
    assert_eq!(systems[0]["games_total"].as_i64(), Some(6));

    let result = client.call("search_games", json!({"query": "test game"}));
    assert_eq!(result["structuredContent"]["count"].as_i64(), Some(6));

    let result = client.call("list_games", json!({"system_id": 1, "limit": 2}));
    assert_eq!(result["structuredContent"]["returned"].as_i64(), Some(2));
    assert_eq!(result["structuredContent"]["total"].as_i64(), Some(6));
    client.close();
}

#[test]
fn destructive_tools_need_confirmation() {
    let mut client = Client::spawn(&[]);
    for tool in ["purge_roms", "purge_systems", "purge_irds", "purge_games"] {
        let arguments = match tool {
            // confirm omitted: it is optional, so absence must refuse too
            "purge_roms" => json!({"trash": true}),
            "purge_games" => json!({"game_ids": [1]}),
            _ => json!({"system_id": 1, "confirm": false}),
        };
        let result = client.call(tool, arguments);
        assert_eq!(
            result["isError"].as_bool(),
            Some(true),
            "{tool} was not gated"
        );
        assert!(
            text(&result).contains("confirm"),
            "{tool} should explain the confirm parameter"
        );
    }
    client.close();
}

#[test]
fn download_dats_reports_each_failed_system() {
    let mut client = Client::spawn(&[]);
    // unknown systems fail before any request, so this needs no network
    let result = client.call(
        "download_dats",
        json!({"systems": ["Not A System", "Nor This One"]}),
    );
    assert_eq!(result["isError"].as_bool(), Some(true));
    let message = text(&result);
    assert!(message.contains("Not A System"), "{message}");
    assert!(message.contains("Nor This One"), "{message}");
    client.close();
}

#[test]
fn convert_roms_accepts_any_case() {
    let mut client = Client::spawn(&[]);
    let dat =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/Test System (20200721).dat");
    let result = client.call("import_dats", json!({"paths": [dat.to_str().unwrap()]}));
    assert_ne!(result["isError"].as_bool(), Some(true), "{}", text(&result));

    let result = client.call("convert_roms", json!({"system_id": 1, "format": "zip"}));
    assert_ne!(result["isError"].as_bool(), Some(true), "{}", text(&result));

    let result = client.call("convert_roms", json!({"system_id": 1, "format": "nkit"}));
    assert_eq!(result["isError"].as_bool(), Some(true));
    assert!(text(&result).contains("unknown format"));
    client.close();
}

#[test]
fn setting_validation_mirrors_the_cli() {
    let mut client = Client::spawn(&[]);
    let result = client.call(
        "set_setting",
        json!({"key": "PREFER_REGIONS", "value": "broad"}),
    );
    assert_ne!(result["isError"].as_bool(), Some(true), "{}", text(&result));

    let result = client.call(
        "set_setting",
        json!({"key": "PREFER_REGIONS", "value": "mars"}),
    );
    assert_eq!(result["isError"].as_bool(), Some(true));
    assert!(text(&result).contains("valid"));

    let result = client.call("set_setting", json!({"key": "NOT_A_SETTING", "value": "1"}));
    assert_eq!(result["isError"].as_bool(), Some(true));

    let result = client.call("get_settings", json!({}));
    let found = result["structuredContent"]["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .any(|setting| {
            setting["key"].as_str() == Some("PREFER_REGIONS")
                && setting["value"].as_str() == Some("broad")
        });
    assert!(found, "the new value should be readable back");
    client.close();
}

#[test]
fn read_only_hides_and_refuses_the_write_tools() {
    let mut client = Client::spawn(&["--read-only"]);
    let tools = client.tool_names();
    assert!(tools.contains(&"status".to_string()));
    for hidden in [
        "sort_roms",
        "import_dats",
        "import_roms",
        "purge_systems",
        "set_setting",
    ] {
        assert!(
            !tools.contains(&hidden.to_string()),
            "{hidden} must not be listed with --read-only"
        );
        let message = client.request("tools/call", json!({"name": hidden, "arguments": {}}));
        assert!(
            message.get("error").is_some(),
            "{hidden} must not be callable with --read-only"
        );
    }
    client.close();
}
