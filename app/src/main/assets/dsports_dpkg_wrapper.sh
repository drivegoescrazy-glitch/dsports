#!/@@PREFIX@@/bin/bash
# Repack Termux-origin packages for DSports' private package root before dpkg runs.
set +e

PREFIX='@@PREFIX@@'
OLD_ROOT='@@OLD_ROOT@@'
NEW_ROOT='@@NEW_ROOT@@'
OLD_PACKAGE='com.termux'
NEW_PACKAGE='@@PACKAGE_NAME@@'
REAL_DPKG="$PREFIX/bin/dpkg.real"
DPKG_DEB="$PREFIX/bin/dpkg-deb"
TMPROOT="${TMPDIR:-$PREFIX/tmp}"
mkdir -p "$TMPROOT" "$PREFIX/var/lib/dpkg" "$PREFIX/share/dpkg" 2>/dev/null
export DPKG_ADMINDIR="$PREFIX/var/lib/dpkg"
export DPKG_DATADIR="$PREFIX/share/dpkg"
declare -a new_args=()
declare -a cleanup_dirs=()

rewrite_package() {
    local source_deb="$1"
    local work_dir root old_dir new_dir file link target fixed rewritten
    work_dir="$("$PREFIX/bin/mktemp" -d "$TMPROOT/dsports-dpkg.XXXXXX" 2>/dev/null)"
    if [ -z "$work_dir" ]; then
        echo "[DSports dpkg] Could not create package staging directory; using original archive." >&2
        echo "$source_deb"
        return 0
    fi

    root="$work_dir/pkg_root"
    if ! "$DPKG_DEB" --raw-extract "$source_deb" "$root" >/dev/null 2>&1; then
        echo "[DSports dpkg] Could not extract $(basename "$source_deb"); using original archive." >&2
        "$PREFIX/bin/rm" -rf "$work_dir"
        echo "$source_deb"
        return 0
    fi

    old_dir="$root/data/data/$OLD_PACKAGE"
    new_dir="$root/data/data/$NEW_PACKAGE"
    if [ -d "$old_dir" ]; then
        "$PREFIX/bin/mkdir" -p "$new_dir"
        "$PREFIX/bin/cp" -a "$old_dir/." "$new_dir/" 2>/dev/null
        "$PREFIX/bin/rm" -rf "$old_dir"
    fi

    # Only alter files grep identifies as text. Do not patch ELF executables,
    # shared libraries, or other binary payloads.
    while IFS= read -r -d '' file; do
        if "$PREFIX/bin/grep" -Iq 'com[.]termux' "$file" 2>/dev/null; then
            # Escape periods explicitly; in a sed expression, the unescaped dot
            # in "com.termux" would otherwise match any character.
            "$PREFIX/bin/sed" -i \
                -e "s|/data/data/com[.]termux|$NEW_ROOT|g" \
                -e "s|com[.]termux|$NEW_PACKAGE|g" "$file" 2>/dev/null
        fi
    done < <("$PREFIX/bin/find" "$root" -type f -print0 2>/dev/null)

    while IFS= read -r -d '' link; do
        target="$("$PREFIX/bin/readlink" "$link" 2>/dev/null)"
        fixed="${target//$OLD_PACKAGE/$NEW_PACKAGE}"
        if [ "$fixed" != "$target" ]; then
            "$PREFIX/bin/rm" -f "$link"
            "$PREFIX/bin/ln" -s "$fixed" "$link"
        fi
    done < <("$PREFIX/bin/find" "$root" -type l -print0 2>/dev/null)

    # Keep maintainer scripts executable after sed rewrites.
    for file in "$root"/DEBIAN/*.preinst "$root"/DEBIAN/*.postinst \
                "$root"/DEBIAN/*.prerm "$root"/DEBIAN/*.postrm \
                "$root"/DEBIAN/*.config "$root"/DEBIAN/*.triggers; do
        [ -f "$file" ] && "$PREFIX/bin/chmod" 0755 "$file" 2>/dev/null
    done

    rewritten="$work_dir/rewritten.deb"
    if "$DPKG_DEB" -Zgzip -z1 --build "$root" "$rewritten" >/dev/null 2>&1; then
        echo "[DSports dpkg] Repacked $(basename "$source_deb") for the local app root." >&2
        echo "$rewritten"
    else
        echo "[DSports dpkg] Could not repack $(basename "$source_deb"); using original archive." >&2
        "$PREFIX/bin/rm" -rf "$work_dir"
        echo "$source_deb"
    fi
}

# APT supplies .deb arguments when it calls dpkg for unpack/install. Other dpkg
# queries have no archives and pass straight through unchanged.
for arg in "$@"; do
    if [[ "$arg" == *.deb && -f "$arg" ]]; then
        rewritten="$(rewrite_package "$arg")"
        if [ "$rewritten" != "$arg" ]; then
            # The function runs in command substitution, so cleanup bookkeeping
            # must be added in this parent shell rather than inside the function.
            cleanup_dirs+=("${rewritten%/*}")
        fi
        new_args+=("$rewritten")
    else
        new_args+=("$arg")
    fi
done

"$REAL_DPKG" "${new_args[@]}"
result=$?
for work_dir in "${cleanup_dirs[@]}"; do
    "$PREFIX/bin/rm" -rf "$work_dir" 2>/dev/null
done
exit "$result"
