//! `xezim --mcp`: the Model Context Protocol server (docs/mcp.md). Each test
//! starts the server, speaks newline-delimited JSON-RPC 2.0 to it the way an
//! MCP client does, and checks the tools' results against the runs they
//! made.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    dir: PathBuf,
    next: u64,
}

impl Server {
    /// A server running in a fresh directory holding `files` (name, text).
    fn start(tag: &str, files: &[(&str, &str)]) -> Server {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp_server_{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_xezim"))
            .arg("--mcp")
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start xezim --mcp");
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut s = Server {
            child,
            input,
            output,
            dir,
            next: 100,
        };
        let init = s.request(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "xezim", "{init}");
        s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        s
    }

    fn send(&mut self, msg: &Value) {
        self.send_line(&msg.to_string());
    }

    fn send_line(&mut self, line: &str) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
    }

    fn read(&mut self) -> Option<Value> {
        let mut line = String::new();
        if self.output.read_line(&mut line).unwrap() == 0 {
            return None;
        }
        Some(serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}")))
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        let reply = self.read().expect("a reply");
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    /// A tool call's result: its `structuredContent`, or the text of an error.
    fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let reply = self.request("tools/call", json!({ "name": name, "arguments": args }));
        let result = &reply["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if result["isError"] == true {
            return Err(text);
        }
        // The text is the same result, for clients that read only `content`.
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            result["structuredContent"]
        );
        Ok(result["structuredContent"].clone())
    }

    /// Close stdin and collect what the server still sends before it exits.
    fn finish(mut self) -> Vec<Value> {
        self.input = None;
        let mut rest = vec![];
        while let Some(v) = self.read() {
            rest.push(v);
        }
        assert!(self.child.wait().unwrap().success());
        rest
    }
}

