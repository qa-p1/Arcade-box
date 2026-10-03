#!/usr/bin/env python3
"""Exercise the MVP CLI against an isolated, disposable Arcade Box profile."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_SHA256_ABC = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"


def run(command: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def require_success(result: subprocess.CompletedProcess[str], operation: str) -> None:
    require(
        result.returncode == 0,
        f"{operation} failed (exit {result.returncode}):\n{result.stderr or result.stdout}",
    )


def read_tool_result(result: subprocess.CompletedProcess[str], operation: str) -> dict[str, Any]:
    require_success(result, operation)
    try:
        document = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"{operation} did not return valid JSON: {error}\n{result.stdout}") from error
    require(document.get("status") == "success", f"{operation} returned status {document.get('status')!r}")
    return document


def main() -> int:
    if not (sys.platform.startswith("linux") or os.name == "nt"):
        raise RuntimeError("the MVP smoke profile is isolated on Linux and Windows only")

    cargo = run(["cargo", "build", "-p", "arcadebox"])
    require_success(cargo, "cargo build -p arcadebox")

    binary_name = "arcadebox.exe" if os.name == "nt" else "arcadebox"
    binary = ROOT / "target" / "debug" / binary_name
    require(binary.is_file(), f"built CLI not found at {binary}")

    with tempfile.TemporaryDirectory(prefix="arcadebox-smoke-") as temporary:
        isolated_root = Path(temporary).resolve()
        data_home = isolated_root / "data"
        config_home = isolated_root / "config"
        cache_home = isolated_root / "cache"
        for directory in (data_home, config_home, cache_home):
            directory.mkdir()

        env = os.environ.copy()
        if sys.platform.startswith("linux"):
            env["XDG_DATA_HOME"] = str(data_home)
            env["XDG_CONFIG_HOME"] = str(config_home)
            env["XDG_CACHE_HOME"] = str(cache_home)
        elif os.name == "nt":
            env["APPDATA"] = str(config_home)
            env["LOCALAPPDATA"] = str(data_home)

        def cli(*arguments: str) -> subprocess.CompletedProcess[str]:
            return run([str(binary), *arguments], env=env)

        case = cli("run", "arcade.text.case", "hello arcade", "--set", "mode=upper")
        require_success(case, "case conversion")
        require(case.stdout.strip() == "HELLO ARCADE", f"unexpected case output: {case.stdout!r}")
        print("PASS case conversion")

        formatted = read_tool_result(
            cli(
                "run",
                "arcade.text.structured",
                '{"ready":true,"items":[1,2]}',
                "--set",
                "to=same",
                "--set",
                "indent=2",
                "--json",
            ),
            "JSON formatting",
        )
        formatted_text = formatted["outputs"][0]["value"]
        require(json.loads(formatted_text) == {"ready": True, "items": [1, 2]}, "formatted JSON changed its value")
        require("\n" in formatted_text and "  \"ready\"" in formatted_text, "JSON was not pretty-printed")
        print("PASS JSON formatting")

        invalid_json = cli(
            "run",
            "arcade.text.structured",
            "{broken",
            "--set",
            "from=json",
        )
        require(invalid_json.returncode != 0, "invalid JSON unexpectedly succeeded")
        require("line 1" in invalid_json.stderr, f"invalid JSON diagnostic was unclear: {invalid_json.stderr!r}")
        print("PASS invalid JSON rejection")

        digest = read_tool_result(
            cli("run", "arcade.developer.hash", "abc", "--set", "algorithm=sha256", "--json"),
            "SHA-256",
        )
        require(digest["outputs"][0]["value"] == EXPECTED_SHA256_ABC, "SHA-256 did not match the known abc digest")
        print("PASS SHA-256")

        calculation = read_tool_result(
            cli("run", "arcade.convert.calculator", "2 + 3 * 4", "--json"),
            "calculator",
        )
        calculation_value = json.loads(calculation["outputs"][0]["value"])
        require(calculation_value["value"] == 14, f"unexpected calculator result: {calculation_value!r}")
        print("PASS calculator")

        qr_payload = "https://example.test/arcadebox-smoke"
        qr = read_tool_result(
            cli(
                "run",
                "arcade.barcode.qr-generate",
                qr_payload,
                "--output-name",
                "smoke-mvp-qr",
                "--json",
            ),
            "QR generation",
        )
        require(len(qr["outputs"]) == 1, "QR generator returned an unexpected number of outputs")
        qr_output = qr["outputs"][0]
        require(qr_output.get("kind") == "file", f"CLI did not resolve QR output to a file: {qr_output!r}")
        qr_path = Path(qr_output["value"]).resolve()
        require(qr_path.is_file(), f"QR output file does not exist: {qr_path}")
        try:
            qr_path.relative_to(isolated_root)
        except ValueError as error:
            raise RuntimeError(f"QR output escaped the isolated temporary profile: {qr_path}") from error

        decoded = read_tool_result(
            cli("run", "arcade.barcode.decode", "--file", str(qr_path), "--json"),
            "QR decoding",
        )
        barcode_result = json.loads(decoded["outputs"][0]["value"])
        require(barcode_result["count"] == 1, f"QR decoder found {barcode_result['count']} symbols")
        require(
            barcode_result["results"][0]["text"] == qr_payload,
            f"QR payload did not round-trip: {barcode_result['results'][0].get('text')!r}",
        )
        print("PASS QR generate and decode")

        pipeline_definition = isolated_root / "smoke-pipeline.json"
        pipeline_definition.write_text(
            json.dumps(
                {
                    "id": "smoke-text",
                    "name": "MVP Smoke Text",
                    "version": 1,
                    "nodes": [
                        {
                            "id": "upper",
                            "toolId": "arcade.text.case",
                            "inputs": [{"kind": "external", "index": 0}],
                            "options": {"mode": "upper"},
                        },
                        {
                            "id": "trim",
                            "toolId": "arcade.text.clean",
                            "inputs": [{"kind": "node", "nodeId": "upper", "outputIndex": 0}],
                            "options": {"trim": True},
                        },
                    ],
                    "outputNodes": ["trim"],
                }
            ),
            encoding="utf-8",
        )
        saved = cli("pipeline", "save", str(pipeline_definition))
        require_success(saved, "pipeline save")
        pipeline = cli("pipeline", "run", "smoke-text", "  hello arcade  ")
        require_success(pipeline, "saved text pipeline")
        require(pipeline.stdout.strip() == "HELLO ARCADE", f"unexpected pipeline output: {pipeline.stdout!r}")
        print("PASS saved text pipeline")

    print("MVP CLI smoke checks passed; temporary data removed.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, RuntimeError) as error:
        print(f"FAIL {error}", file=sys.stderr)
        sys.exit(1)
