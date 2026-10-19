#!/usr/bin/env python3
"""Exercise the on-device dpkg wrapper against a synthetic Termux-style .deb."""
from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
WRAPPER_TEMPLATE = ROOT / "app/src/main/assets/dsports_dpkg_wrapper.sh"
GRADLE_CONFIG = (ROOT / "app/build.gradle.kts").read_text(encoding="utf-8")
PACKAGE_MATCH = re.search(r'applicationId\s*=\s*"([^"]+)"', GRADLE_CONFIG)
if not PACKAGE_MATCH:
    raise RuntimeError("Could not derive the DSports applicationId for the wrapper test.")
OLD_ROOT = "/data/data/com.termux"
NEW_PACKAGE = PACKAGE_MATCH.group(1)
NEW_ROOT = "/data/data/" + NEW_PACKAGE
if len(OLD_ROOT) != len(NEW_ROOT):
    raise RuntimeError("The DSports package root must match the upstream Termux root length.")


def require_command(name: str) -> str:
    result = shutil.which(name)
    if not result:
        raise RuntimeError(f"Required test command is missing: {name}")
    return result


def run(*args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(args, text=True, capture_output=True, env=env)
    if result.returncode:
        raise RuntimeError(
            f"Command failed ({result.returncode}): {' '.join(args)}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def main() -> None:
    dpkg_deb = require_command("dpkg-deb")
    with tempfile.TemporaryDirectory(prefix="dsports-dpkg-test-") as temp:
        base = Path(temp)
        prefix = base / "prefix"
        bin_dir = prefix / "bin"
        bin_dir.mkdir(parents=True)
        tmp_dir = base / "tmp"
        tmp_dir.mkdir()
        for name in (
            "bash", "dpkg-deb", "mktemp", "mkdir", "cp", "rm", "grep",
            "sed", "find", "readlink", "ln", "chmod", "basename",
        ):
            source = Path(require_command(name))
            link = bin_dir / name
            link.symlink_to(source)

        real_dpkg = bin_dir / "dpkg.real"
        captured_deb = base / "captured.deb"
        real_dpkg.write_text(
            "#!/usr/bin/env bash\n"
            "for arg in \"$@\"; do\n"
            "  if [[ \"$arg\" == *.deb && -f \"$arg\" ]]; then\n"
            "    cp \"$arg\" \"$CAPTURED_DEB\"\n"
            "  fi\n"
            "done\n"
            "exit 0\n",
            encoding="utf-8",
        )
        real_dpkg.chmod(0o755)

        wrapper_text = WRAPPER_TEMPLATE.read_text(encoding="utf-8")
        for key, value in {
            "@@PREFIX@@": str(prefix),
            "@@OLD_ROOT@@": OLD_ROOT,
            "@@NEW_ROOT@@": NEW_ROOT,
            "@@PACKAGE_NAME@@": NEW_PACKAGE,
        }.items():
            wrapper_text = wrapper_text.replace(key, value)
        wrapper = bin_dir / "dpkg"
        wrapper.write_text(wrapper_text, encoding="utf-8")
        wrapper.chmod(0o700)

        package_root = base / "package"
        deb_data = package_root / "data/data/com.termux/files/usr/share/dsports-test"
        deb_control = package_root / "DEBIAN"
        deb_data.mkdir(parents=True)
        deb_control.mkdir(parents=True)
        (deb_data / "paths.txt").write_text(
            f"shell={OLD_ROOT}/files/usr/bin/sh\n"
            f"home={OLD_ROOT}/files/home/.termux\n"
            "package=com.termux\n",
            encoding="utf-8",
        )
        (deb_control / "control").write_text(
            "Package: dsports-path-test\n"
            "Version: 1.0\n"
            "Architecture: all\n"
            "Maintainer: DSports CI <ci@example.invalid>\n"
            "Description: test package used only for runtime wrapper validation\n",
            encoding="utf-8",
        )
        postinst = deb_control / "postinst"
        postinst.write_text(
            f"#!/data/data/com.termux/files/usr/bin/sh\n"
            f"mkdir -p {OLD_ROOT}/files/home/.termux\n"
            f"echo com.termux\n",
            encoding="utf-8",
        )
        postinst.chmod(0o755)

        original_deb = base / "dsports-path-test.deb"
        run(dpkg_deb, "--build", str(package_root), str(original_deb))

        env = os.environ.copy()
        env["TMPDIR"] = str(tmp_dir)
        env["CAPTURED_DEB"] = str(captured_deb)
        env["PATH"] = str(bin_dir) + os.pathsep + env.get("PATH", "")
        result = subprocess.run(
            [str(wrapper), "--install", str(original_deb)],
            text=True,
            capture_output=True,
            env=env,
        )
        if result.returncode:
            raise RuntimeError(
                f"dpkg wrapper failed ({result.returncode}):\n"
                f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
            )
        if not captured_deb.is_file():
            raise RuntimeError("The wrapper did not pass a rewritten .deb to dpkg.real.")

        listing = run(dpkg_deb, "--contents", str(captured_deb)).stdout
        if "data/data/com.dsprtl/files/usr/share/dsports-test/paths.txt" not in listing:
            raise RuntimeError("The rewritten .deb still has the stock Termux data path.")

        extracted = base / "extracted"
        run(dpkg_deb, "--extract", str(captured_deb), str(extracted))
        config = extracted / "data/data/com.dsprtl/files/usr/share/dsports-test/paths.txt"
        data_text = config.read_text(encoding="utf-8")
        if OLD_ROOT in data_text or "com.termux" in data_text:
            raise RuntimeError("The rewritten .deb still contains the stock package root in a text file.")
        if NEW_ROOT not in data_text or NEW_PACKAGE not in data_text:
            raise RuntimeError("The rewritten text file is missing the DSports package root.")

        control_dir = base / "control"
        run(dpkg_deb, "--control", str(captured_deb), str(control_dir))
        script_text = (control_dir / "postinst").read_text(encoding="utf-8")
        if OLD_ROOT in script_text or "com.termux" in script_text:
            raise RuntimeError("The rewritten postinst still points at stock Termux.")
        if NEW_ROOT not in script_text or NEW_PACKAGE not in script_text:
            raise RuntimeError("The rewritten postinst is missing the DSports package root.")

        print("OK: dpkg wrapper rewrites .deb data paths, text files, and maintainer scripts.")


if __name__ == "__main__":
    main()
