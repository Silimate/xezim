//! IEEE 1800-2023 §16.5.1: an assertion attempt at time 0 reads default
//! sampled values, under a clock a VPI client raises at time 0 as cocotb
//! does. A variable's default sampled value is its declaration initializer,
//! else its type's default; a net's is x. A continuous assignment, an
//! `always_comb` and an `initial` block all run in the time-0 slot, after
//! its Preponed region, so their operands are x at that attempt; a value a
//! VPI client writes from cbStartOfSimulation is in place before the slot
//! begins and counts. xezim took the time-0 samples after the time-0
//! settle, so the operands of continuous assignments and `always_comb`
//! read their driven values and those attempts passed. The reference
//! simulator, running this driver and design, prints the same lines in each
//! of the four runs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Raises `top.clk` at time 0 and toggles it every 5 ns after, and sets
/// `top.a_in` to 1 at time 0. `T0_CLK` and `T0_IN` pick the routine that
/// makes each time-0 write: `start` (cbStartOfSimulation, the way cocotb
/// writes with `Immediate`) or `rw` (a cbReadWriteSynch at time 0, the way
/// cocotb applies an ordinary write and its Python `Clock`). With
/// `T0_G=start`, cbStartOfSimulation also writes `top.g` the value its
/// `always_comb` gives it, 1 to `top.pd`, which only a procedural `$past`
/// reads, and 1 to bit 0 of `top.v2`, whose `always_comb` drives both bits.
const DRIVER_C: &str = r#"
#include <stdlib.h>
#include <string.h>
#include "vpi_user.h"

static vpiHandle clk, in, g, pd, v2;
static int level = 1;

static const char *env(const char *n, const char *dflt) {
    const char *v = getenv(n);
    return v && *v ? v : dflt;
}

static void put(vpiHandle h, int v) {
    s_vpi_value val;
    s_vpi_time t = {vpiSimTime, 0, 0, 0.0};
    val.format = vpiIntVal;
    val.value.integer = v;
    vpi_put_value(h, &val, &t, vpiNoDelay);
}

static void at(PLI_INT32 (*rtn)(p_cb_data), int reason, unsigned delay) {
    s_cb_data cb;
    s_vpi_time t = {vpiSimTime, 0, delay, 0.0};
    memset(&cb, 0, sizeof cb);
    cb.reason = reason;
    cb.cb_rtn = rtn;
    cb.time = &t;
    vpi_register_cb(&cb);
}

static PLI_INT32 timer(p_cb_data cb) {
    (void)cb;
    level = !level;
    put(clk, level);
    at(timer, cbAfterDelay, 5);
    return 0;
}

static PLI_INT32 rw(p_cb_data cb) {
    (void)cb;
    if (!strcmp(env("T0_IN", "rw"), "rw"))
        put(in, 1);
    if (!strcmp(env("T0_CLK", "rw"), "rw"))
        put(clk, 1);
    return 0;
}

static PLI_INT32 start(p_cb_data cb) {
    (void)cb;
    clk = vpi_handle_by_name("top.clk", NULL);
    in = vpi_handle_by_name("top.a_in", NULL);
    g = vpi_handle_by_name("top.g", NULL);
    pd = vpi_handle_by_name("top.pd", NULL);
    v2 = vpi_handle_by_index(vpi_handle_by_name("top.v2", NULL), 0);
    if (!strcmp(env("T0_G", "none"), "start")) {
        put(g, 1);
        put(pd, 1);
        put(v2, 1);
    }
    if (!strcmp(env("T0_IN", "rw"), "start"))
        put(in, 1);
    if (!strcmp(env("T0_CLK", "rw"), "start"))
        put(clk, 1);
    at(rw, cbReadWriteSynch, 0);
    at(timer, cbAfterDelay, 5);
    return 0;
}

static void boot(void) { at(start, cbStartOfSimulation, 0); }

void (*vlog_startup_routines[])(void) = {boot, 0};
"#;

