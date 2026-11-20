//! IEEE 1800-2023 §16.14.6, §16.4.2: a procedural concurrent assertion that
//! a procedure reaches after a delay or an event control queues one instance,
//! and its attempt starts at the clock's next tick. A deferred assertion
//! there reports at the end of its time slot.
//!
//! The process flushed its pending instances and reports when it looped
//! back to its event control in the same time slot, which is no flush point
//! (only a resume after a suspension is), so `a1`-`a3` and `d1` never
//! reported. And an instance first queued after a delay started its attempt
//! at once, outside a clock tick (`a4` at 6, then nothing at 15).

use xezim::simulate;

const SRC: &str = r#"
module top;
  logic clk = 0;
  logic [7:0] a = 1, b = 2;
  wire [8:0] s = a + b;
  always #5 clk = ~clk;
  initial begin
    repeat (3) #20 begin a = a + 3; b = b + 5; end
    #12 $finish;
  end
  // A procedural concurrent assertion after a delay: one attempt per
  // execution, at the clock's next tick.
  always @(posedge clk) begin
    #1;
    a1: assert property (@(posedge clk) s == a + b) $display("T|a1|%0t", $time);
  end
  // The delay as the statement's prefix.
  always @(posedge clk) #2 a2: assert property (@(posedge clk) s == a + b) $display("T|a2|%0t", $time);
  // After an event control on the other edge.
  always @(posedge clk) begin
    @(negedge clk);
    a3: assert property (@(posedge clk) s == a + b) $display("T|a3|%0t", $time);
  end
  // In an initial forever loop, after an edge and a delay.
  initial forever begin
    @(posedge clk);
    #1;
    a4: assert property (@(posedge clk) s == a + b) $display("T|a4|%0t", $time);
  end
  // A deferred assertion after a delay reports at the end of its slot.
  always @(posedge clk) begin
    #1;
    d1: assert #0 (s == a + b) $display("T|d1|%0t", $time);
  end
endmodule
"#;

#[test]
fn procedural_assertions_after_a_delay_attempt_at_the_next_tick() {
    let sim = simulate(SRC, 1000).expect("simulate failed");
    let got: Vec<String> = sim
        .output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .filter(|l| l.starts_with("T|"))
        .collect();
    // Same-time reports of different assertions come in no set order.
    let times = |label: &str| -> Vec<String> {
        got.iter()
            .filter(|l| l.starts_with(&format!("T|{label}|")))
            .cloned()
            .collect()
    };
    let ticks = ["15", "25", "35", "45", "55", "65"];
    for label in ["a1", "a2", "a3", "a4"] {
        let want: Vec<String> = ticks.iter().map(|t| format!("T|{label}|{t}")).collect();
        assert_eq!(times(label), want, "{label}");
    }
    let want: Vec<String> = ["6", "16", "26", "36", "46", "56", "66"]
        .iter()
        .map(|t| format!("T|d1|{t}"))
        .collect();
    assert_eq!(times("d1"), want, "d1");
}
