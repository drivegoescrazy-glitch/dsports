#!/@@PREFIX@@/bin/bash
# Keep update-alternatives state inside the DSports package prefix.
set -e
PREFIX='@@PREFIX@@'
mkdir -p "$PREFIX/etc/alternatives" "$PREFIX/var/lib/dpkg/alternatives" "$PREFIX/var/log"
exec "$PREFIX/bin/update-alternatives.real" \
  --altdir "$PREFIX/etc/alternatives" \
  --admindir "$PREFIX/var/lib/dpkg/alternatives" \
  --log "$PREFIX/var/log/alternatives.log" \
  "$@"
