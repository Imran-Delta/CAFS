#!/usr/bin/env python3
import os
import hashlib
import sys
import zipfile
import tempfile
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
REVIEWED_HASHES_PATH = REPO_ROOT / "Tools" / ".reviewed-hashes"
# Keep extensions you explicitly want to ignore even if Git tracks them
IGNORE_EXTENSIONS = {'.img', '.pyc'} 

def hash_file(path):
    sha = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(65536), b''):
            sha.update(chunk)
    return sha.hexdigest()

def scan_repo():
    """Returns dict of {relative_path: hash} for tracked files only."""
    manifest = {}
    
    try:
        # Use git to get only tracked files (respects .gitignore perfectly)
        result = subprocess.run(
            ['git', 'ls-files', '-z'],
            cwd=REPO_ROOT,
            capture_output=True,
            check=True,
            text=True
        )
        # -z separates filenames with null bytes, avoiding issues with spaces in names
        tracked_files = result.stdout.split('\0')
    except subprocess.CalledProcessError:
        print("❌ Error: Not a git repository or git is not installed.")
        sys.exit(1)

    for rel_path in tracked_files:
        if not rel_path:
            continue
            
        full_path = REPO_ROOT / rel_path
        
        # FIX 1: Skip the manifest itself
        if full_path.resolve() == REVIEWED_HASHES_PATH.resolve():
            continue

        # Skip explicitly ignored extensions
        if any(rel_path.endswith(ext) for ext in IGNORE_EXTENSIONS):
            continue

        # ADR Zip handling
        if rel_path.endswith('.zip'):
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
                            # Ensure forward slashes for consistency
                            inner_rel_str = str(inner_rel).replace('\\', '/')
                            manifest[f"{rel_path}/{inner_rel_str}"] = inner_hash
            continue

        manifest[rel_path] = hash_file(full_path)
        
    return manifest

def load_reviewed_hashes():
    path_to_hash = {}
    hash_to_paths = {}
    
    if not REVIEWED_HASHES_PATH.exists():
        return path_to_hash, hash_to_paths
    
    with open(REVIEWED_HASHES_PATH, 'r') as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            h, path = line.split(' ', 1)
            path_to_hash[path] = h
            hash_to_paths.setdefault(h, []).append(path)
    
    return path_to_hash, hash_to_paths

def update_reviewed_hashes():
    current = scan_repo()
    
    with open(REVIEWED_HASHES_PATH, 'w') as f:
        for path, h in sorted(current.items()):
            f.write(f"{h} {path}\n")
    
    print(f"✅ Updated {REVIEWED_HASHES_PATH} with {len(current)} hashes.")

def run_checks():
    current = scan_repo()
    path_to_hash, hash_to_paths = load_reviewed_hashes()
    exit_code = 0

    for reviewed_path, reviewed_hash in path_to_hash.items():
        if reviewed_path not in current:
            print(f"❓ FILE REMOVED: {reviewed_path} (was reviewed)")
            exit_code = 1
        elif current[reviewed_path] != reviewed_hash:
            print(f"⚠️ UNREVIEWED MODIFICATION: {reviewed_path} (hash changed)")
            exit_code = 1

    for current_path, current_hash in current.items():
        if current_path not in path_to_hash:
            if current_hash in hash_to_paths:
                originals = ", ".join(hash_to_paths[current_hash])
                print(f"🔴 UNREVIEWED COPY: {current_path} (matches: {originals})")
                exit_code = 1

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
