//! IEEE 1800-2023 §26.3: a name that two wildcard-imported packages both
//! declare is not visible through either import. CORE-ET's `neigh_fill_ff`
//! imports `minion_pkg::*` and `minion_dcache_pkg::*` and casts with
//! `et_link_size_e'(...)`, a typedef both packages declare. The error names
//! both packages, and `--relax-wildcard-import-conflict` (or
//! `XEZIM_RELAX_WILDCARD_IMPORT_CONFLICT=1`) resolves the name to the first
//! of those imports in source order, with a warning, as the simulators
//! that accept these sources do; the relaxed values are theirs. The
//! reference simulator rejects them.
use std::process::Command;

fn run(source: &str, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "xezim_wildcard_conflict_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("repro.sv");
    std::fs::write(&path, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["-s", "top", "--no-cache"])
        .args(args)
        .arg(&path)
        .env_remove("XEZIM_RELAX_WILDCARD_IMPORT_CONFLICT")
        .envs(env.iter().copied())
        .output()
        .expect("run xezim");
    let _ = std::fs::remove_dir_all(&dir);
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

const PKGS: &str = "package p1;\n\
  typedef enum logic [2:0] { SZ_A = 3'd1, SZ_B = 3'd2 } size_e;\n\
  parameter int P = 1;\n\
  function automatic int f(int g); return g + 1; endfunction\n\
endpackage\n\
package p2;\n\
  typedef enum logic [3:0] { SZ_C = 4'd5, SZ_D = 4'd6 } size_e;\n\
  parameter int P = 2;\n\
  function automatic int f(int g); return g + 2; endfunction\n\
endpackage\n";

fn module(imports: &str, body: &str) -> String {
    format!("{PKGS}module top;\n  {imports}\n  logic [3:0] raw = 4'd14;\n{body}\nendmodule\n")
}

const CAST: &str = "  initial $display(\"R|%0d\", $bits(size_e'(raw)));";
const DECL: &str =
    "  size_e s;\n  initial begin s = size_e'(raw); $display(\"R|%0d %0d\", s, $bits(s)); end";
const VALUES: &str = "  initial $display(\"R|%0d %0d\", P, f(10));";

/// The CORE-ET shape: a cast to the type is an error that names both
/// packages, not an undeclared identifier.
#[test]
fn ambiguous_names_are_errors_that_name_both_packages() {
    for (body, what) in [
        (CAST, "type 'size_e'"),
        (DECL, "type 'size_e'"),
        (VALUES, "'P'"),
    ] {
        let (ok, text) = run(&module("import p1::*; import p2::*;", body), &[], &[]);
        assert!(!ok, "accepted:\n{text}");
        for want in [
            what,
            "('p1' and 'p2', all imported into",
            "§26.3",
            "--relax-wildcard-import-conflict",
        ] {
            assert!(text.contains(want), "missing {want:?}:\n{text}");
        }
        assert!(!text.contains("Undeclared identifier"), "{text}");
    }
}

/// With the switch, or its environment variable, the name is the first
/// wildcard import's declaration, in source order, with a warning that
/// names both packages.
#[test]
fn the_switch_takes_the_first_wildcard_import() {
    let cases = [
        ("import p1::*; import p2::*;", CAST, "R|3", "p1"),
        ("import p2::*; import p1::*;", CAST, "R|4", "p2"),
        ("import p1::*; import p2::*;", DECL, "R|6 3", "p1"),
        ("import p2::*; import p1::*;", DECL, "R|14 4", "p2"),
        ("import p1::*; import p2::*;", VALUES, "R|1 11", "p1"),
        ("import p2::*; import p1::*;", VALUES, "R|2 12", "p2"),
    ];
    for (imports, body, want, first) in cases {
        let src = module(imports, body);
        for (args, env) in [
            (&["--relax-wildcard-import-conflict"][..], &[][..]),
            (
                &[][..],
                &[("XEZIM_RELAX_WILDCARD_IMPORT_CONFLICT", "1")][..],
            ),
        ] {
            let (ok, text) = run(&src, args, env);
            assert!(ok && text.contains(want), "{imports} {want}:\n{text}");
            let warning = format!("using '{first}::");
            assert!(
                text.contains(&warning)
                    && text.contains("--relax-wildcard-import-conflict is in effect"),
                "{imports}: no warning naming {first}:\n{text}"
            );
        }
    }
}

/// An enum member is the same import's member in a constant (a localparam)
/// as at run time.
#[test]
fn the_switch_resolves_constant_enum_members_alike() {
    for (imports, want) in [
        ("import a::*; import b::*;", "R|1 1"),
        ("import b::*; import a::*;", "R|4 4"),
    ] {
        let src = format!(
            "package a; typedef enum int {{ N = 1 }} ea; endpackage\n\
             package b; typedef enum int {{ N = 4 }} eb; endpackage\n\
             module top; {imports}\n  localparam int K = N;\n\
             \x20 initial $display(\"R|%0d %0d\", K, N);\nendmodule\n"
        );
        let (ok, text) = run(&src, &["--relax-wildcard-import-conflict"], &[]);
        assert!(ok && text.contains(want), "{imports} {want}:\n{text}");
    }
}

/// An explicit import or a local declaration still decides the name, with
/// no warning; the switch only picks among wildcard imports (§26.3).
/// (A simulator that accepts the ambiguous sources takes the first wildcard
/// import's `size_e` here, 3 bits, over the explicit `import p2::size_e;`.)
#[test]
fn explicit_imports_and_local_declarations_still_win() {
    let src = module(
        "import p1::*; import p2::*; import p2::size_e;",
        "  localparam int P = 7;\n  initial $display(\"R|%0d %0d\", $bits(size_e'(raw)), P);",
    );
    let (ok, text) = run(&src, &["--relax-wildcard-import-conflict"], &[]);
    assert!(ok && text.contains("R|4 7"), "{text}");
    assert!(!text.contains("[xezim][warning] 'size_e'"), "{text}");
}
