//! §14.16.2: a select drive of a clockvar is held as a bit mask and applied to
//! the net when its skew elapses. Issuing it must not touch the net or its
//! inline-bits mirror, and must not wake `@(q)` before the drive matures.
//! Runs the binary because the mirror is enabled through the environment.

use std::path::PathBuf;
use std::process::Command;

fn run(tag: &str, src: &str) -> Vec<String> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("clocking_select_drive_maturity");
    std::fs::create_dir_all(&dir).unwrap();
    let sv = dir.join(format!("{tag}.sv"));
    std::fs::write(&sv, src).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .env("XEZIM_INLINE_BITS", "1")
        .env("XEZIM_ARRAY_SOA_SHADOW", "1")
        .args([
            "--simulate",
            "-s",
            "top",
            "--no-cache",
            sv.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "run failed:\n{text}");
    text.lines()
        .filter_map(|l| l.find("T|").map(|i| l[i..].to_string()))
        .collect()
}

const AT_EVENT: &str = r#"
`timescale 1ns/1ns
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] q = 4'b0000, r;
  wire [3:0] w = q;
  clocking cb @(posedge clk);
    default output #2;
    output q;
  endclocking
  initial begin
    @(cb);
    cb.q[0] <= 1'b1;
    @(cb);
  end
  initial begin
    #6 r = q;
    $display("T|pre t=%0t q=%b r=%b w=%b", $time, q, r, w);
    if (q !== 4'b0000) $display("T|leak t=%0t q=%b", $time, q);
    #2 r = q;
    $display("T|post t=%0t q=%b r=%b w=%b", $time, q, r, w);
    $finish;
  end
  always @(q) $display("T|chg t=%0t q=%b", $time, q);
endmodule
"#;

#[test]
fn select_drive_at_event_is_invisible_until_it_matures() {
    assert_eq!(
        run("at_event", AT_EVENT),
        [
            "T|pre t=6 q=0000 r=0000 w=0000",
            "T|chg t=7 q=0001",
            "T|post t=8 q=0001 r=0001 w=0001",
        ]
    );
}

const BETWEEN_EVENTS: &str = r#"
`timescale 1ns/1ns
module top;
  logic clk = 0;
  always #5 clk = ~clk;
  logic [3:0] q = 4'b0000, r;
  wire [3:0] w = q;
  clocking cb @(posedge clk);
    default output #2;
    output q;
  endclocking
  initial begin
    @(cb);
    #1;
    cb.q[0] <= 1'b1;
    cb.q <= 4'b1010;
    @(cb);
  end
  initial begin
    #16 r = q;
    $display("T|pre t=%0t q=%b r=%b w=%b", $time, q, r, w);
    if (q !== 4'b0000) $display("T|leak t=%0t q=%b", $time, q);
    #2 r = q;
    $display("T|post t=%0t q=%b r=%b w=%b", $time, q, r, w);
    $finish;
  end
  always @(q) $display("T|chg t=%0t q=%b", $time, q);
endmodule
"#;

#[test]
fn composed_drives_between_events_are_invisible_until_they_mature() {
    assert_eq!(
        run("between_events", BETWEEN_EVENTS),
        [
            "T|pre t=16 q=0000 r=0000 w=0000",
            "T|chg t=17 q=1010",
            "T|post t=18 q=1010 r=1010 w=1010",
        ]
    );
}
