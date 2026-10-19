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
        .env_remove("XEZIM_TOGGLE_INTEGERS")
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
        "{\"signal\": \"x\", \"width\": 1, \"type\": \"logic\", \"rise\": [1], \"fall\": [1]}",
        "{\"signal\": \"g\", \"width\": 1, \"type\": \"logic\", \"rise\": [0], \"fall\": [0]}",
        "{\"signal\": \"v\", \"width\": 2, \"type\": \"logic\", \"rise\": [1, 0], \"fall\": [0, 0]}",
        "{\"signal\": \"o\", \"width\": 1, \"type\": \"logic\", \"rise\": [1], \"fall\": [1]}",
    ] {
        assert!(json.contains(row), "{row}: {json}");
    }
    assert!(
        json.contains("{\"signal\": \"n\", \"width\": 32, \"type\": \"int\", \"rise\": [1, 1, 0,"),
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
        "{{\"signal\": \"{}\", \"width\": {}, \"type\": \"logic\", \"rise\": {:?}, \"fall\": {:?}}}",
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
                // A packed struct port: its entry names the struct type.
                toggle("s", &[1, 0, 1], &[1, 0, 1]).replace("\"logic\"", "\"struct\""),
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
            .env_remove("XEZIM_TOGGLE_INPUT_PORTS")
            .env_remove("XEZIM_TOGGLE_INTEGERS");
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

const INTEGERS: &str = "\
typedef int count_t;
module leaf(output int n, m, output logic [1:0] q);
  integer i;
  byte b;
  if (1) begin : blk
    shortint s;
    initial begin s = 0; #1 s = 3; end
  end
  initial begin
    n = 0; m = 0; q = 0; b = 0;
    #1 for (i = 0; i < 2; i = i + 1) q = q + 1;
    n = 3; m = 3; b = 3;
  end
endmodule
module holder;
  struct { int m; logic [1:0] k; } us;
  initial begin us.m = 0; us.k = 0; #1 us.m = 3; us.k = 3; end
endmodule
module tb;
  int a;
  integer b;
  longint c;
  int unsigned d;
  count_t e;
  logic [31:0] f;
  bit [7:0] g;
  enum logic [1:0] {S0, S1, S2} st;
  struct packed { logic [3:0] x; byte y; } ps;
  int n;
  logic [1:0] q;
  leaf u(.n, .m(), .q);
  holder v();
  initial begin
    a = 0; b = 0; c = 0; d = 0; e = 0; f = 0; g = 0; st = S0; ps = 0;
    #1 a = 3; b = 3; c = 3; d = 3; e = 3; f = 3; g = 3; st = S2; ps = 12'h303;
    #1 $finish;
  end
endmodule
";

/// The toggle totals of `scope` in a results file.
fn scope_toggle(json: &str, scope: &str) -> String {
    let at = json
        .find(&format!("\"scope\": \"{scope}\","))
        .unwrap_or_else(|| panic!("no scope {scope}: {json}"));
    let rest = &json[at..];
    let t = rest.find("\"toggle\": ").unwrap() + "\"toggle\": ".len();
    rest[t..t + rest[t..].find('}').unwrap() + 1].to_string()
}

/// §6.11: the toggle entry of each signal names its declared data type,
/// and `--no-toggle-integers` (or XEZIM_TOGGLE_INTEGERS=0) leaves out the
/// variables of an integer atom type: `int`, `integer`, `shortint`,
/// `longint` and `byte`, signed or unsigned, through a typedef, in a
/// generate block or as an output port (ANSI ports inheriting the type
/// too). Vectors, enums, packed structs and the members of unpacked
/// structs stay. The reference simulator, with its integer toggles turned
/// off, lists the same signals with the same counts, and the same totals
/// for `tb.u` (4 bins, 1 hit); for `tb` it reports 111 bins and 11 hits,
/// as it counts the enum by value. With them on, it reports 244 bins and 9
/// hits for `tb.u`, as here.
#[test]
fn toggle_integers_left_out() {
    let d = scratch("integers");
    let out = run(&d, INTEGERS, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let all = results(&d);
    let ints = [
        ("a", 32, "int"),
        ("b", 32, "integer"),
        ("c", 64, "longint"),
        ("d", 32, "int"),
        ("e", 32, "int"),
        ("n", 32, "int"),
        ("m", 32, "int"),
        ("i", 32, "integer"),
        ("b", 8, "byte"),
        ("blk.s", 16, "shortint"),
    ];
    let kept = [
        ("f", 32, "logic"),
        ("g", 8, "bit"),
        ("st", 2, "enum"),
        ("ps", 12, "struct"),
        ("q", 2, "logic"),
        ("us.m", 32, "struct"),
        ("us.k", 2, "struct"),
    ];
    let row = |(name, width, ty): &(&str, u32, &str)| {
        format!("{{\"signal\": \"{name}\", \"width\": {width}, \"type\": \"{ty}\", ")
    };
    for r in ints.iter().chain(&kept) {
        assert!(all.contains(&row(r)), "{}: {all}", row(r));
    }
    assert_eq!(
        scope_toggle(&all, "tb.u"),
        "{\"covered\": 9, \"total\": 244, \"percent\": 3.69}"
    );

    let out = run(&d, INTEGERS, &["--code-coverage=toggle", "--no-toggle-integers"]);
    assert!(out.status.success());
    let some = results(&d);
    for r in &ints {
        assert!(!some.contains(&row(r)), "{}: {some}", row(r));
    }
    for r in &kept {
        assert!(some.contains(&row(r)), "{}: {some}", row(r));
    }
    assert_eq!(
        scope_toggle(&some, "tb"),
        "{\"covered\": 10, \"total\": 112, \"percent\": 8.93}"
    );
    assert_eq!(
        scope_toggle(&some, "tb.u"),
        "{\"covered\": 1, \"total\": 4, \"percent\": 25.00}"
    );
    let code = |json: &str| json[json.find("\"code_coverage\"").unwrap()..].to_string();

    // The variable does what the flag does; the flag wins.
    let env = |val: &str, args: &[&str]| {
        std::fs::write(d.join("t.sv"), INTEGERS).unwrap();
        Command::new(xezim())
            .current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .env("XEZIM_TOGGLE_INTEGERS", val)
            .args(args)
            .arg("t.sv")
            .output()
            .expect("run xezim")
    };
    assert!(env("0", &["--code-coverage=toggle"]).status.success());
    assert_eq!(code(&results(&d)), code(&some));
    let on = &["--code-coverage=toggle", "--toggle-integers"];
    assert!(env("0", on).status.success());
    assert_eq!(code(&results(&d)), code(&all));
    let bad = env("no", &["--code-coverage=toggle"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("XEZIM_TOGGLE_INTEGERS=no: want 0 or 1")
    );
    let stmt = run(&d, INTEGERS, &["--code-coverage=stmt", "--no-toggle-integers"]);
    assert!(
        String::from_utf8_lossy(&stmt.stderr)
            .contains("Warning: --no-toggle-integers has no effect without toggle coverage")
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An input port of an integer type is typed by its declaration like any
/// other signal, so `--no-toggle-integers` leaves it out of its instance
/// too, while a vector input stays (§6.11, §23.2.2.3).
#[test]
fn toggle_integers_left_out_of_input_ports() {
    let d = scratch("integer-inputs");
    let src = "\
module child(input int k, input logic [1:0] l);
endmodule
module tb;
  int a = 0;
  logic [1:0] b = 0;
  child c(.k(a), .l(b));
  initial begin #1 a = 3; b = 3; #1 $finish; end
endmodule
";
    let k = "{\"signal\": \"k\", \"width\": 32, \"type\": \"int\", ";
    let l = "{\"signal\": \"l\", \"width\": 2, \"type\": \"logic\", ";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let all = results(&d);
    let c = scope(&all, "tb.c");
    assert!(c.contains(k) && c.contains(l), "{c}");
    assert!(c.contains(&toggle_total(4, 68, "5.88")), "{c}");

    let out = run(&d, src, &["--code-coverage=toggle", "--no-toggle-integers"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let some = results(&d);
    let c = scope(&some, "tb.c");
    assert!(!c.contains(k) && c.contains(l), "{c}");
    assert!(c.contains(&toggle_total(2, 4, "50.00")), "{c}");
    let _ = std::fs::remove_dir_all(&d);
}

/// An instance that a path-specific `bind` targets runs a specialized copy of
/// its module; its ports are still typed by the source declaration, so
/// `--no-toggle-integers` leaves its `int` port out too (§23.11, §6.11).
#[test]
fn toggle_integers_left_out_of_bound_instance_ports() {
    let d = scratch("integer-bound");
    let src = "\
module target(input int k, input logic [1:0] l);
endmodule
module mon(input int m);
endmodule
module tb;
  int a = 0;
  logic [1:0] b = 0;
  target u(.k(a), .l(b));
  bind target: tb.u mon m0(.m(k));
  initial begin #1 a = 3; b = 3; #1 $finish; end
endmodule
";
    let k = "{\"signal\": \"k\", \"width\": 32, \"type\": \"int\", ";
    let l = "{\"signal\": \"l\", \"width\": 2, \"type\": \"logic\", ";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let all = results(&d);
    let u = scope(&all, "tb.u");
    assert!(u.contains(k) && u.contains(l), "{u}");

    let out = run(&d, src, &["--code-coverage=toggle", "--no-toggle-integers"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let some = results(&d);
    let u = scope(&some, "tb.u");
    assert!(!u.contains(k) && u.contains(l), "{u}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The VPI client of `toggle_baseline_from_vpi_at_time_0`. With
/// `T0_CASE=start`, `a` is written from cbStartOfSimulation (where cocotb
/// writes `Immediate` values and starts its clocks); with `T0_CASE=rw`, `b`
/// and `v` from cbReadWriteSynch at time 0 (where cocotb applies its other
/// writes). Either way all three are set at 10 and cleared at 20, and the
/// time-0 cbReadOnlySynch prints `na`, `nb` and `v`.
const TIME0_VPI: &str = r#"
#include <stdlib.h>
#include <string.h>
#include "vpi_user.h"

static void put(const char *name, int val, int flags) {
    s_vpi_value v;
    s_vpi_time t = {vpiSimTime, 0, 0, 0.0};
    v.format = vpiIntVal;
    v.value.integer = val;
    vpi_put_value(vpi_handle_by_name((PLI_BYTE8 *)name, NULL), &v, &t, flags);
}

static const char *bin(const char *name, char *buf) {
    s_vpi_value v;
    v.format = vpiBinStrVal;
    vpi_get_value(vpi_handle_by_name((PLI_BYTE8 *)name, NULL), &v);
    strncpy(buf, v.value.str, 7);
    buf[7] = 0;
    return buf;
}

static PLI_INT32 cb(p_cb_data d);

static void at(int reason, unsigned t, long tag) {
    s_cb_data c;
    s_vpi_time tm = {vpiSimTime, 0, t, 0.0};
    memset(&c, 0, sizeof c);
    c.reason = reason;
    c.cb_rtn = cb;
    c.time = &tm;
    c.user_data = (PLI_BYTE8 *)tag;
    vpi_register_cb(&c);
}

static PLI_INT32 cb(p_cb_data d) {
    long tag = (long)d->user_data;
    const char *mode = getenv("T0_CASE");
    char x[8], y[8], z[8];
    switch (tag) {
    case 0:
        if (mode && !strcmp(mode, "start"))
            put("top.a", 0, vpiNoDelay);
        else
            at(cbReadWriteSynch, 0, 1);
        at(cbReadOnlySynch, 0, 9);
        at(cbAfterDelay, 10, 2);
        at(cbAfterDelay, 20, 3);
        at(cbAfterDelay, 30, 4);
        break;
    case 1:
        put("top.b", 0, vpiInertialDelay);
        put("top.v", 0, vpiNoDelay);
        break;
    case 2:
    case 3:
        put("top.a", tag == 2, vpiNoDelay);
        put("top.b", tag == 2, vpiNoDelay);
        put("top.v", tag == 2, vpiNoDelay);
        break;
    case 4:
        vpi_control(vpiFinish, 0);
        break;
    case 9:
        vpi_printf("RO na=%s nb=%s v=%s\n", bin("top.na", x), bin("top.nb", y), bin("top.v", z));
        break;
    }
    return 0;
}

static void start(void) {
    s_cb_data c;
    memset(&c, 0, sizeof c);
    c.reason = cbStartOfSimulation;
    c.cb_rtn = cb;
    vpi_register_cb(&c);
}

void (*vlog_startup_routines[])(void) = {start, 0};
"#;

/// Build the VPI client `c` in `d` and run `src` (top `top`) with it and
/// toggle coverage, `T0_CASE` set to `case`; returns the output and the
/// results file.
fn vpi_toggles(d: &Path, c: &str, src: &str, case: &str) -> (String, String) {
    let so = d.join("t0.so");
    if !so.exists() {
        std::fs::write(d.join("t0.c"), c).unwrap();
        let include = Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
        let ok = Command::new("cc")
            .args(["-shared", "-fPIC", "-I"])
            .arg(&include)
            .arg(d.join("t0.c"))
            .arg("-o")
            .arg(&so)
            .status()
            .expect("launch cc")
            .success();
        assert!(ok, "cc failed");
    }
    std::fs::write(d.join("t.sv"), src).unwrap();
    let _ = std::fs::remove_file(d.join("cov.json"));
    let out = Command::new(xezim())
        .current_dir(d)
        .env("XEZIM_COV_DB", d.join("cov.json"))
        .env("T0_CASE", case)
        .env_remove("XEZIM_CODE_COVERAGE")
        .arg("--vpi-lib")
        .arg(&so)
        .args(["--code-coverage=toggle", "-s", "top", "t.sv"])
        .output()
        .expect("run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{case}: {text}");
    (text, results(d))
}

/// Assert the (signal, rise, fall) counts of one-bit signals, whatever other
/// members their entries carry.
fn toggle_rows(json: &str, rows: &[(&str, u32, u32)], what: &str) {
    for (name, rise, fall) in rows {
        let key = format!("{{\"signal\": \"{name}\", \"width\": 1,");
        let start = json
            .find(&key)
            .unwrap_or_else(|| panic!("{what}: no {name}: {json}"));
        let row = &json[start..start + json[start..].find('}').unwrap()];
        let counts = format!("\"rise\": [{rise}], \"fall\": [{fall}]");
        assert!(row.contains(&counts), "{what}: {name} {counts}: {row}");
    }
}

/// IEEE 1800-2017 §38.36.2, §38.36.3: a value a VPI client writes at time 0,
/// from cbStartOfSimulation or from that slot's cbReadWriteSynch, is the
/// toggle baseline, as one the testbench wrote at time 0 would be. Nothing
/// in the design runs at time 0, so only the VPI writes make that slot. In
/// both cases the reference simulator has settled `~a` or `~b` by the time-0
/// cbReadOnlySynch and counts one rise and one fall on each signal written
/// at time 0, the counts below. xezim used to skip the time-0 slot: the
/// cbStartOfSimulation write did not reach `na` until 10, the
/// cbReadWriteSynch writes were first sampled at 10, and each of those
/// signals lost its first toggle.
#[test]
fn toggle_baseline_from_vpi_at_time_0() {
    let d = scratch("vpi_time0");
    let src = "\
module top(input logic a, input logic b, output logic na, output logic nb);
  logic v;
  assign na = ~a;
  assign nb = ~b;
endmodule
";
    // (signal, rise, fall): the reference simulator's counts. A signal not
    // written at time 0 starts at x and loses its first change.
    for (case, ro, rows) in [
        (
            "start",
            "RO na=1 nb=x v=x",
            [("na", 1, 1), ("nb", 1, 0), ("v", 0, 1)],
        ),
        (
            "rw",
            "RO na=x nb=1 v=0",
            [("na", 1, 0), ("nb", 1, 1), ("v", 1, 1)],
        ),
    ] {
        let (text, json) = vpi_toggles(&d, TIME0_VPI, src, case);
        assert!(text.contains(ro), "{case}: {text}");
        toggle_rows(&json, &rows, case);
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A client whose only routine is cbStartOfSimulation (§38.36.3), in a
/// design whose first event is a clock edge at 5: `w` written there is the
/// baseline of `w` and of `nw = ~w`, so `nw` falls at 5 and 25 and rises at
/// 15, as in the reference simulator. The routine is gone once it has run,
/// so nothing VPI holds the scheduler at time 0.
#[test]
fn toggle_baseline_from_lone_start_routine() {
    let d = scratch("vpi_start_only");
    let c = r#"
#include <string.h>
#include "vpi_user.h"
static PLI_INT32 cb(p_cb_data d) {
    s_vpi_value v;
    s_vpi_time t = {vpiSimTime, 0, 0, 0.0};
    v.format = vpiIntVal;
    v.value.integer = 0;
    vpi_put_value(vpi_handle_by_name("top.w", NULL), &v, &t, vpiNoDelay);
    return 0;
}
static void start(void) {
    s_cb_data c;
    memset(&c, 0, sizeof c);
    c.reason = cbStartOfSimulation;
    c.cb_rtn = cb;
    vpi_register_cb(&c);
}
void (*vlog_startup_routines[])(void) = {start, 0};
"#;
    let src = "\
module top;
  logic clk, w, nw;
  assign nw = ~w;
  initial begin clk = 0; forever #5 clk = ~clk; end
  always @(posedge clk) w <= ~w;
  always @(negedge clk) if ($time > 25) $finish;
endmodule
";
    let (_, json) = vpi_toggles(&d, c, src, "");
    toggle_rows(&json, &[("w", 2, 1), ("nw", 1, 2)], "start only");
    let _ = std::fs::remove_dir_all(&d);
}

/// A slot can be sampled more than once: at the end of its tick and again
/// at its end, after cbAtEndOfSimTime (§38.36.2). `x` goes to 1 at 10 and a
/// cbAtEndOfSimTime routine puts it back to 0 in the same slot, so 10 is no
/// toggle; the rise at 20 is the only one, as in the reference simulator.
#[test]
fn toggle_change_undone_in_its_slot() {
    let d = scratch("vpi_undo");
    let c = r#"
#include <string.h>
#include "vpi_user.h"
static PLI_INT32 cb(p_cb_data d) {
    s_cb_data c;
    s_vpi_time t = {vpiSimTime, 0, 10, 0.0};
    if (d->reason == cbAtEndOfSimTime) {
        s_vpi_value v;
        v.format = vpiIntVal;
        v.value.integer = 0;
        vpi_put_value(vpi_handle_by_name("top.x", NULL), &v, &t, vpiNoDelay);
        return 0;
    }
    memset(&c, 0, sizeof c);
    c.reason = cbAtEndOfSimTime;
    c.cb_rtn = cb;
    c.time = &t;
    vpi_register_cb(&c);
    return 0;
}
static void start(void) {
    s_cb_data c;
    memset(&c, 0, sizeof c);
    c.reason = cbStartOfSimulation;
    c.cb_rtn = cb;
    vpi_register_cb(&c);
}
void (*vlog_startup_routines[])(void) = {start, 0};
"#;
    let src = "\
module top;
  logic x;
  initial begin x = 0; #10 x = 1; #10 x = 1; #10 $finish; end
endmodule
";
    let (_, json) = vpi_toggles(&d, c, src, "");
    toggle_rows(&json, &[("x", 1, 0)], "undone");
    let _ = std::fs::remove_dir_all(&d);
}

/// §20.2: `$finish` ends the run inside its time slot, so the flush at the
/// end of the last slot is skipped when `$finish` ended it: `d` rises and
/// `clk` rises a second time only in the finishing slot at 15, and neither
/// counts. The reference simulator reports `d` untoggled and 4 of 6 bins.
#[test]
fn no_slot_end_flush_after_finish() {
    let d = scratch("finish_slot");
    let src = "\
module tb;
  logic clk = 0;
  logic d = 0, e = 0;
  always #5 clk = ~clk;
  initial begin
    #12 e = 1; #1 e = 0;
    #2 d = 1;
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
    toggle_rows(&json, &[("clk", 1, 1), ("d", 0, 0), ("e", 1, 1)], "finish");
    let total = toggle_total(4, 6, "66.67");
    assert!(json.contains(&total), "{total}: {json}");
    let _ = std::fs::remove_dir_all(&d);
}

/// The signals of a scope's toggle entries, in report order.
fn toggled(scope: &str) -> Vec<&str> {
    scope
        .split("\"signal\": \"")
        .skip(1)
        .map(|s| &s[..s.find('"').unwrap()])
        .collect()
}

/// §6.8: a variable declaration's initializer sets the variable before any
/// process starts, so a variable nothing else writes holds that one value
/// for the whole run and has no toggle to count. Toggle coverage leaves it
/// out (`k`), as the reference simulator does with full signal visibility,
/// and leaves out one only a constant assignment at time 0 sets (`w`). A
/// variable nothing writes at all (`n`) stays, as there.
#[test]
fn toggle_leaves_out_variables_fixed_before_the_run() {
    let d = scratch("init_only");
    let src = "\
module dut(input logic [3:0] a, output logic [3:0] y, output logic [3:0] z);
  logic [3:0] k = 4'b1010;
  logic [3:0] n;
  logic [3:0] w;
  initial w = 4'b0101;
  assign y = a ^ k;
  assign z = n ^ w;
endmodule
module tb;
  logic [3:0] a = 0;
  wire [3:0] y, z;
  dut u(.a, .y, .z);
  initial begin #1 a = 4'hf; #1 a = 0; #1 $finish; end
endmodule
";
    let out = run(&d, src, &["--code-coverage=toggle"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let u = scope(&json, "tb.u");
    assert_eq!(toggled(u), ["a", "n", "y", "z"], "{u}");
    assert!(u.contains(&toggle_total(16, 32, "50.00")), "{u}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Which variables count as fixed, against the reference simulator's report
/// on this design with full signal visibility (the same signals and bins in
/// every scope). Left out: an initializer from literals or parameters, 4- or
/// 2-state, with no other write (`k`, `b`, `p`, `fr`, which a function only
/// reads, and `tb.t`, which only feeds an input port), and a single constant
/// assignment in an `initial` block without timing controls (`w`, `j1`,
/// `j2`, `g.gi`). Kept: a variable nothing writes (`n`, `bn`); two writes
/// (`w2`, `i3`); a write after a delay in the block (`d1`, `d2`); a write
/// that never runs (`ka`); a task's output argument, `$sscanf`, `++` and a
/// clocking block drive (`v5`, `v6`, `v9`, `bi.cd`); a class method's write
/// (`tb.cv`); an initializer that calls a function or reads a port (`fc`,
/// `wp`); a struct (`sp`); and an initialized variable in a generate block
/// (`g.gv`).
#[test]
fn toggle_keeps_variables_other_code_writes() {
    let d = scratch("init_kept");
    let src = "\
interface bus_if(input logic clk);
  logic [1:0] cd = 2'b01;
  logic [1:0] idle = 2'b11;
  clocking cb @(posedge clk); output cd; endclocking
endinterface
module dut(input logic clk, input logic [3:0] a, input logic [3:0] tied, output logic [3:0] y);
  localparam logic [3:0] P = 4'd3;
  function automatic logic [3:0] inc(input logic [3:0] x); return x + 1; endfunction
  task automatic get(output logic [3:0] o); o = 4'b0000; endtask
  logic [3:0] k = 4'b1010;
  bit   [3:0] b = 4'b0110;
  logic [3:0] p = P + 1;
  logic [3:0] n;
  bit   [3:0] bn;
  logic [3:0] w; initial w = 4'b0101;
  logic [3:0] j1, j2; initial begin j1 = P; $display(\"j\"); j2 = '1; end
  logic [3:0] w2; initial begin w2 = 4'b0001; w2 = 4'b0010; end
  logic [3:0] d1, d2; initial begin d1 = 4'b0001; #1 d2 = 4'b0010; end
  logic [3:0] i3 = 4'b0001; initial i3 = 4'b0101;
  logic [3:0] ka = 4'b1010; always @(posedge clk) if (a == 4'h7) ka <= 4'b0000;
  logic [3:0] v5 = 4'b0001; initial get(v5);
  logic [3:0] v6 = 4'b0001; initial $sscanf(\"5\", \"%d\", v6);
  logic [3:0] v9 = 4'b0001; initial v9++;
  logic [3:0] fc = inc(4'd2);
  logic [3:0] fr = 4'b0100; logic [3:0] fo; always_comb fo = inc(fr);
  logic [3:0] wp; initial wp = tied;
  struct packed { logic [1:0] hi, lo; } sp = 4'b1001;
  if (1) begin : g
    logic [3:0] gv = 4'b0001;
    logic [3:0] gi; initial gi = 4'b0001;
  end
  assign y = a ^ k ^ b ^ p ^ n ^ bn ^ w ^ j1 ^ j2 ^ w2 ^ d1 ^ d2 ^ i3 ^ ka ^ v5 ^ v6 ^ v9
           ^ fc ^ fo ^ wp ^ sp ^ g.gv ^ g.gi;
endmodule
module tb;
  logic clk = 0;
  logic [3:0] a = 0;
  logic [3:0] t = 4'b1100;
  logic [3:0] cv = 4'b0001;
  wire  [3:0] y;
  class writer; task go(); tb.cv = 4'b1110; endtask endclass
  bus_if bi(clk);
  dut u(.clk, .a, .tied(t), .y);
  initial begin
    writer wr = new;
    wr.go();
    #1 a = 4'hf; clk = 1; bi.cb.cd <= 2'b10;
    #1 clk = 0; a = 0;
    #1 clk = 1; bi.cb.cd <= 2'b01;
    #1 clk = 0;
    #1 $finish;
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
    let u = scope(&json, "tb.u");
    assert_eq!(
        toggled(u),
        [
            "a", "bn", "clk", "d1", "d2", "fc", "fo", "g.gv", "i3", "ka", "n", "sp", "tied", "v5",
            "v6", "v9", "w2", "wp", "y"
        ],
        "{u}"
    );
    assert!(u.contains(&toggle_total(15, 146, "10.27")), "{u}");
    let tb = scope(&json, "tb");
    assert_eq!(toggled(tb), ["a", "clk", "cv", "y"], "{tb}");
    assert!(tb.contains(&toggle_total(14, 26, "53.85")), "{tb}");
    let bi = scope(&json, "tb.bi");
    assert_eq!(toggled(bi), ["cd", "clk"], "{bi}");
    assert!(bi.contains(&toggle_total(6, 6, "100.00")), "{bi}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Writes that are easy to miss keep a variable, as on the reference
/// simulator: `i` changes only through `x[i++]`, `drv` only through the
/// renamed clocking output `oalias` (§14.3), and `b2` is assigned after the
/// intra-assignment delay of `a2 = #2 ...` (§9.4.5), not at time 0. A write
/// the census can't place keeps every variable of that name: `bi.data`,
/// which a class drives through a virtual interface, stays, where the
/// reference simulator leaves it out; `bi.idle`, which nothing writes after
/// its initializer, does not.
#[test]
fn toggle_keeps_variables_written_through_selects_aliases_and_delays() {
    let d = scratch("init_hidden");
    let src = "\
interface bus_if(input logic clk);
  logic [1:0] data = 2'b01;
  logic [1:0] idle = 2'b11;
endinterface
class drv_c;
  virtual bus_if vif;
  task run(); vif.data <= 2'b10; @(posedge vif.clk); vif.data <= 2'b01; endtask
endclass
module dut(input logic clk, output logic [3:0] y);
  logic [1:0] i = 2'd1;
  logic [3:0] x = 4'd0;
  always @(posedge clk) x[i++] = 1'b1;
  logic [7:0] drv = 8'd0;
  clocking cb @(posedge clk); output oalias = drv; endclocking
  logic [3:0] a2, b2;
  initial begin a2 = #2 4'b0101; b2 = 4'b0011; end
  initial begin #1 cb.oalias <= 8'hff; #2 cb.oalias <= 8'h00; end
  assign y = x ^ {2'b0, i} ^ drv[3:0] ^ a2 ^ b2;
endmodule
module tb;
  logic clk = 0;
  wire [3:0] y;
  bus_if bi(clk);
  dut u(.clk, .y);
  initial begin
    drv_c d = new;
    d.vif = bi;
    fork d.run(); join_none
    repeat (8) #1 clk = ~clk;
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
    let u = scope(&json, "tb.u");
    assert_eq!(toggled(u), ["a2", "b2", "clk", "drv", "i", "x", "y"], "{u}");
    assert!(u.contains("\"total\": 54,"), "{u}");
    let bi = scope(&json, "tb.bi");
    assert_eq!(toggled(bi), ["clk", "data"], "{bi}");
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
/// four, and `-coverage <types>` the types listed (its value used to be read
/// as a top module name; `F` is FSM coverage); XEZIM_CODE_COVERAGE does what
/// the flag does.
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
    let all = "\"statement\", \"branch\", \"toggle\", \"fsm\"";
    let (k, err) = kinds(&["+cover=sbcf"]);
    assert_eq!(k, "\"statement\", \"branch\", \"fsm\"");
    assert_eq!(
        err.matches(
            "Warning: +cover=sbcf: xezim has no condition (c) coverage; \
                     collecting statement, branch, FSM coverage"
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
    assert_eq!(
        kinds(&["-coverage", "b:t"]).0,
        "\"statement\", \"branch\", \"toggle\""
    );
    let (k, err) = kinds(&["-coverage", "B:E:F:U"]);
    assert_eq!(k, "\"statement\", \"branch\", \"fsm\"");
    assert_eq!(
        err.matches(
            "Warning: -coverage B:E:F:U: xezim has no expression (e) coverage; \
                     collecting statement, branch, FSM coverage"
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
