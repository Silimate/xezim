//! §11.4.10 / §11.6.1: a shift's left operand has its own declared width,
//! and an assignment to a class property takes the property's width as
//! context. In a compiled class method a `this` property had no known
//! width, so the shift operand was cut to one bit and `imm = imm << 4`
//! stored 0 once the method crossed the compile threshold.

use std::process::Command;

const SOURCE: &str = r#"
class base_c;
  bit [4:0] len;
  bit [31:0] imm;
  virtual function void extend();
    imm = imm << (32 - len);
    imm = imm >> (32 - len);
  endfunction
endclass
class comp_c extends base_c;
  int align;
  virtual function void extend();
    super.extend();
    imm = imm << align;
  endfunction
endclass
class k;
  bit [31:0] imm;
  bit [31:0] other;
  bit [63:0] wide;
  int signed s;
  bit [7:0] narrow;
  int unsigned res;
  function void f_copy();   other = imm << 4; endfunction
  function void f_self();   imm <<= 4; endfunction
  function void f_this();   this.imm = this.imm >> 4; endfunction
  function void f_wide();   res = wide >> 32; endfunction
  function void f_ashr();   s = s >>> 4; endfunction
  function void f_narrow(); res = narrow << 4; endfunction
endclass
class pk #(int W = 16);
  bit [W-1:0] v;
  function void f(); v = v << 4; endfunction
endclass
module top;
  initial begin
    comp_c c;
    base_c b;
    k o;
    pk #(24) p;
    int bad;
    c = new();
    o = new();
    p = new();
    c.len = 8;
    c.align = 2;
    b = c;
    bad = 0;
    for (int i = 0; i < 1200; i++) begin
      b.imm = 32'h13c;
      b.extend();
      if (b.imm != 32'hf0) bad++;
      o.imm = 32'h13c; o.f_copy(); if (o.other != 32'h13c0) bad++;
      o.imm = 32'h13c; o.f_self(); if (o.imm != 32'h13c0) bad++;
      o.imm = 32'h13c; o.f_this(); if (o.imm != 32'h13) bad++;
      o.wide = 64'h1234_5678_9abc_def0; o.f_wide(); if (o.res != 32'h1234_5678) bad++;
      o.s = -256; o.f_ashr(); if (o.s != -16) bad++;
      o.narrow = 8'hff; o.f_narrow(); if (o.res != 32'hff0) bad++;
      p.v = 24'h13c; p.f(); if (p.v != 24'h13c0) bad++;
    end
    if (bad == 0) $display("ALL PASS");
    else $display("FAIL bad=%0d", bad);
    $finish;
  end
endmodule
"#;

fn check(mode: &[(&str, &str)]) {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "xezim_member_shift_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("top.sv");
    std::fs::write(&path, SOURCE).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xezim"))
        .args(["--no-cache", "--relax-implicit-static", "-s", "top"])
        .arg(&path)
        .env_remove("XEZIM_METHOD_TIER")
        .env_remove("XEZIM_COMPILE_METHODS")
        .env_remove("XEZIM_METHOD_CACHE")
        .envs(mode.iter().copied())
        .output()
        .expect("run simulator");
    std::fs::remove_dir_all(&dir).unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains("ALL PASS") && !text.contains("FAIL"),
        "{text}"
    );
}

#[test]
fn member_shift_width_past_the_compile_threshold() {
    check(&[]);
}

#[test]
fn member_shift_width_eager() {
    check(&[("XEZIM_METHOD_TIER", "0")]);
}

#[test]
fn member_shift_width_interpreted() {
    check(&[("XEZIM_COMPILE_METHODS", "0")]);
}
