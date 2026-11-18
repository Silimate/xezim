//! §25.3, §23.3.3: an interface port is connected to an interface instance
//! or to an interface port of the parent. An instance that leaves one
//! unconnected (absent from a named list, `.p()`, or an empty ordered slot)
//! is an elaboration error, as on Verilator and the reference simulator.
//! xezim used to run it, the port's members reading x of their own, which hid
//! a stale Adams Bridge testbench. `--relax-unconnected-interface-port` (or
//! `XEZIM_RELAX_UNCONNECTED_INTERFACE_PORT=1`) keeps that lenience, with a
//! warning; it is part of the design-cache key.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use xezim::simulate;

const RELAX_ENV: &str = "XEZIM_RELAX_UNCONNECTED_INTERFACE_PORT";

fn design(inst: &str) -> String {
    format!(
        r#"
interface mem_if; logic [7:0] rdata; logic rq; modport req (input rdata, output rq); endinterface
module dut (input logic clk, mem_if.req mem, output logic [7:0] q);
  assign mem.rq = 1'b1;
  always_ff @(posedge clk) q <= mem.rdata;
endmodule
module gen (interface bus, output logic o);
  assign o = bus.rq;
endmodule
module t;
  logic clk = 0, o; logic [7:0] q;
  mem_if mem ();
  assign mem.rdata = 8'h3c;
  {inst}
  initial begin
    #1 clk = 1; #1;
    $display("RAN q=%h", q);
    $finish;
  end
endmodule
"#
    )
}

#[test]
fn an_unconnected_interface_port_is_an_elaboration_error() {
    for (inst, port) in [
        ("dut u (.clk(clk), .q(q));", "mem"),
        ("dut u (.clk(clk), .mem(), .q(q));", "mem"),
        ("dut u (clk, , q);", "mem"),
        ("gen g (.o(o));", "bus"),
        // `.*` with no interface named `bus` in the parent (§23.3.2.4).
        ("gen g (.*);", "bus"),
    ] {
        let err = simulate(&design(inst), 10)
            .err()
            .unwrap_or_else(|| panic!("{inst}: elaborated with an unconnected interface port"));
        assert!(
            err.contains(&format!("interface port '{port}'"))
                && err.contains("is not connected")
                && err.contains("§25.3"),
            "{inst}: {err}"
        );
        assert!(
            err.contains("--relax-unconnected-interface-port") && err.contains(RELAX_ENV),
            "{inst}: no relaxation named: {err}"
        );
    }
}

#[test]
fn every_way_of_connecting_an_interface_port_still_elaborates() {
    for inst in [
        "dut u (.clk(clk), .mem(mem), .q(q));",
        "dut u (.clk(clk), .mem(mem.req), .q(q));",
        "dut u (clk, mem, q);",
        "dut u (.*);",
        "dut u (.clk, .mem, .q);",
        "dut u (.clk(clk), .mem(mem), .q(q)); gen g (.bus(mem), .o(o));",
    ] {
        let sim = simulate(&design(inst), 10).unwrap_or_else(|e| panic!("{inst}: {e}"));
        assert!(
            sim.output.iter().any(|l| l.message.trim() == "RAN q=3c"),
            "{inst}: {:?}",
            sim.output.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }
}

/// §23.11, §23.3.2.4: an interface a `bind` adds to the target is a name
/// there, so it fills the `.*` interface port of an instance bound with it.
/// Without it the port is unconnected. Verilator agrees on both.
#[test]
fn a_bound_interface_fills_a_bound_wildcard_interface_port() {
    let src = |with_bus: bool| {
        format!(
            r#"
interface mem_if; logic rq = 1'b1; endinterface
module gen (interface bus, output logic o);
  assign o = bus.rq;
endmodule
module tgt;
  logic o;
  initial begin #1 $display("BOUND o=%0d", o); $finish; end
endmodule
module t;
  tgt u ();
endmodule
{}bind tgt gen g (.*);
"#,
            if with_bus { "bind tgt mem_if bus ();\n" } else { "" }
        )
    };
    let sim = simulate(&src(true), 10).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        sim.output.iter().any(|l| l.message.trim() == "BOUND o=1"),
        "{:?}",
        sim.output.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
    let err = simulate(&src(false), 10)
        .err()
        .expect("elaborated with no interface named bus in the target");
    assert!(
        err.contains("interface port 'bus'") && err.contains("is not connected"),
        "{err}"
    );
}

/// The switch and its environment variable elaborate the instance with a
/// warning; a strict run after a relaxed one does not reuse its design;
/// `--help` and `--show-env-avail` list them.
#[test]
fn the_relaxation_elaborates_with_a_warning() {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "unconnected_interface_port_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("t.sv"), design("dut u (.clk(clk), .q(q));")).unwrap();
    let run = |args: &[&str], env: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xezim"));
        cmd.current_dir(&dir)
            .args(["-s", "t", "--cache-dir", "cache"])
            .args(args)
            .arg("t.sv")
            .env_remove(RELAX_ENV)
            .env_remove("XEZIM_NO_CACHE");
        if env {
            cmd.env(RELAX_ENV, "1");
        }
        let out = cmd.output().expect("run xezim");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned()
                + &String::from_utf8_lossy(&out.stderr),
        )
    };
    for (args, env) in [
        (&["--relax-unconnected-interface-port"][..], false),
        (&[][..], true),
    ] {
        let (code, out) = run(args, env);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("RAN q="), "{out}");
        assert!(
            out.contains("interface port 'mem' of instance 'u'") && out.contains("§25.3"),
            "no warning: {out}"
        );
    }
    let (code, out) = run(&[], false);
    assert_ne!(code, 0, "a strict run reused the relaxed design: {out}");
    assert!(
        out.contains("is not connected") && !out.contains("RAN q="),
        "{out}"
    );
    // `--help` lists the switch and `--show-env-avail` lists the variable.
    for (arg, expected) in [
        ("--help", "--relax-unconnected-interface-port"),
        ("--show-env-avail", RELAX_ENV),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
            .arg(arg)
            .output()
            .expect("run xezim");
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{arg} failed:\n{text}");
        assert!(text.contains(expected), "{arg} omitted {expected}:\n{text}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
