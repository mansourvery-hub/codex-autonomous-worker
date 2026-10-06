import unittest
import subprocess
import sys
from pathlib import Path
from unittest.mock import patch, MagicMock
from importlib.machinery import SourceFileLoader

class TestAutopilotSmoke(unittest.TestCase):
    def setUp(self):
        self.root = Path(__file__).resolve().parent.parent

    def test_autopilot_cli_commands(self):
        commands = [
            ["bin/autopilot", "--help"],
            ["bin/autopilot", "list"],
            ["bin/autopilot", "current"],
            ["bin/autopilot", "history"],
            ["bin/task", "--help"],
            ["bin/task", "list"]
        ]
        for cmd in commands:
            res = subprocess.run([sys.executable, str(self.root / cmd[0])] + cmd[1:], capture_output=True, text=True)
            self.assertEqual(res.returncode, 0, f"Command {' '.join(cmd)} failed with error:\n{res.stderr}")

    def test_cmd_tui_initialization_path(self):
        loader = SourceFileLoader("autopilot", str(self.root / "bin" / "autopilot"))
        autopilot = loader.load_module()

        with patch("subprocess.run") as mock_run:
            def mock_subp(cmd, **kwargs):
                ret = MagicMock()
                if "has-session" in cmd:
                    ret.returncode = 1
                else:
                    ret.returncode = 0
                return ret

            mock_run.side_effect = mock_subp
            # This must execute without NameError, SyntaxError or any unhandled exceptions
            autopilot.cmd_tui()
            self.assertTrue(mock_run.called)

    def test_sidebar_logic_functions(self):
        sys.path.insert(0, str(self.root / "bin"))
        import sidebar
        tasks, current_data = sidebar.get_tasks()
        self.assertIsInstance(tasks, list)
        self.assertIsInstance(current_data, dict)
        duration_str = sidebar.format_duration(125)
        self.assertEqual(duration_str, "2m")

    def test_worker_imports_and_find_task(self):
        sys.path.insert(0, str(self.root))
        import worker
        path, data = worker.find_next_task()
        self.assertTrue(path is None or isinstance(path, Path))

if __name__ == "__main__":
    unittest.main()
