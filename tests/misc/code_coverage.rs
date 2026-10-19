//! Code coverage (`--code-coverage`, docs/coverage-guide.md): statement,
//! branch and toggle counts in xezim_cov.json, per the guide's counting
//! rules. Where a rule matches the reference simulator's report on the same
//! design, the test says so.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn xezim() -> String {
    let mut p = std::env::current_exe().expect("current_exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join("xezim").to_string_lossy().into_owned()
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xezim_code_cov_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Run xezim on `src` (written to `t.sv` in `dir`) with `args`; the results
/// file is `cov.json` there.
fn run(dir: &Path, src: &str, args: &[&str]) -> Output {
    std::fs::write(dir.join("t.sv"), src).unwrap();
    Command::new(xezim())
        .current_dir(dir)
        .env("XEZIM_COV_DB", dir.join("cov.json"))
        .env_remove("XEZIM_CODE_COVERAGE")
        .env_remove("XEZIM_TOGGLE_INPUT_PORTS")
        .env_remove("XEZIM_CODE_COVERAGE_CONSTANT")
        .env_remove("XEZIM_VERBOSE")
        .args(args)
        .arg("t.sv")
        .output()
        .expect("run xezim")
}

fn results(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("cov.json")).unwrap_or_default()
}

fn stmt(line: u32, kind: &str, count: u64) -> String {
    format!(
        "{{\"file\": \"t.sv\", \"line\": {}, \"kind\": \"{}\", \"count\": {}}}",
        line, kind, count
    )
}

/// The counts of the arms of the branch at `line` of kind `kind`.
fn arm_counts(json: &str, line: u32, kind: &str) -> Vec<u64> {
    let key = format!("\"line\": {}, \"kind\": \"{}\", \"arms\": [", line, kind);
    let start = json
        .find(&key)
        .unwrap_or_else(|| panic!("no {kind} at {line}: {json}"));
    let rest = &json[start + key.len()..];
    let arms = &rest[..rest.find("]}").unwrap()];
    arms.split("\"count\": ")
        .skip(1)
        .map(|c| {
            c.chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap()
        })
        .collect()
}

const DESIGN: &str = "\
module sub(input logic clk, input logic [1:0] sel, output logic [3:0] y);
  logic [3:0] r;
  assign y = r;
  always @(posedge clk) begin
    case (sel)
      2'd0: r <= 4'h1;
      2'd1: r <= 4'h2;
      2'd2: r <= 4'h4;
    endcase
  end
endmodule
module tb;
  logic clk = 0;
  logic [1:0] sel;
  logic [3:0] y0;
  int cnt;
  sub u0(.clk(clk), .sel(sel), .y(y0));
  always #5 clk = ~clk;
  function int f(int x);
    if (x > 2)
      return 1;
    return 0;
  endfunction
  task t(input int n);
    for (int i = 0; i < n; i++)
      cnt++;
  endtask
  initial begin
    sel = 0; cnt = 0;
    if (cnt == 5) cnt = 2; else cnt = 3;
    t(3);
    cnt = f(cnt) ? 7 : 8;
    unique case (cnt)
      7: cnt = 1;
      8: cnt = 2;
    endcase
    @(negedge clk) sel = 1;
    @(negedge clk) sel = 3;
    @(negedge clk) sel = 2;
    @(negedge clk);
    $finish;
  end
endmodule
";

