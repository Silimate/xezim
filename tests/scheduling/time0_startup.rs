//! How a testbench's processes start at time 0.
//!
//! An `always` block that starts with an event control, and waits on no
//! other event after it, waits there before any `initial` block writes.
//! §9.2 leaves the start order of processes unspecified, but the reference
//! simulator and Icarus Verilog both arm such a block first, whatever its
//! position in the source, and testbench clocks depend on it:
//! `initial clk = 0; always @(clk) #10 clk = ~clk;` runs. xezim started the
//! `initial` block first when it came first in the source, or when it joined
//! the group of `initial` blocks that cannot suspend, so the x -> 0 change
//! came before the `always` block was waiting and the clock never started.
//!
//! In Verilog, a variable declaration assignment is an `initial` assignment
//! (IEEE 1364-2005 §6.2.1): `reg clk = 1'b0;` changes `clk` from x at time 0
//! and wakes the processes waiting on it. In SystemVerilog it sets the value
//! before time 0 with no event (IEEE 1800-2023 §6.8, §10.5). The reference
//! simulator applies the Verilog rule to a `.v` file unless `-sv` is given,
//! and Icarus Verilog applies it with `-g2005`; xezim always used the
//! SystemVerilog rule, so the clock of every Analog Devices HDL testbench
//! (`reg clk = 1'b0; always @(*) #10 clk <= ~clk;`) never started.
//!
//! Expected values agree with the reference simulator, and with Icarus
//! Verilog 13 except where noted.

use std::path::PathBuf;
use std::process::Command;

