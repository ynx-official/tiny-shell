"""Exercise the embedded updater in Windows PowerShell without running an installer."""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "src/app/updater/windows_setup_update.ps1"
POWERSHELL = shutil.which("powershell.exe")


def ps_literal(value: object) -> str:
    return "'" + str(value).replace("'", "''") + "'"


@unittest.skipUnless(os.name == "nt" and POWERSHELL, "requires Windows PowerShell")
class WindowsSetupUpdateTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(prefix="tiny-shell-setup-test-")
        self.root = Path(self.temp.name).resolve()
        self.assertEqual(self.root.parent, Path(tempfile.gettempdir()).resolve())
        self.addCleanup(self.temp.cleanup)
        self.current = self.root / "安装 O'Brien [test]" / "tiny-shell.exe"
        self.current.parent.mkdir()
        self.current.write_bytes(b"fixture: never execute")
        self.installer = self.root / "download setup.exe"
        self.installer.write_bytes(b"fixture: never execute")
        self.helper = self.root / "helper.ps1"
        self.log = self.root / "update.log"
        self.setup_log = self.root / "update.setup.log"
        self.events_file = self.root / "events.jsonl"

    def run_helper(self, exit_code=0, installed_version="2.2.0", launch_fails=False):
        values = {
            "current": self.current,
            "installer": self.installer,
            "script": self.helper,
            "log": self.log,
            "setupLog": self.setup_log,
            "expectedVersion": "2.2.0",
            "failureTitle": "安装失败",
            "failureDescription": "更新未完成，请查看安装日志。",
            "eventsFile": self.events_file,
            "fixtureVersion": installed_version,
        }
        preamble = "\n".join(f"${key} = {ps_literal(value)}" for key, value in values.items())
        preamble += f"\n$fixtureExitCode = {exit_code}\n"
        preamble += f"$fixtureLaunchFails = ${str(launch_fails).lower()}\n"
        preamble += r"""
$ErrorActionPreference = 'Stop'
$applicationProcessId = 4242
function Record-Event($value) {
    Add-Content -LiteralPath $eventsFile -Value (ConvertTo-Json -InputObject $value -Compress -Depth 5) -Encoding UTF8
}
function Wait-Process {
    param($Id, $ErrorAction)
    Record-Event @{ event = 'wait-parent'; id = $Id }
}
function Start-Process {
    param($FilePath, $ArgumentList, $WindowStyle, $WorkingDirectory, [switch]$Wait, [switch]$PassThru)
    Record-Event @{ event = 'start'; file = $FilePath; arguments = @($ArgumentList); window = "$WindowStyle"; waitTree = [bool]$Wait }
    if ($fixtureLaunchFails) { throw 'fixture launch denied' }
    $process = [pscustomobject]@{ ExitCode = $null; Handle = 1; FinalExitCode = $fixtureExitCode }
    $process | Add-Member ScriptMethod WaitForExit {
        Record-Event @{ event = 'wait-setup' }
        $this.ExitCode = $this.FinalExitCode
    }
    $process | Add-Member ScriptMethod Refresh {}
    $process | Add-Member ScriptMethod Dispose {}
    if ($Wait) { $process.WaitForExit() }
    return $process
}
function Get-Item {
    param($LiteralPath)
    if ($LiteralPath -ne $current) { throw 'unexpected version lookup' }
    return [pscustomobject]@{ VersionInfo = [pscustomobject]@{ ProductVersion = $fixtureVersion } }
}
function New-Object {
    param($ComObject)
    if ($ComObject -ne 'WScript.Shell') { throw 'unexpected COM object' }
    $shell = [pscustomobject]@{}
    $shell | Add-Member ScriptMethod Popup {
        param($message, $timeout, $title, $kind)
        Record-Event @{ event = 'popup'; message = $message; title = $title }
        return 1
    }
    return $shell
}
"""
        # Production also writes a BOM: Windows PowerShell 5.1 otherwise misreads Unicode paths.
        self.helper.write_text(preamble + SCRIPT.read_text(encoding="utf-8"), encoding="utf-8-sig")
        result = subprocess.run(
            [POWERSHELL, "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(self.helper)],
            capture_output=True,
            timeout=20,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        events = [json.loads(line) for line in self.events_file.read_text(encoding="utf-8-sig").splitlines()]
        log = self.log.read_text(encoding="utf-8-sig")
        return result.returncode, events, log

    def test_visible_wizard_uses_original_directory_and_separate_install_log(self):
        code, events, log = self.run_helper()
        self.assertEqual(code, 0)
        starts = [event for event in events if event["event"] == "start"]
        self.assertEqual(len(starts), 1, "only the installer may launch the updated app")
        start = starts[0]
        self.assertEqual(start["file"], str(self.installer))
        self.assertEqual(start["window"], "Normal")
        self.assertFalse(start["waitTree"], "do not wait for the app launched by the wizard")
        self.assertIn(f'/DIR="{self.current.parent}"', start["arguments"])
        self.assertIn(f'/LOG="{self.setup_log}"', start["arguments"])
        self.assertIn("/NORESTART", start["arguments"])
        self.assertIn("/RESTARTEXITCODE=3010", start["arguments"])
        self.assertFalse({"/SILENT", "/VERYSILENT", "/SUPPRESSMSGBOXES"} & set(start["arguments"]))
        self.assertEqual(events[0], {"event": "wait-parent", "id": 4242})
        self.assertIn("setup completed and installed version verified", log)
        self.assertFalse(self.installer.exists())
        self.assertFalse(self.helper.exists())
        self.assertTrue(self.current.exists())

    def test_cancel_before_install_preserves_package_without_relaunch(self):
        code, events, log = self.run_helper(exit_code=2)
        self.assertEqual(code, 2)
        self.assertIn("cancelled or aborted", log)
        self.assertTrue(self.installer.exists())
        self.assertEqual(len([e for e in events if e["event"] == "start"]), 1)
        self.assertFalse(any(e["event"] == "popup" for e in events))

    def test_abort_during_install_is_not_reported_as_success(self):
        code, events, log = self.run_helper(exit_code=5)
        self.assertEqual(code, 5)
        self.assertIn("setup exited with code 5", log)
        self.assertNotIn("version verified", log)
        self.assertTrue(self.installer.exists())
        self.assertEqual(len([e for e in events if e["event"] == "start"]), 1)

    def test_install_error_preserves_package_and_points_to_logs(self):
        code, events, log = self.run_helper(exit_code=4)
        self.assertNotEqual(code, 0)
        self.assertIn("setup exited with code 4", log)
        self.assertIn(str(self.setup_log), log)
        self.assertTrue(self.installer.exists())
        self.assertEqual(len([e for e in events if e["event"] == "start"]), 1)
        self.assertTrue(any(e["event"] == "popup" for e in events))

    def test_launch_error_is_visible_and_retains_package(self):
        code, events, log = self.run_helper(launch_fails=True)
        self.assertNotEqual(code, 0)
        self.assertIn("fixture launch denied", log)
        self.assertTrue(self.installer.exists())
        self.assertTrue(any(e["event"] == "popup" for e in events))

    def test_version_mismatch_never_deletes_package_or_starts_old_app(self):
        code, events, log = self.run_helper(installed_version="2.1.4")
        self.assertNotEqual(code, 0)
        self.assertIn("version mismatch", log)
        self.assertTrue(self.installer.exists())
        self.assertEqual(len([e for e in events if e["event"] == "start"]), 1)

    def test_reboot_required_preserves_package_without_false_success(self):
        code, events, log = self.run_helper(exit_code=3010, installed_version="2.1.4")
        self.assertEqual(code, 3010)
        self.assertIn("system restart required", log)
        self.assertNotIn("version mismatch", log)
        self.assertNotIn("version verified", log)
        self.assertTrue(self.installer.exists())
        self.assertEqual(len([e for e in events if e["event"] == "start"]), 1)


if __name__ == "__main__":
    unittest.main()
