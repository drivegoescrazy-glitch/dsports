#!/usr/bin/env python3
"""Static regression checks for the Android-embedded Termux-compatible runtime."""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print("FAIL:", message, file=sys.stderr)
    raise SystemExit(1)


gradle = (ROOT / "app/build.gradle.kts").read_text(encoding="utf-8")
kotlin = (ROOT / "app/src/main/java/com/dsports/library/EmbeddedToolchain.kt").read_text(encoding="utf-8")
builder = (ROOT / "app/src/main/assets/dsports_embedded_builder.py").read_text(encoding="utf-8")
manifest = (ROOT / "app/src/main/AndroidManifest.xml").read_text(encoding="utf-8")

match = re.search(r'applicationId\s*=\s*"([^"]+)"', gradle)
if not match:
    fail("applicationId not found in app/build.gradle.kts")
app_id = match.group(1)
old_root = "/data/data/com.termux"
new_root = "/data/data/" + app_id

if len(old_root) != len(new_root):
    fail(f"package root length must remain constant for binary relocation: {old_root!r} -> {new_root!r}")
if 'private const val OLD_APP_ROOT = "/data/data/com.termux"' not in kotlin:
    fail("embedded runtime must define the stock package root as the relocation source")
if 'private const val OLD_PACKAGE_NAME = "com.termux"' not in kotlin:
    fail("embedded runtime must define the stock package-name relocation source")
if "OLD_PACKAGE_NAME.toByteArray(Charsets.UTF_8) to context.packageName.toByteArray(Charsets.UTF_8)" not in kotlin:
    fail("embedded runtime must relocate bare package-name strings inside binaries")
if "RandomAccessFile(path.toFile(), \"rw\")" not in kotlin:
    fail("runtime path rewriting must scan in blocks to avoid loading large compiler binaries into memory")
if 'private val appRoot = File("/data/data/${context.packageName}")' not in kotlin:
    fail("embedded runtime must derive its destination root from the installed package name")
for required in (
    "relocateAppRootReferences(prefix.toPath(), log)",
    "private fun relocateAppRootReferences(root: Path, log: (String) -> Unit)",
    'environment()["TERMUX_APP__DATA_DIR"] = appRoot.absolutePath',
    'environment()["TERMUX__PROJECT_DIR"] = termuxProjectDir.absolutePath',
    'environment()["TERMUX__CORE_DIR"] = termuxCoreDir.absolutePath',
    'environment()["TERMUX__APPS_DIR"] = termuxAppsDir.absolutePath',
    'environment()["TERMUX__APPS_DIR_BY_UID"] = termuxAppsByUidDir.absolutePath',
    'environment()["TERMUX__LEGACY_PROJECT_USER_CONFIG_DIR"] = termuxLegacyUserConfigDir.absolutePath',
    '"-o Dir::Cache::archives="',
    'File(aptArchives, "partial").mkdirs()',
    'runAptCommand("update", log)',
):
    if required not in kotlin:
        fail(f"runtime is missing required path/cache recovery logic: {required}")
if 'recentOutput.joinToString("\\\\n")' in kotlin:
    fail("diagnostics still encode literal backslash-n instead of actual line breaks")

expected_prefix = new_root + "/files/usr"
fallback_match = re.search(r'prefix = os\.environ\.get\("PREFIX", "([^"]+)"\)', builder)
if not fallback_match:
    fail("embedded builder does not define a PREFIX fallback")
if fallback_match.group(1) != expected_prefix:
    fail(f"builder fallback prefix mismatch: expected {expected_prefix!r}, got {fallback_match.group(1)!r}")

for required in (
    "def patch_android_ndk_toolchain(ndk_dir, log_fn)",
    "patch_android_ndk_toolchain(ndk, log_fn)",
    '"ANDROID_C_COMPILER": "clang"',
    '"ANDROID_CXX_COMPILER": "clang++"',
    '"ANDROID_ASM_COMPILER": "clang"',
    '"ANDROID_AR": "llvm-ar"',
    '"ANDROID_RANLIB": "llvm-ranlib"',
    '"ANDROID_STRIP": "llvm-strip"',
    'toolchain_file.write_text(text, encoding="utf-8")',
):
    if required not in builder:
        fail(f"embedded builder is missing its native-ARM64 NDK toolchain patch: {required}")

if "com.termux.RUN_COMMAND" in manifest or "com.termux.permission.RUN_COMMAND" in manifest:
    fail("manifest still declares an external Termux command dependency")
if (ROOT / "app/src/main/assets/dsports_termux_builder.py").exists():
    fail("obsolete Termux-dependent builder still exists as an app asset")

print(f"OK: DSports {app_id} has length-safe app-root relocation, a private apt cache,")
print("    active embedded builder prefix, and no legacy Termux external-command dependency.")