const FSM: &str = r#"
module sub(input logic clk, input logic rst, input logic go, output logic [1:0] st);
  always_ff @(posedge clk) begin
    if (rst) st <= 2'd0;
    else case (st)
      2'd0: if (go) st <= 2'd1;
      2'd1: st <= 2'd2;
      2'd2: st <= 2'd0;
      2'd3: st <= 2'd0; // never reached
    endcase
  end
  c_err: cover property (@(posedge clk) st == 2'd3);
endmodule

module tb;
  logic clk = 0, rst = 1, go = 0;
  logic [1:0] st;
  always #5 clk = ~clk;
  sub uut(.clk, .rst, .go, .st);
  a_legal: assert property (@(posedge clk) disable iff (rst) st != 2'd3);
  initial begin
    @(negedge clk) rst = 0;
    @(negedge clk) go = 1;
    @(negedge clk) go = 0;
    repeat (4) @(negedge clk);
    $finish;
  end
endmodule
"#;

/// Two instances of a checker: one whose antecedent never holds, so its
/// assertion only passes vacuously and its cover never matches (§16.14.8).
const CHECKER: &str = r#"
module chk(input logic clk, input logic a, input logic b);
  a_imp: assert property (@(posedge clk) a |-> b);
  c_a: cover property (@(posedge clk) a);
endmodule

module tb2;
  logic clk = 0, a = 0, b = 0;
  always #5 clk = ~clk;
  chk u_quiet(.clk, .a(1'b0), .b);
  chk u_busy(.clk, .a, .b);
  initial begin
    @(negedge clk) begin a = 1; b = 1; end
    @(negedge clk) begin a = 0; b = 0; end
    @(negedge clk) $finish;
  end
endmodule
"#;

const FAILING: &str = r#"
module tb3;
  logic clk = 0, req = 0, gnt = 0;
  always #5 clk = ~clk;
  a_gnt: assert property (@(posedge clk) gnt |-> $past(req)) else $error("grant without request");
  initial begin
    @(negedge clk) gnt = 1;
    @(negedge clk) $finish;
  end
endmodule
"#;

#[test]
fn mcp_protocol() {
    let mut s = Server::start("protocol", &[]);
    // The client's protocol revision when the server speaks it, else the newest.
    let old = s.request("initialize", json!({ "protocolVersion": "2024-11-05" }));
    assert_eq!(old["result"]["protocolVersion"], "2024-11-05");
    let new = s.request("initialize", json!({ "protocolVersion": "1999-01-01" }));
    assert_eq!(new["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(new["result"]["capabilities"]["tools"], json!({}));

    let list = s.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["compile", "simulate", "coverage", "assertions"]);
    for t in tools {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["inputSchema"]["properties"]["sources"].is_object(), "{t}");
        assert!(!t["description"].as_str().unwrap().is_empty());
    }
    assert!(tools[2]["inputSchema"]["properties"]["counting"].is_object());
    assert!(tools[3]["inputSchema"]["properties"]["db"].is_object());

    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    assert_eq!(s.request("nope", json!({}))["error"]["code"], -32601);
    assert_eq!(
        s.request("tools/call", json!({ "name": "nope", "arguments": {} }))["error"]["code"],
        -32602
    );
    // A line that is not JSON gets a parse error with a null id; a
    // notification gets nothing, so the next line read is the ping's reply.
    s.send_line("{not json");
    let err = s.read().unwrap();
    assert_eq!(err["error"]["code"], -32700, "{err}");
    assert_eq!(err["id"], Value::Null);
    s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": {} }));
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    assert!(s.finish().is_empty());
}

#[test]
fn mcp_bad_arguments() {
    let mut s = Server::start("bad_arguments", &[("t.sv", "module t; endmodule\n")]);
    let e = s.call("simulate", json!({})).unwrap_err();
    assert!(e.contains("sources"), "{e}");
    let e = s
        .call(
            "compile",
            json!({ "sources": ["t.sv"], "parameters": ["W"] }),
        )
        .unwrap_err();
    assert!(e.contains("NAME=VALUE"), "{e}");
    let e = s
        .call(
            "coverage",
            json!({ "sources": ["t.sv"], "counting": "bogus" }),
        )
        .unwrap_err();
    assert!(e.contains("counting"), "{e}");
    let e = s
        .call("compile", json!({ "sources": ["t.sv"], "cwd": "missing" }))
        .unwrap_err();
    assert!(e.contains("not a directory"), "{e}");
    let e = s
        .call("simulate", json!({ "sources": "t.sv" }))
        .unwrap_err();
    assert!(e.contains("list of strings"), "{e}");
    let e = s
        .call("assertions", json!({ "db": "none.json" }))
        .unwrap_err();
    assert!(e.contains("cannot read"), "{e}");
    s.finish();
}

#[test]
fn mcp_compile() {
    let mut s = Server::start(
        "compile",
        &[
            (
                "bad.sv",
                "module bad(input logic a, output logic y);\n  assign y = a +;\nendmodule\n",
            ),
            (
                "warn.sv",
                "module warn(output logic y);\n  logic [7:0] w;\n  assign y = 1'b0;\n  sub s(.p(w));\nendmodule\nmodule sub(input logic [3:0] p); endmodule\n",
            ),
            (
                "lib.sv",
                "module top #(parameter int W = 1) (output logic [W-1:0] y);\n  assign y = '0;\nendmodule\n",
            ),
        ],
    );
    let r = s.call("compile", json!({ "sources": ["bad.sv"] })).unwrap();
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(r["exit_code"], 1);
    assert_eq!(r["errors"], 2, "{r}");
    let d = &r["diagnostics"][0];
    assert_eq!(
        (&d["severity"], &d["file"], &d["line"], &d["column"]),
        (&json!("error"), &json!("bad.sv"), &json!(2), &json!(17)),
        "{r}"
    );
    assert!(
        r["log_tail"].as_str().unwrap().contains("assign y = a +;"),
        "{r}"
    );
    // The log is in the server's scratch directory, which only this user
    // can enter and whose name another user can't predict.
    let log = std::path::Path::new(r["log"].as_str().unwrap());
    assert!(log.is_file());
    let root = log.parent().unwrap().parent().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(root).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700, "{}", root.display());
    let name = root.file_name().unwrap().to_str().unwrap();
    assert!(
        name.len() > format!("xezim-mcp-{}", s.child.id()).len() + 8,
        "{name}"
    );

    // A warning keeps the design compilable and names where it is.
    let r = s
        .call("compile", json!({ "sources": ["warn.sv"], "top": "warn" }))
        .unwrap();
    assert_eq!(
        (&r["ok"], &r["errors"], &r["warnings"]),
        (&json!(true), &json!(0), &json!(1)),
        "{r}"
    );
    assert_eq!(
        (&r["diagnostics"][0]["file"], &r["diagnostics"][0]["line"]),
        (&json!("warn.sv"), &json!(4))
    );

    // A missing top, then parameters, defines and include directories.
    let r = s
        .call("compile", json!({ "sources": ["lib.sv"], "top": "nosuch" }))
        .unwrap();
    assert_eq!(r["ok"], false, "{r}");
    let r = s
        .call(
            "compile",
            json!({ "sources": ["lib.sv"], "top": "top", "parameters": ["W=4"], "defines": ["X=1"], "include_dirs": ["."] }),
        )
        .unwrap();
    assert_eq!(r["ok"], true, "{r}");
    s.finish();
}

#[test]
fn mcp_simulate() {
    let mut s = Server::start(
        "simulate",
        &[
            ("checker.sv", CHECKER),
            ("failing.sv", FAILING),
            (
                "err.sv",
                "module e; initial begin $error(\"boom\"); $finish; end endmodule\n",
            ),
            (
                "seed.sv",
                "module r; initial begin $display(\"draw=%0d\", $urandom); $finish; end endmodule\n",
            ),
            (
                "plus.sv",
                "module p; int n; initial begin if (!$value$plusargs(\"N=%d\", n)) $fatal(1, \"no N\"); $display(\"n=%0d\", n); end endmodule\n",
            ),
        ],
    );
    let r = s
        .call("simulate", json!({ "sources": ["checker.sv"] }))
        .unwrap();
    assert_eq!(
        (&r["passed"], &r["exit_code"], &r["finished"]),
        (&json!(true), &json!(0), &json!(true)),
        "{r}"
    );
    assert_eq!(r["sim_time"], 30, "{r}");
    assert_eq!(
        (&r["assertions"]["total"], &r["assertions"]["failed"]),
        (&json!(4), &json!(0)),
        "{r}"
    );

    let r = s
        .call("simulate", json!({ "sources": ["failing.sv"] }))
        .unwrap();
    assert_eq!(
        (&r["passed"], &r["exit_code"]),
        (&json!(false), &json!(1)),
        "{r}"
    );
    let f = &r["assertions"]["failures"][0];
    assert_eq!(
        (&f["instance"], &f["name"], &f["line"]),
        (&json!("tb3"), &json!("a_gnt"), &json!(5)),
        "{r}"
    );
    let d = &r["diagnostics"][0];
    assert_eq!(
        (&d["severity"], &d["message"], &d["scope"]),
        (
            &json!("error"),
            &json!("grant without request"),
            &json!("tb3.a_gnt")
        ),
        "{r}"
    );
    assert_eq!(d["time"], "15", "{r}");

    // $error fails the run without any assertion.
    let r = s
        .call("simulate", json!({ "sources": ["err.sv"] }))
        .unwrap();
    assert_eq!(
        (&r["passed"], &r["errors"]),
        (&json!(false), &json!(1)),
        "{r}"
    );

    // The seed and plusargs reach the run.
    let draw = |s: &mut Server, seed: u64| {
        s.call("simulate", json!({ "sources": ["seed.sv"], "seed": seed }))
            .unwrap()["log_tail"]
            .clone()
    };
    assert_eq!(draw(&mut s, 7), draw(&mut s, 7));
    assert_ne!(draw(&mut s, 7), draw(&mut s, 8));
    let r = s
        .call(
            "simulate",
            json!({ "sources": ["plus.sv"], "plusargs": ["+N=42"] }),
        )
        .unwrap();
    assert!(r["log_tail"].as_str().unwrap().contains("n=42"), "{r}");
    assert_eq!(r["passed"], true);
    s.finish();
}

const SPIN: &str = "module spin; logic clk = 0; always #1 clk = ~clk; endmodule\n";

#[test]
fn mcp_timeout_and_cancel() {
    let mut s = Server::start("timeout", &[("spin.sv", SPIN)]);
    let start = Instant::now();
    let r = s
        .call(
            "simulate",
            json!({ "sources": ["spin.sv"], "max_time": "none", "timeout": 1 }),
        )
        .unwrap();
    assert_eq!(
        (&r["timed_out"], &r["passed"], &r["exit_code"]),
        (&json!(true), &json!(false), &Value::Null),
        "{r}"
    );
    assert!(start.elapsed() < Duration::from_secs(30));

    // A cancelled call's run is stopped and gets no reply, and the server
    // goes on serving.
    let server = s.child.id();
    s.send(&json!({
        "jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": { "name": "simulate", "arguments": { "sources": ["spin.sv"], "max_time": "none" } },
    }));
    let start = Instant::now();
    while proc() && children(server).is_empty() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the run never started"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    s.send(&json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 7 } }));
    // A reply to the cancelled call would come before the ping's.
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(s.request("ping", json!({}))["result"], json!({}));
    assert!(children(server).is_empty(), "the cancelled run still runs");
    let rest = s.finish();
    assert!(rest.iter().all(|m| m["id"] != 7), "{rest:?}");
}

/// Whether this system lists its processes in /proc.
fn proc() -> bool {
    std::path::Path::new("/proc/self/stat").exists()
}

/// The processes whose parent is `pid`, from /proc (none without it).
fn children(pid: u32) -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return vec![];
    };
    dir.flatten()
        .filter_map(|e| {
            let child: u32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            // `pid (comm) state ppid ...`; comm may hold spaces and parentheses.
            let ppid: u32 = stat[stat.rfind(')')? + 2..]
                .split(' ')
                .nth(1)?
                .parse()
                .ok()?;
            (ppid == pid).then_some(child)
        })
        .collect()
}

/// A client that closes stdin right after its calls, as a script piping
/// requests in does, still gets their answers.
#[test]
fn mcp_answers_after_stdin_closes() {
    let mut s = Server::start(
        "eof",
        &[("t.sv", "module t; initial #5 $finish; endmodule\n")],
    );
    for id in [8, 9] {
        s.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "simulate", "arguments": { "sources": ["t.sv"] } },
        }));
    }
    let mut rest = s.finish();
    rest.sort_by_key(|m| m["id"].as_u64());
    assert_eq!(rest.len(), 2, "{rest:?}");
    for (m, id) in rest.iter().zip([8, 9]) {
        assert_eq!(m["id"], id);
        assert_eq!(m["result"]["structuredContent"]["passed"], true, "{m}");
        assert_eq!(m["result"]["structuredContent"]["sim_time"], 5, "{m}");
    }
}

