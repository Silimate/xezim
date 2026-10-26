//! IEEE 1800-2023 clause 14: clocking blocks.

use crate::harness::{Order, check_clocking};
use xezim::compiler::simulator::ClockingDrive::{self, Lrm, NextEdge};

/// `check` under both `--clocking-drive` modes, for a fixture whose drives
/// both modes place alike.
fn check_both(name: &str, src: &str, top: &str, order: Order, expected: &[&str], skip: &[&str]) {
    for mode in [Lrm, NextEdge] {
        check_clocking(mode, name, src, top, order, expected, skip);
    }
}

// §14.10: clocking block events
#[test]
fn c14_10_clocking_block_events() {
    check_both(
        "c14_more",
        include_str!("sv/c14_more.sv"),
        "c14x",
        Order::Exact,
        &[
            "T|14.9a|t=15 req=11 gnt_sampled=01",
            "T|14.9b|t=27 bidir=5a cb.bidir=5a",
            "T|14.16.2|tgt=02",
            "T|14.10|cb3.ev_sig changed t=65 val=3",
        ],
        &[],
    );
}

// §14.14, §6.24.2: global clocking, $cast on enums
#[test]
fn c14_14_global_clocking_and_6_24_2_cast() {
    check_both(
        "c_left",
        include_str!("sv/c_left.sv"),
        "cleft",
        Order::Exact,
        &["T|20.11|fails=1", "T|6.24.2d|e=B", "T|14.14|gcnt=3"],
        &[],
    );
}

// §14.16.2: Driving an inout clockvar that is a net inside an interface
#[test]
fn c14_16_clocking_inout_iface_net() {
    check_both(
        "14.16_clocking_inout_iface_net",
        include_str!("sv/14.16_clocking_inout_iface_net.sv"),
        "rvd",
        Order::Exact,
        &["T|r1|t=8 req=11 bidir=5a", "T|r2|t=18 bidir=77"],
        &[],
    );
}

// §14.4: Output skew (default output #2)
#[test]
fn c14_16_clocking_output_skew() {
    check_both(
        "14.16_clocking_output_skew",
        include_str!("sv/14.16_clocking_output_skew.sv"),
        "rcb",
        Order::Exact,
        &[
            "T|r1|t=5000 cb.d=0 cb.dl=0 d=1",
            "T|r1|t=15000 cb.d=1 cb.dl=1 d=2",
            "T|r1|t=25000 cb.d=2 cb.dl=2 d=3",
            "T|r1|t=35000 cb.d=3 cb.dl=3 d=4",
            "T|r2|t=45000 cb.d=3 d=4",
            "T|r2|t=55000 cb.d=4 d=5",
            "T|r3|t=56000 dout=xx",
            "T|r4|t=58000 dout=ab",
        ],
        &[],
    );
}

// §14.3: clocking block inout signals
#[test]
fn c14_3_clocking_inout_signals() {
    check_both(
        "r_cbinout",
        include_str!("sv/r_cbinout.sv"),
        "rci",
        Order::Exact,
        &[
            "T|r1|t=5 cb.gnt=0 gnt=1",
            "T|r2|t=8 bidir=5a",
            "T|r3|t=15 cb.bidir=5a",
        ],
        &[],
    );
}

// §14.4, §14.13: Input and output skews: input sampling (#1step, #N)
#[test]
fn c14_4_clocking_input_sampling() {
    check_both(
        "14.4_clocking_input_sampling",
        include_str!("sv/14.4_clocking_input_sampling.sv"),
        "rcb",
        Order::Exact,
        &[
            "T|r1|t=5000 cb.d=0 cb.dl=0 d=1",
            "T|r1|t=15000 cb.d=1 cb.dl=1 d=2",
            "T|r1|t=25000 cb.d=2 cb.dl=2 d=3",
            "T|r1|t=35000 cb.d=3 cb.dl=3 d=4",
            "T|r2|t=45000 cb.d=3 d=4",
            "T|r2|t=55000 cb.d=4 d=5",
            "T|r3|t=56000 dout=xx",
            "T|r4|t=58000 dout=ab",
        ],
        &[],
    );
}

