#!/usr/bin/env bash
# Télécharge les 16 meshes de collision soccar (mesh_0.cmf..mesh_15.cmf) depuis le sdist PyPI
# rlgym-rocket-league 2.0.1, et vérifie leur intégrité (sha256 de l'archive, puis de chaque fichier).
#
#   scripts/fetch-meshes.sh [dossier-de-sortie]   # défaut : $ROCKETSIM_MESHES_DIR ou ./collision_meshes
#
# Résultat : <dossier>/soccar/mesh_*.cmf. Idempotent : ne fait rien si tout est déjà présent et valide.
# Ne jamais commiter ces fichiers (géométrie Psyonix) : collision_meshes/ est ignoré par git.
set -euo pipefail

VERSION=2.0.1
URL="https://files.pythonhosted.org/packages/99/21/b87ef7ebb0d76e654753349a4f343068d08e1a9eaffd9c77c0593b43668f/rlgym-rocket-league-${VERSION}.tar.gz"
ARCHIVE_SHA256=3be8e9d2f1cfb6514e0e3d455fb33ff95e26eb7c399f819fff0258025cf4d501
SRC_PATH="rlgym-rocket-league-${VERSION}/rlgym/rocket_league/sim/collision_meshes/soccar"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sums="$here/meshes.sha256"
out="${1:-${ROCKETSIM_MESHES_DIR:-collision_meshes}}"
dest="$out/soccar"

verify() { (cd "$dest" && sha256sum --check --quiet "$sums" >/dev/null 2>&1); }

if [ -d "$dest" ] && verify; then
  echo "meshes déjà présents et valides : $dest"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "téléchargement de rlgym-rocket-league ${VERSION}…"
curl -fsSL --retry 3 -o "$tmp/sdist.tar.gz" "$URL"
actual="$(sha256sum "$tmp/sdist.tar.gz" | cut -d' ' -f1)"
if [ "$actual" != "$ARCHIVE_SHA256" ]; then
  echo "ERREUR : sha256 de l'archive différent (attendu $ARCHIVE_SHA256, obtenu $actual)" >&2
  exit 1
fi

mkdir "$tmp/x"
files=()
for i in $(seq 0 15); do files+=("$SRC_PATH/mesh_$i.cmf"); done
tar -xzf "$tmp/sdist.tar.gz" -C "$tmp/x" --strip-components=6 "${files[@]}"

# Vérifier avant d'installer : on n'écrase rien si une empreinte diffère.
if ! (cd "$tmp/x" && sha256sum --check --quiet "$sums"); then
  echo "ERREUR : empreinte(s) de mesh différente(s), rien n'a été installé." >&2
  exit 1
fi

mkdir -p "$dest"
cp "$tmp"/x/mesh_*.cmf "$dest/"
verify || { echo "ERREUR : vérification finale échouée dans $dest" >&2; exit 1; }
echo "16 meshes installés et vérifiés : $dest"
