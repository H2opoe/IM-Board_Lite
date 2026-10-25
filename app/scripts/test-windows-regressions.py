#!/usr/bin/env python3
"""Run the real Windows supervision/auth modules without building the desktop UI."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

APP = Path(__file__).resolve().parents[1]
RUST = APP / "src-tauri"


def main():
    if os.name != "nt":
        raise SystemExit("This regression runner requires native Windows.")
    locked = tomllib.loads((RUST / "Cargo.lock").read_text(encoding="utf-8"))["package"]

    def version(name, prefix=""):
        values = {p["version"] for p in locked if p["name"] == name and p["version"].startswith(prefix)}
        if len(values) != 1:
            raise RuntimeError(f"Ambiguous locked probe dependency: {name}: {values}")
        return "=" + values.pop()

    dependencies = {
        "anyhow": version("anyhow"),
        "tokio": {"version": version("tokio"), "features": ["process", "rt-multi-thread", "macros", "sync", "time", "io-util"]},
        "windows": {"version": version("windows", "0.62."), "features": ["Win32_Foundation", "Win32_System_Threading", "Win32_System_JobObjects", "Win32_System_Diagnostics_ToolHelp", "Win32_Security"]},
        "reqwest": {"version": version("reqwest", "0.12."), "default-features": False, "features": ["json"]},
        "serde_json": version("serde_json"), "sha2": version("sha2", "0.10."),
        "flate2": version("flate2"), "tar": version("tar"),
        "uuid": {"version": version("uuid"), "features": ["v4"]},
        "zip": {"version": version("zip", "9."), "default-features": False, "features": ["deflate"]},
        "tempfile": version("tempfile"),
    }
    with tempfile.TemporaryDirectory(prefix="imboard-windows-regressions-") as temporary:
        root = Path(temporary)
        manifest = {"package": {"name": "imboard-windows-regression-probe", "version": "0.0.0", "edition": "2021", "publish": False}, "lib": {"path": "probe.rs"}, "dependencies": dependencies}
        (root / "Cargo.toml").write_text("[package]\n" + "\n".join(f"{k} = {json.dumps(v)}".replace(": false", " = false") for k, v in manifest["package"].items()) + "\n[lib]\npath = \"probe.rs\"\n[dependencies]\n" + "\n".join(f"{name} = " + (json.dumps(value) if isinstance(value, str) else "{ " + ", ".join(f"{key} = {json.dumps(item)}" for key, item in value.items()) + " }") for name, value in dependencies.items()) + "\n", encoding="utf-8")
        # env! in the included production modules must still resolve packaged
        # fixture resources relative to the real application manifest.
        (root / "build.rs").write_text('fn main() { println!("cargo:rustc-env=CARGO_MANIFEST_DIR={}", std::env::var("IMBOARD_REGRESSION_SOURCE").unwrap()); }\n', encoding="utf-8")
        def module(name, path):
            return f'#[path = {json.dumps(str(RUST / path).replace(chr(92), "/"), ensure_ascii=False)}] pub(crate) mod {name};\n'
        source = "#![allow(dead_code)]\n"
        for name, path in [("process_job", "src/process_job.rs"), ("local_runtime_supervisor", "src/local_runtime_supervisor.rs"), ("bridge_registry", "src/bridge_registry.rs"), ("bridge_runner", "src/bridge_runner/command.rs"), ("extraction", "src/runtime/official_cli/extraction.rs")]:
            source += module(name, path)
        source += "mod profile_manager {\n" + module("activity", "src/profile_manager/activity.rs") + "}\n"
        source += "mod runtime {\n" + module("file_integrity", "src/runtime/file_integrity.rs")
        for name in ["isolated_dws", "dws_auth"]:
            if (RUST / f"src/runtime/{name}.rs").exists():
                source += module(name, f"src/runtime/{name}.rs")
        source += "}\n"
        (root / "probe.rs").write_text(source, encoding="utf-8")
        # Seed dependency resolution from the production lock; no application
        # source or production lock is rewritten by this temporary test crate.
        (root / "Cargo.lock").write_bytes((RUST / "Cargo.lock").read_bytes())
        subprocess.run(["cargo", "test", "--manifest-path", str(root / "Cargo.toml"), "--lib", "--", "--nocapture"], check=True, env={**os.environ, "IMBOARD_REGRESSION_SOURCE": str(RUST)}, timeout=600)


if __name__ == "__main__":
    main()
