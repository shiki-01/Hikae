#!/usr/bin/env bash
# macOS 用の同梱 git を、公式ソースからビルドして src-tauri/app/resources/git/ に置く。
#
# - 取得元は kernel.org の git 公式ソース tarball（バージョンと SHA-256 を固定。不一致なら失敗する）
# - arm64 と x86_64 をそれぞれビルドし、lipo で universal binary にまとめる
# - RUNTIME_PREFIX=yes で、バイナリの位置から libexec / share を解決する（配置場所に依存しない）
# - TLS と HTTP は macOS 標準の libcurl（/usr/lib/libcurl.4.dylib。Apple の証明書ストアを使う）に動的リンクする。
#   OpenSSL を同梱・静的リンクしないので、サイズと更新対象が減る。CA 証明書は OS のものに従う
# - Perl・Python・Tcl/Tk・gettext・expat（git-http-push）は使わない
# - 展開済みで版が一致していれば何もしない（再ビルドしない）
#
# 要件: macOS、Xcode Command Line Tools（clang、make、lipo）、curl、shasum、tar
# 使い方: scripts/build-git-macos.sh [--force]
#
# 署名・公証は扱わない（配布の段階で、同梱バイナリごと署名する）。
#
# ※ この仕組みは Windows 上では実行できず、CI（macOS）でも未確認。

set -euo pipefail

GIT_VERSION="2.56.0"
GIT_TARBALL="git-${GIT_VERSION}.tar.xz"
GIT_URL="https://mirrors.edge.kernel.org/pub/software/scm/git/${GIT_TARBALL}"
GIT_SHA256="26c56c296b38c0695b26fa95f475f1d01704d2d38e73465ca30b0b2f5dc789d3"
MIN_MACOS="11.0"
MARKER_CONTENT="git-${GIT_VERSION} ${GIT_SHA256} macos-min-${MIN_MACOS} universal"

log() { printf '[build-git-macos] %s\n' "$*"; }
die() { printf '[build-git-macos] 失敗: %s\n' "$*" >&2; exit 1; }

if [ "$(uname -s)" != "Darwin" ]; then
  log "macOS 以外のためスキップします"
  exit 0
fi

FORCE=0
[ "${1:-}" = "--force" ] && FORCE=1

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESOURCES="${ROOT}/src-tauri/app/resources"
DEST="${RESOURCES}/git"
MARKER="${DEST}/.hikae-git-version"

if [ "$FORCE" -eq 0 ] && [ -x "${DEST}/bin/git" ] && [ -f "$MARKER" ] \
  && [ "$(cat "$MARKER")" = "$MARKER_CONTENT" ]; then
  log "ビルド済みです (git ${GIT_VERSION})"
  exit 0
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/hikae-git-build.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# --- 1. ダウンロードと SHA-256 検証（検証前は展開しない） ---
log "ダウンロード: ${GIT_URL}"
curl --fail --silent --show-error --location --output "${WORK}/${GIT_TARBALL}" "$GIT_URL"
ACTUAL="$(shasum -a 256 "${WORK}/${GIT_TARBALL}" | awk '{print $1}')"
if [ "$ACTUAL" != "$GIT_SHA256" ]; then
  rm -f "${WORK}/${GIT_TARBALL}"
  die "SHA-256 が一致しません。破棄しました (期待: ${GIT_SHA256} / 実際: ${ACTUAL})"
fi
log "SHA-256 の検証に成功しました"

tar -xJf "${WORK}/${GIT_TARBALL}" -C "$WORK"
SRC="${WORK}/git-${GIT_VERSION}"
[ -d "$SRC" ] || die "展開した tarball にソースのフォルダがありません"

