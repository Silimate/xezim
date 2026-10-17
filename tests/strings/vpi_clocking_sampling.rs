//! IEEE 1800-2017 §14.13 clocking-block sampling under a clock driven through
//! VPI (§38.34 `vpi_put_value`), the way cocotb drives one.
//!
//! A clocking block samples its `#1step` inputs in the Preponed region of the
//! slot its clocking event occurs in (§14.4), before that slot's Pre-Active
//! VPI callbacks (§4.4.3.2), and processes the clocking event in the delta
//! that made the edge: its clockvars update, its matured outputs drive, and
//! the processes waiting on it resume after the edge's NBA updates. cocotb's
//! Python `Clock` writes the clock from cbReadWriteSynch, after the NBA
//! region. The clocking event was processed in the next delta or the next
//! slot, after the samples had been retaken: clockvars read the values flops
//! took at that edge, `@(cb)` resumed at the next edge's time and output
//! drives matured half a period late. A value written in the edge's own
//! cbAfterDelay counted as already sampled. The reference simulator, loading
//! the same C module and design, gives the expected results in all of these
//! runs.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Drives `top.clk` and `top.d`. `CB_CLK` picks the routine that writes the
/// clock: `delay` (a cbAfterDelay timer), `rw` (the timer queues it for
/// cbReadWriteSynch, as cocotb's Python `Clock` does) or `vc` (a
/// cbValueChange on `top.tick`). `CB_DATA` picks where `d` changes: `rw`
/// (cbReadWriteSynch after the falling edge), `delay` (the falling edge's
/// timer), `vc` (a cbValueChange on the clock's fall) or `same` (the rising
/// edge's own routine, before the clock). `CB_FLAG` is the `vpi_put_value`
/// flag: `nodelay`, `inertial` or `force`. With `CB_FINISH=<t>`, a
/// cbValueChange on the clock calls `vpi_control(vpiFinish)` at its first
/// rise at or after `t`, as cocotb does when a test ends on `RisingEdge`.
const DRIVER_C: &str = r#"
#include <stdlib.h>
#include <string.h>
#include "vpi_user.h"

static vpiHandle clk, d;
static int flag = vpiNoDelay;
static const char *cmode = "delay", *dmode = "rw";
static const int pat[] = {0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 1, 1, 0, 0, 1, 0, 1, 0, 0, 1};
#define NPAT (int)(sizeof pat / sizeof pat[0])
static int level, cyc, steps;

static const char *env(const char *n, const char *dflt) {
    const char *v = getenv(n);
    return v && *v ? v : dflt;
}

static void put(vpiHandle h, int v) {
    s_vpi_value val;
    s_vpi_time t = {vpiSimTime, 0, 0, 0.0};
    val.format = vpiIntVal;
    val.value.integer = v;
    vpi_put_value(h, &val, &t, flag);
}

static void at(PLI_INT32 (*rtn)(p_cb_data), int reason, unsigned delay, vpiHandle obj) {
    s_cb_data cb;
    s_vpi_time t = {vpiSimTime, 0, delay, 0.0};
    s_vpi_value v;
    memset(&cb, 0, sizeof cb);
    v.format = vpiIntVal;
    cb.reason = reason;
    cb.cb_rtn = rtn;
    cb.obj = obj;
    cb.time = &t;
    cb.value = &v;
    vpi_register_cb(&cb);
}

static void data(void) { put(d, pat[cyc++ % NPAT]); }
static PLI_INT32 rw_data(p_cb_data cb) { (void)cb; data(); return 0; }
static PLI_INT32 rw_clk(p_cb_data cb) { (void)cb; put(clk, level); return 0; }

static void step(int lvl) {
    level = lvl;
    if (lvl && !strcmp(dmode, "same"))
        data();
    if (!strcmp(cmode, "rw"))
        at(rw_clk, cbReadWriteSynch, 0, NULL);
    else
        put(clk, lvl);
    if (!lvl && !strcmp(dmode, "rw"))
        at(rw_data, cbReadWriteSynch, 0, NULL);
    if (!lvl && !strcmp(dmode, "delay"))
        data();
}

static PLI_INT32 timer(p_cb_data cb) {
    (void)cb;
    step(!level);
    if (++steps < 2 * NPAT)
        at(timer, cbAfterDelay, 5, NULL);
    return 0;
}

static PLI_INT32 tick_vc(p_cb_data cb) {
    if (steps++ < 2 * NPAT)
        step(cb->value->value.integer & 1);
    return 0;
}

