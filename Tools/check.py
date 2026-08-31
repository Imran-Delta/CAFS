#!/usr/bin/env python3
import os
import hashlib
import sys
import zipfile
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
REVIEWED_HASHES_PATH = REPO_ROOT / "Tools" / ".reviewed-hashes"
IGNORE_DIRS = {'.git', '__pycache__', '.pytest_cache'}
IGNORE_EXTENSIONS = {'.img', '.pyc'}

def hash_file(path):
    sha = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(65536), b''):
            sha.update(chunk)
    return sha.hexdigest()

def scan_repo():
    """Returns dict of {relative_path: hash}."""
    manifest = {}
    for root, dirs, files in os.walk(REPO_ROOT):
        dirs[:] = [d for d in dirs if d not in IGNORE_DIRS]
        for file in files:
            full_path = Path(root) / file
            rel_path = str(full_path.relative_to(REPO_ROOT))
            
            if any(rel_path.endswith(ext) for ext in IGNORE_EXTENSIONS):
                continue

            # ADR Zip handling (same as before)
            if file.endswith('.zip'):
                if not rel_path.startswith('Docs/ADR/'):
                    print(f"⚠️ WARNING: Zip outside ADR: {rel_path}")
                    continue
                with tempfile.TemporaryDirectory() as tmpdir:
                    with zipfile.ZipFile(full_path, 'r') as z:
                        z.extractall(tmpdir)
                        for extracted in Path(tmpdir).rglob('*'):
                            if extracted.is_file():
                                inner_hash = hash_file(extracted)
                                inner_rel = extracted.relative_to(tmpdir)
                                manifest[f"{rel_path}/{inner_rel}"] = inner_hash
                continue

            manifest[rel_path] = hash_file(full_path)
    return manifest

def load_reviewed_hashes():
    """
    Loads Tools/.reviewed-hashes.
    Returns:
        path_to_hash: dict {path: hash}
        hash_to_paths: dict {hash: [list_of_paths]}
    """
    path_to_hash = {}
    hash_to_paths = {}
    
    if not REVIEWED_HASHES_PATH.exists():
        return path_to_hash, hash_to_paths
    
    with open(REVIEWED_HASHES_PATH, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            # Split on FIRST space only (handles spaces in filenames)
            h, path = line.split(' ', 1)
            path_to_hash[path] = h
            hash_to_paths.setdefault(h, []).append(path)
    
    return path_to_hash, hash_to_paths

def update_reviewed_hashes():
    """Writes current manifest in 'hash path' format."""
    current = scan_repo()
    # Remove zip wrappers (we only store actual files)
    clean_manifest = {k: v for k, v in current.items() if not k.endswith('.zip')}
    
    with open(REVIEWED_HASHES_PATH, 'w') as f:
        # Sort by path for deterministic ordering
        for path, h in sorted(clean_manifest.items()):
            f.write(f"{h} {path}\n")
    
    print(f"✅ Updated {REVIEWED_HASHES_PATH} with {len(clean_manifest)} hashes.")

def run_checks():
    current = scan_repo()
    path_to_hash, hash_to_paths = load_reviewed_hashes()
    exit_code = 0

    # 1. Check all reviewed paths still exist and match
    for reviewed_path, reviewed_hash in path_to_hash.items():
        if reviewed_path not in current:
            print(f"❓ FILE REMOVED: {reviewed_path} (was reviewed)")
            exit_code = 1
        elif current[reviewed_path] != reviewed_hash:
            print(f"⚠️ UNREVIEWED MODIFICATION: {reviewed_path} (hash changed)")
            exit_code = 1

    # 2. Check new/unreviewed files for "CP accident" (hash matches a reviewed file)
    for current_path, current_hash in current.items():
        if current_path not in path_to_hash:
            if current_hash in hash_to_paths:
                originals = ", ".join(hash_to_paths[current_hash])
                print(f"🔴 UNREVIEWED COPY: {current_path} (matches: {originals})")
                exit_code = 1
            # Else: truly new, unreviewed file → ignore silently

    if exit_code == 0:
        print("✅ Tripwire clear.")
    else:
        print("\n⚠️ Issues found. Run 'python3 Tools/check.py --update' to accept.")
    
    return exit_code

if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == '--update':
        update_reviewed_hashes()
        sys.exit(0)
    
    sys.exit(run_checks())
