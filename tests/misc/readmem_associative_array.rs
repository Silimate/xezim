//! Issue #301: `$readmemh`/`$readmemb` into an associative array loaded
//! nothing and reported `'mem' does not name an unpacked memory`. IEEE
//! 1800-2023 §21.4 allows the memory to be an associative array whose index
//! type is integral; each address is used as the key. §21.5 lets
//! `$writememh`/`$writememb` dump one: the elements that exist, in index
//! order. The VeeR EH1/EH2/EL2 testbenches load their program this way
//! (`bit [7:0] mem [bit [31:0]]` in an instance, `$readmemh(.., lmem.mem)`).
//! The reference simulator and Verilator load the reproducer.

use xezim::simulate;

fn t_lines(src: &str) -> Vec<String> {
    simulate(src, 100)
        .expect("simulate")
        .output
        .iter()
        .map(|o| o.message.trim_end().to_string())
        .filter(|l| l.starts_with("T|") || l.contains("[xezim]"))
        .collect()
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xezim_rmaa_{}_{}", tag, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The issue's reproducer: a local associative array, one in another
/// instance (`lmem.mem`), and an unpacked memory alongside.
#[test]
fn readmem_into_associative_array_reproducer() {
    let dir = scratch("repro");
    let hex = dir.join("p.hex");
    std::fs::write(&hex, "@10\n11 22 33\n@20\n44\n").unwrap();
    let src = r#"
module m; bit [7:0] mem [bit [31:0]]; endmodule
module u; bit [7:0] arr [0:63]; endmodule
module t;
  m lmem ();
  u umem ();
  bit [7:0] mem [bit [31:0]];
  initial begin
    $readmemh("@HEX@", mem);
    $readmemh("@HEX@", lmem.mem);
    $readmemh("@HEX@", umem.arr);
    if (mem.num() == 4 && mem[32'h10] == 8'h11 && mem[32'h12] == 8'h33 && mem[32'h20] == 8'h44)
      $display("T|PASS local assoc");
    else $display("T|FAIL local assoc num=%0d", mem.num());
    if (lmem.mem.num() == 4 && lmem.mem[32'h20] == 8'h44) $display("T|PASS hier assoc");
    else $display("T|FAIL hier assoc num=%0d", lmem.mem.num());
    if (umem.arr[16] == 8'h11 && umem.arr[32] == 8'h44) $display("T|PASS hier unpacked");
    else $display("T|FAIL hier unpacked");
  end
endmodule
"#
    .replace("@HEX@", &hex.display().to_string());
    let got = t_lines(&src);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        got,
        [
            "T|PASS local assoc",
            "T|PASS hier assoc",
            "T|PASS hier unpacked"
        ]
    );
}

/// §21.4 addressing: the file's own addresses; address 0 when the file has
/// none (keys already present are kept); a start address; start and finish
/// addresses, ascending and descending; `$readmemb`; a signed and an 8-bit
/// index type; a member-access target inside a task. §21.5: `$writememh`
/// round-trips, whole and over an address range.
#[test]
fn readmem_writemem_associative_array_forms() {
    let dir = scratch("forms");
    let a = dir.join("a.hex");
    let b = dir.join("b.hex");
    let c = dir.join("c.mem");
    let w = dir.join("w.hex");
    let w2 = dir.join("w2.hex");
    std::fs::write(&a, "@10\n11 22 33\n@20\n44\n").unwrap();
    std::fs::write(&b, "aa\nbb\ncc\n").unwrap();
    std::fs::write(&c, "@3\n1010\n0101\n").unwrap();
    let src = r#"
module m;
  bit [7:0] mem [bit [31:0]];
endmodule
module top;
  m lmem ();
  bit   [7:0] la [bit [31:0]];
  logic [7:0] nb [int];
  bit   [7:0] st [bit [31:0]];
  bit   [7:0] se [bit [31:0]];
  bit   [7:0] ds [bit [31:0]];
  bit   [3:0] bb [byte unsigned];
  bit   [7:0] rb [bit [31:0]];
  bit   [7:0] rr [bit [31:0]];
  task load;
    $readmemh("@A@", lmem.mem);
  endtask
  initial begin
    $readmemh("@A@", la);
    $display("T|local num=%0d [10]=%h [11]=%h [12]=%h [20]=%h", la.num(), la[32'h10], la[32'h11], la[32'h12], la[32'h20]);
    load();
    $display("T|task num=%0d [10]=%h [20]=%h", lmem.mem.num(), lmem.mem[32'h10], lmem.mem[32'h20]);
    nb[100] = 8'h77;
    $readmemh("@B@", nb);
    $display("T|noaddr num=%0d [0]=%h [1]=%h [2]=%h [100]=%h", nb.num(), nb[0], nb[1], nb[2], nb[100]);
    $readmemh("@B@", st, 5);
    $display("T|start num=%0d [5]=%h [7]=%h", st.num(), st[5], st[7]);
    $readmemh("@B@", se, 5, 6);
    $display("T|range num=%0d [5]=%h [6]=%h exists7=%0d", se.num(), se[5], se[6], se.exists(7));
    $readmemh("@B@", ds, 7, 5);
    $display("T|desc num=%0d [7]=%h [6]=%h [5]=%h", ds.num(), ds[7], ds[6], ds[5]);
    $readmemb("@C@", bb);
    $display("T|bin num=%0d [3]=%h [4]=%h", bb.num(), bb[3], bb[4]);
    $writememh("@W@", la);
    $readmemh("@W@", rb);
    $display("T|writemem num=%0d [10]=%h [11]=%h [12]=%h [20]=%h", rb.num(), rb[32'h10], rb[32'h11], rb[32'h12], rb[32'h20]);
    $writememh("@W2@", la, 32'h11, 32'h20);
    $readmemh("@W2@", rr);
    $display("T|writemem-range num=%0d exists10=%0d [11]=%h [20]=%h", rr.num(), rr.exists(32'h10), rr[32'h11], rr[32'h20]);
  end
endmodule
"#
    .replace("@A@", &a.display().to_string())
    .replace("@B@", &b.display().to_string())
    .replace("@C@", &c.display().to_string())
    .replace("@W2@", &w2.display().to_string())
    .replace("@W@", &w.display().to_string());
    let got = t_lines(&src);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        got,
        [
            "T|local num=4 [10]=11 [11]=22 [12]=33 [20]=44",
            "T|task num=4 [10]=11 [20]=44",
            "T|noaddr num=4 [0]=aa [1]=bb [2]=cc [100]=77",
            "T|start num=3 [5]=aa [7]=cc",
            "T|range num=2 [5]=aa [6]=bb exists7=0",
            "T|desc num=3 [7]=aa [6]=bb [5]=cc",
            "T|bin num=2 [3]=a [4]=5",
            "T|writemem num=4 [10]=11 [11]=22 [12]=33 [20]=44",
            "T|writemem-range num=3 exists10=0 [11]=22 [20]=44",
        ],
    );
}

/// §21.4 asks for an integral index type: a string-indexed array is still
/// no memory.
#[test]
fn readmem_rejects_string_indexed_array() {
    let dir = scratch("str");
    let hex = dir.join("p.hex");
    std::fs::write(&hex, "11\n").unwrap();
    let src = r#"
module t;
  bit [7:0] sm [string];
  initial begin
    $readmemh("@HEX@", sm);
    $display("T|num=%0d", sm.num());
  end
endmodule
"#
    .replace("@HEX@", &hex.display().to_string());
    let got = t_lines(&src);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        got,
        [
            "[xezim][error] $readmemh: 'sm' does not name an unpacked memory (t=0)",
            "T|num=0",
        ]
    );
}

/// Keys outside `0..=i64::MAX` keep their value through a dump and a load
/// (§21.4, §21.5, §7.8): a negative key of an `int` index is written as its
/// 64-bit two's complement and read back as negative (an `int`-width
/// `@ffffffff` reads as -1 too), and a `bit [63:0]` key above `i64::MAX`
/// keeps its value in the file, in a start address and in the default
/// finish address. Negative indices of an unpacked memory round-trip the
/// same way.
#[test]
fn readmem_writemem_keys_beyond_i64_range() {
    let dir = scratch("wide");
    let f = |n: &str| dir.join(n).display().to_string();
    std::fs::write(dir.join("i.hex"), "@ffffffff\n5a\n").unwrap();
    std::fs::write(dir.join("u.hex"), "@8000000000000000\naa bb\n").unwrap();
    std::fs::write(dir.join("n.hex"), "aa bb cc\n").unwrap();
    let src = r#"
module t;
  bit [7:0] sm [int];
  bit [7:0] sr [int];
  bit [7:0] si [int];
  bit [7:0] um [bit [63:0]];
  bit [7:0] ur [bit [63:0]];
  bit [7:0] uf [bit [63:0]];
  bit [7:0] us [longint unsigned];
  bit [7:0] pm [-2:1];
  bit [7:0] pr [-2:1];
  initial begin
    sm[-1] = 8'h11; sm[-2] = 8'h22; sm[5] = 8'h33;
    $writememh("@SW@", sm);
    $readmemh("@SW@", sr);
    $display("T|signed num=%0d [-2]=%h [-1]=%h [5]=%h exists0=%0d", sr.num(), sr[-2], sr[-1], sr[5], sr.exists(0));
    $readmemh("@I@", si);
    $display("T|int-width num=%0d [-1]=%h", si.num(), si[-1]);
    um[64'h8000_0000_0000_0000] = 8'h44; um[64'hffff_ffff_ffff_fffe] = 8'h55; um[1] = 8'h66;
    $writememh("@UW@", um);
    $readmemh("@UW@", ur);
    $display("T|unsigned num=%0d [1]=%h [8000..]=%h [ff..fe]=%h exists0=%0d", ur.num(), ur[1],
             ur[64'h8000_0000_0000_0000], ur[64'hffff_ffff_ffff_fffe], ur.exists(0));
    $readmemh("@U@", uf);
    $display("T|unsigned-file num=%0d [8000..]=%h [8000..01]=%h", uf.num(),
             uf[64'h8000_0000_0000_0000], uf[64'h8000_0000_0000_0001]);
    $readmemh("@B@", us, 64'hffff_ffff_ffff_fffe);
    $display("T|unsigned-start num=%0d [ff..fe]=%h [ff..ff]=%h", us.num(),
             us[64'hffff_ffff_ffff_fffe], us[64'hffff_ffff_ffff_ffff]);
    pm = '{8'h01, 8'h02, 8'h03, 8'h04};
    $writememh("@PW@", pm, -1, 1);
    $readmemh("@PW@", pr);
    $display("T|unpacked [-2]=%h [-1]=%h [0]=%h [1]=%h", pr[-2], pr[-1], pr[0], pr[1]);
  end
endmodule
"#
    .replace("@SW@", &f("sw.hex"))
    .replace("@UW@", &f("uw.hex"))
    .replace("@PW@", &f("pw.hex"))
    .replace("@I@", &f("i.hex"))
    .replace("@U@", &f("u.hex"))
    .replace("@B@", &f("n.hex"));
    let got = t_lines(&src);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        got,
        [
            "T|signed num=3 [-2]=22 [-1]=11 [5]=33 exists0=0",
            "T|int-width num=1 [-1]=5a",
            "T|unsigned num=3 [1]=66 [8000..]=44 [ff..fe]=55 exists0=0",
            "T|unsigned-file num=2 [8000..]=aa [8000..01]=bb",
            "T|unsigned-start num=2 [ff..fe]=aa [ff..ff]=bb",
            "T|unpacked [-2]=00 [-1]=02 [0]=03 [1]=04",
        ]
    );
}
