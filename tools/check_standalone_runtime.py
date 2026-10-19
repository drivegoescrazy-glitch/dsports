#!/usr/bin/env python3
"""Static regression checks for the Android-embedded build runtime and the port builder."""
from pathlib import Path
import ast
import re
import sys

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print("FAIL:", message, file=sys.stderr)
    raise SystemExit(1)


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


gradle = read("app/build.gradle.kts")
kotlin = read("app/src/main/java/com/dsports/library/EmbeddedToolchain.kt")
builder = read("app/src/main/assets/dsports_embedded_builder.py")
catalog = read("app/src/main/java/com/dsports/library/GameCatalog.kt")
launcher = read("app/src/main/java/com/dsports/library/LauncherActivity.kt")
main_activity = read("app/src/main/java/com/dsports/library/MainActivity.kt")
dpkg_wrapper = read("app/src/main/assets/dsports_dpkg_wrapper.sh")
alternatives_wrapper = read("app/src/main/assets/dsports_update_alternatives_wrapper.sh")
manifest = read("app/src/main/AndroidManifest.xml")

# ---------------------------------------------------------------------------
# Package identity: relocation requires a package id the same length as com.termux.
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
    fail("embedded runtime must relocate package-name strings in text files")
if "Files.readSymbolicLink(file).toString()" not in kotlin or "Os.symlink(fixedTarget, file.toAbsolutePath().toString())" not in kotlin:
    fail("existing absolute symlink targets must also be relocated during migration")
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
    '"Dir::Cache::archives="',
    'File(aptArchives, "partial").mkdirs()',
    'runAptCommand("update", log)',
):
    if required not in kotlin:
        fail(f"runtime is missing required path/cache recovery logic: {required}")
if 'recentOutput.joinToString("\\\\\\\\n")' in kotlin:
    fail("diagnostics still encode literal backslash-n instead of actual line breaks")
if 'ProcessBuilder(linker, shell.absolutePath, "-lc", command)' in kotlin:
    fail("embedded shell still starts login mode and can read stock Termux profile paths")
if "sample.any { it == 0.toByte() }" not in kotlin:
    fail("runtime relocation must skip binary files containing NUL bytes")
if "ensureDpkgWrapper(log)" not in kotlin or "ensureUpdateAlternativesWrapper(log)" not in kotlin:
    fail("dpkg and update-alternatives wrappers are not enabled")

# ---------------------------------------------------------------------------
# The builder: pinned upstream, ARM64 host tools, display defaults, APK checks.
if 'TAG = "v0.4.2"' not in builder:
    fail("the builder must pin a specific SSRecomp tag")
if "https://github.com/ClickingMaster/ssrecomp.git" not in builder:
    fail("the builder must clone the SSRecomp source it recompiles")
for required in (
    "def patch_ndk_toolchain(ndk_dir, log_fn=None, prefix=None)",
    "def preflight_toolchain(ndk_dir, work_dir, log_fn=None)",
    "def patch_frontend_defaults(root, options, log_fn=None)",
    "def verify_apk(path, min_bytes=100_000)",
    "def create_embedded_android_steps(setup, options, args_output)",
    "patch_ndk_toolchain(ndk, log_fn)",
    "preflight_toolchain(ndk,",
    "patch_frontend_defaults(root, args)",
    "verify_apk(out)",
    '"ANDROID_C_COMPILER"',
    '"ANDROID_CXX_COMPILER"',
    '"ANDROID_ASM_COMPILER"',
    '"ANDROID_AR"',
    '"ANDROID_RANLIB"',
    '"ANDROID_STRIP"',
    "ANDROID_STL=c++_static",
    "CMAKE_FIND_ROOT_PATH_MODE_PACKAGE=BOTH",
    "ANDROID_USE_LEGACY_TOOLCHAIN_FILE=ON",
):
    if required not in builder:
        fail(f"builder is missing required on-device build logic: {required}")
if "-static-libstdc++" not in builder:
    fail("builder must strip the unsupported GCC-only -static-libstdc++ flag")
if "--no-widescreen" not in builder or "--layout" not in builder or "--resolution" not in builder:
    fail("builder does not accept the display options DSports passes it")

expected_prefix = new_root + "/files/usr"
if f'os.environ.get("PREFIX", "{expected_prefix}")' not in builder:
    fail(f"builder fallback prefix mismatch: expected {expected_prefix!r}")

# The builder must not depend on the NDK's x86_64 host binaries to run.
if "android-legacy.toolchain.cmake" not in builder:
    fail("builder no longer patches the NDK toolchain file it relies on")
if '"android.toolchain.cmake"' not in builder:
    fail("builder must pass the NDK toolchain file explicitly")

# Every patch pattern must be anchored so a silent no-op is impossible.
if "if len(matches) != 1:" not in builder:
    fail("frontend display defaults must fail the build when a pattern does not match exactly once")

# Nothing may be parsed at import time, otherwise the builder cannot be tested.
tree = ast.parse(builder)
for node in tree.body:
    if isinstance(node, ast.Assign) and any(
        isinstance(target, ast.Name) and target.id == "ARGS" for target in node.targets
    ):
        fail("builder still parses command-line arguments at import time")

# ---------------------------------------------------------------------------
# Launcher: the catalogue is the only entry point to the builder.
if "object GameCatalog" not in catalog:
    fail("missing supported-game catalogue")
if "soulsilver-us" not in catalog:
    fail("the catalogue must list Pokemon SoulSilver")
if "available = true" not in catalog:
    fail("SoulSilver must be marked as supported")
if "PortDisplay" not in catalog or "PortLayout" not in catalog or "PortResolution" not in catalog:
    fail("catalogue must define the port display settings")
if 'android:name=".LauncherActivity"' not in manifest:
    fail("manifest does not declare the launcher activity")
if 'android:name=".MainActivity"' not in manifest or 'android:exported="false"' not in manifest:
    fail("the builder activity must not be exported once the launcher owns the app entry point")
if "GameCatalog.games" not in launcher:
    fail("launcher must render the whole supported-game catalogue")
if "EXTRA_GAME_ID" not in main_activity:
    fail("the builder does not read the selected game")
if "toBuilderArguments()" not in main_activity:
    fail("the builder UI does not pass the chosen display settings to the builder")

# ---------------------------------------------------------------------------
# On-device package handling.
for required in (
    "$PREFIX/bin/dpkg-deb",
    "--raw-extract",
    "--build",
    "data/data/$OLD_PACKAGE",
    "DEBIAN/*.postinst",
    "grep",
    "sed",
):
    if required not in dpkg_wrapper:
        fail(f"dpkg wrapper is missing required archive rewriting logic: {required}")
if "@@NEW_ROOT@@" not in dpkg_wrapper or "@@PACKAGE_NAME@@" not in dpkg_wrapper:
    fail("dpkg wrapper package-root placeholders are missing")
for required in ("--altdir", "--admindir", "--log", "update-alternatives.real"):
    if required not in alternatives_wrapper:
        fail(f"update-alternatives wrapper is missing private state override: {required}")

if "com.termux.RUN_COMMAND" in manifest or "com.termux.permission.RUN_COMMAND" in manifest:
    fail("manifest still declares an external Termux command dependency")
if (ROOT / "app/src/main/assets/dsports_termux_builder.py").exists():
    fail("obsolete Termux-dependent builder still exists as an app asset")

print(f"OK: DSports {app_id} ships a launcher catalogue, display options, an ARM64 port builder,")
print("    private package relocation and no legacy Termux external-command dependency.")
