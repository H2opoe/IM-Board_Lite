"""Build deterministic macOS DWS archives from a verified, patched source tree."""
import gzip
import hashlib
import io
import json
import subprocess
import sys
import tarfile


def build_macos(go, source, env, lock, work, output, app):
    output.mkdir(parents=True, exist_ok=True)
    results = []
    for arch, target in [('arm64', 'darwin-arm64'), ('amd64', 'darwin-x64')]:
        binary = work / ('dws-' + target)
        flags = '-s -w -X github.com/DingTalk-Real-AI/dingtalk-workspace-cli/internal/app.version=' + lock['adaptationVersion']
        subprocess.run([go, 'build', '-mod=readonly', '-trimpath', '-buildvcs=false', '-ldflags', flags, '-o', str(binary), './cmd'],
                       cwd=source, env={**env, 'GOOS': 'darwin', 'GOARCH': arch, 'CGO_ENABLED': '1', 'CC': 'clang -arch ' + ('arm64' if arch == 'arm64' else 'x86_64'), 'CXX': 'clang++ -arch ' + ('arm64' if arch == 'arm64' else 'x86_64')}, check=True, timeout=300)
        subprocess.run([sys.executable, str(app / 'scripts/collect-dws-notices.py'), '--go', go, '--binary', str(binary),
                        '--source', str(source), '--module-cache', env['GOMODCACHE'], '--output', str(binary), *(['--allow-unverified-vendor-licenses'] if env.get('IMBOARD_DWS_ALLOW_UNVERIFIED_LICENSES') == '1' else [])],
                       cwd=source, env=env, check=True, timeout=60)
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        provenance = {**lock, 'target': target, 'binarySha256': digest, 'nativeApplicationTested': False}
        files = {
            'package.json': json.dumps({'name': 'dingtalk-workspace-cli', 'version': lock['adaptationVersion'], 'imBoardSecurityPatch': True}, sort_keys=True).encode() + b'\n',
            'vendor/dws-' + target: binary.read_bytes(),
            'LICENSE': (source / 'LICENSE').read_bytes(),
            'THIRD-PARTY-NOTICES.txt': binary.with_suffix('.THIRD-PARTY-NOTICES.txt').read_bytes(),
            'dependencies.json': binary.with_suffix('.dependencies.json').read_bytes(),
            'provenance.json': json.dumps(provenance, indent=2).encode() + b'\n',
        }
        buffer = io.BytesIO()
        with gzip.GzipFile(fileobj=buffer, mode='wb', filename='', mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode='w', format=tarfile.USTAR_FORMAT) as tar:
                for name, data in sorted(files.items()):
                    entry = tarfile.TarInfo('package/' + name)
                    entry.size = len(data)
                    entry.mode = 0o755 if name.startswith('vendor/') else 0o644
                    entry.mtime = entry.uid = entry.gid = 0
                    tar.addfile(entry, io.BytesIO(data))
        data = buffer.getvalue()
        (output / ('package-' + target + '.tgz')).write_bytes(data)
        results.append({'target': target, 'binarySha256': digest, 'archiveSha256': hashlib.sha256(data).hexdigest(), 'sizeBytes': len(data)})
    (output / 'manifest.json').write_text(json.dumps(results, indent=2) + '\n', encoding='utf-8', newline='\n')
    print(json.dumps(results, indent=2))