static PLI_INT32 clk_vc(p_cb_data cb) {
    if ((cb->value->value.integer & 1) == 0)
        data();
    return 0;
}

static PLI_INT32 fin_vc(p_cb_data cb) {
    s_vpi_time t = {vpiSimTime, 0, 0, 0.0};
    vpi_get_time(NULL, &t);
    if ((cb->value->value.integer & 1) && (int)t.low >= atoi(env("CB_FINISH", "0")))
        vpi_control(vpiFinish, 0);
    return 0;
}

static PLI_INT32 start(p_cb_data cb) {
    const char *f = env("CB_FLAG", "nodelay");
    (void)cb;
    cmode = env("CB_CLK", "delay");
    dmode = env("CB_DATA", "rw");
    flag = !strcmp(f, "inertial") ? vpiInertialDelay : !strcmp(f, "force") ? vpiForceFlag : vpiNoDelay;
    clk = vpi_handle_by_name("top.clk", NULL);
    d = vpi_handle_by_name("top.d", NULL);
    if (!strcmp(cmode, "vc"))
        at(tick_vc, cbValueChange, 0, vpi_handle_by_name("top.tick", NULL));
    else
        at(timer, cbAfterDelay, 5, NULL);
    if (!strcmp(dmode, "vc"))
        at(clk_vc, cbValueChange, 0, clk);
    if (*env("CB_FINISH", ""))
        at(fin_vc, cbValueChange, 0, clk);
    return 0;
}

static void boot(void) { at(start, cbStartOfSimulation, 0, NULL); }

void (*vlog_startup_routines[])(void) = {boot, 0};
"#;

