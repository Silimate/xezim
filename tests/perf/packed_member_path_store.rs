//! §7.2, §7.4.2: a member write through constant element selects of a packed
//! struct (`st.e[1].dw[0].value <= d`) is one static bit range of the
//! container, which elaboration lists in its flattened layout. The compiler
//! sent every such NBA and blocking write to the AST interpreter
//! (`nba_member_access`, `blocking_target_member_access`); a key-vault-shaped
//! design with 384 of them in generate loops ran about 100x slower than its
//! flat-array twin. Expected values are Verilator's (5.046).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn constant_element_member_writes_compile_and_match() {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/perf/packed_member_path_store.sv");
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args([
            "--simulate",
            "-s",
            "packed_member_path_store_check",
            source.to_str().unwrap(),
            "--no-cache",
        ])
        .env("XEZIM_FALLBACK_SITES", "1")
        .output()
        .expect("run packed member path workload");

    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        output.status.success(),
        "packed member path workload failed:\n{text}"
    );
    assert!(
        text.contains("st=23222120 c=2120 a=9a0a1a2 top_st=77556688 rd=22 s2_o=99000000 s2=11")
            && text.contains("PACKED_MEMBER_PATH_OK"),
        "packed member path values did not match:\n{text}"
    );
    assert!(
        !text.contains("member_access"),
        "a member write through element selects fell back to the interpreter:\n{text}"
    );
}
