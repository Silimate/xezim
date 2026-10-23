"""Tests for the cocotb runs of `xezim --mcp` (docs/mcp.md): the simulate,
coverage and assertions tools run a cocotb test module against a top, as
cocotb's own runners do, and report its tests.

Needs cocotb 2, pytest and an xezim build; see test_xezim_runner.py.
"""

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
XEZIM = os.environ.get("XEZIM_BIN", "xezim")


class Server:
    """`xezim --mcp` in HERE, so runner_checks.py is importable."""

    def __init__(self, env):
        self.proc = subprocess.Popen(
            [XEZIM, "--mcp"],
            cwd=HERE,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        self.next = 0
        self.request("initialize", {"protocolVersion": "2025-06-18"})

    def request(self, method, params):
        self.next += 1
        msg = {"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}
        self.proc.stdin.write(json.dumps(msg) + "\n")
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        assert reply["id"] == self.next, reply
        return reply["result"]

    def call(self, name, **args):
        result = self.request("tools/call", {"name": name, "arguments": args})
        assert not result["isError"], result
        return result["structuredContent"]

    def close(self):
        self.proc.stdin.close()
        assert self.proc.wait(timeout=60) == 0


def run(tool, expect, **args):
    """Run runner_checks.py against runner_dut: `expect` maps runner_dut
    outputs to their values."""
    server = Server(dict(os.environ, EXPECT=json.dumps(expect)))
    try:
        cocotb = {"module": "runner_checks", "python": sys.executable}
        cocotb.update(args.pop("cocotb", {}))
        return server.call(
            tool,
            sources=["runner_dut.sv"],
            include_dirs=["include"],
            top="runner_dut",
            cocotb=cocotb,
            **args,
        )
    finally:
        server.close()


def test_passing_test():
    r = run("simulate", {"w": 8, "pa": 9}, parameters=["W=8"], plusargs=["+PA=9"])
    assert r["passed"], r
    assert r["exit_code"] == 0
    tests = r["tests"]
    assert (tests["total"], tests["passed"], tests["failed"]) == (1, 1, 0), r
    assert tests["cases"][0]["name"] == "check"
    assert tests["cases"][0]["module"] == "runner_checks"


def test_failing_test():
    """A failing cocotb test fails the run, though the simulator exits 0."""
    r = run("simulate", {"w": 3}, parameters=["W=8"])
    assert not r["passed"], r
    assert r["tests"]["failed"] == 1, r
    assert r["tests"]["cases"][0]["status"] == "failed"


def test_filter():
    """A filter that matches no test leaves nothing that passed: no pass."""
    r = run("simulate", {"w": 1}, cocotb={"filter": "check"})
    assert r["passed"] and r["tests"]["passed"] == 1, r
    r = run("simulate", {"w": 1}, cocotb={"filter": "nosuch"})
    assert not r["passed"], r
    assert r["tests"]["passed"] == 0, r


def test_missing_cocotb():
    """A Python without cocotb is a tool error that says so."""
    server = Server(dict(os.environ))
    try:
        result = server.request(
            "tools/call",
            {
                "name": "simulate",
                "arguments": {
                    "sources": ["runner_dut.sv"],
                    "top": "runner_dut",
                    "cocotb": {"module": "runner_checks", "python": "/bin/false"},
                },
            },
        )
        assert result["isError"], result
        assert "cocotb" in result["content"][0]["text"]
    finally:
        server.close()


def test_coverage_of_a_cocotb_run():
    r = run("coverage", {"w": 1})
    assert r["run"]["tests"]["passed"] == 1, r
    paths = [i["instance"] for i in r["instances"]]
    assert paths == ["runner_dut"], r
    assert r["instances"][0]["metrics"]["statement"]["total"] > 0, r