/// The log is read whole for its diagnostics, however long: here an error
/// in the middle of a 40 MB log.
#[test]
fn mcp_long_log() {
    let line = "x".repeat(100);
    let src = format!(
        "module big;\n  initial begin\n    repeat (200000) $display(\"{line}\");\n    $error(\"in the middle\");\n    repeat (200000) $display(\"{line}\");\n    $finish;\n  end\nendmodule\n"
    );
    let mut s = Server::start("long_log", &[("big.sv", &src)]);
    let r = s
        .call("simulate", json!({ "sources": ["big.sv"], "log_lines": 2 }))
        .unwrap();
    let size = std::fs::metadata(r["log"].as_str().unwrap()).unwrap().len();
    assert!(size > 32 << 20, "{size}");
    assert_eq!(
        (&r["errors"], &r["passed"]),
        (&json!(1), &json!(false)),
        "{r}"
    );
    assert_eq!(r["diagnostics"][0]["message"], "in the middle", "{r}");
    assert_eq!(
        r["log_tail"],
        format!("{line}\nSimulation finished at time 0 ($finish called)")
    );
    assert_eq!(r["finished"], true);
    s.finish();
}

/// A fatal message fails the run, though here xezim exits 0 after it.
#[test]
fn mcp_fatal_message() {
    let src = "class C; int x; endclass\nmodule nd;\n  C c;\n  initial #1 $display(\"x=%0d\", c.x);\nendmodule\n";
    let mut s = Server::start("fatal", &[("nd.sv", src)]);
    let r = s.call("simulate", json!({ "sources": ["nd.sv"] })).unwrap();
    let d = &r["diagnostics"][0];
    assert_eq!(d["severity"], "fatal", "{r}");
    assert!(
        d["message"]
            .as_str()
            .unwrap()
            .contains("null object dereference"),
        "{r}"
    );
    assert_eq!(
        (&r["errors"], &r["passed"]),
        (&json!(1), &json!(false)),
        "{r}"
    );
    s.finish();
}