# --- 2. アーキテクチャごとにビルド ---
build_arch() {
  local arch="$1"
  local build="${WORK}/build-${arch}"
  local out="${WORK}/out-${arch}"
  log "ビルド: ${arch}"
  cp -R "$SRC" "$build"
  # 設定は make の変数で渡す（configure は使わない。クロスビルド時の検出の揺れを避ける）
  (
    cd "$build"
    make -j"$(sysctl -n hw.ncpu)" install \
      prefix="$out" \
      RUNTIME_PREFIX=yes \
      CFLAGS="-O2 -arch ${arch} -mmacosx-version-min=${MIN_MACOS}" \
      LDFLAGS="-arch ${arch} -mmacosx-version-min=${MIN_MACOS}" \
      NO_OPENSSL=YesPlease \
      NO_EXPAT=YesPlease \
      NO_GETTEXT=YesPlease \
      NO_TCLTK=YesPlease \
      NO_PERL=YesPlease \
      NO_PYTHON=YesPlease \
      NO_INSTALL_HARDLINKS=YesPlease \
      INSTALL_SYMLINKS=YesPlease \
      SKIP_DASHED_BUILT_INS=YesPlease
  )
}

build_arch arm64
build_arch x86_64

# --- 3. lipo で universal binary にする ---
log "universal binary を作成します"
UNI="${WORK}/uni"
mkdir -p "$UNI"
cp -R "${WORK}/out-arm64/." "$UNI/"

# arm64 側の Mach-O を、x86_64 側の同じパスのものと結合する（シンボリックリンクと通常ファイルは対象外）
while IFS= read -r -d '' file; do
  rel="${file#"${WORK}/out-arm64/"}"
  other="${WORK}/out-x86_64/${rel}"
  [ -f "$other" ] || die "x86_64 側に ${rel} がありません"
  if lipo -info "$file" >/dev/null 2>&1; then
    lipo -create "$file" "$other" -output "${UNI}/${rel}"
  fi
done < <(find "${WORK}/out-arm64" -type f -perm -u+x -print0)

# 同梱しないもの（Hikae が使わない実行ファイル、man・doc・メッセージ・不要な補完）を削る
rm -f "${UNI}/bin/scalar" "${UNI}/bin/git-shell"
rm -rf "${UNI}/share/man" "${UNI}/share/doc" "${UNI}/share/locale" "${UNI}/share/bash-completion" \
  "${UNI}/share/gitweb" "${UNI}/share/git-gui" "${UNI}/share/gitk" "${UNI}/share/perl5"

# --- 4. 動作確認（universal binary で最小の流れを実行する） ---
GIT_BIN="${UNI}/bin/git"
[ -x "$GIT_BIN" ] || die "bin/git がありません"
lipo -info "$GIT_BIN" | grep -q "arm64" || die "arm64 が含まれていません"
lipo -info "$GIT_BIN" | grep -q "x86_64" || die "x86_64 が含まれていません"
[ -x "${UNI}/libexec/git-core/git-remote-https" ] || die "git-remote-https がありません"

SMOKE="${WORK}/smoke"
mkdir -p "$SMOKE"
sg() {
  env -i PATH="/usr/bin:/bin" HOME="$SMOKE" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_TERMINAL_PROMPT=0 LC_ALL=C "$GIT_BIN" -C "$SMOKE" "$@"
}
sg init -q .
sg config --local user.name "Hikae Smoke"
sg config --local user.email "smoke@example.invalid"
echo hello > "${SMOKE}/a.txt"
sg add a.txt
sg -c core.hooksPath= commit -q -m smoke
[ "$(sg log --format=%s)" = "smoke" ] || die "git log の結果が想定と違います"

# --- 5. 配置 ---
# ソースの著作権表示（GPLv2）も同梱する
cp "${SRC}/COPYING" "${UNI}/COPYING"
mkdir -p "$RESOURCES"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -R "${UNI}/." "$DEST/"
: > "${DEST}/.gitkeep"
printf '%s' "$MARKER_CONTENT" > "$MARKER"
log "配置しました: ${DEST}"
