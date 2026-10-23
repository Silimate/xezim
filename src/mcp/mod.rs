//! `xezim --mcp`: a Model Context Protocol server on stdin and stdout.
//!
//! Messages are newline-delimited JSON-RPC 2.0, as the MCP stdio transport
//! defines them. The server offers four tools: `compile`, `simulate`,
//! `coverage` and `assertions` (see `docs/mcp.md`).
//!
//! Each tool call runs this executable again as a child process, in the
//! directory the call names, with its output going to a log file. A crash, a
//! hang or a runaway allocation in one simulation therefore costs that call
//! only, and a call can be cancelled or time out by killing its child. Calls
//! run on threads of their own, so a long simulation does not hold up the
//! others.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};

mod cov;
mod run;

/// Protocol revisions this server speaks, newest first. A client asking for
/// one of them gets it; any other request gets the newest.
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "Runs xezim, a SystemVerilog simulator, on HDL sources. \
Every tool runs in `cwd` (default: the server's working directory) and resolves relative \
paths against it. `compile` checks that a design parses and elaborates; `simulate` runs it, \
with an SV top or a cocotb test module; `coverage` reports code coverage per instance with \
the bins still uncovered; `assertions` reports each assertion's passes, failures and vacuous \
passes per instance. `coverage` and `assertions` run a simulation, or read the results file \
(xezim_cov.json) of an earlier run when given `db`.";

/// The calls in flight: request id to the process id of the child running
/// it, once it has one.
pub(crate) type Live = Arc<Mutex<HashMap<String, Option<u32>>>>;

/// Serve until stdin closes and the calls in flight have answered.
pub fn serve() -> i32 {
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let live: Live = Arc::default();
    let root = run::root();
    let mut calls: Vec<std::thread::JoinHandle<()>> = Vec::new();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                send(
                    &out,
                    &fail(&Value::Null, -32700, &format!("parse error: {e}")),
                );
                continue;
            }
        };
        let Some(method) = msg.get("method").and_then(Value::as_str) else {
            // A response, or a batch: this server sends no requests, and the
            // stdio transport has no batches since 2025-06-18.
            if msg.is_array() {
                send(
                    &out,
                    &fail(&Value::Null, -32600, "batches are not supported"),
                );
            }
            continue;
        };
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        if method == "notifications/cancelled" {
            cancel(&live, &params["requestId"]);
            continue;
        }
        // Any other notification needs no answer.
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        match method {
            "initialize" => send(&out, &reply(&id, initialize(&params))),
            "ping" => send(&out, &reply(&id, json!({}))),
            "tools/list" => send(&out, &reply(&id, json!({ "tools": tools() }))),
            "tools/call" => {
                let key = id.to_string();
                live.lock().unwrap().insert(key.clone(), None);
                let (out, live, root) = (out.clone(), live.clone(), root.clone());
                calls.retain(|c| !c.is_finished());
                calls.push(std::thread::spawn(move || {
                    let job = run::Job {
                        key: key.clone(),
                        live: live.clone(),
                        root,
                    };
                    let msg = call(&params, &job)
                        .map(|r| reply(&id, r))
                        .unwrap_or_else(|(code, e)| fail(&id, code, &e));
                    // A cancelled call gets no answer.
                    if live.lock().unwrap().remove(&key).is_some() {
                        send(&out, &msg);
                    }
                }));
            }
            _ => send(
                &out,
                &fail(&id, -32601, &format!("method not found: {method}")),
            ),
        }
    }
    // A client that closes stdin still gets the answers to its calls, as a
    // script piping requests in does. One that kills the server stops them:
    // on Linux each child dies with the thread that started it.
    for c in calls {
        let _ = c.join();
    }
    let _ = std::fs::remove_dir_all(&root);
    0
}

