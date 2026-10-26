#!/usr/bin/env python3
"""Build the pinned Windows or macOS DWS adaptation; never reads user credentials."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.request

APP = Path(__file__).resolve().parents[1]
SPEC = APP / 'cli-adapters/dws-isolation'


def checked_run(args, cwd, env):
    subprocess.run(args, cwd=cwd, env=env, check=True, timeout=300)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work-dir', type=Path, required=True, help='New or empty isolated build directory')
    parser.add_argument('--output', type=Path, required=True, help='Windows executable, or macOS archive output directory')
    parser.add_argument('--target', choices=['windows', 'macos'], default='windows')
    parser.add_argument('--source-archive', type=Path, help='Optional local archive; the pinned size/hash validation still applies')
    parser.add_argument('--allow-unverified-vendor-licenses', action='store_true', help='Create a local test package with explicit unresolved upstream license evidence')
    args = parser.parse_args()
    lock = json.loads((SPEC / 'source-lock.json').read_text())
    patch = SPEC / 'isolation.patch'
    if hashlib.sha256(patch.read_bytes()).hexdigest() != lock['patchSha256']:
        raise RuntimeError('DWS isolation patch hash mismatch')
    work = args.work_dir.resolve()
    if work.exists() and any(work.iterdir()):
        raise RuntimeError('Build directory must be empty; existing files are preserved')
    work.mkdir(parents=True, exist_ok=True)
    go = shutil.which('go')
    if not go:
        raise RuntimeError('Pinned Go toolchain is required: ' + lock['goVersion'])
    env = os.environ.copy()
    env.update(GOTOOLCHAIN='local', CGO_ENABLED='1', GOSUMDB='sum.golang.org', GOPROXY='https://proxy.golang.org',
               GOPATH=str(work / 'gopath'), GOMODCACHE=str(work / 'modules'), GOCACHE=str(work / 'gocache'))
    # Host verification must not inherit a cross-compilation target from the shell.
    env.pop('GOOS', None)
    env.pop('GOARCH', None)
    for protocol, proxy in urllib.request.getproxies().items():
        if protocol in ('http', 'https'):
            env.setdefault(protocol.upper() + '_PROXY', proxy)
    if args.allow_unverified_vendor_licenses:
        env['IMBOARD_DWS_ALLOW_UNVERIFIED_LICENSES'] = '1'
    version = subprocess.check_output([go, 'version'], env=env, text=True, timeout=15).strip()
    if not version.startswith('go version ' + lock['goVersion'] + ' '):
        raise RuntimeError('Unexpected Go toolchain: ' + version)
    archive = work / 'source.tar.gz'
    if args.source_archive:
        with args.source_archive.open('rb') as stream:
            payload = stream.read(64 * 1024 * 1024 + 1)
    else:
        with urllib.request.urlopen(lock['url'], timeout=60) as response:
            payload = response.read(64 * 1024 * 1024 + 1)
    if len(payload) > 64 * 1024 * 1024 or hashlib.sha256(payload).hexdigest() != lock['sha256']:
        raise RuntimeError('DWS source archive size/hash mismatch')
    archive.write_bytes(payload)
    unpacked = work / 'source'
    unpacked.mkdir()
    with tarfile.open(archive) as source:
        for entry in source.getmembers():
            destination = (unpacked / entry.name).resolve()
            if not destination.is_relative_to(unpacked) or not (entry.isfile() or entry.isdir()):
                raise RuntimeError('Unsafe source archive member: ' + entry.name)
        source.extractall(unpacked, filter='data')
    roots = list(unpacked.iterdir())
    if len(roots) != 1 or not roots[0].is_dir():
        raise RuntimeError('Unexpected source archive layout')
    source = roots[0]
    checked_run(['git', 'apply', '--check', str(patch)], source, env)
    checked_run(['git', 'apply', str(patch)], source, env)
    checked_run([go, 'test', '-mod=readonly', './internal/keychain', '-run', '^TestIMBoardNamespace', '-count=1'], source, env)
    checked_run([go, 'test', '-mod=readonly', './internal/security', '-run', '^Test(Encrypt|Decrypt|DeriveKey|IMBoard)', '-count=1'], source, env)
    checked_run([go, 'test', '-mod=readonly', './internal/shortcut/smart', '-run', '^TestIMBoardSearch', '-count=1'], source, env)
    if args.target == 'macos':
        from dws_macos_bundle import build_macos
        build_macos(go, source, env, lock, work, args.output.resolve(), APP)
        return
    windows_env = {**env, 'GOOS': 'windows', 'GOARCH': 'amd64'}
    if sys.platform != 'win32':
        if not env.get('IMBOARD_DWS_WINDOWS_CC'):
            raise RuntimeError('A Windows CGO compiler is required: IMBOARD_DWS_WINDOWS_CC')
        windows_env['CC'] = env['IMBOARD_DWS_WINDOWS_CC']
        windows_env['CXX'] = env.get('IMBOARD_DWS_WINDOWS_CXX', env['IMBOARD_DWS_WINDOWS_CC'])
    tests = work / 'keychain.test.exe'
    checked_run([go, 'test', '-mod=readonly', '-c', '-o', str(tests), './internal/keychain'], source, windows_env)
    output = args.output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    staged = work / 'dws-windows-x64.exe'
    flags = '-s -w -X github.com/DingTalk-Real-AI/dingtalk-workspace-cli/internal/app.version=' + lock['adaptationVersion']
    checked_run([go, 'build', '-mod=readonly', '-trimpath', '-buildvcs=false', '-ldflags', flags, '-o', str(staged), './cmd'], source, windows_env)
    evidence = {**lock, 'toolchain': version, 'binarySha256': hashlib.sha256(staged.read_bytes()).hexdigest(),
                'nativeWindowsTestsRun': False, 'windowsTestExecutable': str(tests)}
    if os.name == 'nt':
        checked_run([str(tests), '-test.run', '^TestIMBoardWindows', '-test.v'], source, env)
        marker = subprocess.check_output([str(staged), '--im-board-isolation-version'], env=env, text=True, timeout=15).strip()
        if marker != 'im-board-dws-isolation-v1':
            raise RuntimeError('DWS isolation capability probe failed')
        evidence['nativeWindowsTestsRun'] = True
    checked_run([sys.executable, str(APP / 'scripts/collect-dws-notices.py'), '--go', go,
                 '--binary', str(staged), '--source', str(source), '--module-cache', env['GOMODCACHE'],
                 '--output', str(output), *(['--allow-unverified-vendor-licenses'] if args.allow_unverified_vendor_licenses else [])], source, env)
    shutil.copyfile(staged, output)
    output.with_suffix('.provenance.json').write_text(json.dumps(evidence, indent=2) + '\n')
    print(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