/// One assertion per kind of operand; each passes when the operand's
/// sampled value is the one its first time-0 write gives it. At 10 ns,
/// `$past` reads the time-0 samples.
const TOP_SV: &str = r#"
module top;
  logic clk, a_in;
  logic r;                         // never written: x
  logic ri = 1'b1;                 // declaration initializer
  int   n;                         // 2-state default: 0
  wire  w = 1'b1;                  // net declaration assignment
  logic c; assign c = ri;          // continuous assignment
  logic g; always_comb g = ri;     // always_comb
  logic f; initial f = 1'b1;       // initial block
  logic m [2]; always_comb m[0] = ri;  // an array cell
  // A procedural $past at the first tick: the default of `a_in & pb`, from
  // both operands' defaults (pb is x before the time-0 settle drives it).
  logic pb; always_comb pb = ri;
  always @(posedge clk) if ($time == 0) $display("T|ppast0|%b", $past(a_in & pb));
  logic pd;                        // written only by the driver
  always @(posedge clk) if ($time == 0) $display("T|pdpast0|%b", $past(pd));
  logic [1:0] v2; always_comb v2 = {ri, ri};  // the driver writes bit 0
  a_r:  assert property (@(posedge clk) r === 1'bx) $display("T|r|pass|%0t", $time); else $display("T|r|fail|%0t", $time);
  a_ri: assert property (@(posedge clk) ri === 1'b1) $display("T|ri|pass|%0t", $time); else $display("T|ri|fail|%0t", $time);
  a_n:  assert property (@(posedge clk) n === 0) $display("T|n|pass|%0t", $time); else $display("T|n|fail|%0t", $time);
  a_w:  assert property (@(posedge clk) w === 1'b1) $display("T|w|pass|%0t", $time); else $display("T|w|fail|%0t", $time);
  a_c:  assert property (@(posedge clk) c === 1'b1) $display("T|c|pass|%0t", $time); else $display("T|c|fail|%0t", $time);
  a_g:  assert property (@(posedge clk) g === 1'b1) $display("T|g|pass|%0t", $time); else $display("T|g|fail|%0t", $time);
  a_f:  assert property (@(posedge clk) f === 1'b1) $display("T|f|pass|%0t", $time); else $display("T|f|fail|%0t", $time);
  a_in_a: assert property (@(posedge clk) a_in === 1'b1) $display("T|in|pass|%0t", $time); else $display("T|in|fail|%0t", $time);
  a_m:  assert property (@(posedge clk) m[0] === 1'b1) $display("T|m|pass|%0t", $time); else $display("T|m|fail|%0t", $time);
  a_p:  assert property (@(posedge clk) $time != 10 || 0)
          else $display("T|past|%0t|%b%b%b%b", $time, $past(a_in), $past(c), $past(w), $past(ri));
  // At the first tick, $past reads default sampled values, of an
  // expression's operands too.
  a_p0: assert property (@(posedge clk) $time != 0 || 0)
          else $display("T|past0|%b%b", $past(a_in), $past(a_in | 1'b0));
  a_v2: assert property (@(posedge clk) $time != 0 || v2 === 2'bx1)
          begin if ($time == 0) $display("T|v2|pass|0"); end
          else $display("T|v2|fail|%0t", $time);
  initial #12 $finish;
endmodule
"#;

/// A scratch directory holding the compiled driver and `top.sv`.
fn setup() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("xezim_vpi_sva_t0_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let c = dir.join("drv.c");
    let so = dir.join("drv.so");
    std::fs::write(&c, DRIVER_C).unwrap();
    std::fs::write(dir.join("top.sv"), TOP_SV).unwrap();
    let include = Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
    let ok = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-I"])
        .arg(&include)
        .arg(&c)
        .arg("-o")
        .arg(&so)
        .status()
        .expect("failed to launch cc")
        .success();
    assert!(ok, "cc failed for {}", c.display());
    (dir, so)
}

/// The `T|` lines of one run, sorted.
fn run(dir: &Path, so: &Path, clk: &str, input: &str, g: &str) -> Vec<String> {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .arg("--vpi-lib")
        .arg(so)
        .args(["--no-cache", "-s", "top", "top.sv"])
        .env("T0_CLK", clk)
        .env("T0_IN", input)
        .env("T0_G", g)
        .current_dir(dir)
        .output()
        .expect("failed to run xezim");
    let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("T|"))
        .map(str::to_string)
        .collect();
    lines.sort();
    lines
}

#[test]
fn time0_attempt_reads_default_sampled_values() {
    let (dir, so) = setup();
    let mut bad = Vec::new();
    let runs = [
        ("rw", "rw", "none"),
        ("rw", "start", "none"),
        ("start", "rw", "start"),
        ("start", "start", "start"),
    ];
    for (clk, input, g) in runs {
        // Only a write from cbStartOfSimulation precedes the slot, even one
        // of the value the slot's `always_comb` writes again.
        let early = input == "start";
        let mut want = vec![
            "T|c|fail|0".to_string(),
            "T|c|pass|10".to_string(),
            "T|f|fail|0".to_string(),
            "T|f|pass|10".to_string(),
            format!("T|g|{}|0", if g == "start" { "pass" } else { "fail" }),
            "T|g|pass|10".to_string(),
            format!("T|in|{}|0", if early { "pass" } else { "fail" }),
            "T|in|pass|10".to_string(),
            "T|m|fail|0".to_string(),
            "T|m|pass|10".to_string(),
            "T|n|pass|0".to_string(),
            "T|n|pass|10".to_string(),
            format!("T|past0|{}", if early { "11" } else { "xx" }),
            "T|ppast0|x".to_string(),
            format!("T|pdpast0|{}", if g == "start" { "1" } else { "x" }),
            format!("T|v2|{}|0", if g == "start" { "pass" } else { "fail" }),
            format!("T|past|10|{}xx1", if early { "1" } else { "x" }),
            "T|r|pass|0".to_string(),
            "T|r|pass|10".to_string(),
            "T|ri|pass|0".to_string(),
            "T|ri|pass|10".to_string(),
            "T|w|fail|0".to_string(),
            "T|w|pass|10".to_string(),
        ];
        want.sort();
        let got = run(&dir, &so, clk, input, g);
        if got != want {
            bad.push(format!(
                "T0_CLK={clk} T0_IN={input} T0_G={g}:\n got {got:?}\nwant {want:?}"
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