/// Flops on both clock edges and four clocking blocks: `cb` (posedge, default
/// `#1step` inputs, `#0` and `#3` outputs, and the default clocking for
/// `##1`), `cb0` (`input #0`), `cb2` (`input #2`) and `cbn` (negedge). The
/// waits are `@(cb)` in an `initial` and in a `program`, `@(cb iff cb.q)` and
/// `##1`. Each checks that it resumes in the slot of its edge, after the
/// edge's NBA updates, and reads that edge's samples: `cb.q` is the value
/// `q2` has just taken, `cb0.q` the value `q` has just taken, and `cb.m` and
/// `cb2.m` are `m` (which toggles 1 before each rising edge) at and 2 before
/// the edge. The outputs must change at the edge and 3 after it.
const TOP_SV: &str = r#"
module top;
  logic clk = 0, d = 0, tick = 0, q = 0, q2 = 0, n = 0, n2 = 0, m = 0, o0 = 0, o3 = 0;
  bit same;
  int fails = 0, evals = 0, late = 0, ones = 0, guarded = 0, negs = 0, o0s = 0, o3s = 0, init = 0;
  time tpos = 0, tneg = 0;
  initial same = $test$plusargs("same");
  initial #203 $finish;
  always #5 tick = ~tick;
  always_ff @(posedge clk) begin q <= d; q2 <= q; end
  always_ff @(negedge clk) begin n <= q; n2 <= n; end
  // m toggles 1 before each rising edge.
  always @(negedge clk) #4 m = ~m;
  always @(posedge clk) tpos = $time;
  always @(negedge clk) tneg = $time;
  // §14.3/§14.13: inputs default to #1step, outputs to #0.
  clocking cb @(posedge clk);
    input q, d, m;
    output o0;
    output #3 o3;
  endclocking
  default clocking cb;
  // §14.4: input #0 samples in the Observed region, after the edge's NBAs.
  clocking cb0 @(posedge clk);
    default input #0;
    input q;
  endclocking
  // §14.4: input #2 samples 2 before the edge, before m toggles.
  clocking cb2 @(posedge clk);
    default input #2;
    input m;
  endclocking
  clocking cbn @(negedge clk);
    input n;
  endclocking
  task automatic fail(string what);
    fails++;
    $display("FAIL %s t=%0t", what, $time);
  endtask
  initial forever begin
    @(cb);
    evals++;
    $display("E t=%0t", $time);
    if ($time != tpos) late++;
    if (cb.q !== q2) fail("cb.q");
    // With +same, d changes in the rising edge's own slot, so its sample is
    // the value q held before the edge.
    if (cb.d !== (same ? q2 : q)) fail("cb.d");
    if (cb.q === 1'b1) ones++;
    if (evals > 1 && cb.m !== m) fail("cb.m");
    cb.o0 <= evals[0];
    cb.o3 <= evals[0];
  end
  initial forever begin
    @(cb0);
    if (cb0.q !== q) fail("cb0.q");
  end
  initial forever begin
    @(cb2);
    if (evals > 1 && cb2.m !== !m) fail("cb2.m");
  end
  initial forever begin
    @(cbn);
    negs++;
    if ($time != tneg) late++;
    if (cbn.n !== n2) fail("cbn.n");
  end
  initial forever begin
    @(cb iff cb.q);
    guarded++;
    if (q2 !== 1'b1) fail("iff");
  end
  always @(o0) begin o0s++; if ($time != tpos) fail("o0"); end
  always @(o3) begin o3s++; if ($time != tpos + 3) fail("o3"); end
  initial forever begin
    ##1;
    init++;
    if ($time != tpos || cb.q !== q2) fail("cycle");
  end
  program automatic prg;
    int pevals = 0, pfails = 0;
    initial forever begin
      @(cb);
      pevals++;
      if ($time != tpos || cb.q !== q2) begin
        pfails++;
        $display("FAIL prg t=%0t", $time);
      end
    end
    final $display("PROGRAM evals=%0d fails=%0d", pevals, pfails);
  endprogram
  final $display("RESULT evals=%0d negs=%0d init=%0d fails=%0d late=%0d ones=%0d guarded=%0d o0s=%0d o3s=%0d",
                 evals, negs, init, fails, late, ones, guarded, o0s, o3s);
endmodule
"#;

/// A scratch directory holding the compiled driver and `top.sv`.
fn setup(tag: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("xezim_vpi_cb_{}_{}", tag, std::process::id()));
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

/// Run `top.sv` with the driver under `env`; returns the combined output of
/// a run that exited successfully.
fn run(dir: &Path, so: &Path, env: &[(&str, &str)], extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .arg("--vpi-lib")
        .arg(so)
        .args(["--no-cache", "-s", "top", "top.sv"])
        .args(extra)
        .envs(env.iter().copied())
        .current_dir(dir)
        .output()
        .expect("failed to run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "xezim exited with {} under {env:?}:\n{text}",
        out.status
    );
    text
}

/// Every clock routine, data routine and write flag: 20 clocking events on
/// each edge, each in its own slot, and no failure. A value written with an
/// inertial delay in the same slot as the edge it should miss races the
/// edge, so `same` runs only with the immediate flags.
#[test]
fn vpi_driven_clock_samples_clocking_inputs_preponed() {
    let (dir, so) = setup("matrix");
    let mut bad = Vec::new();
    for clk in ["delay", "rw", "vc"] {
        for data in ["rw", "delay", "vc", "same"] {
            for flag in ["nodelay", "inertial", "force"] {
                if data == "same" && flag == "inertial" {
                    continue;
                }
                let env = [("CB_CLK", clk), ("CB_DATA", data), ("CB_FLAG", flag)];
                let extra: &[&str] = if data == "same" { &["+same"] } else { &[] };
                let text = run(&dir, &so, &env, extra);
                let ok = text.contains(
                    "RESULT evals=20 negs=20 init=20 fails=0 late=0 ones=9 guarded=9 o0s=20 o3s=20",
                ) && text.contains("PROGRAM evals=20 fails=0");
                if !ok {
                    bad.push(format!("clk={clk} data={data} flag={flag}:\n{text}"));
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A routine that ends the run from the clock's value-change callback, as
/// cocotb does when a test returns on `RisingEdge`, ends it in that delta:
/// no clocking event is processed at that edge. The reference simulator
/// processes them at 5 to 85 and not at 95, where the run ends.
#[test]
fn vpi_finish_at_an_edge_runs_no_clocking_event() {
    let (dir, so) = setup("finish");
    let env = [("CB_CLK", "rw"), ("CB_DATA", "rw"), ("CB_FINISH", "95")];
    let text = run(&dir, &so, &env, &[]);
    let _ = std::fs::remove_dir_all(&dir);
    let events: Vec<&str> = text.lines().filter(|l| l.starts_with("E t=")).collect();
    let want: Vec<String> = (0..9).map(|k| format!("E t={}", 5 + 10 * k)).collect();
    assert_eq!(events, want, "{text}");
    assert!(
        text.contains("RESULT evals=9 negs=9 init=9 fails=0 late=0 ones=3 guarded=3 o0s=9 o3s=9")
            && text.contains("PROGRAM evals=9 fails=0"),
        "{text}"
    );
}
