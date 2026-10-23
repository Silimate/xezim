#!/bin/sh
# Runs an unpacked release on a bare EL8 userland (release/run.sh smoke): the
# binary starts, simulates, and serves MCP when it has the server.
set -eu
x=$1/bin/xezim
"$x" --version
cd "$(mktemp -d)"
printf 'module t;\n  initial begin\n    $display("smoke %%0d", 6 * 7);\n    $finish;\n  end\nendmodule\n' > t.sv
"$x" t.sv > out.txt
cat out.txt
grep -q 'smoke 42' out.txt
if "$x" --help 2>&1 | grep -q -- '--mcp'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"simulate","arguments":{"sources":["t.sv"]}}}' |
    "$x" --mcp > mcp.txt
  grep -q '"passed":true' mcp.txt
fi
test -f "$1/share/doc/xezim/THIRD_PARTY_NOTICES"
test -f "$1/share/xezim/cocotb/xezim_runner.py"
echo 'smoke test passed'
