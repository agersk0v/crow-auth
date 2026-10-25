#!/usr/bin/env python3
"""Provision the reviewed public dataset; never sends user passwords anywhere."""
import hashlib
import json
import os
from pathlib import Path
import tempfile
import urllib.request

root = Path(__file__).resolve().parent.parent
manifest = json.loads((root / "data/password-blocklist.json").read_text())
# HTTPS uses normal CA verification and the inherited proxy configuration.
with urllib.request.urlopen(manifest["url"], timeout=30) as response:
    content = response.read(4 * 1024 * 1024 + 1)
if len(content) > 4 * 1024 * 1024:
    raise SystemExit("Password dataset exceeds provisioner size limit")
if hashlib.sha256(content).hexdigest() != manifest["sha256"]:
    raise SystemExit("Password dataset checksum mismatch; existing file unchanged")
if not content.decode("utf-8").splitlines():
    raise SystemExit("Password dataset is empty")
destination = root / "data/common-passwords.txt"
with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
    temporary.write(content)
    temp_path = Path(temporary.name)
try:
    os.replace(temp_path, destination)
finally:
    temp_path.unlink(missing_ok=True)
print("Provisioned checksum-verified local password blocklist")
