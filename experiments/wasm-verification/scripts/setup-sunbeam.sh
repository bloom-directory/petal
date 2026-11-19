#!/bin/sh
# Reproduce the tested macOS/arm64 tool environment, without global installs.
# Requires Rust 1.97.0 + wasm32-unknown-unknown, gh, git, curl, unzip, Python 3.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
task_root=${M1_TOOLS_ROOT:-/tmp/petal-m1}
case $(uname -sm) in
  'Darwin arm64') ;;
  *) echo 'This bootstrap pins the tested Darwin arm64 assets only.' >&2; exit 1 ;;
esac
case $(rustc --version) in
  'rustc 1.97.0 '*) ;;
  *) echo 'Select Rust 1.97.0 before running this script.' >&2; exit 1 ;;
esac
mkdir -p "$task_root/tools" "$task_root/src" "$task_root/logs"
cd "$task_root/tools"
download() {
  if [ ! -f "$3" ]; then
    gh release download "$2" --repo "$1" --pattern "$3"
  fi
}
download cvc5/cvc5 cvc5-1.4.2 cvc5-macOS-arm64-static.zip
download Z3Prover/z3 z3-5.3.3 z3-5.3.3-arm64-osx-13.3.zip
download adoptium/temurin21-binaries jdk-21.0.13.1+2 OpenJDK21U-jdk_aarch64_mac_hotspot_21.0.13.1_2.tar.gz
shasum -a 256 -c "$here/scripts/tool-assets.sha256"
mkdir -p z3 cvc5 jdk bin
if [ ! -x z3/z3-5.3.3-arm64-osx-13.3/bin/z3 ]; then
  unzip -q z3-5.3.3-arm64-osx-13.3.zip -d z3
fi
if [ ! -x cvc5/cvc5-macOS-arm64-static/bin/cvc5 ]; then
  unzip -q cvc5-macOS-arm64-static.zip -d cvc5
fi
if [ ! -x jdk/jdk-21.0.13.1+2/Contents/Home/bin/java ]; then
  tar xzf OpenJDK21U-jdk_aarch64_mac_hotspot_21.0.13.1_2.tar.gz -C jdk
fi
if [ ! -x wasm-tools-1.249.0/bin/wasm-tools ]; then
  cargo install wasm-tools --version =1.249.0 --locked --root "$task_root/tools/wasm-tools-1.249.0"
fi
if [ ! -x rustfilt/bin/rustfilt ]; then
  cargo install rustfilt --version =0.2.1 --locked --root "$task_root/tools/rustfilt"
fi
ln -sf "$task_root/tools/z3/z3-5.3.3-arm64-osx-13.3/bin/z3" bin/z3
ln -sf "$task_root/tools/cvc5/cvc5-macOS-arm64-static/bin/cvc5" bin/cvc5
ln -sf "$task_root/tools/rustfilt/bin/rustfilt" bin/rustfilt

source_dir="$task_root/src/CertoraProver"
revision=4ef546d00cd67d333914845dead2d72081e1f014
if [ ! -d "$source_dir" ]; then
  git clone --no-checkout https://github.com/Certora/CertoraProver.git "$source_dir"
  git -C "$source_dir" checkout --detach "$revision"
  git -C "$source_dir" submodule update --init --recursive
fi
test "$(git -C "$source_dir" rev-parse HEAD)" = "$revision"
git -C "$source_dir" diff --quiet HEAD --

# Quote environment values as shell data; preserve the caller's PATH.
python3 - "$task_root" > "$task_root/env.sh" <<'PY'
import shlex
import sys
r = sys.argv[1]
for name, value in {
    'JAVA_HOME': r + '/tools/jdk/jdk-21.0.13.1+2/Contents/Home',
    'CERTORA': r + '/certora-out',
    'GRADLE_USER_HOME': r + '/gradle-home',
    'JAVA_TOOL_OPTIONS': '-Djava.awt.headless=true -Dapple.awt.UIElement=true',
}.items():
    print('export ' + name + '=' + shlex.quote(value))
paths = ':'.join([r + '/tools/jdk/jdk-21.0.13.1+2/Contents/Home/bin',
                  r + '/tools/bin', r + '/tools/wasm-tools-1.249.0/bin'])
print('export PATH=' + shlex.quote(paths) + ':"$PATH"')
PY
. "$task_root/env.sh"
if [ ! -f "$CERTORA/emv.jar" ]; then
  (cd "$source_dir" && ./gradlew assemble --no-daemon) > "$task_root/logs/certora-build.log" 2>&1
fi
java -version
z3 --version
cvc5 --version
wasm-tools --version
echo "Source $task_root/env.sh before running the experiment."
