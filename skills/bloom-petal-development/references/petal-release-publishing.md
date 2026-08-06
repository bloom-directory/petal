# Publishing a Petal GitHub Release

After a petal is merged to `main` and tagged, a GitHub release with
prebuilt artifacts must be published before `bloom init` can download it
as a preinstalled petal. This guide covers the exact artifact formats,
the creation sequence, and a reusable release script.

## Prerequisites

1. **Merge the feature branch to `main`** (fast-forward if possible):
   ```sh
   git checkout main && git merge <branch> --ff-only && git push origin main
   ```

2. **Create and push the version tag** pointing at the merge commit:
   ```sh
   git tag -a v<version> -m "<petal-name> v<version>" <commit-sha>
   git push origin v<version>
   ```

3. **Build and package** from the tagged commit:
   ```sh
   bash scripts/build.sh
   ./target/petal-tool/bin/petal package --out /tmp/<name>-v<version>.petal.tar.gz
   ```
   The package output JSON provides `package_hash` and `sha256` — save both.

## Three required artifacts

Download an existing petal's release to confirm the exact format:
```sh
curl -sL "https://github.com/bloom-directory/bloom-petal-<name>/releases/download/v<version>/petal-release.json"
curl -sL "https://github.com/bloom-directory/bloom-petal-<name>/releases/download/v<version>/SHA256SUMS"
```

### 1. `petal-release.json`

```json
{
  "schema": "bloom.petal.release.v1",
  "petal_name": "<name>",
  "source_repository": "bloom-directory/bloom-petal-<name>",
  "source_commit": "<40-char commit SHA of main HEAD at tag>",
  "release_tag": "v<version>",
  "archive": "<name>-v<version>.petal.tar.gz",
  "archive_sha256": "<sha256 from petal package output>",
  "package_hash": "<package_hash from petal package output>",
  "tooling_repository": "bloom-directory/petal",
  "tooling_commit": "<PETAL_REV from scripts/build.sh>"
}
```

Field sources:
- `archive_sha256` — the `sha256` field from `petal package` JSON output
  (NOT the `package_hash` — that goes in the separate field)
- `package_hash` — the `package_hash` field from `petal package` JSON output
  (BLAKE3 hash, must match `expected_hash` in bloom core's `github_source.rs`)
- `tooling_commit` — read `PETAL_REV` from `scripts/build.sh`:
  ```sh
  grep 'PETAL_REV=' scripts/build.sh
  ```

### 2. `SHA256SUMS`

```
<archive_sha256>  <name>-v<version>.petal.tar.gz
```

Single line, two spaces between hash and filename. Must match the
`archive_sha256` in `petal-release.json`.

### 3. `<name>-v<version>.petal.tar.gz`

The archive produced by `petal package`. Copy/rename it to match the
`archive` field in the manifest.

## Creating the release

### Option A: Automated (requires `GH_TOKEN`)

Use the release script pattern (see `templates/create-release.sh`).
Set `GH_TOKEN` to a personal access token with `repo` scope and run:

```sh
GH_TOKEN=*** bash scripts/create-release.sh
```

The script creates the release via the GitHub API, then uploads each
artifact as a release asset.

### Option B: Manual (web UI)

1. Go to: `https://github.com/bloom-directory/bloom-petal-<name>/releases/new`
2. Select the tag created above
3. Upload all three artifacts
4. Publish

### Option C: GitHub API with curl

```sh
REPO="bloom-directory/bloom-petal-<name>"
TAG="v<version>"

# Create release
RELEASE_ID=$(curl -s -X POST \
  -H "Authorization: token $GH_TOKEN" \
  -H "Content-Type: application/json" \
  "https://api.github.com/repos/$REPO/releases" \
  -d '{"tag_name":"'$TAG'","target_commitish":"<sha>","name":"'$TAG'","body":"...","draft":false,"prerelease":false}' \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")

# Upload each asset
for file in petal-release.json SHA256SUMS <name>-v<version>.petal.tar.gz; do
  curl -s -X POST \
    -H "Authorization: token $GH_TOKEN" \
    -H "Content-Type: $(file $file | grep -q gzip && echo application/gzip || echo text/plain)" \
    --data-binary "@$file" \
    "https://uploads.github.com/repos/$REPO/releases/$RELEASE_ID/assets?name=$file"
done
```

## Verification

After publishing, verify the release is downloadable:

```sh
curl -sI "https://github.com/bloom-directory/bloom-petal-<name>/releases/download/v<version>/petal-release.json" \
  | head -1
# Should return HTTP 302 (redirect to download)
```

And test `bloom init` in a fresh `BLOOM_HOME` to confirm the full
preinstalled download flow works end-to-end.

## Pitfall: tag must exist before release

The GitHub API rejects release creation if the tag doesn't exist and
`target_commitish` is a SHA (not a branch name). Always `git push origin
v<version>` before creating the release via API.

## Pitfall: `gh` CLI may not be authenticated

On machines where `gh auth login` hasn't been run, use the GitHub API
directly with `GH_TOKEN` and `curl`, or create the release through the
web UI. SSH push works without a token, but release creation requires
API authentication.

## Pitfall: replacing existing release assets

`gh release upload --clobber` may silently fail (no error, no upload)
when an asset with the same name already exists. To replace assets:

```sh
# 1. Get numeric asset IDs (NOT the global node IDs starting with RA_)
gh api repos/<org>/<repo>/releases/tags/v<version> \
  --jq '.assets[] | "\(.id) \(.name)"'

# 2. Delete each asset by numeric ID
gh api repos/<org>/<repo>/releases/assets/<numeric_id> -X DELETE

# 3. Upload replacements
gh release upload v<version> <files...>
```

The `--jq '.assets[].id'` from `gh release view` returns GraphQL node
IDs (`RA_kwDO...`) which the REST DELETE endpoint rejects with 404.
Always use the REST releases API to get numeric IDs.

## Pitfall: hand-writing petal-release.json with wrong field names

When creating `petal-release.json` manually (not via the release script),
it is easy to use shortened field names (`petal` instead of `petal_name`,
`tag` instead of `release_tag`, `commit` instead of `source_commit`) and
omit required fields (`archive_sha256`, `tooling_repository`,
`tooling_commit`). **Always validate against an existing petal's release
before uploading:**

```sh
# Download a petal's release as a template
curl -sL "https://github.com/bloom-directory/bloom-petal-<name>/releases/download/v<version>/petal-release.json" \
  | python3 -m json.tool

# After uploading, verify via the API (bypasses CDN cache):
gh api repos/<org>/<repo>/releases/tags/v<version> \
  --jq '.assets[] | select(.name=="petal-release.json") | .url' \
  | xargs -I{} curl -s -H "Authorization: token $(gh auth token)" \
      -H "Accept: application/octet-stream" -L {} | python3 -m json.tool
```

The public `releases/download/` URL serves a CDN-cached copy for several
minutes after upload. To verify immediately, fetch the asset content via
the authenticated API instead.
