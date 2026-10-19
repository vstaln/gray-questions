#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/questions.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/questions" ;;
    *) rel="x86_64-apple-darwin/questions" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/questions" ;;
    *) rel="x86_64-unknown-linux-musl/questions" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/questions.exe" "$here/target/release/questions" "$here/target/debug/questions.exe" "$here/target/debug/questions"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "questions binary not found in $here" >&2
exit 1
