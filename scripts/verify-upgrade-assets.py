"""Verify installed assets against the reviewed, tracked release manifest."""
import pathlib,json,hashlib
root=pathlib.Path(__file__).resolve().parents[1]
for entry in json.loads((root/'scripts/upgrade-models.lock.json').read_text()):
    path=root/'models'/entry['file']
    if hashlib.sha256(path.read_bytes()).hexdigest()!=entry['sha256']:
        raise SystemExit(f"Integrity mismatch: {entry['file']}. Do not promote this export.")
print('All pinned upgrade assets verified')
