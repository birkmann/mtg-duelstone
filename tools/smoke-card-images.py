#!/usr/bin/env python3
"""Release gate: the bundled engine/JRE must load both card textures with an empty image cache."""
import sys
import urllib.parse
import urllib.request

base = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8099"
for version in ("large", "art_crop"):
    query = urllib.parse.urlencode({"name": "Plains", "version": version})
    with urllib.request.urlopen(f"{base}/api/img?{query}", timeout=45) as response:
        data = response.read()
        assert response.headers.get_content_type() == "image/jpeg", response.headers
        assert len(data) > 1024 and data.startswith(b"\xff\xd8") and data.endswith(b"\xff\xd9"), (
            f"{version}: expected a complete JPEG, got {len(data)} bytes"
        )
        print(f"Card image OK: {version} ({len(data)} bytes)")
