import subprocess
import sys
import unittest

class GreeterTest(unittest.TestCase):
    def test_default(self):
        self.assertEqual(subprocess.check_output([sys.executable, 'main.py'], text=True), 'Hello, world!\n')

    def test_name(self):
        self.assertEqual(subprocess.check_output([sys.executable, 'main.py', 'Ada'], text=True), 'Hello, Ada!\n')

if __name__ == '__main__':
    unittest.main()
