"""The Android app's protocol code (android/src/pttswitch/app/Protocol.java)
against the firmware's: the same UUIDs and company ID as
firmware/src/protocol.rs and docs/BLE.md, and the touch values the firmware
sends parsed the way the firmware means them. Protocol.java avoids Android
classes, so it runs on the desktop JVM.
"""

import re
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
APP = ROOT / "android"
PROTOCOL_JAVA = APP / "src/pttswitch/app/Protocol.java"
PROTOCOL_RS = ROOT / "firmware/src/protocol.rs"
BLE_DOC = ROOT / "docs/BLE.md"


def java_uuids():
    src = PROTOCOL_JAVA.read_text()
    return dict(re.findall(r'(\w+) = UUID\.fromString\("([0-9a-f-]{36})"\)', src))


def firmware_uuids():
    src = PROTOCOL_RS.read_text()
    return dict(re.findall(r'pub const (\w+)_TEXT: &\[u8; 36\] = b"([0-9a-f-]{36})"', src))


def test_uuids_match_the_firmware_and_docs():
    app, fw = java_uuids(), firmware_uuids()
    doc = BLE_DOC.read_text()
    for name in ("SERVICE", "TOUCH"):
        assert app[name] == fw[name], name
        assert app[name] in doc, name
    # The standard Client Characteristic Configuration descriptor.
    assert app["CCC"] == "00002902-0000-1000-8000-00805f9b34fb"


def test_company_id_matches_the_firmware():
    app = re.search(r"COMPANY_ID = (0x[0-9A-Fa-f]+);", PROTOCOL_JAVA.read_text())
    fw = re.search(r"pub const COMPANY_ID: u16 = (0x[0-9A-Fa-f]+);", PROTOCOL_RS.read_text())
    assert app and fw
    assert int(app.group(1), 16) == int(fw.group(1), 16) == 0xFFFF


@pytest.fixture(scope="module")
def check(tmp_path_factory):
    if not (shutil.which("javac") and shutil.which("java")):
        pytest.skip("needs a JDK (javac, java)")
    out = tmp_path_factory.mktemp("classes")
    subprocess.run(["javac", "--release", "8", "-Xlint:-options", "-d", str(out),
                    str(PROTOCOL_JAVA), str(APP / "test/pttswitch/app/ProtocolCheck.java")],
                   check=True)

    def run(*values):
        r = subprocess.run(["java", "-cp", str(out), "pttswitch.app.ProtocolCheck", "touch",
                            *values], check=True, capture_output=True, text=True)
        return r.stdout.splitlines()

    return run


def test_touch_values(check):
    # [touched, changes], as the firmware's touch_value() builds them; the
    # change count is unsigned and wraps at 256.
    assert check("0000", "0107", "00ff", "01fe") == [
        "released 0", "touched 7", "released 255", "touched 254"]


def test_other_values_are_refused(check):
    # Wrong length, or a first byte that is neither 0 nor 1: another
    # firmware, which the app must not show as a state.
    assert check("", "01", "010203", "0200", "ff00") == ["invalid"] * 5