// §14.3, §14.4, §14.11, §14.12, §14.16: clocking blocks: skews, cycle delays, default clocking, ## drives
#[test]
fn c14_clocking_blocks() {
    check_both(
        "c14_clk",
        include_str!("sv/c14_clk.sv"),
        "c14",
        Order::Exact,
        &[
            "T|14.13a|t=5000 cb.d=0 d=1",
            "T|14.13b|t=15000 cb.d=1 d=2 cb.dl=1",
            "T|14.16a|t=15000 dout=xx",
            "T|14.16b|t=16000 dout=xx",
            "T|14.16c|t=18000 dout=ab",
            "T|14.11a|t=35000",
            "T|14.11b|t=35000",
            "T|14.16d|t=35000 dout=ab",
            "T|14.16e|t=45000 dout=ab",
            "T|14.16f|t=58000 dout=cd",
            "T|14.4|t=60000 d0=6 d=6",
            "T|14.12|t=65000 dcb.d=6",
            "T|14.13c|t=95000 n=3",
            "T|14.16g|t=108000 dout=11",
        ],
        &[],
    );
}

// §14.3, §14.4: an edge output skew drives on that edge of the clock after the
// clocking event.
#[test]
fn c14_4_clocking_edge_output_skew() {
    check_both(
        "c14_edge_skew",
        include_str!("sv/c14_edge_skew.sv"),
        "c14e",
        Order::Exact,
        &[
            "T|edge|t=14000 u=0 d=0",
            "T|edge|t=18000 u=0 d=1",
            "T|edge|uz rose t=20000",
            "T|edge|t=21000 u=1 d=1",
        ],
        &[],
    );
}

// §14.16: drives at, after `##1`, and between clocking events through a
// negedge skew.
fn negedge_skew_timing(mode: ClockingDrive, expected: &[&str]) {
    check_clocking(
        mode,
        "c14_negedge_timing",
        include_str!("sv/c14_negedge_timing.sv"),
        "c14nt",
        Order::Multiset,
        expected,
        &[],
    );
}

#[test]
fn c14_16_clocking_negedge_skew_timing() {
    negedge_skew_timing(
        Lrm,
        &[
            "T|chg|t=20000 u=1",
            "T|chg|t=30000 w=1",
            "T|chg|t=30000 v=1",
        ],
    );
}

// `next-edge`: the drive between events at 17 lands on the negedge at 20.
#[test]
fn c14_16_clocking_negedge_skew_timing_next_edge() {
    negedge_skew_timing(
        NextEdge,
        &[
            "T|chg|t=20000 u=1",
            "T|chg|t=30000 w=1",
            "T|chg|t=20000 v=1",
        ],
    );
}

// §14.16 / §14.16.2: a between-event drive and the next event's drive mature
// together and the last one wins.
fn offedge_negedge(mode: ClockingDrive, expected: &[&str]) {
    check_clocking(
        mode,
        "c14_offedge_negedge",
        include_str!("sv/c14_offedge_negedge.sv"),
        "c14on",
        Order::Exact,
        expected,
        &[],
    );
}

#[test]
fn c14_16_2_clocking_offedge_negedge_last_wins() {
    offedge_negedge(
        Lrm,
        &[
            "T|pos|t=5000 branch=0",
            "T|pos|t=15000 branch=0",
            "T|pos|t=25000 branch=0",
            "T|pos|t=35000 branch=0",
        ],
    );
}

// `next-edge`: the drive at 5.001 lands on the negedge at 10, and the drive
// at the event at 15 on the negedge at 20.
#[test]
fn c14_16_2_clocking_offedge_negedge_next_edge() {
    offedge_negedge(
        NextEdge,
        &[
            "T|pos|t=5000 branch=0",
            "T|chg|t=10000 branch=1",
            "T|pos|t=15000 branch=1",
            "T|chg|t=20000 branch=0",
            "T|pos|t=25000 branch=0",
            "T|pos|t=35000 branch=0",
        ],
    );
}

// §14.16.2: of two drives maturing together, only the last reaches the
// output, so a 1-then-0 pair never wakes the output's waiters. Held,
// `##1`-matured and `#0`-skew drives alike.
fn lastwins_no_glitch(mode: ClockingDrive, expected: &[&str]) {
    check_clocking(
        mode,
        "c14_lastwins_glitch",
        include_str!("sv/c14_lastwins_glitch.sv"),
        "c14lg",
        Order::Multiset,
        expected,
        &[],
    );
}

#[test]
fn c14_16_2_clocking_last_wins_no_glitch() {
    lastwins_no_glitch(Lrm, &["T|end a=0 b=0 c=0 d=0"]);
}

// `next-edge`: a's two drives land on different negedges (10 and 20), so a
// pulses; b's both land at 10, c's and d's as with `lrm`.
#[test]
fn c14_16_2_clocking_last_wins_no_glitch_next_edge() {
    lastwins_no_glitch(
        NextEdge,
        &[
            "T|wake|t=10000 a=1",
            "T|rise|t=10000 a",
            "T|wake|t=20000 a=0",
            "T|end a=0 b=0 c=0 d=0",
        ],
    );
}
