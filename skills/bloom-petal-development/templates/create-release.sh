#!/usr/bin/env bash
# Creates a GitHub release for a Bloom petal with all required artifacts.
# Usage: GH_TOKEN=*** bash scripts/create-release.sh
#
# Before running:
#   1. Merge feature branch to main and push
#   2. Create and push the version tag (git tag -a v<version> -m "..." && git push origin v<version>)
#   3. Build and package (bash scripts/build.sh && petal package --out <archive>)
#   4. Prepare artifacts in a staging directory
#   5. Edit the variables below to match your petal
set -euo pipefail

# ── EDIT THESE ──────────────────────────────────────────────
# REPO format: <owner>/<repo> — use bloom-directory/ for org petals,
# or the user's account (e.g. 0xdewy/) for pre-transfer development.
REPO="bloom-directory/bloom-petal-<name>"
TAG="v0.1.0"
COMMIT="<40-char SHA>"
STAGING="/tmp/<name>-release"
# ────────────────────────────────────────────────────────────

if [ -z "${GH_TOKEN:-}" ]; then
  echo "Error: GH_TOKEN environment variable is required"
  echo "Create a token at https://github.com/settings/tokens (needs repo scope)"
  exit 1
fi

echo "Creating release $TAG..."

# Create the release
RELEASE_JSON=$(curl -s -X POST \
  -H "Authorization: token $GH_TOKEN" \
  -H "Content-Type: application/json" \
  "https://api.github.com/repos/$REPO/releases" \
  -d "$(cat <<EOF
{
  "tag_name": "$TAG",
  "target_commitish": "$COMMIT",
  "name": "$TAG",
  "body": "Release $TAG",
  "draft": false,
  "prerelease": false
}
EOF
)")

RELEASE_ID=$(echo "$RELEASE_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
UPLOAD_URL="https://uploads.github.com/repos/$REPO/releases/$RELEASE_ID/assets"

echo "Release ID: $RELEASE_ID"
echo "Uploading assets..."

for file in petal-release.json SHA256SUMS; do
  echo "  Uploading $file..."
  curl -sf -X POST \
    -H "Authorization: token $GH_TOKEN" \
    -H "Content-Type: text/plain" \
    --data-binary "@$STAGING/$file" \
    "$UPLOAD_URL?name=$file" > /dev/null
  echo "    done"
done

# Upload the archive (find it by extension)
ARCHIVE=$(ls "$STAGING"/*.petal.tar.gz | head -1)
ARCHIVE_NAME=$(basename "$ARCHIVE")
echo "  Uploading $ARCHIVE_NAME..."
curl -sf -X POST \
  -H "Authorization: token $GH_TOKEN" \
  -H "Content-Type: application/gzip" \
  --data-binary "@$ARCHIVE" \
  "$UPLOAD_URL?name=$ARCHIVE_NAME" > /dev/null
echo "    done"

echo ""
echo "Release $TAG published: https://github.com/$REPO/releases/tag/$TAG"