/// cocotb's setup runs Python, which the call's timeout bounds too.
#[test]
fn mcp_cocotb_setup_times_out() {
    let mut s = Server::start("cocotb_setup", &[("t.sv", "module t; endmodule\n")]);
    let python = s.dir.join("slow.sh");
    std::fs::write(&python, "#!/bin/sh\nexec sleep 600\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).unwrap();
    let start = Instant::now();
    let e = s
        .call(
            "simulate",
            json!({ "sources": ["t.sv"], "top": "t", "timeout": 1, "cocotb": { "module": "m", "python": python } }),
        )
        .unwrap_err();
    assert!(e.contains("timed out"), "{e}");
    assert!(start.elapsed() < Duration::from_secs(30));
    assert!(children(s.child.id()).is_empty());
    s.finish();
}

/// The holes of `instance` with `metric`.
fn holes<'a>(r: &'a Value, instance: &str, metric: &str) -> Vec<&'a Value> {
    r["holes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|h| h["instance"] == instance && h["metric"] == metric)
        .collect()
}

fn instance<'a>(r: &'a Value, path: &str) -> &'a Value {
    r["instances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["instance"] == path)
        .unwrap_or_else(|| panic!("no {path} in {r}"))
}

#[test]
fn mcp_coverage() {
    let mut s = Server::start("coverage", &[("fsm.sv", FSM)]);
    let r = s
        .call("coverage", json!({ "sources": ["fsm.sv"] }))
        .unwrap();
    assert_eq!(
        (&r["run"]["passed"], &r["counting"]),
        (&json!(true), &json!("xezim")),
        "{r}"
    );
    // The scope above the instances, then each instance.
    let paths: Vec<&str> = r["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["instance"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["tb", "tb.uut"]);
    let uut = instance(&r, "tb.uut");
    assert_eq!(uut["design_unit"], "sub");
    // The 2'd3 arm never runs: its statement, its case item and its cover
    // are holes of tb.uut.
    let st = &uut["metrics"]["statement"];
    assert_eq!(
        st["total"].as_u64().unwrap() - st["covered"].as_u64().unwrap(),
        1,
        "{uut}"
    );
    let line = |h: &Value| h["line"].as_u64().unwrap();
    assert_eq!(
        holes(&r, "tb.uut", "statement")
            .iter()
            .map(|h| line(h))
            .collect::<Vec<_>>(),
        [9],
        "{r}"
    );
    assert!(
        holes(&r, "tb.uut", "branch")
            .iter()
            .any(|h| line(h) == 9 && h["arm"] == "item"),
        "{r}"
    );
    let a = holes(&r, "tb.uut", "assertion");
    assert_eq!(
        (a.len(), &a[0]["name"], &a[0]["status"]),
        (1, &json!("c_err"), &json!("uncovered")),
        "{r}"
    );
    assert_eq!(uut["holes"]["assertion"], 1);
    // tb's subtree pools its own counts and tb.uut's; its assertion passed.
    let tb = instance(&r, "tb");
    assert_eq!(
        tb["subtree"]["assertion"],
        json!({ "covered": 1, "total": 2, "percent": 50.0 }),
        "{tb}"
    );
    assert_eq!(
        tb["metrics"]["assertion"],
        json!({ "covered": 1, "total": 1, "percent": 100.0 }),
        "{tb}"
    );
    let pooled = |i: &Value, m: &str| i["metrics"][m]["total"].as_u64().unwrap_or(0);
    assert_eq!(
        pooled(tb, "statement") + pooled(uut, "statement"),
        tb["subtree"]["statement"]["total"].as_u64().unwrap()
    );
    assert_eq!(r["total"]["statement"], tb["subtree"]["statement"]);
    assert!(
        tb["average"].as_f64().unwrap() > 0.0 && tb["average"].as_f64().unwrap() < 100.0,
        "{tb}"
    );
    assert!(std::path::Path::new(r["db"].as_str().unwrap()).is_file());

    // Holes are listed most telling first, up to max_holes.
    let few = s
        .call("coverage", json!({ "sources": ["fsm.sv"], "max_holes": 1 }))
        .unwrap();
    assert_eq!(few["holes"].as_array().unwrap().len(), 1);
    assert_eq!(few["holes"][0]["metric"], "assertion");
    assert_eq!(few["holes_total"], r["holes_total"]);
    assert!(r["holes_total"].as_u64().unwrap() > 3);

    // A leaf name selects that instance's subtree.
    let only = s
        .call(
            "coverage",
            json!({ "sources": ["fsm.sv"], "instance": "uut" }),
        )
        .unwrap();
    let paths: Vec<&str> = only["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["instance"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["tb.uut"]);
    assert!(
        only["holes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["instance"] == "tb.uut")
    );

    // The reference profile counts blocks, not statements.
    let refr = s
        .call(
            "coverage",
            json!({ "sources": ["fsm.sv"], "counting": "reference" }),
        )
        .unwrap();
    assert_eq!(refr["counting"], "reference");
    assert_ne!(
        instance(&refr, "tb.uut")["metrics"]["statement"]["total"],
        uut["metrics"]["statement"]["total"],
        "{refr}"
    );

    let e = s
        .call(
            "coverage",
            json!({ "sources": ["fsm.sv"], "instance": "nosuch" }),
        )
        .unwrap_err();
    assert!(e.contains("no instance nosuch"), "{e}");

    // coverage_scope limits what is instrumented.
    let scoped = s
        .call(
            "coverage",
            json!({ "sources": ["fsm.sv"], "coverage_scope": ["tb.uut"] }),
        )
        .unwrap();
    assert!(
        instance(&scoped, "tb")["metrics"]["statement"].is_null(),
        "{scoped}"
    );
    s.finish();
}

/// `y`'s top two bits are tied to 0: holes under xezim's counting, and out of
/// the totals, so no holes, under the reference profile.
#[test]
fn mcp_coverage_tied_bits() {
    let src = r#"
module tied(input logic [1:0] x, output logic [3:0] y);
  assign y = {2'b00, x};
endmodule
module tbt;
  logic [1:0] x = 0;
  logic [3:0] y;
  tied u(.x, .y);
  initial begin #1 x = 3; #1 x = 0; #1 $finish; end
endmodule
"#;
    let mut s = Server::start("coverage_tied", &[("tied.sv", src)]);
    let signals = |r: &Value| -> Vec<String> {
        holes(r, "tbt.u", "toggle")
            .iter()
            .map(|h| h["signal"].as_str().unwrap().to_string())
            .collect()
    };
    let all = s
        .call("coverage", json!({ "sources": ["tied.sv"] }))
        .unwrap();
    assert_eq!(signals(&all), ["y[2]", "y[3]"], "{all}");
    let refr = s
        .call(
            "coverage",
            json!({ "sources": ["tied.sv"], "counting": "reference" }),
        )
        .unwrap();
    assert!(signals(&refr).is_empty(), "{refr}");
    let toggle = &instance(&refr, "tbt.u")["metrics"]["toggle"];
    assert_eq!(toggle["covered"], toggle["total"], "{refr}");
    s.finish();
}

#[test]
fn mcp_coverage_from_db() {
    let mut s = Server::start("coverage_db", &[("fsm.sv", FSM)]);
    let db = s.dir.join("cov").join("xezim_cov.json");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_COV_DB", &db)
        .current_dir(&s.dir)
        .args(["--code-coverage", "fsm.sv"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let ran = s
        .call("coverage", json!({ "sources": ["fsm.sv"] }))
        .unwrap();
    for path in ["cov/xezim_cov.json", "cov"] {
        let r = s.call("coverage", json!({ "db": path })).unwrap();
        assert!(r.get("run").is_none() && r.get("counting").is_none(), "{r}");
        assert_eq!(r["instances"], ran["instances"], "{path}");
        assert_eq!(r["holes"], ran["holes"], "{path}");
    }
    let r = s.call("assertions", json!({ "db": "cov" })).unwrap();
    assert_eq!(r["summary"]["total"], 2, "{r}");

    // A directory's results file is its xezim_cov.json, whatever else is
    // there; a JSON file that holds no results is an error.
    std::fs::write(s.dir.join("cov").join("a.json"), "{}").unwrap();
    let r = s.call("coverage", json!({ "db": "cov" })).unwrap();
    assert_eq!(r["instances"], ran["instances"]);
    let e = s
        .call("coverage", json!({ "db": "cov/a.json" }))
        .unwrap_err();
    assert!(e.contains("not an xezim results file"), "{e}");
    std::fs::remove_file(&db).unwrap();
    let e = s.call("assertions", json!({ "db": "cov" })).unwrap_err();
    assert!(e.contains("xezim_cov.json"), "{e}");
    s.finish();
}

#[test]
fn mcp_assertions() {
    let mut s = Server::start("assertions", &[("checker.sv", CHECKER)]);
    let r = s
        .call("assertions", json!({ "sources": ["checker.sv"] }))
        .unwrap();
    assert_eq!(r["run"]["passed"], true, "{r}");
    assert!(r["run"].get("assertions").is_none());
    let status = |inst: &str, name: &str| {
        let a = r["assertions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["instance"] == inst && a["name"] == name)
            .unwrap_or_else(|| panic!("no {inst} {name} in {r}"));
        a["status"].as_str().unwrap().to_string()
    };
    assert_eq!(status("tb2.u_quiet", "a_imp"), "vacuous");
    assert_eq!(status("tb2.u_quiet", "c_a"), "uncovered");
    assert_eq!(status("tb2.u_busy", "a_imp"), "passed");
    assert_eq!(status("tb2.u_busy", "c_a"), "covered");
    let sum = &r["summary"];
    assert_eq!(
        [
            &sum["total"],
            &sum["passed"],
            &sum["vacuous"],
            &sum["covered"],
            &sum["uncovered"],
            &sum["failed"]
        ],
        [
            &json!(4),
            &json!(1),
            &json!(1),
            &json!(1),
            &json!(1),
            &json!(0)
        ],
        "{r}"
    );
    let quiet = r["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["instance"] == "tb2.u_quiet" && a["name"] == "a_imp")
        .unwrap();
    assert_eq!((&quiet["pass"], &quiet["fail"]), (&json!(0), &json!(0)));
    assert!(quiet["vacuous"].as_u64().unwrap() > 0);

    let busy = s
        .call(
            "assertions",
            json!({ "sources": ["checker.sv"], "instance": "u_busy" }),
        )
        .unwrap();
    assert_eq!(busy["summary"]["total"], 2, "{busy}");
    assert!(
        busy["assertions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["instance"] == "tb2.u_busy")
    );
    s.finish();
}
