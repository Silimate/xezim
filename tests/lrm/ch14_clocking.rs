//! IEEE 1800-2023 clause 14: clocking blocks.

use crate::harness::{Order, check};

// §14.10: clocking block events
#[test]
fn c14_10_clocking_block_events() {
    check(
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
    check(
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
    check(
        "14.16_clocking_inout_iface_net",
        include_str!("sv/14.16_clocking_inout_iface_net.sv"),
        "rvd",
        Order::Exact,
        &["T|r1|t=8 req=11 bidir=5a", "T|r2|t=18 bidir=77"],
        &[],
    );
}

// §14.3, §14.5: clockvars bound to expressions in interface instances
#[test]
fn c14_5_clocking_expr_iface() {
    check(
        "14.5_clocking_expr_iface",
        include_str!("sv/14.5_clocking_expr_iface.sv"),
        "top",
        Order::Exact,
        &["T|q|u1=1 u2=0", "T|d|u1=1 u2=0"],
        &[],
    );
}

// §14.4: Output skew (default output #2)
#[test]
fn c14_16_clocking_output_skew() {
    check(
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

// §14.5: a clockvar bound to a part-select samples and drives that slice
#[test]
fn c14_5_clockvar_slice_binding() {
    check(
        "14.5_clockvar_slice_binding",
        include_str!("sv/14.5_clockvar_slice_binding.sv"),
        "c145s",
        Order::Exact,
        &[
            "T|14.5|in a=a hi=5",
            "T|14.5|out o_drv=09",
            "T|14.5|out2 o_drv=c9",
            "T|14.5|stale a=a",
            "T|14.5|fresh a=3 hi=c",
        ],
        &[],
    );
}

// §14.16.2: drives that mature in one time step apply in issue order: a whole
// drive replaces an earlier select of the same output, whatever unrelated
// drive was issued between them
#[test]
fn c14_16_clocking_interleaved_drives() {
    check(
        "14.16_clocking_interleaved_drives",
        include_str!("sv/14.16_clocking_interleaved_drives.sv"),
        "c14ic",
        Order::Exact,
        &["T|a|t=16000 o1=1111 o2=0000", "T|b|t=18000 o1=0000 o2=1010"],
        &[],
    );
}

// §14.16.1, §14.16.2: a select issued at the clocking event applies on top of
// the whole drive queued for that edge
#[test]
fn c14_16_clocking_event_select_after_whole() {
    check(
        "14.16_clocking_event_select_after_whole",
        include_str!("sv/14.16_clocking_event_select_after_whole.sv"),
        "c14ew",
        Order::Exact,
        &["T|a|t=6000 q=0000", "T|b|t=8000 q=1011"],
        &[],
    );
}

// §14.16.2: inertial CA must drop a pending clocking select mask
#[test]
fn c14_16_clocking_mask_ca_replace() {
    check(
        "14.16_clocking_mask_ca_replace",
        include_str!("sv/14.16_clocking_mask_ca_replace.sv"),
        "c14mca",
        Order::Exact,
        &["T|a|t=7000 q=1110", "T|b|t=9000 q=1110"],
        &[],
    );
}

// §14.16.2: a select drive changes only its bits, so a bit written during the
// skew window survives
#[test]
fn c14_16_clocking_select_keeps_other_bits() {
    check(
        "14.16_clocking_select_keeps_other_bits",
        include_str!("sv/14.16_clocking_select_keeps_other_bits.sv"),
        "c14kb",
        Order::Exact,
        &["T|a|t=7000 q=1000", "T|b|t=9000 q=1001"],
        &[],
    );
}

// §14.4, §14.16.2: select drives through aliases of one net with different
// output skews each mature at their own skew
#[test]
fn c14_16_clocking_alias_skews() {
    check(
        "14.16_clocking_alias_skews",
        include_str!("sv/14.16_clocking_alias_skews.sv"),
        "c14as",
        Order::Exact,
        &["T|a|t=6000 q=00", "T|b|t=8000 q=10", "T|c|t=10000 q=11"],
        &[],
    );
}

// §14.16, §14.16.2: a bit-select or slice of a clockvar is driven after the output skew,
// and a later drive in the same cycle overrides only the bits it selects
#[test]
fn c14_16_clocking_select_drive() {
    check(
        "14.16_clocking_select_drive",
        include_str!("sv/14.16_clocking_select_drive.sv"),
        "csel",
        Order::Exact,
        &[
            "T|a|t=16000 nib=0000 asc=0000 o1=0000 o2=0000",
            "T|b|t=18000 nib=0111 asc=1100 o1=1011 o2=1010",
            "T|c|t=26000 nib=0111",
            "T|d|t=28000 nib=1111",
            "T|e|t=36000 o1=1011",
            "T|f|t=38000 o1=1111",
        ],
        &[],
    );
}

// §14.4, §14.16: the output skew of a renamed clockvar applies to drives
// issued between clocking events
#[test]
fn c14_4_renamed_clockvar_output_skew() {
    check(
        "14.4_renamed_clockvar_output_skew",
        include_str!("sv/14.4_renamed_clockvar_output_skew.sv"),
        "c144r",
        Order::Exact,
        &["T|14.4|q t=17", "T|14.4|r t=28"],
        &[],
    );
}

// §14.3: clocking block inout signals
#[test]
fn c14_3_clocking_inout_signals() {
    check(
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
    check(
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
    check(
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
    check(
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
// negedge skew, a renamed posedge-skew output and a two-state output.
#[test]
fn c14_16_clocking_negedge_skew_timing() {
    check(
        "c14_negedge_timing",
        include_str!("sv/c14_negedge_timing.sv"),
        "c14nt",
        Order::Multiset,
        &[
            "T|chg|t=20000 u=1",
            "T|chg|t=30000 w=1",
            "T|chg|t=30000 v=1",
            "T|chg|t=35000 qn=1",
            "T|chg|t=25000 b=0",
        ],
        &[],
    );
}

// §14.16 / §14.16.2: a between-event drive and the next event's drive mature
// together and the last one wins.
#[test]
fn c14_16_2_clocking_offedge_negedge_last_wins() {
    check(
        "c14_offedge_negedge",
        include_str!("sv/c14_offedge_negedge.sv"),
        "c14on",
        Order::Exact,
        &[
            "T|pos|t=5000 branch=0",
            "T|pos|t=15000 branch=0",
            "T|pos|t=25000 branch=0",
            "T|pos|t=35000 branch=0",
        ],
        &[],
    );
}

// §14.16.2: of two drives maturing together, only the last reaches the
// output, so a 1-then-0 pair never wakes the output's waiters. Held,
// `##1`-matured and `#0`-skew drives alike.
#[test]
fn c14_16_2_clocking_last_wins_no_glitch() {
    check(
        "c14_lastwins_glitch",
        include_str!("sv/c14_lastwins_glitch.sv"),
        "c14lg",
        Order::Multiset,
        &["T|end a=0 b=0 c=0 d=0"],
        &[],
    );
}

// §14.4, §14.16: an edge skew applies to a selected output and to an output
// whose net another clockvar names, and edge-plus-delay drives made a cycle
// apart each reach the net.
#[test]
fn c14_4_clocking_edge_skew_select_alias_and_delay() {
    check(
        "c14_edge_select_alias",
        include_str!("sv/c14_edge_select_alias.sv"),
        "c14es",
        Order::Multiset,
        &[
            "T|chg|t=20000 q=1",
            "T|chg|t=20000 sel=01",
            "T|chg|t=25000 t=1",
            "T|chg|t=25000 w=01",
            "T|chg|t=35000 t=0",
        ],
        &[],
    );
}