fn send(out: &Mutex<std::io::Stdout>, msg: &Value) {
    let mut out = out.lock().unwrap();
    let _ = writeln!(out, "{msg}");
    let _ = out.flush();
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn fail(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn cancel(live: &Live, id: &Value) {
    if let Some(Some(pid)) = live.lock().unwrap().remove(&id.to_string()) {
        run::kill(pid);
    }
}

fn initialize(params: &Value) -> Value {
    let asked = params["protocolVersion"].as_str().unwrap_or("");
    let version = PROTOCOLS
        .iter()
        .find(|v| **v == asked)
        .unwrap_or(&PROTOCOLS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "xezim", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

/// Run one tool. A JSON-RPC error is an unknown tool; a bad argument or a
/// run that could not start is a result with `isError`, which the model sees.
fn call(params: &Value, job: &run::Job) -> Result<Value, (i64, String)> {
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let name = params["name"].as_str().unwrap_or("");
    let result = match name {
        "compile" => run::compile(&args, job),
        "simulate" => run::simulate(&args, job),
        "coverage" => cov::coverage(&args, job),
        "assertions" => cov::assertions(&args, job),
        _ => return Err((-32602, format!("unknown tool: {name}"))),
    };
    Ok(match result {
        Ok(v) => json!({
            "content": [{ "type": "text", "text": v.to_string() }],
            "structuredContent": v,
            "isError": false,
        }),
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    })
}

/// The arguments every tool that runs xezim takes.
fn spec(simulate: bool) -> serde_json::Map<String, Value> {
    let list =
        |what: &str| json!({ "type": "array", "items": { "type": "string" }, "description": what });
    let mut p = serde_json::Map::new();
    p.insert("cwd".into(), json!({ "type": "string", "description": "Directory to run in; relative paths resolve against it. Default: the server's working directory" }));
    p.insert("sources".into(), list("Source files, in compile order"));
    p.insert("top".into(), json!({ "type": "string", "description": "Top-level module. Default: found automatically" }));
    p.insert("include_dirs".into(), list("Include directories"));
    p.insert("defines".into(), list("Macros, as NAME or NAME=VALUE"));
    p.insert(
        "parameters".into(),
        list("Top-level parameter overrides, as NAME=VALUE"),
    );
    p.insert(
        "args".into(),
        list(
            "More xezim arguments, such as [\"-f\", \"files.f\"], [\"--sv2017\"] or [\"--compat\"]",
        ),
    );
    p.insert("timeout".into(), json!({ "type": "integer", "description": "Seconds before the run is stopped. Default: 600" }));
    p.insert("log_lines".into(), json!({ "type": "integer", "description": "Lines of the log's end to return. Default: 40" }));
    if simulate {
        p.insert(
            "plusargs".into(),
            list("Plusargs, such as +VERBOSE or +N=4"),
        );
        p.insert("max_time".into(), json!({ "type": "string", "description": "Simulated time limit, such as 10us, or none. Default: 100ms" }));
        p.insert(
            "seed".into(),
            json!({ "type": "integer", "description": "Random seed. Default: 1" }),
        );
        p.insert("cocotb".into(), json!({
            "type": "object",
            "description": "Run a cocotb test module against `top` instead of an SV testbench",
            "properties": {
                "module": { "type": "string", "description": "Test module(s), comma-separated, importable from cwd" },
                "filter": { "type": "string", "description": "Regular expression: only the tests whose names match run. Default: all" },
                "python": { "type": "string", "description": "Python with cocotb 2 installed. Default: python3" },
            },
            "required": ["module"],
        }));
    }
    p
}

fn tools() -> Value {
    let compile = spec(false);
    let simulate = spec(true);
    let mut coverage = spec(true);
    coverage.insert("db".into(), json!({ "type": "string", "description": "Read this results file (xezim_cov.json, or a directory holding one) instead of running" }));
    coverage.insert("counting".into(), json!({ "type": "string", "enum": ["xezim", "reference"], "description": "What a run counts: xezim's full rules, or the reference simulator's documented defaults (no enum or generate-block toggles, no bits tied to constants, only && and || terms, blocks instead of statements, no FSM reset transitions). Default: xezim" }));
    coverage.insert("coverage_scope".into(), json!({ "type": "array", "items": { "type": "string" }, "description": "Only cover these instance subtrees, such as tb.dut. Default: the whole design" }));
    coverage.insert("instance".into(), json!({ "type": "string", "description": "Only report this instance and its subtree: a dotted path or a leaf name" }));
    coverage.insert(
        "max_holes".into(),
        json!({ "type": "integer", "description": "Uncovered bins to list. Default: 100" }),
    );
    let mut assertions = spec(true);
    assertions.insert("db".into(), coverage["db"].clone());
    assertions.insert("instance".into(), coverage["instance"].clone());
    let schema = |p: serde_json::Map<String, Value>| json!({ "type": "object", "properties": p });
    json!([
        {
            "name": "compile",
            "description": "Parse and elaborate a design without simulating it. Returns ok, the error and warning counts, and the diagnostics with file, line and column.",
            "inputSchema": schema(compile),
            "annotations": { "readOnlyHint": true },
        },
        {
            "name": "simulate",
            "description": "Simulate a design from an SV top, or from a cocotb test module. $error fails the run. Returns passed, the exit code, the end time, error and warning messages, assertion failures, cocotb test results, the log's end and the log's path.",
            "inputSchema": schema(simulate),
        },
        {
            "name": "coverage",
            "description": "Code coverage per instance: statement, branch, toggle, condition, expression, FSM and assertion counts, each instance's subtree totals and their average, and the uncovered bins (holes). Runs a simulation with coverage on, or reads `db`.",
            "inputSchema": schema(coverage),
        },
        {
            "name": "assertions",
            "description": "Each assert, assume and cover per instance: attempts, nonvacuous passes, vacuous passes, failures and disabled attempts, with a status (passed, failed, vacuous, unattempted; covered or uncovered for a cover). Runs a simulation, or reads `db`.",
            "inputSchema": schema(assertions),
        },
    ])
}
