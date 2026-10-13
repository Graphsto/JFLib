"""Small Linux runner used natively or through WSL; no shell interpolation."""
import argparse
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
from pathlib import Path


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--watch-stdin', action='store_true')
    p.add_argument('--cwd')
    p.add_argument('action', choices=['run', 'deb'])
    p.add_argument('args', nargs=argparse.REMAINDER)
    a = p.parse_args()
    if a.cwd:
        os.chdir(a.cwd)
    if a.watch_stdin:
        child = subprocess.Popen([sys.executable, __file__, a.action, *a.args],
                                 stdin=subprocess.DEVNULL, start_new_session=True)
        def watch():
            # A daemon must not hold BufferedReader's lock during interpreter shutdown.
            # The raw descriptor still provides EOF-based owner-lifetime monitoring.
            while os.read(sys.stdin.fileno(), 4096):
                pass
            if child.poll() is not None:
                return
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        threading.Thread(target=watch, daemon=True).start()
        result = child.wait()
        sys.stdout.flush()
        sys.stderr.flush()
        os._exit(result)
    if a.action == 'run':
        return subprocess.call(a.args)
    tree, destination = map(Path, a.args)
    with tempfile.TemporaryDirectory(prefix='jfgui-deb-') as tmp:
        package = Path(tmp) / 'pkg'
        shutil.copytree(tree, package)
        for entry in [package, *package.rglob('*')]:
            executable = entry.is_dir() or (entry.parent.name == 'DEBIAN' and entry.name in
                          ('config', 'postinst', 'prerm', 'postrm')) or '/usr/lib/' in entry.as_posix()
            entry.chmod(0o755 if executable else 0o644)
        output = Path(tmp) / 'out.deb'
        subprocess.run(['dpkg-deb', '--root-owner-group', '-Zxz', '--build', str(package), str(output)], check=True)
        shutil.copyfile(output, destination)
    return 0


if __name__ == '__main__':
    sys.exit(main())
