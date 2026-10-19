//! The reporters' own test cases from GitHub issues #17/#22/#23/#24/#25,
//! run verbatim as regressions (they were previously verified by hand only).
//! Plus two formerly-ORPHANED .sv testcases under tests/ that no cargo suite
//! ever executed: fork_wait_deadlock.sv (passing) and
//! dpi/force_release_compliance.sv (ratcheted — see its test).

use xezim::simulate;

fn outputs(src: &str, max_time: u64) -> Vec<String> {
    let sim = simulate(src, max_time).expect("simulate failed");
    sim.output.iter().map(|o| o.message.clone()).collect()
}

#[test]
fn issue_17_dynamic_array_of_mailboxes() {
    let msgs = outputs(include_str!("../issue_cases/dyn.arr.of.mbox.sv"), 100_000);
    let data_lines = msgs.iter().filter(|m| m.starts_with("Data from")).count();
    assert_eq!(data_lines, 80, "all 5x16 mailbox entries must round-trip");
    assert!(
        !msgs.iter().any(|m| m.contains("[x]")),
        "no unbound foreach index may appear"
    );
}

#[test]
fn issue_17_mailbox_in_interface() {
    let msgs = outputs(
        include_str!("../issue_cases/mbox_in_interface.sv"),
        2_000_000,
    );
    let received = msgs.iter().filter(|m| m.contains("Received")).count();
    // 1000 post-reset cycles: sender0 every 3, sender1 every 5.
    assert_eq!(received, 533, "1000/3 + 1000/5 sender puts must arrive");
}

#[test]
fn issue_22_final_blocks() {
    let msgs = outputs(
        include_str!("../issue_cases/final.blocks.test.case.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST PASSED")), "{:?}", msgs);
    let finals = msgs.iter().filter(|m| m.contains("inal block")).count();
    assert!(finals >= 4, "all four final blocks must run: {:?}", msgs);
}

#[test]
fn issue_23_string_methods() {
    let msgs = outputs(
        include_str!("../issue_cases/string.compliance.tests.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST PASSED")), "{:?}", msgs);
}

#[test]
fn issue_24_swrite_sformat() {
    let msgs = outputs(
        include_str!("../issue_cases/data.to.string.fmt.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST PASSED")), "{:?}", msgs);
}

#[test]
fn issue_25_format_specifiers() {
    let msgs = outputs(include_str!("../issue_cases/fmt.specifiers.sv"), 100_000);
    let pass = msgs.iter().filter(|m| m.starts_with("[PASS]")).count();
    let fail = msgs.iter().filter(|m| m.starts_with("[ERROR]")).count();
    assert_eq!(pass, 42, "passing-check count changed: {:?}", msgs);
    assert_eq!(fail, 0, "all format specifier checks should pass");
}

#[test]
fn orphan_fork_wait_deadlock() {
    let msgs = outputs(include_str!("../fork_wait_deadlock.sv"), 100_000);
    assert!(
        msgs.iter()
            .any(|m| m.contains("PASS: fork-local variable sharing works")),
        "{:?}",
        msgs
    );
}

#[test]
fn fork_child_blocking_live_share() {
    // The UVM sequencer watchdog (`m_safe_select_item`) writes a task-local
    // automatic from a `fork … join_none` child that then blocks forever on
    // `await()`, while the parent parks on `wait(select_process != null)`. A
    // merge-on-child-completion model can never deliver the value (the child
    // never completes) — the write must be LIVE-shared with the suspended
    // parent. This regression compiles that exact child-blocks-forever idiom.
    let msgs = outputs(include_str!("../fork_child_blocking_share.sv"), 100_000);
    assert!(
        msgs.iter()
            .any(|m| m.contains("PASS: fork-child live-shared write wakes a parked wait")),
        "{:?}",
        msgs
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:?}", msgs);
}

#[test]
fn fork_child_merge_wake() {
    // A fork...join_none child's write to the parent's task-local automatic
    // reaches the suspended parent through the child->parent frame merge, and
    // must wake the parked level-sensitive `wait` (§9.4.2) — inside a clock
    // edge's cascade, before that edge's NBA region (reference: seen=0,1).
    let msgs = outputs(include_str!("../fork_child_merge_wake.sv"), 100_000);
    assert!(
        msgs.iter().any(|m| m.contains("PASS result=1 seen=0,1")),
        "{:?}",
        msgs
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:?}", msgs);
}

