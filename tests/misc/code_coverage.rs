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

/// Other simulators' spellings: `+cover=<letters>` picks the kinds (`c`
/// condition and `e` expression included) and warns once about the ones
/// xezim lacks; bare `+cover` and `-coverage` collect all five, and
/// `-coverage <types>` the types listed (its value used to be read as a top
/// module name; `E` is condition and expression coverage);
/// XEZIM_CODE_COVERAGE does what the flag does.
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
    let all = "\"statement\", \"branch\", \"toggle\", \"condition\", \"expression\"";
    let (k, err) = kinds(&["+cover=sbcfx"]);
    assert_eq!(k, "\"statement\", \"branch\", \"condition\"");
    assert_eq!(
        err.matches(
            "Warning: +cover=sbcfx: xezim has no FSM (f), extended toggle (x) coverage; \
                     collecting statement, branch, condition coverage"
        )
        .count(),
        1,
        "{err}"
    );
    assert_eq!(kinds(&["+cover=t"]).0, "\"toggle\"");
    assert_eq!(kinds(&["+cover=ec"]).0, "\"condition\", \"expression\"");
    assert_eq!(kinds(&["+cover"]).0, all);
    assert_eq!(kinds(&["-coverage"]).0, all);
    assert_eq!(kinds(&["--code-coverage"]).0, all);
    assert_eq!(kinds(&["--code-coverage=all"]).0, all);
    assert_eq!(kinds(&["-coverage", "+cover=s"]).0, "\"statement\"");
    assert_eq!(kinds(&["-coverage", "all"]).0, all);
    assert_eq!(kinds(&["-coverage", "T"]).0, "\"toggle\"");
    assert_eq!(
        kinds(&["-coverage", "b:t"]).0,
        "\"statement\", \"branch\", \"toggle\""
    );
    let (k, err) = kinds(&["-coverage", "B:E:F:U"]);
    assert_eq!(
        k,
        "\"statement\", \"branch\", \"condition\", \"expression\""
    );
    assert_eq!(
        err.matches(
            "Warning: -coverage B:E:F:U: xezim has no FSM (f) coverage; \
                     collecting statement, branch, condition, expression coverage"
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
    assert_eq!(
        kinds(&["--code-coverage=expr,cond"]).0,
        "\"condition\", \"expression\""
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

/// The input terms of the condition or expression item of kind `kind` at
/// `line`, with the counts of their two rows (the term at 0, and at 1,
/// deciding the result).
fn terms(json: &str, line: u32, kind: &str) -> Vec<(String, [u64; 2])> {
    // Statements and branches have a line and kind too; items go on with
    // their text (or generate block).
    let key = format!("\"line\": {}, \"kind\": \"{}\", ", line, kind);
    let start = json
        .match_indices(&key)
        .map(|(i, _)| i)
        .find(|&i| {
            let after = &json[i + key.len()..];
            after.starts_with("\"text\"") || after.starts_with("\"block\"")
        })
        .unwrap_or_else(|| panic!("no {kind} item at {line}: {json}"));
    let rest = &json[start..];
    let rest = &rest[rest.find("\"terms\": [").unwrap() + "\"terms\": [".len()..];
    let list = &rest[..rest.find("}]}").unwrap() + 1];
    list.split("{\"term\": \"")
        .skip(1)
        .map(|t| {
            let name = t[..t.find('"').unwrap()].to_string();
            let h = &t[t.find("\"hits\": [").unwrap() + "\"hits\": [".len()..];
            let h: Vec<u64> = h[..h.find(']').unwrap()]
                .split(", ")
                .map(|n| n.parse().unwrap())
                .collect();
            (name, [h[0], h[1]])
        })
        .collect()
}

fn has_item(json: &str, line: u32) -> bool {
    json.contains(&format!("\"line\": {}, \"kind\": \"if\", \"text\"", line))
        || ["while", "ternary", "statement", "assign", "port"]
            .iter()
            .any(|k| {
                json.contains(&format!(
                    "\"line\": {}, \"kind\": \"{}\", \"text\"",
                    line, k
                ))
            })
}

/// Condition coverage is focused expression coverage, short-circuit aware
/// (IEEE 1800-2023 §11.3.5: `&&` skips its right operand after a 0, `||`
/// after a 1): a term's row counts when the term alone decides the result.
/// The left of `&&` decides at 0 whatever the right would be; at 1 only
/// when the right is 1. Rows and terms covered are the reference
/// simulator's on this design (4 of 6 terms; `half` never sees x or y at 1
/// deciding).
#[test]
fn condition_rows_short_circuit() {
    let d = scratch("cond");
    let src = "\
module tb;
  int n;
  task automatic check(input logic x, input logic y);
    if (x && y) n++;
    if (x || y) n++;
  endtask
  task automatic half(input logic x, input logic y);
    if (x && y) n++;
  endtask
  initial begin
    n = 0;
    check(0, 0); check(1, 0); check(1, 1); check(0, 1);
    half(0, 0); half(1, 0); half(0, 1);
    $display(\"n=%0d\", n);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=cond"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l == "n=4")
    );
    let json = results(&d);
    assert!(json.contains("\"kinds\": [\"condition\"]"), "{json}");
    let t = |x: [u64; 2], y: [u64; 2]| vec![("x".to_string(), x), ("y".to_string(), y)];
    assert_eq!(terms(&json, 4, "if"), t([2, 1], [1, 1]));
    assert_eq!(terms(&json, 5, "if"), t([1, 2], [1, 1]));
    assert_eq!(terms(&json, 8, "if"), t([2, 0], [1, 0]));
    assert!(
        json.contains("\"condition\": {\"covered\": 4, \"total\": 6, \"percent\": 66.67}"),
        "{json}"
    );
    assert!(
        json.contains(
            "{\"file\": \"t.sv\", \"line\": 4, \"kind\": \"if\", \"text\": \"x && y\", \"terms\": \
             [{\"term\": \"x\", \"hits\": [2, 1]}, {\"term\": \"y\", \"hits\": [1, 1]}]}"
        ),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Expression coverage covers the 1-bit logic values assigned, continuous
/// and procedural: `&`, `|`, `^`, `~^` on 1-bit operands decide only when
/// the other operand does not (bitwise operators evaluate both, §11.4.8),
/// a `?:` on 1-bit arms is a multiplexer whose condition always decides
/// when the other arm was not evaluated, and an item whose code never ran
/// is listed with no hits. Vector `&` and a lone reduction are not
/// expressions. The items, terms and covered terms (8 of 10) are the
/// reference simulator's; continuous evaluations are not counted the same,
/// so only rows hit are compared there.
#[test]
fn expression_items() {
    let d = scratch("expr");
    let src = "\
module tb;
  logic a, b, c, s, x, m, n, p, q;
  logic [3:0] v, w, z;
  assign x = a & b | c;
  assign m = s ? a : b;
  assign z = v & w;
  assign p = ~^v;
  always_comb n = !(a ^ b);
  task never();
    q = a && b;
  endtask
  initial begin
    v = 4'h5; w = 4'h3;
    for (int i = 0; i < 8; i++) begin
      {a, b, c} = i[2:0];
      s = i[0];
      #1;
    end
    $display(\"x=%b m=%b n=%b z=%h p=%b\", x, m, n, z, p);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|l| l == "x=1 m=1 n=1 z=1 p=1")
    );
    let json = results(&d);
    assert!(
        json.contains("\"expression\": {\"covered\": 8, \"total\": 10, \"percent\": 80.00}"),
        "{json}"
    );
    let hit = |t: &[(String, [u64; 2])]| t.iter().all(|(_, h)| h[0] > 0 && h[1] > 0);
    let x = terms(&json, 4, "assign");
    assert_eq!(
        x.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );
    assert!(hit(&x), "{json}");
    let m = terms(&json, 5, "assign");
    assert_eq!(
        m.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["s", "a", "b"]
    );
    assert!(hit(&m), "{json}");
    assert!(hit(&terms(&json, 8, "statement")), "{json}");
    assert_eq!(
        terms(&json, 10, "statement"),
        vec![("a".to_string(), [0, 0]), ("b".to_string(), [0, 0])]
    );
    assert!(!has_item(&json, 6) && !has_item(&json, 7), "{json}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Which expressions are items, as in the reference simulator's report on
/// this design: a condition or value with a logic operator on two
/// non-constant operands, or a relation; not a lone (possibly negated)
/// signal, nor a 1-bit signal compared with a constant (it is that signal),
/// nor a `for` loop's condition; a `while` condition, a `?:` condition (in
/// an assignment or a message) and an input port's connection (an
/// expression of the parent) are.
#[test]
fn which_expressions_count() {
    let d = scratch("which");
    let src = "\
module sub(input logic en, output logic q);
  assign q = en;
endmodule
module tb;
  logic a, b, c, y, y2, t;
  logic [3:0] v, w;
  logic [7:0] r;
  int i, k;
  sub u(.en(a & c), .q(t));
  initial begin
    a = 1; b = 0; c = 1; v = 3; w = 4; k = 0;
    if (a) k++;
    if (!a) k++;
    if (a == 1'b0) k++;
    if (v == 4'd3) k++;
    if (v == 3 && a) k++;
    if (!$isunknown(v) && a) k++;
    for (i = 0; i < 2; i++) k++;
    i = 0;
    while (i < 2 && a) i++;
    r = (a && b) ? 8'd1 : 8'd2;
    y = v[0] & w[1];
    y2 = ^v ^ a;
    y = r & v;
    y = (v > 2) | (w < 1);
    $display(\"%s\", (a || b) ? \"yes\" : \"no\");
    #1 $display(\"k=%0d i=%0d r=%0d y=%b y2=%b t=%b\", k, i, r, y, y2, t);
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=cond,expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for line in [15, 16, 20, 21, 26, 9, 22, 23, 25] {
        assert!(has_item(&json, line), "line {line}: {json}");
    }
    for line in [12, 13, 14, 17, 18, 24] {
        assert!(!has_item(&json, line), "line {line}: {json}");
    }
    assert_eq!(
        terms(&json, 15, "if"),
        vec![("v == 4'd3".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 9, "port"),
        vec![("a".to_string(), [0, 1]), ("c".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 23, "statement"),
        vec![("^v".to_string(), [1, 0]), ("a".to_string(), [0, 1])]
    );
    assert_eq!(terms(&json, 26, "ternary").len(), 2);
    assert!(
        json.contains("\"condition\": {\"covered\": 1, \"total\": 9, \"percent\": 11.11}"),
        "{json}"
    );
    assert!(
        json.contains("\"expression\": {\"covered\": 0, \"total\": 8, \"percent\": 0.00}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The interpreter evaluates an item as the design would: the right of
/// `&&` and `||` only when the left does not decide (a function with a side
/// effect runs as often as without coverage, §11.3.5), and an x term gives
/// no row. The output is the same with coverage as without.
#[test]
fn items_keep_results() {
    let d = scratch("same");
    let src = "\
module tb;
  int calls = 0;
  function automatic bit f(input bit v);
    calls++;
    return v;
  endfunction
  logic a, b, y;
  initial begin
    a = 0; b = 1;
    if (a && f(b)) $display(\"taken\");
    y = a || f(b);
    if (f(a) || f(b)) $display(\"or\");
    a = 1'bx;
    if (a && b) $display(\"x\"); else $display(\"not x\");
    $display(\"calls=%0d y=%b\", calls, y);
  end
endmodule
";
    let off = run(&d, src, &[]);
    let on = run(&d, src, &["--code-coverage"]);
    assert!(on.status.success());
    assert_eq!(off.stdout, on.stdout);
    assert!(
        String::from_utf8_lossy(&on.stdout).contains("calls=3 y=1"),
        "{}",
        String::from_utf8_lossy(&on.stdout)
    );
    let json = results(&d);
    assert_eq!(
        terms(&json, 10, "if"),
        vec![("a".to_string(), [1, 0]), ("f(b)".to_string(), [0, 0])]
    );
    assert_eq!(
        terms(&json, 11, "statement"),
        vec![("a".to_string(), [0, 0]), ("f(b)".to_string(), [0, 1])]
    );
    assert_eq!(
        terms(&json, 14, "if"),
        vec![("a".to_string(), [0, 0]), ("b".to_string(), [0, 0])]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An item of a continuous assignment is evaluated by the interpreter in the
/// assignment's instance, and leaves name resolution as it found it: the
/// testbench's `$monitor` of its own `op_select` and `start`, also the names
/// of ports of the two instances, keeps its bindings (once, it printed the
/// second instance's ports instead, `5 op_select=5 start=1`). The items of
/// the two iterations of a generate loop stay apart, as in the reference
/// simulator.
#[test]
fn items_in_instances() {
    let d = scratch("inst");
    let src = "\
module m #(parameter int N = 4) (input logic clk, input logic [2:0] op_select,
                                 input logic start, output logic last);
  logic [3:0] row = 0;
  logic [1:0] l;
  assign last = (int'(row) == N - 1) && start;
  for (genvar i = 0; i < 2; i++) begin : g
    assign l[i] = op_select[i] | ~start;
  end
  always_ff @(posedge clk) if (start) row <= row + 1;
endmodule
module tb;
  logic clk = 0;
  logic [2:0] op_select, op_select_s;
  logic start, start_s;
  logic last, last_s;
  m #(.N(3)) dut(.clk, .op_select, .start, .last);
  m #(.N(2)) dut_small(.clk, .op_select(op_select_s), .start(start_s), .last(last_s));
  always #5 clk = ~clk;
  initial $monitor(\"%0t op_select=%0d start=%b last=%b\", $time, op_select, start, last);
  initial begin
    op_select = 0; start = 0; op_select_s = 0; start_s = 0;
    #2 start_s = 1; op_select_s = 5;
    #10 op_select = 1; start = 1;
    #20 op_select = 2;
    #20 start = 0;
    #10 $finish;
  end
endmodule
";
    let off = run(&d, src, &[]);
    let on = run(&d, src, &["--code-coverage"]);
    assert!(on.status.success());
    let out = String::from_utf8_lossy(&on.stdout);
    assert_eq!(String::from_utf8_lossy(&off.stdout), out);
    assert!(!out.contains("op_select=5"), "{out}");
    let json = results(&d);
    let rest = &json[json.find("\"scope\": \"tb.dut_small\"").unwrap() + 1..];
    let scope = &rest[..rest.find("\"design_units\"").unwrap()];
    let scope = &scope[..scope.find("\"scope\": \"").unwrap_or(scope.len())];
    assert_eq!(
        scope
            .matches("\"line\": 7, \"kind\": \"assign\", \"text\"")
            .count(),
        2,
        "{scope}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Each kind's results are the same whichever other kinds are collected: the
/// `?:` of a 1-bit value is a multiplexer of the expression item (its
/// condition no condition item) even when only conditions are collected,
/// and every `?:` is a branch either way. A `?:` of a wider value has a
/// condition item.
#[test]
fn kinds_are_independent() {
    let d = scratch("kinds");
    let src = "\
module tb;
  logic a, b, c, e, y;
  logic [3:0] w;
  initial begin
    {a, b, c, e} = 4'b1010;
    y = (a && b) ? c : e;
    w = (a || b) ? 4'd1 : 4'd2;
    #1 $display(\"y=%b w=%0d\", y, w);
  end
endmodule
";
    let total = |args: &[&str], kind: &str| {
        let out = run(&d, src, args);
        assert!(out.status.success());
        let json = results(&d);
        let key = format!("\"{}\": {{", kind);
        let at = json.find(&key).unwrap_or_else(|| panic!("{json}")) + key.len();
        json[at..at + json[at..].find('}').unwrap()].to_string()
    };
    // Line 7's `a || b` (a at 1 decides alone; b is not evaluated).
    let cond = total(&["--code-coverage=cond"], "condition");
    assert_eq!(cond, total(&["--code-coverage=cond,expr"], "condition"));
    assert!(cond.starts_with("\"covered\": 0, \"total\": 2"), "{cond}");
    let branch = total(&["--code-coverage=branch"], "branch");
    assert_eq!(
        branch,
        total(&["--code-coverage=branch,cond,expr"], "branch")
    );
    assert!(
        branch.starts_with("\"covered\": 2, \"total\": 4"),
        "{branch}"
    );
    // Line 6: a, b, c and e; b and e decide at 0.
    let expr = total(&["--code-coverage=expr"], "expression");
    assert!(expr.starts_with("\"covered\": 0, \"total\": 4"), "{expr}");
    assert_eq!(
        terms(&results(&d), 6, "statement"),
        vec![
            ("a".to_string(), [0, 0]),
            ("b".to_string(), [1, 0]),
            ("c".to_string(), [0, 0]),
            ("e".to_string(), [1, 0])
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// A part of an item that folds to a constant still has its `?:` branch; a
/// block's local declaration ends with the block (`p` is the task's 1-bit
/// argument again after it, so `p & q` is an item), a `for` loop's variable
/// is its body's (the `int p` there makes `p & q` a vector operation, no
/// item) and a `foreach` index is an `int` (§12.7.3: `k[0] & q` is an item);
/// and the instances of a module whose parameters fold different terms away
/// keep separate items in the design unit.
#[test]
fn folded_parts_locals_and_units() {
    let d = scratch("fold");
    let src = "\
module m #(parameter bit P = 1, parameter bit Q = 0) (input logic a, b, c, d, output logic y);
  assign y = (a & b & P) | (c & d & Q);
endmodule
module tb;
  logic a, b, c, e, y, y1, y2; logic [3:0] arr [3];
  m #(.P(1), .Q(0)) u1(.a, .b, .c, .d(e), .y(y1));
  m #(.P(0), .Q(1)) u2(.a, .b, .c, .d(e), .y(y2));
  task automatic t(input logic p, input logic q);
    begin : inner
      logic [7:0] p;
      p = 8'hff;
    end
    y = p & q;
    for (int p = 0; p < 2; p++) y = p & q;
    foreach (arr[k]) y = k[0] & q;
  endtask
  initial begin
    {a, b, c, e} = 4'b1010;
    y = (a & b) | ((c ? a : e) & 1'b0);
    t(a, b);
    #1 $display(\"y=%b y1=%b y2=%b\", y, y1, y2);
  end
endmodule
";
    let branch = |kinds: &str| {
        assert!(run(&d, src, &[kinds]).status.success());
        let json = results(&d);
        let at = json.find("\"branch\": {").unwrap();
        json[at..at + json[at..].find('}').unwrap()].to_string()
    };
    assert_eq!(
        branch("--code-coverage=branch"),
        branch("--code-coverage=all")
    );
    let json = results(&d);
    assert!(
        json.contains("\"branch\": {\"covered\": 1, \"total\": 2"),
        "{json}"
    );
    let names = |line: u32, kind: &str| {
        terms(&json, line, kind)
            .into_iter()
            .map(|t| t.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(13, "statement"), ["p", "q"]);
    assert!(!has_item(&json, 14), "{json}");
    assert_eq!(names(15, "statement"), ["k[0]", "q"]);
    assert_eq!(names(19, "statement"), ["a", "b"]);
    let unit = &json[json
        .find("\"design_unit\": \"m\",\n        \"instances\": 2")
        .unwrap()..];
    let unit = &unit[..unit.find("\"design_unit\": \"tb\"").unwrap_or(unit.len())];
    assert_eq!(terms(unit, 2, "assign").len(), 2, "{unit}");
    assert_eq!(
        unit.matches("\"line\": 2, \"kind\": \"assign\", \"text\"")
            .count(),
        2,
        "{unit}"
    );
    assert!(unit.contains("{\"term\": \"c\", "), "{unit}");
    let _ = std::fs::remove_dir_all(&d);
}

/// Results that do not depend on the executor: the two-state executors hand
/// a block with an item to the four-state VM, which counts it once.
#[test]
fn item_counts_do_not_depend_on_executor() {
    let d = scratch("itemengine");
    let src = "\
module tb;
  logic clk = 0;
  logic [3:0] q, d;
  logic e;
  always #5 clk = ~clk;
  always_ff @(posedge clk)
    if (d[0] && !d[1]) q <= d;
    else q <= ~d;
  always_ff @(posedge clk) e <= d[2] ^ d[3];
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
            .args(["--code-coverage=cond,expr", "t.sv"])
            .output()
            .expect("run xezim");
        assert!(out.status.success());
        let json = results(&d);
        json[json.find("\"code_coverage\"").unwrap()..].to_string()
    };
    let default = code(&[]);
    let four_state = code(&[("XEZIM_TWO_STATE", "0")]);
    assert_eq!(default, four_state);
    // d is x at the first edge (no row), 5 at two, 2 at the last two.
    assert_eq!(
        terms(&default, 7, "if"),
        vec![("d[0]".to_string(), [2, 2]), ("d[1]".to_string(), [2, 0])]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Issue #75: the reference simulator evaluates a continuous assignment's
/// item at each change of its inputs, so it also sees the delta cycle in
/// which one input has its new value and another not yet: here `v` has
/// changed and `d`, one block behind, not yet. xezim settles in dependency
/// order and replays those states (both terms covered, as there).
#[test]
fn transient_states() {
    let d = scratch("glitch");
    let src = "\
// A continuous assignment's expression item is evaluated whenever an input changes,
// also in a delta cycle where one input has its new value and another not yet.
module lane(input logic valid, output logic ready);
  always_comb ready = valid;          // follows valid one delta later
endmodule
module t;
  logic v = 0;
  logic d;
  lane u(.valid(v), .ready(d));
  wire y = d | ~v;                    // rows d=0 and v=1 only between v's change and d's
  initial begin
    #1 v = 1;
    #1 v = 0;
    #1 v = 1;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let t = scope(&json, "t");
    assert_eq!(
        terms(t, 10, "assign"),
        vec![("d".to_string(), [2, 2]), ("v".to_string(), [2, 2])]
    );
    assert!(
        t.contains("\"expression\": {\"covered\": 2, \"total\": 2, \"percent\": 100.00}"),
        "{t}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Which transient states an item sees, as the reference simulator does on
/// this design (every item's covered terms are its). A continuous
/// assignment sees each delta cycle in which its inputs change: a block
/// output one cycle after the block's inputs, a net driven by continuous
/// assignments half a cycle after the variables it reads but with the
/// other nets, and an inverter of a block's output with the output. A
/// combinational block runs after the continuous assignments of its
/// inputs' cycle, and sees the outputs of the blocks above it in its
/// module that ran in that cycle too. One process writing both inputs
/// changes them together.
#[test]
fn transient_state_rules() {
    let d = scratch("glitchrules");
    let src = "\
module lane(input logic valid, output logic ready);
  always_comb ready = valid;
endmodule
module tb;
  logic v = 0, clk = 0;
  logic d;
  lane u(.valid(v), .ready(d));
  wire y1 = d | ~v;
  logic a, b;
  always_comb a = v;
  always_comb b = v;
  wire y2 = a | ~b;
  wire n1 = v;
  wire n2 = ~n1;
  wire y3 = n2 | ~n1;
  logic f, g;
  always_ff @(posedge clk) begin f <= v; g <= v; end
  wire fl = ~f;
  wire y4 = fl | g;
  logic z1, z2, z3, p, q;
  always_comb z1 = d | ~v;
  always_comb p = v;
  always_comb z2 = p | ~v;
  always_comb z3 = q | ~v;
  always_comb q = v;
  logic s, t;
  wire y5 = s | ~t;
  logic z5;
  always_comb if (d || !v) z5 = 1; else z5 = 0;
  initial begin
    repeat (2) begin
      #1 v = 1; s = 1; t = 1;
      #1 clk = 1;
      #1 clk = 0;
      #1 v = 0; s = 0; t = 0;
      #1 clk = 1;
      #1 clk = 0;
    end
    #1 $finish;
  end
endmodule
";
    let code = |extra: &[(&str, &str)]| {
        std::fs::write(d.join("t.sv"), src).unwrap();
        let out = Command::new(xezim())
            .current_dir(&d)
            .env("XEZIM_COV_DB", d.join("cov.json"))
            .envs(extra.iter().copied())
            .args(["--code-coverage=cond,expr", "t.sv"])
            .output()
            .expect("run xezim");
        assert!(out.status.success());
        let json = results(&d);
        json[json.find("\"code_coverage\"").unwrap()..].to_string()
    };
    let json = code(&[]);
    assert_eq!(json, code(&[("XEZIM_TWO_STATE", "0")]));
    let t = |a: &str, x: [u64; 2], b: &str, y: [u64; 2]| {
        vec![(a.to_string(), x), (b.to_string(), y)]
    };
    // `d` is a block and a port connection behind `v`.
    assert_eq!(terms(&json, 8, "assign"), t("d", [2, 2], "v", [3, 2]));
    // Two block outputs of one cycle, two nets: no state between them.
    assert_eq!(terms(&json, 12, "assign"), t("a", [0, 2], "b", [3, 0]));
    assert_eq!(terms(&json, 15, "assign"), t("n2", [2, 0], "n1", [0, 2]));
    assert_eq!(terms(&json, 19, "assign"), t("fl", [0, 2], "g", [0, 2]));
    // Blocks: `d` is late, `p`'s block is above, `q`'s below.
    assert_eq!(terms(&json, 21, "statement"), t("d", [2, 2], "v", [3, 2]));
    assert_eq!(terms(&json, 23, "statement"), t("p", [0, 2], "v", [3, 0]));
    assert_eq!(terms(&json, 24, "statement"), t("q", [2, 2], "v", [3, 2]));
    assert_eq!(terms(&json, 27, "assign"), t("s", [0, 2], "t", [2, 0]));
    assert_eq!(terms(&json, 29, "if"), t("d", [2, 4], "v", [3, 2]));
    let _ = std::fs::remove_dir_all(&d);
}

/// A transient state is counted only when the order of the inputs'
/// changes is certain whichever signals upstream changed, decided from the
/// inputs that changed. `a` reads a deep input that does not change, so its
/// block can settle late, but when `v` changes, `a` and `b` change
/// together, as the reference simulator shows: no state with `a` at 0 and
/// `b` at 1 (in an assignment or a block). `c3` is two blocks behind `b`
/// whatever changed, so the state between their changes counts. `r` could
/// settle with `p` or with `q`, but it does not change, so the state
/// between `p`'s change and `q`'s counts (`r` at 0 decides only there).
/// Every item's covered terms are the reference simulator's with full
/// signal visibility (it folds `s & enable`, constant here, otherwise).
#[test]
fn transient_state_order() {
    let d = scratch("glitchorder");
    let src = "\
module tb;
  logic v = 0, s = 0;
  logic en0 = 1;
  logic e1, e2, e3, enable;
  always_comb e1 = en0;
  always_comb e2 = e1;
  always_comb e3 = e2;
  always_comb enable = e3;
  // a's block also reads a deep input that does not change
  logic a, b;
  always_comb a = v & enable;
  always_comb b = v;
  wire y1 = a | ~b;
  // a later block of a's, against b: changes after b whichever input changed
  logic c1, c2, c3;
  always_comb c1 = v & enable;
  always_comb c2 = c1;
  always_comb c3 = c2;
  wire y2 = c3 | ~b;
  // a combinational block's item on a and b
  logic z3;
  always_comb z3 = a | ~b;
  // a term that does not change beside two that change in order
  logic p, q, r;
  always_comb p = v;
  always_comb q = p;
  always_comb r = s & enable;
  wire y4 = q | ~p | r;
  initial begin
    repeat (3) begin
      #1 v = 1;
      #1 v = 0;
    end
    s = 0;
    en0 = 1;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let t = |a: &str, x: [u64; 2], b: &str, y: [u64; 2]| {
        vec![(a.to_string(), x), (b.to_string(), y)]
    };
    assert_eq!(terms(&json, 13, "assign"), t("a", [0, 3], "b", [4, 0]));
    assert_eq!(terms(&json, 22, "statement"), t("a", [0, 3], "b", [4, 0]));
    assert_eq!(terms(&json, 19, "assign"), t("c3", [3, 3], "b", [4, 3]));
    assert_eq!(
        terms(&json, 28, "assign"),
        vec![
            ("q".to_string(), [3, 3]),
            ("p".to_string(), [4, 3]),
            ("r".to_string(), [3, 0])
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// An item with `^` or `~^` is bimodal, as in the reference simulator: its
/// rows count by the item's value too (`outputs`: row 0 at 0 and at 1, row
/// 1 at 0 and at 1), and a term is covered when its two rows were hit at
/// different values of the item (`a1` hits both rows, always at 0). A
/// negated `&&` or `||` is the reference simulator's `~&` or `~|`: both
/// operands decide as for `&` and `|` (`p6` at 0 decides only with `q6` at
/// 1). Every item's covered terms are the reference simulator's on this
/// design.
#[test]
fn bimodal_and_negated_items() {
    let d = scratch("bimodal");
    let src = "\
module tb;
  // XOR items, each with its own inputs
  logic a1, b1, a2, b2, a3, b3, c3, a4, b4, a5, b5;
  wire y1 = a1 ^ b1;            // only (0,0), (1,1): every row at output 0
  wire y2 = a2 ^ b2;            // (0,0), (1,0): a2 both outputs, b2 only row 0
  wire y3 = (a3 ^ b3) & c3;     // a3, b3 at (0,0),(1,1) with c3=1; c3 both values with xor 1
  wire y4 = a4 ~^ b4;           // (0,1), (1,0): every row at output 0
  logic k5;
  always_comb if (a5 ^ b5) k5 = 1; else k5 = 0;   // (0,0), (1,1)
  // negated && and ||
  logic p6, q6, p7, q7, p8, q8, p9, q9, p10, q10;
  wire y6 = ~(p6 && q6);        // (0,0), (1,0), (1,1): p6 row 0 only with q6 at 0
  wire y7 = !(p7 && q7);        // same
  wire y8 = ~(p8 || q8);        // (1,1), (0,1), (0,0)
  logic k9, k10;
  always_comb if (!(p9 && q9)) k9 = 1; else k9 = 0;   // (0,0), (1,0), (1,1)
  always_comb if (!(p10 || q10)) k10 = 1; else k10 = 0; // (1,1), (0,1), (0,0)
  initial begin
    {a1, b1} = 2'b00; {a2, b2} = 2'b00; {a3, b3, c3} = 3'b001; {a4, b4} = 2'b01; {a5, b5} = 2'b00;
    {p6, q6} = 2'b00; {p7, q7} = 2'b00; {p8, q8} = 2'b11; {p9, q9} = 2'b00; {p10, q10} = 2'b11;
    #1;
    {a1, b1} = 2'b11; {a2, b2} = 2'b10; {a3, b3, c3} = 3'b111; {a4, b4} = 2'b10; {a5, b5} = 2'b11;
    {p6, q6} = 2'b10; {p7, q7} = 2'b10; {p8, q8} = 2'b01; {p9, q9} = 2'b10; {p10, q10} = 2'b01;
    #1;
    {a3, b3, c3} = 3'b100; {p6, q6} = 2'b11; {p7, q7} = 2'b11; {p8, q8} = 2'b00; {p9, q9} = 2'b11; {p10, q10} = 2'b00;
    #1;
    {a3, b3, c3} = 3'b101;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=cond,expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    // A bimodal item lists each term's rows by the item's value.
    assert!(
        json.contains(
            "{\"term\": \"a2\", \"hits\": [1, 1], \"outputs\": [[1, 0], [0, 1]]}, \
             {\"term\": \"b2\", \"hits\": [2, 0], \"outputs\": [[1, 1], [0, 0]]}"
        ),
        "{json}"
    );
    let t = |a: &str, x: [u64; 2], b: &str, y: [u64; 2]| {
        vec![(a.to_string(), x), (b.to_string(), y)]
    };
    assert_eq!(terms(&json, 12, "assign"), t("p6", [0, 1], "q6", [1, 1]));
    assert_eq!(terms(&json, 13, "assign"), t("p7", [0, 1], "q7", [1, 1]));
    assert_eq!(terms(&json, 14, "assign"), t("p8", [1, 0], "q8", [1, 1]));
    assert_eq!(terms(&json, 16, "if"), t("p9", [0, 1], "q9", [1, 1]));
    assert_eq!(terms(&json, 17, "if"), t("p10", [1, 0], "q10", [1, 1]));
    // Covered: a2, a3, b3, c3 and q6, q7, q8 of 15 expression terms; q9
    // and q10 of 6 condition terms.
    assert!(
        json.contains("\"expression\": {\"covered\": 7, \"total\": 15, \"percent\": 46.67}"),
        "{json}"
    );
    assert!(
        json.contains("\"condition\": {\"covered\": 2, \"total\": 6, \"percent\": 33.33}"),
        "{json}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The reference simulator runs a continuous assignment, and a
/// combinational block, once a process writing its inputs waits, so it
/// never sees the process's writes one at a time (`p = 1; q = 1;`): no row
/// of `p` or `q` at 0 here. xezim runs the blocks after each blocking
/// write, and counts their last run only. Every item's covered terms are
/// the reference simulator's on this design.
#[test]
fn process_writes_count_once() {
    let d = scratch("procwrites");
    let src = "\
module tb;
  logic p = 0, q = 0, w = 0;
  // V1: a variable driven by a continuous assignment, read by one below it
  logic cs1;
  assign cs1 = p & q;
  logic y1;
  assign y1 = cs1 & w;
  // V2: the same with the reader above the driver
  logic y2, cs2;
  assign y2 = cs2 & w;
  assign cs2 = p & q;
  // V3: a net item reading the variable
  wire y3 = cs1 & w;
  // V4: a variable item reading a net
  wire cs4 = p & q;
  logic y4;
  assign y4 = cs4 & w;
  // V5: a variable item reading an always_comb output above it
  logic cs5;
  always_comb cs5 = p & q;
  logic y5;
  assign y5 = cs5 & w;
  initial begin
    repeat (3) begin
      #1 p = 1; q = 1; w = 1;
      #1 p = 0; q = 0; w = 0;
    end
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let t = |a: &str, x: [u64; 2], b: &str, y: [u64; 2]| {
        vec![(a.to_string(), x), (b.to_string(), y)]
    };
    for line in [5, 11, 15] {
        assert_eq!(terms(&json, line, "assign"), t("p", [0, 3], "q", [0, 3]));
    }
    assert_eq!(terms(&json, 20, "statement"), t("p", [0, 3], "q", [0, 3]));
    // The states between the continuous assignments' updates still count.
    assert_eq!(terms(&json, 7, "assign"), t("cs1", [3, 3], "w", [3, 3]));
    assert_eq!(terms(&json, 22, "assign"), t("cs5", [3, 3], "w", [3, 3]));
    let _ = std::fs::remove_dir_all(&d);
}

/// A testbench writing the top's inputs through the simulator's interface
/// the way cocotb does (deposits in the read-write phase, with inertial
/// delay): a variable an `assign` drives from those inputs changes with
/// them in the reference simulator, a net one half a cycle later. Every
/// item's covered terms are the reference simulator's on this design and
/// library.
#[test]
fn testbench_interface_writes() {
    let d = scratch("vpiwrites");
    let c = d.join("drive.c");
    std::fs::write(
        &c,
        r#"#include <stddef.h>
#include "vpi_user.h"
static vpiHandle h[6];
static const char *names[6] = {"top.p", "top.q", "top.w", "top.p3", "top.q3", "top.w3"};
static int step = 0;
static PLI_INT32 tick(p_cb_data d);
static void at(PLI_UINT32 t) {
    s_vpi_time tm = {vpiSimTime, 0, t, 0.0};
    s_cb_data cb = {cbAfterDelay, tick, NULL, &tm, NULL, 0, NULL};
    vpi_free_object(vpi_register_cb(&cb));
}
static void put_all(int v) {
    for (int i = 0; i < 6; i++) {
        s_vpi_value val;
        val.format = vpiIntVal;
        val.value.integer = v;
        s_vpi_time tm = {vpiSimTime, 0, 0, 0.0};
        vpi_put_value(h[i], &val, &tm, vpiInertialDelay);
    }
}
static PLI_INT32 write(p_cb_data d) {
    (void)d;
    put_all(step & 1);
    return 0;
}
static PLI_INT32 tick(p_cb_data d) {
    (void)d;
    step++;
    s_vpi_time tm = {vpiSimTime, 0, 0, 0.0};
    s_cb_data cb = {cbReadWriteSynch, write, NULL, &tm, NULL, 0, NULL};
    vpi_free_object(vpi_register_cb(&cb));
    if (step < 6)
        at(1);
    return 0;
}
static PLI_INT32 start(p_cb_data d) {
    (void)d;
    for (int i = 0; i < 6; i++)
        h[i] = vpi_handle_by_name((char *)names[i], NULL);
    put_all(0);
    at(1);
    return 0;
}
static void reg(void) {
    s_cb_data cb = {cbStartOfSimulation, start, NULL, NULL, NULL, 0, NULL};
    vpi_free_object(vpi_register_cb(&cb));
}
void (*vlog_startup_routines[])(void) = {reg, 0};
"#,
    )
    .unwrap();
    let so = d.join("drive.so");
    let include = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
    let ok = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Werror", "-I"])
        .arg(&include)
        .arg(&c)
        .arg("-o")
        .arg(&so)
        .status()
        .expect("failed to launch cc")
        .success();
    assert!(ok, "cc failed");
    let src = "\
module top(input logic p, q, w, p3, q3, w3, output logic cs, output wire cs3);
  assign cs = p & q;
  logic y1;
  assign y1 = cs & w;
  assign cs3 = p3 & q3;
  wire y3 = cs3 & w3;
endmodule
";
    let out = run(
        &d,
        src,
        &["--code-coverage=expr", "--vpi-lib", so.to_str().unwrap(), "-s", "top"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let t = |a: &str, x: [u64; 2], b: &str, y: [u64; 2]| {
        vec![(a.to_string(), x), (b.to_string(), y)]
    };
    assert_eq!(terms(&json, 4, "assign"), t("cs", [0, 3], "w", [0, 3]));
    assert_eq!(terms(&json, 6, "assign"), t("cs3", [3, 3], "w3", [3, 3]));
    let _ = std::fs::remove_dir_all(&d);
}

/// A block a process's writes run counts its last run before the process
/// waits, at each wait (`#0` too: `a` at 0 decides `a && b` there, as in
/// the reference simulator). A `?:` makes an item bimodal like `^`. And
/// reading what a negated `&&` or `||` skipped, for its other operand's
/// rows, counts nothing it holds: the rows are the same with branch
/// coverage on, whose counter the `?:` condition then carries. Every
/// item's covered terms are the reference simulator's on this design.
#[test]
fn waits_mux_items_and_quiet_reads() {
    let d = scratch("waits");
    let src = "\
module tb;
  // (b) a block's evaluation between a process's #0 waits
  logic a = 1, b = 1, y;
  always_comb y = a && b;
  // (c) a negated && / || with a ?: in its right operand
  logic p, q, s, u, v, w;
  logic y2, y3;
  always_comb y2 = !(p && (s ? q : u));
  always_comb y3 = ~(v || (s ? q : w));
  initial begin
    {p, q, s, u, v, w} = 6'b0;
    #1 a = 0;
    #0 a = 1;
    #1 p = 0; s = 1; q = 1; u = 0; v = 1; w = 0;
    #1 q = 0;
    #1 p = 1; q = 1;
    #1 v = 0; q = 0;
    #1 $finish;
  end
endmodule
";
    let code = |kinds: &str| {
        let out = run(&d, src, &[kinds]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let json = results(&d);
        json[json.find("\"scopes\"").unwrap()..].to_string()
    };
    let json = code("--code-coverage=expr");
    assert_eq!(
        terms(&json, 4, "statement"),
        vec![("a".to_string(), [1, 2]), ("b".to_string(), [0, 2])]
    );
    assert!(
        json.contains(
            "{\"term\": \"p\", \"hits\": [1, 1], \"outputs\": [[0, 1], [1, 0]]}, \
             {\"term\": \"s\", \"hits\": [0, 2], \"outputs\": [[0, 0], [1, 1]]}"
        ),
        "{json}"
    );
    // a; p, q; v.
    assert!(
        results(&d).contains("\"expression\": {\"covered\": 4, \"total\": 10, \"percent\": 40.00}"),
        "{json}"
    );
    let all = code("--code-coverage=all");
    for line in [4, 8, 9] {
        assert_eq!(terms(&all, line, "statement"), terms(&json, line, "statement"));
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A design unit adds up its instances' copies of an item only when they
/// count alike: here one instance's parameters fold the `^` away, so its
/// copy is not bimodal and the other's is.
#[test]
fn design_units_keep_bimodal_copies_apart() {
    let d = scratch("unitbimodal");
    let src = "\
module m #(parameter bit P = 0, parameter bit Q = 0) (input logic a, b, output logic y);
  always_comb y = (a & b & P) | ((a ^ b) & Q);
endmodule
module tb;
  logic a, b, y1, y2;
  m #(.P(1), .Q(0)) u1(.a(a), .b(b), .y(y1));
  m #(.P(0), .Q(1)) u2(.a(a), .b(b), .y(y2));
  initial begin
    {a, b} = 2'b00;
    #1 {a, b} = 2'b10;
    #1 {a, b} = 2'b11;
    #1 {a, b} = 2'b01;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    let units = &json[json.find("\"design_units\"").unwrap()..];
    assert!(
        units.contains(
            "[{\"term\": \"a\", \"hits\": [1, 1]}, {\"term\": \"b\", \"hits\": [1, 1]}]"
        ),
        "{units}"
    );
    assert!(
        units.contains(
            "[{\"term\": \"a\", \"hits\": [2, 2], \"outputs\": [[1, 1], [1, 1]]}, \
             {\"term\": \"b\", \"hits\": [2, 2], \"outputs\": [[1, 1], [1, 1]]}]"
        ),
        "{units}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Every statement of a combinational block run between a process's
/// writes counts its last run only, also the ones after a statement whose
/// write the interpreter settles (an item makes each of these one): `y`
/// never sees `a` and `b` apart, as in the reference simulator.
#[test]
fn block_statements_after_a_write_count_once() {
    let d = scratch("blockwrites");
    let src = "\
module tb;
  logic a = 0, b = 0, x, y;
  always_comb begin
    x = a & b;
    y = a & b;
  end
  initial begin
    repeat (3) begin
      #1 a = 1; b = 1;
      #1 a = 0; b = 0;
    end
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    for line in [4, 5] {
        assert_eq!(
            terms(&json, line, "statement"),
            vec![("a".to_string(), [0, 3]), ("b".to_string(), [0, 3])]
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A loop evaluates an item once per iteration: a block's run between a
/// process's writes keeps all of them, not only the last (every row is hit
/// here, as in the reference simulator).
#[test]
fn loop_items_in_process_writes() {
    let d = scratch("loopwrites");
    let src = "\
module tb;
  logic [1:0] v = 2'b00;
  logic e = 0, t;
  always_comb begin
    t = 1'b1;
    for (int i = 0; i < 2; i++) t = t & v[i];
  end
  initial begin
    #1 v = 2'b01; e = 1;
    #1 v = 2'b11; e = 0;
    #1 v = 2'b10; e = 1;
    #1 $finish;
  end
endmodule
";
    let out = run(&d, src, &["--code-coverage=expr"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = results(&d);
    assert_eq!(
        terms(&json, 6, "statement"),
        vec![("t".to_string(), [1, 3]), ("v[i]".to_string(), [3, 3])]
    );
    let _ = std::fs::remove_dir_all(&d);
}
