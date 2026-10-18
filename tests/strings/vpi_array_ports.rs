//! IEEE 1800-2017 §23.3.3.7: a port connected to a net is one net with it,
//! for an unpacked-array port as for a scalar one. A value a VPI client
//! writes to an element of an instance's array port is what the instance
//! reads, and the actual's name reads it too; a value written to the actual
//! reaches the port's name. xezim gave the port's elements storage of their
//! own, which the instance never read (it reads the actual), so writes to
//! `u.a[i]` were lost: a cocotb test driving `dut.inst.port[i]` left the
//! instance reading z.
//!
//! tests/dpi/vpi_array_ports.c writes through one, two and three port
//! levels, to wide, one-bit, two-dimensional and descending arrays, to a
//! whole port with `vpi_put_value_array`, to the actual of a port, and to
//! ports connected to variables, then reads every element back. The expected
//! lines are the reference simulator's with the ports of net kind, as
//! §23.2.2.3 declares an `input logic` port.

use std::path::{Path, PathBuf};
use std::process::Command;

fn setup() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("xezim_vpi_array_ports_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let so = dir.join("vpi_array_ports.so");
    let ok = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Werror", "-I"])
        .arg(root.join("include"))
        .arg(root.join("tests/dpi/vpi_array_ports.c"))
        .arg("-o")
        .arg(&so)
        .status()
        .expect("failed to launch cc")
        .success();
    assert!(ok, "cc failed for vpi_array_ports.c");
    (dir, so)
}

fn run(dir: &Path, so: &Path, env: &[(&str, &str)]) -> String {
    let sv = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/dpi/vpi_array_ports.sv");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xezim"));
    cmd.arg("--vpi-lib")
        .arg(so)
        .args(["--no-cache", "-s", "top"])
        .arg(sv)
        .current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("failed to run xezim");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "xezim failed:\n{text}");
    assert!(!text.contains("FAIL:"), "VPI check failed:\n{text}");
    text
}

const EXPECTED: &[&str] = &[
    "T|one level|10 20",
    "T|three levels|30 40",
    "T|wide|123456789abcdef0123456789 fedcba9876543210fedcba987",
    "T|one bit|1011",
    "T|parent write|50 60",
    "T|whole array|70 71",
    "T|two dims|21 12",
    "T|descending|43 40",
    "T|variable 8|77 88",
    "T|variable 64|0123456789abcdef fedcba9876543210",
    "T|variable 1|11",
    "T|names|10 40 40 30 60",
    "CHK|top.u1.a[0]",
    "CHK|top.a8[1]",
    "CHK|top.o.m.l.a[1]",
    "CHK|top.o.m.a[0]",
    "CHK|top.o.a[1]",
    "CHK|top.c8[0]",
    "CHK|top.uw.a[0]",
    "CHK|top.w[1]",
    "CHK|top.us.a[2]",
    "CHK|top.s1[3]",
    "CHK|top.ur.a[0]",
    "CHK|top.ud.a[0]",
    "CHK|top.d8[1]",
    "CHK|top.ug.a[0][1]",
    "CHK|top.g8[1][2]",
    "CHK|top.un.a[0]",
    "CHK|top.n8[3]",
    "CHK|top.um8.a[5]",
    "CHK|top.um64.a[3]",
    "CHK|top.um1.a[5]",
];

fn expect(text: &str) {
    for line in EXPECTED {
        assert!(text.contains(line), "missing `{line}`:\n{text}");
    }
}

#[test]
fn vpi_writes_reach_unpacked_array_ports() {
    let (dir, so) = setup();
    // Element names stored, as for arrays of this size by default.
    expect(&run(&dir, &so, &[]));
    // Virtual element names (computed from the array's first cell).
    expect(&run(&dir, &so, &[("XEZIM_VIRTUAL_NAME_MIN_CELLS", "0")]));
    // No element names for arrays over four elements: the ports of `peek`
    // reach their cells through `array_first_id` alone, and the variables
    // they connect to live in the packed memory arena. Elements are looked
    // up by name (vpi_handle_by_index needs element names).
    let packed = run(
        &dir,
        &so,
        &[
            ("XEZIM_LARGE_ARRAY_NAME_THRESHOLD", "4"),
            ("XEZIM_PACKED_MEM", "1"),
            ("AP_BY_NAME", "1"),
            ("XEZIM_MEM_CENSUS", "1"),
        ],
    );
    let _ = std::fs::remove_dir_all(&dir);
    expect(&packed);
    assert!(
        packed.contains("[PACKED-MEM] arrays=3 cells=24"),
        "the variables are not in the packed arena:\n{packed}"
    );
}
