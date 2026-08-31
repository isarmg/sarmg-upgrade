#!/usr/bin/env python3
"""Create a deterministic, bounded CycloneDX component list from Cargo.lock."""

import json
import pathlib
import sys
import tomllib

manifest = tomllib.loads(pathlib.Path("Cargo.toml").read_text(encoding="utf-8"))
application = manifest["package"]
if application["name"] != "sarmg-upgrade":
    raise SystemExit("Cargo.toml does not describe sarmg-upgrade")

lock = tomllib.loads(pathlib.Path("Cargo.lock").read_text(encoding="utf-8"))
components = []
for package in sorted(lock.get("package", []), key=lambda value: (value["name"], value["version"])):
    component = {
        "type": "library",
        "name": package["name"],
        "version": package["version"],
        "bom-ref": f"pkg:cargo/{package['name']}@{package['version']}",
    }
    checksum = package.get("checksum")
    if checksum:
        component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
    components.append(component)

document = {
    "bomFormat": "CycloneDX",
    "specVersion": "1.5",
    "version": 1,
    "metadata": {
        "component": {
            "type": "application",
            "name": application["name"],
            "version": application["version"],
        }
    },
    "components": components,
}
pathlib.Path(sys.argv[1]).write_text(
    json.dumps(document, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