/// Statements count each time they start: leaves, `always` constructs,
/// timing controls and loops (once per start), continuous assignments; `if`
/// and `case` are branches, not statements. The flop's statements and the
/// function's `return`s have the reference simulator's counts.
#[test]
fn statement_counts() {
    let d = scratch("stmt");
    let out = run(&d, DESIGN, &["--code-coverage=stmt"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    assert!(json.contains("\"kinds\": [\"statement\"]"), "{json}");
    assert!(
        !json.contains("\"branches\"") && !json.contains("\"toggles\""),
        "{json}"
    );
    for (line, kind, count) in [
        (18, "always", 8),
        (18, "statement", 8),
        (21, "statement", 1),
        (22, "statement", 0),
        (25, "for", 1),
        (26, "statement", 3),
        (30, "statement", 0),
        (30, "statement", 1),
        (32, "statement", 1),
        (34, "statement", 1),
        (35, "statement", 0),
        (37, "event", 1),
        (37, "statement", 1),
        (40, "event", 1),
        (41, "statement", 1),
        (4, "always", 4),
        (6, "statement", 1),
        (7, "statement", 1),
        (8, "statement", 1),
    ] {
        assert!(
            json.contains(&stmt(line, kind, count)),
            "line {line}: {json}"
        );
    }
    assert!(json.contains("\"line\": 3, \"kind\": \"assign\""), "{json}");
    for line in [5, 20, 33] {
        assert!(
            !json.contains(&format!("\"line\": {line}, \"kind\"")),
            "{json}"
        );
    }
    assert!(
        json.contains("\"scope\": \"tb.u0\",\n        \"design_unit\": \"sub\""),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Branch arms: `if` with its implicit `else`, `if`/`else`, `?:`, a `case`
/// without `default` (an implicit arm) and a `unique case` (none). The
/// counts match the reference simulator's report for the same design.
#[test]
fn branch_arms() {
    let d = scratch("branch");
    let out = run(&d, DESIGN, &["--code-coverage=branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 20, \"kind\": \"if\", \"arms\": [\
         {\"line\": 21, \"arm\": \"if\", \"count\": 1}, \
         {\"line\": 20, \"arm\": \"else (implicit)\", \"count\": 0}]}"
        ),
        "{json}"
    );
    assert_eq!(arm_counts(&json, 30, "if"), [0, 1]);
    assert_eq!(arm_counts(&json, 32, "ternary"), [1, 0]);
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 33, \"kind\": \"case\", \"arms\": [\
         {\"line\": 34, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 35, \"arm\": \"item\", \"count\": 0}]}"
        ),
        "{json}"
    );
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 5, \"kind\": \"case\", \"arms\": [\
         {\"line\": 6, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 7, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 8, \"arm\": \"item\", \"count\": 1}, \
         {\"line\": 5, \"arm\": \"default (implicit)\", \"count\": 1}]}"
        ),
        "{json}"
    );
    assert!(
        json.contains("\"branch\": {\"covered\": 8, \"total\": 12, \"percent\": 66.67}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An `else if` chain is one branch, and a `?:` in a continuous assignment
/// is a branch too (combinational counts are evaluations, so only which arms
/// were taken is checked).
#[test]
fn branch_chain_and_assign_ternary() {
    let d = scratch("chain");
    let src = "\
module tb;
  logic a, b, c;
  logic [1:0] k;
  wire w = a ? b : c;
  always @(a, b)
    if (a)
      k = 1;
    else if (b)
      k = 2;
    else
      k = 3;
  initial begin
    a = 0; b = 0; c = 0;
    #1 b = 1;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let chain = arm_counts(&json, 6, "if");
    assert_eq!(chain.len(), 3, "{json}");
    assert!(chain[0] == 0 && chain[1] > 0 && chain[2] > 0, "{json}");
    let w = arm_counts(&json, 4, "ternary");
    assert!(w[0] == 0 && w[1] > 0, "{json}");
    assert!(json.contains("\"arm\": \"else if\""), "{json}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Toggles: per bit, 0->1 and 1->0 between the ends of time slots, never to
/// or from x; memories and reals untracked, and input ports too under
/// `--no-toggle-input-ports`. As in the reference simulator's report, a
/// 0-1-0 within one time slot is no toggle.
#[test]
fn toggle_counts() {
    let d = scratch("toggle");
    let src = "\
module leaf(input logic a, output logic o);
  assign o = ~a;
endmodule
module tb;
  logic x, g;
  logic [1:0] v;
  logic [7:0] mem [4];
  int unsigned n;
  real r;
  leaf u(.a(x), .o());
  initial begin
    x = 0; g = 0; v = 0; n = 0; r = 0.5;
    #1 x = 1;
    #1 g = 1; g = 0;
    #1 v = 2'b10; v = 2'b01;
    #1 v = 2'bx1;
    #1 x = 0;
    mem[0] = 8'hff;
    n = 3;
  end
endmodule
";
    let out = run(
        &d,
        src,
        &["--code-coverage=toggle", "--no-toggle-input-ports"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for row in [
        "{\"signal\": \"x\", \"width\": 1, \"rise\": [1], \"fall\": [1]}",
        "{\"signal\": \"g\", \"width\": 1, \"rise\": [0], \"fall\": [0]}",
        "{\"signal\": \"v\", \"width\": 2, \"rise\": [1, 0], \"fall\": [0, 0]}",
        "{\"signal\": \"o\", \"width\": 1, \"rise\": [1], \"fall\": [1]}",
    ] {
        assert!(json.contains(row), "{row}: {json}");
    }
    assert!(
        json.contains("{\"signal\": \"n\", \"width\": 32, \"rise\": [1, 1, 0,"),
        "{json}"
    );
    for name in ["mem", "r", "a", "u"] {
        assert!(
            !json.contains(&format!("\"signal\": \"{name}\"")),
            "{name}: {json}"
        );
    }
    assert!(
        json.contains("\"toggle\": {\"covered\": 7, \"total\": 74, \"percent\": 9.46}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The entry of scope `name` in the results file, up to the next scope's.
fn scope<'a>(json: &'a str, name: &str) -> &'a str {
    let key = format!("\"scope\": \"{}\",", name);
    let start = json
        .find(&key)
        .unwrap_or_else(|| panic!("no scope {name}: {json}"));
    let rest = &json[start + key.len()..];
    let end = rest
        .find("\"scope\": ")
        .or_else(|| rest.find("\"design_units\""))
        .unwrap_or(rest.len());
    &json[start..start + key.len() + end]
}

fn toggle(signal: &str, rise: &[u32], fall: &[u32]) -> String {
    format!(
        "{{\"signal\": \"{}\", \"width\": {}, \"rise\": {:?}, \"fall\": {:?}}}",
        signal,
        rise.len(),
        rise,
        fall
    )
}

fn toggle_total(covered: u32, total: u32, percent: &str) -> String {
    format!(
        "\"toggle\": {{\"covered\": {}, \"total\": {}, \"percent\": {}}}",
        covered, total, percent
    )
}

/// A child with one input port per kind of connection, an inout, an
/// unpacked port array and an interface port.
const PORTS: &str = "\
interface bus_if;
  logic [1:0] req;
  modport mp (input req);
endinterface
module leaf (input logic [1:0] l, output logic [1:0] q);
  assign q = l;
endmodule
module child (
  input  logic       clk,
  input  logic [3:0] a,
  input  logic [3:0] b,
  input  logic [3:0] c,
  input  logic [3:0] d,
  input  logic [1:0] e,
  input  logic [3:0] f,
  input  logic [3:0] g,
  input  logic [1:0] arr [2],
  inout  wire  [1:0] io,
  bus_if.mp          bi,
  output logic [3:0] y
);
  logic [1:0] lq;
  leaf u_leaf (.l(e), .q(lq));
  always_ff @(posedge clk)
    y <= a ^ b ^ c ^ {e, e} ^ f ^ g ^ {arr[0], arr[1]} ^ {io, io} ^ {bi.req, bi.req};
endmodule
module tb;
  logic clk = 0;
  logic [3:0] a = 0;
  logic [1:0] n = 0;
  logic [1:0] m [2];
  logic [1:0] drv = 0;
  wire  [1:0] io;
  logic [3:0] y;
  assign io = drv;
  bus_if bif();
  child u (.clk, .a(a), .b(~a), .c(4'd5), .d(), .e(a[2:1]), .f({n, a[1:0]}),
           .g(n), .arr(m), .io(io), .bi(bif.mp), .y);
  always #5 clk = ~clk;
  initial begin
    m[0] = 0; m[1] = 0; bif.req = 0;
    #12 a = 4'b0011; n = 2'b01; drv = 2'b01; m[0] = 2'b11; bif.req = 2'b01;
    #10 a = 4'b0000; n = 2'b00; drv = 2'b00; m[0] = 2'b00; bif.req = 2'b00;
    #10 a = 4'b1000;
    #10 $finish;
  end
endmodule
";

/// §23.3.3: a port connection is a continuous assignment into the port's
/// own net, so an instance's toggle coverage counts its input and inout
/// ports like its outputs, with the value the connection gives them: a
/// whole net's counts (`a`, `clk`, `io`, and `u_leaf.l` through the
/// parent's port `e`), an expression's (`b = ~a`), a select's or
/// concatenation's (`e`, `f`), a narrower actual's low bit (`g`). A constant
/// (`c`) or open (`d`) port never toggles. The unpacked port array and the
/// interface port are not tracked. Which bins are hit, and every scope's
/// totals, are the reference simulator's with full signal visibility.
#[test]
fn input_ports_toggle_in_their_instance() {
    let d = scratch("ports");
    let out = run(&d, PORTS, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let u = scope(&json, "tb.u");
    for row in [
        toggle("a", &[1, 1, 0, 1], &[1, 1, 0, 0]),
        toggle("b", &[1, 1, 0, 0], &[1, 1, 0, 1]),
        toggle("c", &[0, 0, 0, 0], &[0, 0, 0, 0]),
        toggle("clk", &[4], &[4]),
        toggle("d", &[0, 0, 0, 0], &[0, 0, 0, 0]),
        toggle("e", &[1, 0], &[1, 0]),
        toggle("f", &[1, 1, 1, 0], &[1, 1, 1, 0]),
        toggle("g", &[1, 0, 0, 0], &[1, 0, 0, 0]),
        toggle("io", &[1, 0], &[1, 0]),
        toggle("y", &[1, 1, 1, 1], &[1, 1, 1, 1]),
    ] {
        assert!(u.contains(&row), "{row}: {u}");
    }
    for name in ["arr", "bi"] {
        assert!(!u.contains(&format!("\"signal\": \"{name}")), "{name}: {u}");
    }
    assert!(u.contains(&toggle_total(34, 70, "48.57")), "{u}");
    let leaf = scope(&json, "tb.u.u_leaf");
    assert!(leaf.contains(&toggle("l", &[1, 0], &[1, 0])), "{leaf}");
    assert!(leaf.contains(&toggle_total(4, 8, "50.00")), "{leaf}");
    assert!(
        scope(&json, "tb").contains(&toggle_total(21, 30, "70.00")),
        "{json}"
    );
    assert!(
        scope(&json, "tb.bif").contains(&toggle_total(2, 4, "50.00")),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// More port shapes, against the same reference report: an interface's own
/// input ports count in the interface instance (`id` is tied to a
/// constant), each element of an instance array has its slice of the
/// connection, and packed-array, packed-struct, truncated and net ports
/// toggle bit by bit.
#[test]
fn port_shapes_toggle_in_their_instance() {
    let d = scratch("port_shapes");
    let src = "\
typedef struct packed { logic [1:0] hi; logic lo; } st_t;
interface clk_if (input logic clk, input logic [1:0] id);
  logic v = 0;
  always @(posedge clk) v <= ~v;
endinterface
module leaf (input logic [1:0] l, output logic [1:0] q);
  assign q = l;
endmodule
module pk (input logic [1:0][1:0] p2, input st_t s, input logic [1:0] t,
           input wire [1:0] wn, output logic o);
  assign o = ^p2 ^ ^s ^ ^t ^ ^wn;
endmodule
module tb;
  logic clk = 0;
  logic [3:0] bus = 0;
  logic [1:0][1:0] pp = 0;
  st_t ss = 0;
  logic [3:0] wide = 0;
  wire [1:0] net;
  logic [3:0] qa;
  logic o;
  assign net = bus[3:2];
  clk_if ci (.clk(clk), .id(2'd2));
  leaf ua [1:0] (.l(bus), .q(qa));
  pk up (.p2(pp), .s(ss), .t(wide), .wn(net), .o);
  always #5 clk = ~clk;
  initial begin
    #12 bus = 4'b0110; pp = 4'b1001; ss = 3'b101; wide = 4'b1110;
    #10 bus = 4'b1100; pp = 4'b0000; ss = 3'b000; wide = 4'b0000;
    #10 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let expect: [(&str, &[String], (u32, u32, &str)); 4] = [
        (
            "tb.ci",
            &[toggle("clk", &[3], &[3]), toggle("id", &[0, 0], &[0, 0])],
            (4, 8, "50.00"),
        ),
        (
            "tb.ua[0]",
            &[toggle("l", &[0, 1], &[0, 1])],
            (4, 8, "50.00"),
        ),
        (
            "tb.ua[1]",
            &[toggle("l", &[1, 1], &[0, 0])],
            (4, 8, "50.00"),
        ),
        (
            "tb.up",
            &[
                toggle("p2", &[1, 0, 0, 1], &[1, 0, 0, 1]),
                toggle("s", &[1, 0, 1], &[1, 0, 1]),
                toggle("t", &[0, 1], &[0, 1]),
                toggle("wn", &[1, 1], &[0, 0]),
            ],
            (12, 24, "50.00"),
        ),
    ];
    for (name, rows, (covered, total, percent)) in expect {
        let s = scope(&json, name);
        for row in rows {
            assert!(s.contains(row.as_str()), "{row}: {s}");
        }
        assert!(s.contains(&toggle_total(covered, total, percent)), "{s}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `--no-toggle-input-ports` and XEZIM_TOGGLE_INPUT_PORTS=0 leave input and
/// inout ports to the signal that drives them, as before they were counted;
/// `--toggle-input-ports` wins over the variable.
#[test]
fn input_ports_opt_out() {
    let d = scratch("ports_off");
    std::fs::write(d.join("t.sv"), PORTS).unwrap();
    let child = |args: &[&str], var: Option<&str>| {
        let mut cmd = Command::new(xezim());
        cmd.current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .env_remove("XEZIM_CODE_COVERAGE")
            .env_remove("XEZIM_TOGGLE_INPUT_PORTS");
        if let Some(v) = var {
            cmd.env("XEZIM_TOGGLE_INPUT_PORTS", v);
        }
        let out = cmd
            .arg("--code-coverage=toggle")
            .args(args)
            .arg("t.sv")
            .output()
            .expect("run xezim");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        scope(&results(&d), "tb.u").to_string()
    };
    let off = child(&["--no-toggle-input-ports"], None);
    for name in ["a", "b", "c", "clk", "d", "e", "f", "g", "io"] {
        assert!(
            !off.contains(&format!("\"signal\": \"{name}\"")),
            "{name}: {off}"
        );
    }
    // The output `y` and the net `lq` are left.
    assert!(off.contains(&toggle_total(10, 12, "83.33")), "{off}");
    assert_eq!(child(&[], Some("0")), off);
    let on = toggle_total(34, 70, "48.57");
    assert!(child(&[], Some("1")).contains(&on));
    assert!(child(&["--toggle-input-ports"], Some("0")).contains(&on));
    let _ = std::fs::remove_dir_all(&d);
}

/// The two-state executors take back a count when they hand a block to the
/// four-state VM, so both give the same counts (the flop reads x before
/// `d` is driven).
#[test]
fn counts_do_not_depend_on_executor() {
    let d = scratch("engine");
    let src = "\
module tb;
  logic clk = 0;
  logic [3:0] q, d;
  always #5 clk = ~clk;
  always_ff @(posedge clk)
    if (d[0]) q <= d;
    else q <= ~d;
  initial begin
    #12 d = 4'h5;
    #20 d = 4'h2;
    #20 $finish;
  end
endmodule
";
    let code = |extra: &[(&str, &str)]| {
        std::fs::write(d.join("t.sv"), src).unwrap();
        let out = Command::new(xezim())
            .current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .envs(extra.iter().copied())
            .args(["--code-coverage", "t.sv"])
            .output()
            .expect("run xezim");
        assert!(out.status.success());
        let json = results(&d);
        json[json.find("\"code_coverage\"").unwrap()..].to_string()
    };
    let default = code(&[]);
    let four_state = code(&[("XEZIM_TWO_STATE", "0")]);
    assert_eq!(default, four_state);
    assert!(default.contains(&stmt(5, "always_ff", 5)), "{default}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Instances and generate blocks are scopes, not signals (§23.6): the names
/// of an instance array, of an instance in a generate block and of `for`,
/// `if` and `case` generate blocks, chosen or not and nested, have no toggle
/// bins. The signals declared in a generate block count in the enclosing
/// instance, as does a signal named like a nested generate block. The top's
/// 20 signals and 40 bins are the reference simulator's.
#[test]
fn toggle_skips_instance_and_generate_names() {
    let d = scratch("scope_names");
    let src = "\
module leaf(input logic clk, input logic d, output logic q);
  always_ff @(posedge clk) q <= d;
endmodule
module wrap(input logic clk, input logic [1:0] d, output logic [1:0] q);
  for (genvar g = 0; g < 2; g++) begin : gw
    logic w;
    always_ff @(posedge clk) w <= d[g];
    assign q[g] = w;
  end
endmodule
module tb;
  logic clk = 0;
  logic [3:0] d = 0;
  logic [3:0] q;
  logic [1:0] p;
  logic inner = 0;
  leaf ua[1:0] (.clk(clk), .d(d[1:0]), .q(q[1:0]));
  wrap u_w (.clk(clk), .d(d[3:2]), .q(q[3:2]));
  for (genvar i = 0; i < 2; i++) begin : u_r
    logic r;
    always_ff @(posedge clk) r <= d[i];
    leaf u_s (.clk(clk), .d(d[i]), .q(p[i]));
  end
  if (1) begin : u_if
    logic s;
    always_ff @(posedge clk) s <= d[0];
  end else begin : u_else
    logic s;
  end
  case (2)
    1: begin : c1 logic c; end
    default: begin : cd logic c; always_ff @(posedge clk) c <= d[1]; end
  endcase
  for (genvar i = 0; i < 2; i++) begin : outer
    for (genvar j = 0; j < 2; j++) begin : inner
      logic t;
      always_ff @(posedge clk) t <= d[i + j];
    end
  end
  always #5 clk = ~clk;
  initial begin
    repeat (2) @(negedge clk) d = 4'hf;
    inner = 1;
    repeat (2) @(negedge clk) d = 4'h0;
    inner = 0;
    repeat (2) @(negedge clk);
    $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    // A scope's entry up to the end of its toggle rows.
    let scope = |name: &str| {
        let at = json
            .find(&format!("\"scope\": \"{name}\","))
            .unwrap_or_else(|| panic!("no {name}: {json}"));
        let rows = at + json[at..].find("\"toggles\": [").unwrap();
        json[at..rows + json[rows..].find('\n').unwrap()].to_string()
    };
    let tb = scope("tb");
    for name in [
        "ua", "u_w", "u_r", "u_s", "u_if", "u_else", "c1", "cd", "outer",
    ] {
        assert!(
            !tb.contains(&format!("\"signal\": \"{name}\"")),
            "{name}: {tb}"
        );
    }
    for name in [
        "u_r[1].r",
        "u_if.s",
        "cd.c",
        "outer[1].inner[0].t",
        "inner",
        "p",
    ] {
        assert!(
            tb.contains(&format!("\"signal\": \"{name}\", ")),
            "{name}: {tb}"
        );
    }
    assert!(
        tb.contains("\"toggle\": {\"covered\": 40, \"total\": 40, \"percent\": 100.00}"),
        "{tb}"
    );
    let wrap = scope("tb.u_w");
    for name in ["gw[0].w", "gw[1].w"] {
        assert!(
            wrap.contains(&format!("\"signal\": \"{name}\", ")),
            "{name}: {wrap}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

const SMALL: &str = "\
module sub(input logic a, output logic y);
  assign y = ~a;
  initial $display(\"sub\");
endmodule
module tb;
  int x;
  logic a = 0, y;
  sub u(.a(a), .y(y));
  initial begin
    x = 1;
    #1 if (x > 0) $display(\"x=%0d y=%b\", x, y);
    $error(\"boom\");
  end
endmodule
";

/// Without code coverage nothing changes: the output and exit status are
/// the same as with it, and no results file is written for a design with
/// no covergroup or assertion. The instance's blocks are elaborated lazily;
/// instrumenting them keeps the order they run in.
#[test]
fn off_by_default_output_unchanged() {
    let d = scratch("off");
    for extra in [&[][..], &["--error-exit"][..]] {
        let _ = std::fs::remove_file(d.join("cov.json"));
        let off = run(&d, SMALL, extra);
        assert!(!d.join("cov.json").exists());
        let mut args = extra.to_vec();
        args.push("--code-coverage");
        let on = run(&d, SMALL, &args);
        assert!(results(&d).contains("\"code_coverage\""));
        assert_eq!(off.status.code(), on.status.code());
        assert_eq!(off.stdout, on.stdout);
        assert_eq!(off.stderr, on.stderr);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Other simulators' spellings: `+cover=<letters>` picks the kinds and warns
/// once about the ones xezim lacks; bare `+cover` and `-coverage` collect all
/// three, and `-coverage <types>` the types listed (its value used to be read
/// as a top module name); XEZIM_CODE_COVERAGE does what the flag does.
#[test]
fn compatible_spellings() {
    let d = scratch("compat");
    let kinds = |args: &[&str]| {
        let out = run(&d, SMALL, args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json = results(&d);
        let start = json.find("\"kinds\": [").unwrap() + "\"kinds\": [".len();
        let k = json[start..start + json[start..].find(']').unwrap()].to_string();
        (k, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let all = "\"statement\", \"branch\", \"toggle\"";
    let (k, err) = kinds(&["+cover=sbcf"]);
    assert_eq!(k, "\"statement\", \"branch\"");
    assert_eq!(
        err.matches(
            "Warning: +cover=sbcf: xezim has no condition (c), FSM (f) coverage; \
                     collecting statement, branch coverage"
        )
        .count(),
        1,
        "{err}"
    );
    assert_eq!(kinds(&["+cover=t"]).0, "\"toggle\"");
    assert_eq!(kinds(&["+cover"]).0, all);
    assert_eq!(kinds(&["-coverage"]).0, all);
    assert_eq!(kinds(&["-coverage", "+cover=s"]).0, "\"statement\"");
    assert_eq!(kinds(&["-coverage", "all"]).0, all);
    assert_eq!(kinds(&["-coverage", "T"]).0, "\"toggle\"");
    assert_eq!(kinds(&["-coverage", "b:t"]).0, all);
    let (k, err) = kinds(&["-coverage", "B:E:F:U"]);
    assert_eq!(k, "\"statement\", \"branch\"");
    assert_eq!(
        err.matches(
            "Warning: -coverage B:E:F:U: xezim has no expression (e), FSM (f) coverage; \
                     collecting statement, branch coverage"
        )
        .count(),
        1,
        "{err}"
    );
    assert_eq!(kinds(&["-coverage", "B", "+cover=t"]).0, "\"toggle\"");
    assert_eq!(
        kinds(&["--code-coverage=branch,stmt"]).0,
        "\"statement\", \"branch\""
    );
    std::fs::write(d.join("t.sv"), SMALL).unwrap();
    let env = Command::new(xezim())
        .current_dir(&d)
        .env("XEZIM_COV_DB", d.join("cov.json"))
        .env("XEZIM_CODE_COVERAGE", "toggle")
        .arg("t.sv")
        .output()
        .expect("run xezim");
    assert!(env.status.success());
    assert!(results(&d).contains("\"kinds\": [\"toggle\"]"));
    let bad = run(&d, SMALL, &["--code-coverage=lines"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown code coverage kind 'lines'"));
    let _ = std::fs::remove_dir_all(&d);
}

/// `-covworkdir <dir>`, `-covscope <name>` and `-covtest <name>` put the
/// results file in `<dir>/<scope>/<test>/` (defaults `cov_work`, `scope`,
/// `test`), creating it, instead of the current directory. XEZIM_COV_DB
/// still names the file when it is set. `-covoverwrite` changes nothing:
/// the file is always replaced.
#[test]
fn coverage_work_area() {
    let d = scratch("workarea");
    std::fs::write(d.join("t.sv"), SMALL).unwrap();
    let go = |args: &[&str], db: Option<PathBuf>| {
        let mut cmd = Command::new(xezim());
        cmd.current_dir(&d)
            .env_remove("XEZIM_COV_DB")
            .env_remove("XEZIM_CODE_COVERAGE")
            .args(args)
            .arg("t.sv");
        if let Some(db) = db {
            cmd.env("XEZIM_COV_DB", db);
        }
        let out = cmd.output().expect("run xezim");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    let read = |p: &str| std::fs::read_to_string(d.join(p)).unwrap_or_default();
    go(
        &["-coverage", "all", "-covtest", "t1", "-covoverwrite"],
        None,
    );
    assert!(read("cov_work/scope/t1/xezim_cov.json").contains("\"code_coverage\""));
    assert!(!d.join("xezim_cov.json").exists());
    go(
        &[
            "-covworkdir",
            "cw",
            "-covscope",
            "s",
            "-covtest",
            "t2",
            "-coverage",
            "T",
        ],
        None,
    );
    assert!(read("cw/s/t2/xezim_cov.json").contains("\"kinds\": [\"toggle\"]"));
    go(&["-coverage", "-covscope", "s3"], None);
    assert!(d.join("cov_work/s3/test/xezim_cov.json").exists());
    go(
        &["-coverage", "all", "-covtest", "t4"],
        Some(d.join("env.json")),
    );
    assert!(read("env.json").contains("\"code_coverage\""));
    assert!(!d.join("cov_work/scope/t4").exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// `--code-coverage-scope` keeps only the listed subtree, and `--verbose`
/// prints the totals and one line per scope.
#[test]
fn scope_limit_and_summary() {
    let d = scratch("scope");
    let out = run(
        &d,
        DESIGN,
        &[
            "--code-coverage",
            "--code-coverage-scope=tb.u0",
            "--verbose",
        ],
    );
    assert!(out.status.success());
    let json = results(&d);
    assert!(json.contains("\"scope\": \"tb.u0\""), "{json}");
    assert!(!json.contains("\"scope\": \"tb\""), "{json}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("[COV] code coverage: statement 5/5 (100.00%), branch 4/4 (100.00%), toggle"),
        "{err}"
    );
    assert!(
        err.contains("[COV]   tb.u0 (sub): statement 5/5 (100.00%)"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The entry of scope `name` in the results file.
fn scope_entry<'a>(json: &'a str, name: &str) -> &'a str {
    let start = json
        .find(&format!("\"scope\": \"{}\",", name))
        .unwrap_or_else(|| panic!("no scope {name}: {json}"));
    let rest = &json[start..];
    &rest[..rest.find("\n      }").unwrap_or(rest.len())]
}

/// The design-unit entry of module `name` in the results file.
fn unit_entry<'a>(json: &'a str, name: &str) -> &'a str {
    let start = json
        .find(&format!(
            "\"design_unit\": \"{}\",\n        \"instances\"",
            name
        ))
        .unwrap_or_else(|| panic!("no design unit {name}: {json}"));
    let rest = &json[start..];
    &rest[..rest.find("\n      }").unwrap_or(rest.len())]
}

/// Whether the row starting with `key` in a results entry is marked
/// excluded; panics when there is no such row.
fn row_excluded(entry: &str, key: &str) -> bool {
    let start = entry
        .find(key)
        .unwrap_or_else(|| panic!("no {key}: {entry}"));
    let rest = &entry[start..];
    rest[..rest.find('}').unwrap()].contains("\"excluded\": \"constant\"")
}

fn stmt_excluded(entry: &str, line: u32) -> bool {
    row_excluded(
        entry,
        &format!("\"line\": {line}, \"kind\": \"statement\", \"count\": "),
    )
}

fn arm_excluded(entry: &str, line: u32) -> bool {
    row_excluded(entry, &format!("{{\"line\": {line}, \"arm\": "))
}

fn tally(kind: &str, covered: u64, total: u64, excluded: u64) -> String {
    format!(
        "\"{}\": {{\"covered\": {}, \"total\": {}, \"percent\": {:.2}, \"excluded\": {}}}",
        kind,
        covered,
        total,
        100.0 * covered as f64 / total as f64,
        excluded
    )
}

/// Two instances of a module whose `case` selector, `if` conditions and `?:`
/// condition read its parameters.
const CONSTANT_DESIGN: &str = "\
module red #(parameter [1:0] OP = 0, parameter W = 4) (
  input  logic [3:0] a,
  input  logic       s,
  output logic       y,
  output logic       z,
  output logic       m,
  output logic       t
);
  always @(*) begin
    case (OP)
      0, 3: y = &a;
      1: y = |a;
      default: y = ^a;
    endcase
  end
  always @(*) begin
    if (W > 4)
      z = a[0];
    else
      z = a[1];
    if (s && W == 8)
      z = ~z;
  end
  always @(*) begin
    if (a[0])
      m = 1;
    else if (OP == 1)
      m = s;
    else
      m = 0;
  end
  always @(*) t = (OP == 2) ? a[2] : (s ? a[3] : a[1]);
endmodule

module tb;
  logic [3:0] a;
  logic s;
  logic y0, z0, m0, t0, y1, z1, m1, t1;
  red #(.OP(0), .W(4)) u0 (.a, .s, .y(y0), .z(z0), .m(m0), .t(t0));
  red #(.OP(1), .W(8)) u1 (.a, .s, .y(y1), .z(z1), .m(m1), .t(t1));
  initial begin
    a = 0; s = 0;
    #1 a = 4'hf; s = 1;
    #1 a = 4'h5;
    #1 a = 4'ha; s = 0;
    #1 $finish;
  end
endmodule
";

/// Parameters are elaboration-time constants (IEEE 1800 §11.2.1), so in
/// each instance the code a condition on them rules out (§12.4 `if`, §12.5
/// `case`, §11.4.11 `?:`) can never run. Like the reference simulator,
/// xezim leaves it out of the instance's totals: the arms that cannot be
/// taken and their statements, and the whole branch when its decision is a
/// constant (`s && W == 8` is one where `W` is 4). An `else if` chain with
/// a constant condition after a run-time one keeps its other arms. The
/// instances' totals are the reference simulator's. A design unit counts an
/// item that any instance counts.
#[test]
fn constant_conditions_leave_code_out() {
    let d = scratch("constant");
    let out = run(
        &d,
        CONSTANT_DESIGN,
        &["--code-coverage=stmt,branch", "--verbose"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let u0 = scope_entry(&json, "tb.u0");
    assert!(u0.contains(&tally("statement", 9, 9, 5)), "{u0}");
    assert!(u0.contains(&tally("branch", 4, 4, 10)), "{u0}");
    for (line, out) in [(11, false), (12, true), (13, true), (18, true), (20, false)] {
        assert_eq!(stmt_excluded(u0, line), out, "line {line}: {u0}");
    }
    for (line, out) in [(22, true), (26, false), (28, true), (30, false)] {
        assert_eq!(stmt_excluded(u0, line), out, "line {line}: {u0}");
    }
    for (line, kind) in [(10, "case"), (17, "if"), (21, "if")] {
        assert!(
            u0.contains(&format!(
                "\"line\": {line}, \"kind\": \"{kind}\", \"excluded\": \"constant\", \"arms\""
            )),
            "{kind} at {line}: {u0}"
        );
    }
    // The chain at line 25 keeps its `if` and `else` arms.
    assert!(
        u0.contains("\"line\": 25, \"kind\": \"if\", \"arms\""),
        "{u0}"
    );
    for (line, out) in [(26, false), (28, true), (30, false)] {
        assert_eq!(arm_excluded(u0, line), out, "arm at {line}: {u0}");
    }
    let u1 = scope_entry(&json, "tb.u1");
    assert!(u1.contains(&tally("statement", 10, 10, 4)), "{u1}");
    assert!(u1.contains(&tally("branch", 6, 6, 8)), "{u1}");
    assert!(
        u1.contains("\"line\": 21, \"kind\": \"if\", \"arms\""),
        "{u1}"
    );
    for (line, out) in [(26, false), (28, false), (30, true)] {
        assert_eq!(arm_excluded(u1, line), out, "arm at {line}: {u1}");
    }
    let unit = unit_entry(&json, "red");
    assert!(unit.contains(&tally("statement", 13, 13, 1)), "{unit}");
    assert!(unit.contains(&tally("branch", 7, 7, 7)), "{unit}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(
            "[COV]   tb.u0 (red): statement 9/9 (100.00%; 5 excluded), \
             branch 4/4 (100.00%; 10 excluded)"
        ),
        "{err}"
    );
    // Statement coverage alone leaves out the same statements.
    run(&d, CONSTANT_DESIGN, &["--code-coverage=stmt"]);
    let json = results(&d);
    assert!(
        scope_entry(&json, "tb.u0").contains(&tally("statement", 9, 9, 5)),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `--code-coverage-constant=keep` (or XEZIM_CODE_COVERAGE_CONSTANT=keep)
/// counts constant-folded code like any other; the flag wins over the
/// variable, and an unknown mode is an error.
#[test]
fn constant_code_kept_on_request() {
    let d = scratch("keep");
    let keep = |args: &[&str], env: Option<&str>| {
        std::fs::write(d.join("t.sv"), CONSTANT_DESIGN).unwrap();
        let mut cmd = Command::new(xezim());
        cmd.current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .env_remove("XEZIM_CODE_COVERAGE")
            .env_remove("XEZIM_CODE_COVERAGE_CONSTANT")
            .args(args)
            .arg("t.sv");
        if let Some(v) = env {
            cmd.env("XEZIM_CODE_COVERAGE_CONSTANT", v);
        }
        let out = cmd.output().expect("run xezim");
        assert!(out.status.success());
        results(&d)
    };
    let kept = keep(
        &[
            "--code-coverage=stmt,branch",
            "--code-coverage-constant=keep",
        ],
        None,
    );
    assert!(!kept.contains("excluded"), "{kept}");
    assert!(
        scope_entry(&kept, "tb.u0")
            .contains("\"statement\": {\"covered\": 9, \"total\": 14, \"percent\": 64.29}"),
        "{kept}"
    );
    assert_eq!(keep(&["--code-coverage=stmt,branch"], Some("keep")), kept);
    let excluded = keep(
        &[
            "--code-coverage=stmt,branch",
            "--code-coverage-constant=exclude",
        ],
        Some("keep"),
    );
    assert!(
        scope_entry(&excluded, "tb.u0").contains(&tally("statement", 9, 9, 5)),
        "{excluded}"
    );
    let bad = run(
        &d,
        CONSTANT_DESIGN,
        &["--code-coverage", "--code-coverage-constant=maybe"],
    );
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("unknown constant-code mode 'maybe'"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Which constant decisions leave code out, each as in the reference
/// simulator (§12.5, §12.5.1, §11.4.7, §27.5):
/// - a `case` with a constant selector is decided only when every item is
///   a constant too: `case (P)` with an item `a[1:0]` keeps all its arms;
/// - an `&&` with a constant-false operand and an `|` with a constant-true
///   one are constants whatever the other operand is;
/// - `casez` items match the constant selector with their wildcards;
/// - a `unique case` that no item matches leaves out every item;
/// - a genvar is a constant in each generate-loop iteration.
#[test]
fn constant_decision_rules() {
    let d = scratch("rules");
    let src = "\
module sub #(parameter P = 1, parameter [2:0] K = 3'b010) (
  input  logic [3:0] a,
  output logic       v,
  output logic       c,
  output logic       x,
  output logic       e
);
  always @(*) begin
    case (P)
      a[1:0]: v = 1;
      1: v = 0;
      default: v = a[3];
    endcase
  end
  always @(*) begin
    c = a[0];
    if (a[2] && P == 1)
      c = 1;
    if ((P == 2) | a[3])
      c = 0;
  end
  always @(*) begin
    casez (K)
      3'b1??: x = a[0];
      3'b0?1: x = a[1];
      default: x = a[2];
    endcase
  end
  always @(*) begin
    e = 0;
    unique case (K)
      3'b000: e = a[0];
      3'b010: e = a[1];
      3'b011: e = a[2];
    endcase
  end
endmodule

module tb;
  logic [3:0] a;
  logic [1:0] g;
  logic v0, c0, x0, e0, v1, c1, x1, e1;
  sub #(.P(1), .K(3'b011)) u0 (.a, .v(v0), .c(c0), .x(x0), .e(e0));
  sub #(.P(2), .K(3'b110)) u1 (.a, .v(v1), .c(c1), .x(x1), .e(e1));
  for (genvar i = 0; i < 2; i++) begin : gl
    always @(*) begin
      if (i == 0)
        g[i] = a[0];
      else
        g[i] = a[1];
    end
  end
  initial begin
    a = 0;
    #1 a = 4'hf;
    #1 a = 4'h5;
    #1 a = 4'ha;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=stmt,branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let u0 = scope_entry(&json, "tb.u0");
    assert!(u0.contains(&tally("statement", 12, 13, 4)), "{u0}");
    assert!(u0.contains(&tally("branch", 6, 7, 6)), "{u0}");
    let u1 = scope_entry(&json, "tb.u1");
    assert!(u1.contains(&tally("statement", 10, 11, 6)), "{u1}");
    assert!(u1.contains(&tally("branch", 2, 3, 10)), "{u1}");
    for entry in [u0, u1] {
        assert!(
            entry.contains("\"line\": 9, \"kind\": \"case\", \"arms\""),
            "{entry}"
        );
        assert!(
            entry.contains("\"line\": 23, \"kind\": \"casez\", \"excluded\": \"constant\""),
            "{entry}"
        );
    }
    for (line, out) in [(10, false), (11, false), (12, false)] {
        assert_eq!(arm_excluded(u0, line), out, "arm at {line}: {u0}");
        assert_eq!(arm_excluded(u1, line), out, "arm at {line}: {u1}");
    }
    // `a[2] && 1` and `0 | a[3]` in u0; `a[2] && 0` and `1 | a[3]` in u1.
    for line in [17, 19] {
        let key = format!("\"line\": {line}, \"kind\": \"if\", ");
        assert!(u0.contains(&format!("{key}\"arms\"")), "{u0}");
        assert!(
            u1.contains(&format!("{key}\"excluded\": \"constant\"")),
            "{u1}"
        );
    }
    assert!(stmt_excluded(u1, 18) && !stmt_excluded(u1, 20), "{u1}");
    // `casez` K = 3'b011 takes `3'b0?1`; K = 3'b110 takes `3'b1??`.
    for (line, out) in [(24, true), (25, false), (26, true)] {
        assert_eq!(arm_excluded(u0, line), out, "arm at {line}: {u0}");
    }
    for (line, out) in [(24, false), (25, true), (26, true)] {
        assert_eq!(arm_excluded(u1, line), out, "arm at {line}: {u1}");
    }
    // No item of the `unique case` matches K = 3'b110.
    for line in [32, 33, 34] {
        assert!(arm_excluded(u1, line) && stmt_excluded(u1, line), "{u1}");
    }
    // Each generate-loop iteration takes one arm: both statements count.
    let tb = scope_entry(&json, "tb");
    assert!(
        tb.contains("\"line\": 47, \"kind\": \"if\", \"excluded\": \"constant\""),
        "{tb}"
    );
    assert!(!stmt_excluded(tb, 48) && !stmt_excluded(tb, 50), "{tb}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A constant condition is decided the way the run evaluates it, so no
/// excluded statement or arm ever runs: operands sized by their context
/// (§11.6.1: `4'hf + 4'h1 == 8'h10` adds in 8 bits and is true, `2'b11 +
/// 2'b01 > 2'b11` in 2 bits and is false), signed arithmetic (§11.8.1) and
/// string literals. A constant operand of `&` takes the other operand's
/// width, so `{a, a} & (4'hf + 4'h1)` is not decided by `4'hf + 4'h1`
/// alone. The right operand of an `&&` whose left operand is constantly
/// false never runs (§11.4.7), so a `?:` in it is excluded.
#[test]
fn constant_decisions_follow_the_run() {
    let d = scratch("sizing");
    let src = "\
module leaf #(parameter [1:0] P = 2'b11, parameter int S = 0);
  logic [3:0] a = 0;
  logic [6:0] r;
  always @(*) begin
    if ((4'hf + 4'h1) == 8'h10) r[0] = a[0]; else r[0] = a[1];
    if ((4'hf + 4'h1) == 4'h0) r[1] = a[0]; else r[1] = a[1];
    if (2'b11 + 2'b01 > 2'b11) r[2] = a[0]; else r[2] = a[1];
    if (~P == 0) r[3] = a[0]; else r[3] = a[1];
    if (S - 1 < 0) r[4] = a[0]; else r[4] = a[1];
    if (\"z\" < \"aa\") r[5] = a[0]; else r[5] = a[1];
    if ({a, a} & (4'hf + 4'h1)) r[6] = a[0]; else r[6] = a[1];
  end
  wire y = (1'b1 ? 1'b0 : 1'b1) && (a[0] ? a[1] : a[2]);
  initial begin
    #1 a = 4'h3;
    #1 a = 4'hc;
  end
endmodule
module tb;
  leaf #(.P(2'b11), .S(0)) u0 ();
  leaf #(.P(2'b01), .S(5)) u1 ();
  initial #5 $finish;
endmodule
";
    let out = run(&d, src, &["--code-coverage=stmt,branch"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for scope in ["tb.u0", "tb.u1"] {
        let e = scope_entry(&json, scope);
        assert_eq!(
            e.matches("\"kind\": \"if\", \"excluded\": \"constant\"")
                .count(),
            6,
            "{e}"
        );
        assert_eq!(
            e.matches("\"kind\": \"ternary\", \"excluded\": \"constant\"")
                .count(),
            2,
            "{e}"
        );
        assert!(
            e.contains("\"line\": 11, \"kind\": \"if\", \"arms\""),
            "{e}"
        );
        let arms = e.find("\"branches\"").unwrap();
        for (line, taken_if) in [(5, true), (6, true), (7, false)] {
            let key = format!("\"line\": {line}, \"arm\": \"if\", \"count\": ");
            assert_eq!(
                row_excluded(&e[arms..], &key),
                !taken_if,
                "line {line}: {e}"
            );
        }
    }
    // Whatever the run does, an excluded row never counts.
    let mut rest = json.as_str();
    let mut rows = 0;
    while let Some(i) = rest.find(", \"excluded\": \"constant\"}") {
        let row = &rest[rest[..i].rfind('{').unwrap()..i];
        if let Some(c) = row.find("\"count\": ") {
            assert_eq!(&row[c + "\"count\": ".len()..], "0", "{row}");
            rows += 1;
        }
        rest = &rest[i + 1..];
    }
    assert!(rows > 0, "{json}");
    let _ = std::fs::remove_dir_all(&d);
}
