#!/usr/bin/env python3
"""Static regression checks for the Android-embedded Termux-compatible runtime."""
from pathlib import Path
import ast
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    print("FAIL:", message, file=sys.stderr)
    raise SystemExit(1)


gradle = (ROOT / "app/build.gradle.kts").read_text(encoding="utf-8")
kotlin = (ROOT / "app/src/main/java/com/dsports/library/EmbeddedToolchain.kt").read_text(encoding="utf-8")
builder = (ROOT / "app/src/main/assets/dsports_embedded_builder.py").read_text(encoding="utf-8")
dpkg_wrapper = (ROOT / "app/src/main/assets/dsports_dpkg_wrapper.sh").read_text(encoding="utf-8")
alternatives_wrapper = (ROOT / "app/src/main/assets/dsports_update_alternatives_wrapper.sh").read_text(encoding="utf-8")
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
if 'recentOutput.joinToString("\\\\n")' in kotlin:
    fail("diagnostics still encode literal backslash-n instead of actual line breaks")
if 'ProcessBuilder(linker, shell.absolutePath, "-lc", command)' in kotlin:
    fail("embedded shell still starts login mode and can read stock Termux profile paths")
if "sample.any { it == 0.toByte() }" not in kotlin:
    fail("runtime relocation must skip binary files containing NUL bytes")
if "ensureDpkgWrapper(log)" not in kotlin or "ensureUpdateAlternativesWrapper(log)" not in kotlin:
    fail("dpkg and update-alternatives wrappers are not enabled")
expected_prefix = new_root + "/files/usr"
fallback_match = re.search(r'prefix = os\.environ\.get\("PREFIX", "([^"]+)"\)', builder)
if not fallback_match:
    fail("embedded builder does not define a PREFIX fallback")
if fallback_match.group(1) != expected_prefix:
    fail(f"builder fallback prefix mismatch: expected {expected_prefix!r}, got {fallback_match.group(1)!r}")

# Execute the NDK patch function against a representative copy of the real
# multi-line NDK assignments. This verifies both its matching logic and that it
# actually writes the modified toolchain file back to disk.
try:
    builder_tree = ast.parse(builder)
    patch_function = next(
        node for node in builder_tree.body
        if isinstance(node, ast.FunctionDef) and node.name == "patch_android_ndk_toolchain"
    )
    isolated = ast.Module(body=[patch_function], type_ignores=[])
    namespace = {"Path": Path}
    exec(compile(isolated, "patch_android_ndk_toolchain", "exec"), namespace)
except (SyntaxError, StopIteration) as exc:
    fail(f"could not isolate the NDK toolchain patch for testing: {exc}")

with tempfile.TemporaryDirectory() as temp_dir:
    toolchain = Path(temp_dir) / "build/cmake/android-legacy.toolchain.cmake"
    toolchain.parent.mkdir(parents=True)
    toolchain.write_text(
        'set(ANDROID_C_COMPILER\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/clang\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(ANDROID_CXX_COMPILER\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/clang++\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(ANDROID_ASM_COMPILER\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/clang\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(ANDROID_AR\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/llvm-ar\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(ANDROID_RANLIB\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/llvm-ranlib\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(ANDROID_STRIP\n  "\\u0024{ANDROID_TOOLCHAIN_ROOT}/bin/llvm-strip\\u0024{ANDROID_TOOLCHAIN_SUFFIX}")\n'
        'set(CMAKE_SYSROOT "/test/ndk/sysroot")\n',
        encoding="utf-8",
    )
    messages = []
    namespace["patch_android_ndk_toolchain"](temp_dir, messages.append)
    patched = toolchain.read_text(encoding="utf-8")
    for variable, executable in (
        ("ANDROID_C_COMPILER", "clang"),
        ("ANDROID_CXX_COMPILER", "clang++"),
        ("ANDROID_ASM_COMPILER", "clang"),
        ("ANDROID_AR", "llvm-ar"),
        ("ANDROID_RANLIB", "llvm-ranlib"),
        ("ANDROID_STRIP", "llvm-strip"),
    ):
        expected = f'set({variable} "$ENV{{PREFIX}}/bin/{executable}")'
        if expected not in patched:
            fail(f"NDK patch did not persist the expected assignment: {expected}")
    if 'set(CMAKE_SYSROOT "/test/ndk/sysroot")' not in patched:
        fail("NDK patch must preserve the target sysroot")
    if not messages:
        fail("NDK patch did not report that its work finished")

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

for required in (
    "\u0024PREFIX/bin/dpkg-deb",
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
if "ensureDpkgWrapper(log)" not in kotlin or "ensureUpdateAlternativesWrapper(log)" not in kotlin:
    fail("runtime does not install both package compatibility wrappers")
if "OLD_PACKAGE_NAME.toByteArray(Charsets.UTF_8) to context.packageName.toByteArray(Charsets.UTF_8)" not in kotlin:
    fail("runtime is missing same-length package-name relocation")

if "com.termux.RUN_COMMAND" in manifest or "com.termux.permission.RUN_COMMAND" in manifest:
    fail("manifest still declares an external Termux command dependency")
if (ROOT / "app/src/main/assets/dsports_termux_builder.py").exists():
    fail("obsolete Termux-dependent builder still exists as an app asset")

print(f"OK: DSports {app_id} has length-safe app-root relocation, a private apt cache,")
print("    active embedded builder prefix, and no legacy Termux external-command dependency.")
