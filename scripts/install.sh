#!/bin/sh
# Installe rst (Rocket Sim Trainer) dans ~/.local/bin : dernier binaire Linux x86_64 de la release GitHub,
# SHA256 vérifié. Usage :
#   curl -fsSL https://raw.githubusercontent.com/Elohyrr/rocketsim-trainer/main/scripts/install.sh | sh
# Ensuite : rst update pour les mises à jour. Variables : RST_INSTALL_DIR (défaut ~/.local/bin),
# RST_BASE_URL (défaut : dernière release ; pour les tests).
set -eu

REPO="Elohyrr/rocketsim-trainer"
ASSET="rst-linux-x86_64"
BASE="${RST_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
DEST="${RST_INSTALL_DIR:-${HOME:?HOME non défini}/.local/bin}"

die() { echo "erreur : $*" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || die "Linux uniquement"
case "$(uname -m)" in x86_64 | amd64) ;; *) die "x86_64 uniquement" ;; esac
command -v curl >/dev/null 2>&1 || die "curl est requis"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum est requis"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "téléchargement de $ASSET…"
curl -fsSL --retry 3 -o "$tmp/$ASSET" "$BASE/$ASSET" || die "téléchargement impossible (hors ligne ?)"
curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$BASE/SHA256SUMS" || die "SHA256SUMS introuvable"

expected="$(awk -v f="$ASSET" '{ n = $2; sub(/^\*/, "", n); if (n == f) print $1 }' "$tmp/SHA256SUMS")"
[ -n "$expected" ] || die "$ASSET absent de SHA256SUMS"
actual="$(sha256sum "$tmp/$ASSET" | awk '{ print $1 }')"
[ "$expected" = "$actual" ] || die "SHA256 différent (attendu $expected, obtenu $actual)"

mkdir -p "$DEST"
chmod 755 "$tmp/$ASSET"
# Même dossier + rename : remplacement atomique.
cp "$tmp/$ASSET" "$DEST/.rst.new.$$"
mv -f "$DEST/.rst.new.$$" "$DEST/rst"
echo "installé : $DEST/rst"

case ":$PATH:" in
  *":$DEST:"*) ;;
  *)
    echo
    echo "$DEST n'est pas dans votre PATH. Ajoutez à ~/.bashrc :"
    echo "  export PATH=\"$DEST:\$PATH\""
    ;;
esac
echo "Lancez : rst   (rst --help pour les commandes, rst update pour mettre à jour)"
