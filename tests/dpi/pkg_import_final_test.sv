// §9.2.3 / §26.3 / §35.5.4: a final procedure calls a package's DPI imports
// by their `pkg::` name, as a statement and in an expression.
package pif;
  import "DPI-C" function void pif_mark();
  import "DPI-C" function int pif_calls();
endpackage
module pkg_import_final;
  initial begin
    #5 pif::pif_mark();
    $finish;
  end
  final begin
    int n;
    pif::pif_mark();
    n = pif::pif_calls();
    if (n == 2) $display("TEST_PASS"); else $display("TEST_FAIL calls=%0d", n);
  end
endmodule
