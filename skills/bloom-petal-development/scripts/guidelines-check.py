#!/usr/bin/env python3
"""Bloom Petal guidelines checker — runs 30+ structural checks against a petal repo.

Usage: python3 scripts/guidelines-check.py [petal-root]

If petal-root is omitted, uses the current directory.

Checks:
  petal.toml: schema, name, [source], [consent], capability minimality
  Security: no path dispatch, no secret accessors in route files,
    secret-namespace wiring, public-view field exclusions
  VFS: is_safe_segment usage, deny_unknown_fields, $index.rs lists docs,
    status.json self-documenting
  Docs: README route table + lifecycle + field reference + boundaries,
    AGENTS state machine + next-action + error recovery + safety checklist
  Tooling: architecture script, release script, SDK rev pin match,
    no vendored SDK, no committed .wasm
"""
import os, re, sys

def root():
    return sys.argv[1] if len(sys.argv) > 1 else os.getcwd()

def read(path):
    try:
        with open(path) as f:
            return f.read()
    except FileNotFoundError:
        return ""

def grep_count(pattern, path):
    return int(os.popen(f"grep -rEn --include='*.rs' '{pattern}' '{path}' 2>/dev/null | wc -l").read().strip() or 0)

def main():
    R = root()
    results = []
    def check(desc, passed, detail=""):
        results.append((desc, passed, detail))

    petal_toml = read(f"{R}/petal.toml")
    petal_build = read(f"{R}/petal-build.toml")
    build_sh = read(f"{R}/scripts/build.sh")
    types_rs = read(f"{R}/route/src/types.rs")

    # --- petal.toml ---
    check("petal.toml has schema", 'schema = "bloom.petal.package' in petal_toml)
    check("petal.toml has name", 'name = ' in petal_toml)
    check("petal.toml has [source]", '[source]' in petal_toml)
    check("petal.toml [source] has kind=github", 'kind = "github"' in petal_toml)
    check("petal.toml [source] has repository", 'repository' in petal_toml and '[source]' in petal_toml)
    check("petal.toml has [consent] summary", '[consent]' in petal_toml and 'summary' in petal_toml)

    # --- capabilities ---
    check("No bloom:http (unless intended)", 'bloom:http' not in petal_toml)
    check("No bloom:sign (unless intended)", 'bloom:sign' not in petal_toml)
    check("No [[net.allow]] entries (unless intended)", '[[net.allow]]' not in petal_toml)
    check("No signing intents (unless intended)", 'signing_intent' not in petal_toml and '[[signing_intent]]' not in petal_toml)

    # --- security: path dispatch ---
    n = grep_count(r'current_route_(canonical_)?path', f"{R}/route/src")
    check("No path-based dispatch in shared code", n == 0, f"{n} matches")

    # --- security: secrets ---
    # Route files must not access secrets directly
    n = grep_count(r'secret_key|load_secret_bytes|load_secret_json|"secrets"', f"{R}/route/files")
    check("No secret accessors in route files", n == 0, f"{n} matches")

    # Secret-namespace wiring (generic check)
    n = grep_count(r'store_put.*true\)', f"{R}/route/src")
    check("Sensitive state stored with secret=true", n > 0, "no store_put with secret=true found")

    # --- VFS wiring ---
    n = grep_count(r'is_safe_segment', f"{R}/route/src")
    check("Uses is_safe_segment for path components", n > 0)

    check("Write request types use deny_unknown_fields", 'deny_unknown_fields' in types_rs)

    # $index.rs lists docs
    idx = read(f"{R}/route/files/$index.rs")
    check("$index.rs lists README.md", 'README.md' in idx)
    check("$index.rs lists AGENTS.md", 'AGENTS.md' in idx)

    # status.json self-documenting
    sj = read(f"{R}/route/files/status.json.rs")
    check("status.json has description", 'description' in sj)
    check("status.json lists operations", 'operations' in sj or 'supported' in sj.lower())
    check("status.json has docs pointers", 'docs' in sj or 'README' in sj)

    # --- docs quality bar ---
    readme = read(f"{R}/README.md")
    check("README has route table", 'Route' in readme and '|---|' in readme)
    check("README has lifecycle/state machine", 'lifecycle' in readme.lower() or 'state machine' in readme.lower())
    check("README has write body reference", 'write' in readme.lower())
    check("README has secrets/capabilities boundary", 'secrets' in readme.lower() or 'capabilit' in readme.lower())

    agents = read(f"{R}/AGENTS.md")
    check("AGENTS has state machine", all(s in agents for s in ['stag']))
    check("AGENTS has next-action table", 'next action' in agents.lower() or 'next-action' in agents.lower())
    check("AGENTS has error recovery", 'error recovery' in agents.lower() or 'resolution' in agents.lower())
    check("AGENTS has safety validation", 'safety' in agents.lower())
    check("AGENTS has secrets boundary", 'secrets boundary' in agents.lower() or 'secrets' in agents.lower())
    check("AGENTS has route table", '| Route' in agents or '| /petals' in agents or '| route' in agents.lower())

    # --- tooling ---
    check("check-route-architecture.sh exists", os.path.exists(f"{R}/scripts/check-route-architecture.sh"))
    check("create-release.sh exists", os.path.exists(f"{R}/scripts/create-release.sh"))

    # SDK rev pin match
    sdk_rev = ''
    for line in petal_build.split('\n'):
        if 'rev = "' in line:
            m = re.search(r'rev = "([^"]+)"', line)
            if m: sdk_rev = m.group(1)
    check("petal-build.toml pins SDK rev", bool(sdk_rev))
    check("build.sh PETAL_REV matches", sdk_rev in build_sh if sdk_rev else False)

    # No vendored SDK
    check("No vendored SDK", 'path = "../sdk"' not in petal_build)

    # No committed wasm in route/files
    wasm_count = int(os.popen(f"find '{R}/route/files' -name '*.wasm' 2>/dev/null | wc -l").read().strip() or 0)
    check("No committed .wasm in route/files", wasm_count == 0, f"{wasm_count} found")

    # --- report ---
    print("=" * 70)
    print("BLOOM PETAL GUIDELINES CHECK")
    print("=" * 70)
    passed = sum(1 for _, ok, _ in results if ok)
    for desc, ok, detail in results:
        status = "✅" if ok else "❌"
        line = f"  {status} {desc}"
        if detail and not ok:
            line += f" — {detail}"
        print(line)
    print(f"\n{passed}/{len(results)} passed")
    return 0 if passed == len(results) else 1

if __name__ == "__main__":
    sys.exit(main())
