"""Collect the upstream notices for the Windows x64 release dependency graph."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output([
    "cargo", "metadata", "--locked", "--format-version", "1",
    "--filter-platform", "x86_64-pc-windows-msvc", "--features", "ag-psd",
], cwd=ROOT))
packages = {p["id"]: p for p in metadata["packages"]}
nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
root = metadata["resolve"]["root"]
seen = set()
pending = [root]
while pending:
    package = pending.pop()
    if package in seen:
        continue
    seen.add(package)
    pending.extend(dep["pkg"] for dep in nodes[package]["deps"]
                   if any(kind["kind"] is None for kind in dep["dep_kinds"]))

entries = []
texts = {}
for package in sorted((packages[p] for p in seen - {root}), key=lambda p: (p["name"], p["version"])):
    base = Path(package["manifest_path"]).parent
    files = sorted(p for p in base.iterdir() if p.is_file()
                   and p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE")))
    if package["license_file"]:
        files = sorted(set(files) | {base / package["license_file"]})
    if not files:
        raise SystemExit(f'Missing license text for {package["name"]} {package["version"]}')
    references = []
    for path in files:
        content = "\n".join(line.rstrip() for line in path.read_text(encoding="utf-8-sig").splitlines()).strip()
        digest = hashlib.sha256(content.encode()).hexdigest()
        if digest not in texts:
            texts[digest] = (len(texts) + 1, content)
        references.append(f"{path.name} [text {texts[digest][0]}]")
    entries.append(f'{package["name"]} {package["version"]} | {package["license"] or "see license file"}\n'
                   f'  {", ".join(references)}')
header = "ObjectLift Windows x64 binary dependency notices (experimental PSD enabled).\n"
sections = [header, "\n".join(entries)]
sections += [f"===== Text {index} =====\n{content}" for index, content in texts.values()]
(ROOT / "DEPENDENCY-LICENSES.txt").write_text("\n\n".join(sections) + "\n", encoding="utf-8")
print(f"Collected notices for {len(entries)} packages")