fn output(source: &str) -> String {
    let sim = xezim::simulate(source, 5000).expect("simulate");
    sim.output
        .iter()
        .map(|line| line.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Runs `source` written to `file` with the xezim binary, which picks the
/// language from the file name as the reference simulator does.
fn run_file(case: &str, file: &str, source: &str, args: &[&str]) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("time0_startup")
        .join(case);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--no-cache", "-s", "t"])
        .args(args)
        .arg(&path)
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    text
}

fn assert_lines(result: &str, want: &[&str]) {
    for line in want {
        assert!(
            result.lines().any(|l| l == *line),
            "no `{line}` in:\n{result}"
        );
    }
}

/// `initial c = 0;` starts each clock however the two blocks are ordered.
/// A blocking `c = ~c` is made while its own block runs, so it wakes nothing
/// and the clock stops after one edge; a nonblocking update lands once the
/// block waits again.
#[test]
fn always_waits_before_an_initial_block_writes() {
    let src = r#"
`timescale 1ns/1ns
module t;
  reg c1, c2, c3, c4, c5;
  int n1, n2, n3, n4, n5;
  initial begin n1 = 0; n2 = 0; n3 = 0; n4 = 0; n5 = 0; end
  initial c1 = 1'b0;
  always @(c1) #10 c1 = ~c1;
  always @(c2) #10 c2 = ~c2;
  initial c2 = 1'b0;
  initial c3 = 1'b0;
  always @(c3) #10 c3 <= ~c3;
  initial c4 = 1'b0;
  always begin @(c4) #10 c4 = ~c4; end
  initial c5 = 1'b0;
  always @(*) #10 c5 <= ~c5;
  always @(posedge c1) n1++;
  always @(posedge c2) n2++;
  always @(posedge c3) n3++;
  always @(posedge c4) n4++;
  always @(posedge c5) n5++;
  initial begin
    #2000;
    $display("n1=%0d n2=%0d n3=%0d n4=%0d n5=%0d", n1, n2, n3, n4, n5);
    $finish;
  end
endmodule
"#;
    assert_lines(&output(src), &["n1=1 n2=1 n3=100 n4=1 n5=100"]);
}

/// The write may come from a block that suspends, and the waiting block may
/// be in another instance, either way round. Icarus Verilog leaves `to` at
/// x: it does not wake `@(posedge o)` on the port net the child drives.
#[test]
fn always_waits_across_instances_and_behind_a_suspending_initial() {
    let src = r#"
`timescale 1ns/1ns
module child(input i, output reg o);
  time ti;
  initial o = 1'b1;
  always @(i) begin ti = $time; #1; end
endmodule
module t;
  reg a, i;
  wire o;
  time ta, to;
  initial begin a = 1'b0; #1; end
  always @(a) begin ta = $time; #1; end
  initial i = 1'b0;
  child c(.i(i), .o(o));
  always @(posedge o) begin to = $time; #1; end
  initial begin
    #5;
    $display("ta=%0d ti=%0d to=%0d", ta, c.ti, to);
    $finish;
  end
endmodule
"#;
    assert_lines(&output(src), &["ta=0 ti=0 to=0"]);
}

/// The reference simulator arms a block early only when its event control
/// has no `iff` and nothing after it waits on an event again: a delay, a
/// task that only delays and a `fork ... join_none` keep it early, while a
/// `wait`, another event control, a task that waits on one or a `fork` that
/// joins one start it in source order, after the write. Icarus Verilog
/// (without `iff`) arms all of them early.
#[test]
fn only_a_block_that_waits_once_arms_before_initial_blocks() {
    let src = r#"
`timescale 1ns/1ns
module t;
  logic p, q;
  logic w1 = 0, w2 = 0, w3 = 0, w4 = 0, w5 = 0, w6 = 0, w7 = 0;
  task tdelay; #1; endtask
  task twait; @(q); endtask
  initial begin p = 0; #100; p = 1; end
  always @(p) begin if ($time == 0) w1 = 1; tdelay; end
  always @(p) begin if ($time == 0) w2 = 1; fork @(q); join_none #1; end
  always @(p iff 1'b1) begin if ($time == 0) w3 = 1; #1; end
  always @(p) begin if ($time == 0) w4 = 1; wait (q === 1); end
  always @(p) begin if ($time == 0) w5 = 1; twait; end
  always @(p) begin if ($time == 0) w6 = 1; fork @(q); join end
  always @(p) begin if ($time == 0) w7 = 1; #1 @(q); end
  initial begin
    #50;
    $display("w=%b%b%b%b%b%b%b", w1, w2, w3, w4, w5, w6, w7);
    $finish;
  end
endmodule
"#;
    assert_lines(&output(src), &["w=1100000"]);
}

/// A clock and counters declared the Verilog way, and one waiter of each
/// kind on a declaration assignment. Icarus Verilog (`-g2005`) agrees on
/// the clocks but wakes only `@(a)` and `@(v)` at time 0, not the edges.
const DECL_INIT: &str = r#"`timescale 1ns/1ns
module t;
  reg c1 = 1'b0;
  reg c2 = 1'b0;
  reg c3 = 1'b0;
  reg a = 1'b0;
  reg b = 1'b1;
  reg [3:0] v = 4'd5;
  integer n1 = 0, n2 = 0, n3 = 0;
  time any_a, neg_a, pos_b, any_v;
  always @(*) #10 c1 <= ~c1;
  always @(c2) #10 c2 <= ~c2;
  always @(c3) #10 c3 = ~c3;
  always @(posedge c1) n1 = n1 + 1;
  always @(posedge c2) n2 = n2 + 1;
  always @(posedge c3) n3 = n3 + 1;
  always @(a) any_a = $time;
  always @(negedge a) neg_a = $time;
  always @(posedge b) pos_b = $time;
  always @(v) any_v = $time;
  initial begin
    #2000;
    $display("n1=%0d n2=%0d n3=%0d", n1, n2, n3);
    $display("any_a=%0d neg_a=%0d pos_b=%0d any_v=%0d", any_a, neg_a, pos_b, any_v);
    $finish;
  end
endmodule
"#;

/// IEEE 1364-2005 §6.2.1: in a `.v` file each declaration assignment is a
/// time-0 update, so the clocks start and every waiter wakes at time 0.
#[test]
fn verilog_declaration_assignment_is_a_time0_update() {
    let result = run_file("verilog", "t.v", DECL_INIT, &[]);
    assert_lines(
        &result,
        &["n1=100 n2=100 n3=1", "any_a=0 neg_a=0 pos_b=0 any_v=0"],
    );
}

/// IEEE 1800-2023 §6.8, §10.5: a SystemVerilog file, or a `.v` file with
/// `-sv`, sets the values before time 0: nothing wakes and no clock starts.
#[test]
fn systemverilog_declaration_assignment_is_not_an_event() {
    let want = ["n1=0 n2=0 n3=0", "any_a=x neg_a=x pos_b=x any_v=x"];
    assert_lines(&run_file("sv", "t.sv", DECL_INIT, &[]), &want);
    assert_lines(&run_file("v_with_sv", "t.v", DECL_INIT, &["-sv"]), &want);
}

/// The declaration assignments run before every `initial` block, which
/// sees their values, and an `initial` assignment to the same variable
/// lands after them.
#[test]
fn verilog_declaration_assignments_run_before_initial_blocks() {
    let src = r#"`timescale 1ns/1ns
module t;
  reg [7:0] a = 8'd5;
  initial $display("first a=%0d", a);
  initial a = 8'd7;
  initial #1 $display("final a=%0d", a);
endmodule
"#;
    assert_lines(
        &run_file("order", "t.v", src, &[]),
        &["first a=5", "final a=7"],
    );
}

/// Runtime declaration initializers in a Verilog module, at the top and in
/// an instance. Verilog requires a constant there (IEEE 1364-2005 §6.2.1),
/// and the reference simulator and Icarus Verilog reject this file as `.v`;
/// as `.sv` both print the SystemVerilog values below. xezim accepts it as
/// `.v` and runs each such initializer at time 0 with the module's other
/// declaration assignments, after their literal values: `r` reads `k`, `m`
/// keeps the value `$test$plusargs` gave it, and the `@(m)` and `@(r)`
/// blocks wake at time 0. (As SystemVerilog, those blocks stay asleep on the
/// reference simulator but wake in xezim, Silimate/xezim#385, so only the
/// values are checked there.)
const RUNTIME_INIT: &str = r#"`timescale 1ns/1ns
module c;
  reg m = $test$plusargs("MODE");
  time any_m;
  always @(m) any_m = $time;
  initial #1 begin $display("c.m=%b", m); $display("c.any_m=%0d", any_m); end
endmodule
module t;
  reg m = $test$plusargs("MODE");
  reg [7:0] k = 8'd5;
  integer r = k + $test$plusargs("MODE");
  integer seen_m, seen_r;
  time any_m, any_r;
  always @(m) any_m = $time;
  always @(r) any_r = $time;
  initial begin seen_m = m; seen_r = r; end
  c u();
  initial #1 begin
    $display("m=%b seen_m=%0d r=%0d seen_r=%0d", m, seen_m, r, seen_r);
    $display("any_m=%0d any_r=%0d", any_m, any_r);
    $finish;
  end
endmodule
"#;

const RUNTIME_INIT_V: [&str; 4] = [
    "m=1 seen_m=1 r=6 seen_r=6",
    "any_m=0 any_r=0",
    "c.m=1",
    "c.any_m=0",
];
const RUNTIME_INIT_SV: [&str; 2] = ["m=1 seen_m=1 r=6 seen_r=6", "c.m=1"];

#[test]
fn verilog_runtime_initializers_run_with_the_declaration_assignments() {
    assert_lines(
        &run_file("runtime", "t.v", RUNTIME_INIT, &["+MODE"]),
        &RUNTIME_INIT_V,
    );
    assert_lines(
        &run_file("runtime_off", "t.v", RUNTIME_INIT, &[]),
        &[
            "m=0 seen_m=0 r=5 seen_r=5",
            "any_m=0 any_r=0",
            "c.m=0",
            "c.any_m=0",
        ],
    );
    assert_lines(
        &run_file("runtime_sv", "t.v", RUNTIME_INIT, &["-sv", "+MODE"]),
        &RUNTIME_INIT_SV,
    );
}

/// `--compile -o` keeps the language the design was compiled as: an
/// artifact of a `.v` file compiled with `-sv` runs it as SystemVerilog
/// without `-sv`, and one compiled without it runs the Verilog rule,
/// runtime initializers included (`r=6`: `r` still reads `k` after it is
/// assigned).
#[test]
fn a_compiled_artifact_keeps_the_language_choice() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("time0_startup")
        .join("artifact");
    std::fs::create_dir_all(&dir).unwrap();
    let bin = env!("CARGO_BIN_EXE_xezim");
    let run = |cmd: &mut Command| {
        let out = cmd.output().unwrap();
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{text}");
        text
    };
    for (src, file) in [(DECL_INIT, "clk.v"), (RUNTIME_INIT, "rt.v")] {
        std::fs::write(dir.join(file), src).unwrap();
        for (model, extra) in [("v", &[][..]), ("sv", &["-sv"][..])] {
            let out = dir.join(format!("{}_{model}_model", &file[..file.len() - 2]));
            run(Command::new(bin)
                .args(["--no-cache", "--compile", "-s", "t"])
                .args(extra)
                .arg(dir.join(file))
                .arg("-o")
                .arg(&out));
            let text = run(Command::new(bin).arg(&out).arg("+MODE"));
            let want: &[&str] = match (file, model) {
                ("clk.v", "v") => &["n1=100 n2=100 n3=1", "any_a=0 neg_a=0 pos_b=0 any_v=0"],
                ("clk.v", _) => &["n1=0 n2=0 n3=0", "any_a=x neg_a=x pos_b=x any_v=x"],
                (_, "v") => &RUNTIME_INIT_V,
                _ => &RUNTIME_INIT_SV,
            };
            assert_lines(&text, want);
        }
    }
}
