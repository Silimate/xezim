#!/bin/sh
# Runs an unpacked release on a bare EL8 userland (release/run.sh smoke): the
# binary starts and simulates a design, and the release holds its notices and
# the cocotb runner.
set -eu
x=$1/bin/xezim
"$x" --version
cd "$(mktemp -d)"
printf 'module t;\n  initial begin\n    $display("smoke %%0d", 6 * 7);\n    $finish;\n  end\nendmodule\n' > t.sv
"$x" t.sv > out.txt
cat out.txt
grep -q 'smoke 42' out.txt
test -f "$1/share/doc/xezim/THIRD_PARTY_NOTICES"
test -f "$1/share/xezim/cocotb/xezim_runner.py"
echo 'smoke test passed'