#[test]
fn orphan_force_release_compliance_ratchet() {
    let msgs = outputs(include_str!("../dpi/force_release_compliance.sv"), 100_000);
    let fails = msgs.iter().filter(|m| m.starts_with("FAIL")).count();
    assert_eq!(
        fails,
        0,
        "force/release known-gap count changed — new regression or a fixed \
         gap (lower the count): {:?}",
        msgs.iter()
            .filter(|m| m.starts_with("FAIL"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn issue_21_timescale_handling() {
    // §3.14.3 precision quantization + per-module directive scales.
    let msgs = outputs(
        include_str!("../issue_cases/timescale.handling.sv"),
        1_000_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST PASSED")), "{:?}", msgs);
}

#[test]
fn issue_18_type_parameters() {
    // §6.20.3 type params: structs, arrays, class handles.
    let msgs = outputs(
        include_str!("../issue_cases/type-parameter-compliance.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST PASSED")), "{:?}", msgs);
}

#[test]
fn issue_28_constraint_foreach() {
    // §18.5.7 foreach constraint bodies beyond `inside`.
    let msgs = outputs(
        include_str!("../issue_cases/constraint.foreach.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST_PASS")), "{:?}", msgs);
}

#[test]
fn issue_29_constraint_typecast() {
    // §18.3/§6.24.1/§11.6.1 casts inside constraint expressions.
    let msgs = outputs(
        include_str!("../issue_cases/constraint.typecast.sv"),
        100_000,
    );
    assert!(msgs.iter().any(|m| m.contains("TEST_PASS")), "{:?}", msgs);
}

#[test]
fn issue_26_static_init_sysfuncs() {
    // §6.21/§10.5 sim-time syscall inits + §20.6 type operands
    // ($size(logic [7:0])). The test reads plusargs, so it needs the
    // args-aware entry point.
    let src = include_str!("../issue_cases/static.init.sysfuncs.sv").to_string();
    let plusargs = vec!["TEST_MODE".to_string(), "SEED_VAL=42".to_string()];
    let sim = xezim::simulate_multi(
        &[src],
        100_000,
        None,
        &[],
        &[],
        None,
        false,
        None,
        None,
        &[],
        &plusargs,
        None,
        &[],
        0,
        u64::MAX,
        None,
        &[],
        None,
        None,
        None,
        None,
        false,
    )
    .expect("simulate failed");
    let msgs: Vec<String> = sim.output.iter().map(|o| o.message.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("TEST_PASS")), "{:?}", msgs);
}

/// Issue #233: the reporter's parenless-call matrix (enum methods on locals,
/// foreach keys and class members; free, package and interface functions;
/// string and queue built-ins), run verbatim.
#[test]
fn issue_233_parenless_calls() {
    let msgs = outputs(include_str!("../issue_cases/parenless.calls.sv"), 100_000);
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// Issue #246: the reporter's randomize()-over-struct-fields MWE, run
/// verbatim — member dist/inside/relational/equality targets, part-selects,
/// nested members, unpacked members, `unique` on a 2-D array, the handle vs
/// method storage views, and the nested rand-handle solve that used to spin
/// for minutes (it must finish well inside the run).
#[test]
fn issue_246_struct_rand_fields() {
    let start = std::time::Instant::now();
    let msgs = outputs(
        include_str!("../issue_cases/struct.rand.fields.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(60),
        "took {:?}",
        start.elapsed()
    );
}

/// The randomize() width-overflow family and dist obedience MWE, run
/// verbatim: `==` / `!=` forcing a value wider than its rand variable, a
/// >64-bit rand struct with member-sum constraints, and the dist-frequency
/// and width-boundary guards around them. `tests/classes/rand_width_overflow.rs`
/// restates each section as its own test.
#[test]
fn rand_width_overflow_and_dist_mwe() {
    let msgs = outputs(include_str!("../issue_cases/rand.width.dist.sv"), 100_000);
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// Issue #262: $fscanf %c must match exactly one character (§21.3.4.3) —
/// no whitespace skipping for %c or literal-only formats, newlines kept
/// across repeated %c, all 256 byte values round-tripping through
/// $fwrite/$fscanf %c, task-form $fscanf converting, and C-stdio pushback
/// semantics ($ftell subtracts, $fread drains, $fseek/$rewind discard).
#[test]
fn issue_262_fscanf_percent_c() {
    let msgs = outputs(include_str!("../issue_cases/fscanf_262.sv"), 100_000);
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// Issue #312: `foreach` over an associative array declared through a package
/// typedef of unpacked-struct elements, run verbatim (§12.7.3, §7.8).
#[test]
fn issue_312_foreach_typedef_assoc_of_structs() {
    let msgs = outputs(
        include_str!("../issue_cases/foreach_aa_typedef_312.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// Issue #313: a static task calling another static task of its class by bare
/// name must block like any task call, so `mailbox.get()` waits for the
/// puts at 10 and 20 (§8.10, §13.3, §15.4), run verbatim.
#[test]
fn issue_313_static_task_calls_block() {
    let msgs = outputs(
        include_str!("../issue_cases/static_task_blocking_313.sv"),
        100_000,
    );
    assert_eq!(
        msgs.iter()
            .filter(|m| m.starts_with("[t=") || m.starts_with("TEST"))
            .cloned()
            .collect::<Vec<_>>(),
        [
            "[t=10] execute_phase(42)",
            "[t=20] execute_phase(43)",
            "[t=20] phase runner finished",
            "TEST PASSED",
        ],
        "{:#?}",
        msgs
    );
}

/// Issue #316: an unpacked dimension sized by a class-scoped localparam
/// (`a[2 * model_c::BEATS]`) gets all 146 elements (§8.23, §7.4.2), run
/// verbatim.
#[test]
fn issue_316_class_scoped_dimension() {
    let msgs = outputs(
        include_str!("../issue_cases/class_scoped_dimension_316.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.contains("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "PASS"), "{:#?}", msgs);
}

/// Issue #317: `@(cb iff cb.sig ...)` judges its guard on the clockvars
/// sampled at that clocking event, so the monitor wakes at 25 on the first
/// beat (§14.13), run verbatim.
#[test]
fn issue_317_clocking_iff_sees_this_edge() {
    let msgs = outputs(
        include_str!("../issue_cases/clocking_iff_sample_317.sv"),
        100_000,
    );
    assert!(
        msgs.iter()
            .any(|m| m == "[25] woke: cb.cs_n=0 cb.data=0xf000"),
        "{:#?}",
        msgs
    );
    assert!(msgs.iter().any(|m| m == "PASS"), "{:#?}", msgs);
}

/// Issue #312 follow-up (reporter's survey, 41 cases): `foreach` and the
/// collection methods over typedef'd associative arrays, dynamic arrays and
/// queues at package, module, class, interface and block scope, every case
/// in its own top module, run verbatim. The reference passes all 41.
#[test]
fn issue_312_survey_multi_module() {
    let msgs = outputs(
        include_str!("../issue_cases/foreach_typedef_312_multi_module.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// The same 41-case survey with every case in one module, run verbatim. The
/// reference passes all 41.
#[test]
fn issue_312_survey_single_module() {
    let msgs = outputs(
        include_str!("../issue_cases/foreach_typedef_312_single_module.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// Issue #390: a queue-of-structs element pushed as a function's struct
/// return (`q.push_back(make(...))`) keeps its string members when the queue
/// is passed to a function, copied to another queue, or iterated with
/// foreach (§7.10.2.2, §13.4.1). One integrated MWE: the issue's shape plus
/// the foreach-formal, queue-copy, multi-element and mixed-sourcing cousins,
/// with working shapes (ref formal, struct var from a call) as controls.
#[test]
fn issue_390_queue_struct_call_elems() {
    let msgs = outputs(
        include_str!("../issue_cases/issue390_queue_struct_call_elems.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}

/// A call is evaluated once even when a deeply nested struct has no leaf
/// layout available (§13.5.1, §7.10.2.2). Expected counts and queue sizes
/// were checked with the reference simulator.
#[test]
fn queue_struct_value_arguments_run_once() {
    let msgs = outputs(
        include_str!("../issue_cases/queue_struct_value_once.sv"),
        100_000,
    );
    let mut rows: Vec<_> = msgs
        .iter()
        .filter(|m| m.starts_with("T|"))
        .cloned()
        .collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            "T|back|1 1",
            "T|conditional|2 2",
            "T|front|1 1",
            "T|insert|1 1",
            "T|method|1 2",
            "T|parenthesized|2 2",
        ],
        "{msgs:#?}",
    );
}

/// A full-word first concatenation part is copied without a shift by 64
/// (§11.4.12). Adjacent widths, nesting and multi-part forms preserve both
/// value and X/Z planes. Expected rows match the reference simulator.
#[test]
fn full_word_concatenation_preserves_value_and_xz_planes() {
    let out = outputs(
        include_str!("../issue_cases/narrow_concat_boundaries.sv"),
        1000,
    );
    let mut rows: Vec<_> = out.into_iter().filter(|m| m.starts_with("T|")).collect();
    rows.sort();
    assert_eq!(
        rows,
        [
            "T|boundaries|x123456789abcdef 10123456789abcdez x123456789abcdezx123456789abcdez",
            "T|known_boundaries|7123456789abcdef 10123456789abcdef fedcba9876543210fedcba9876543210",
            "T|known|fedcba9876543210 fedcba9876543210 fedcba9876543210 fedcba9876543210 fedcba9876543210",
            "T|unknown|x123456789abcdez x123456789abcdez x123456789abcdez x123456789abcdez x123456789abcdez",
        ]
    );
}

/// Issue #391: appending one-character string casts — out = {out, string'(c)}
/// — padded every character with 127 NUL bytes once the function tiered up
/// to bytecode ("HELLO" came back with len 513 = 1 + 4*128). The cast now
/// converts to a string value (§6.16) and the concat takes the §11.4.12
/// string path; bare and prepended casts are covered as cousins.
#[test]
fn issue_391_string_cast_concat_padding() {
    let msgs = outputs(
        include_str!("../issue_cases/issue391_string_cast_concat.sv"),
        100_000,
    );
    assert!(!msgs.iter().any(|m| m.starts_with("FAIL")), "{:#?}", msgs);
    assert!(msgs.iter().any(|m| m == "TEST_PASS"), "{:#?}", msgs);
}
