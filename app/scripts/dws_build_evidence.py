"""Compare pinned DWS inputs and validate each binary's own evidence across native toolchains."""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile


def load_payload(path, target):
    if target == 'windows':
        return (path.read_bytes(), json.loads(path.with_suffix('.dependencies.json').read_text()),
                path.with_suffix('.THIRD-PARTY-NOTICES.txt').read_bytes(),
                json.loads(path.with_suffix('.provenance.json').read_text()))
    with tarfile.open(path) as archive:
        def read(name):
            member = archive.getmember('package/' + name)
            if not member.isfile():
                raise ValueError('DWS evidence must be a regular file')
            return archive.extractfile(member).read()
        return (read('vendor/dws-' + target), json.loads(read('dependencies.json')),
                read('THIRD-PARTY-NOTICES.txt'), json.loads(read('provenance.json')))


def compare(bundled, rebuilt, lock):
    for binary, dependencies, notices, provenance in [bundled, rebuilt]:
        digest = hashlib.sha256(binary).hexdigest()
        if dependencies.get('binarySha256') != digest or provenance.get('binarySha256') != digest:
            raise ValueError('DWS binary hash does not match its evidence')
        if dependencies.get('goVersion') != lock['goVersion']:
            raise ValueError('DWS Go version differs from source lock')
        if not notices or not dependencies.get('modules'):
            raise ValueError('DWS dependency evidence is empty')
        for key, value in lock.items():
            if provenance.get(key) != value:
                raise ValueError('DWS pinned source differs: ' + key)
    left = {k: v for k, v in bundled[1].items() if k != 'binarySha256'}
    right = {k: v for k, v in rebuilt[1].items() if k != 'binarySha256'}
    if left != right or bundled[2] != rebuilt[2]:
        raise ValueError('DWS dependency or license evidence differs')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bundled', type=Path, required=True)
    parser.add_argument('--rebuilt', type=Path, required=True)
    parser.add_argument('--target', choices=['windows', 'darwin-arm64', 'darwin-x64'], required=True)
    parser.add_argument('--audit-dir', type=Path, required=True)
    args = parser.parse_args()
    lock = json.loads((Path(__file__).resolve().parents[1] / 'cli-adapters/dws-isolation/source-lock.json').read_text())
    # The shipped Windows evidence uses fixed filenames alongside dws.exe.
    if args.target == 'windows':
        p = args.bundled.parent
        bundled = (args.bundled.read_bytes(), json.loads((p / 'dependencies.json').read_text()),
                   (p / 'THIRD-PARTY-NOTICES.txt').read_bytes(), json.loads((p / 'provenance.json').read_text()))
    else:
        bundled = load_payload(args.bundled, args.target)
    rebuilt = load_payload(args.rebuilt, args.target)
    compare(bundled, rebuilt, lock)
    args.audit_dir.mkdir(parents=True, exist_ok=True)
    suffix = '.exe' if args.target == 'windows' else ''
    for name, payload in [('bundled', bundled), ('rebuilt', rebuilt)]:
        (args.audit_dir / (name + suffix)).write_bytes(payload[0])
    print(json.dumps({'target': args.target, 'pinnedInputsVerified': True,
                      'binaryIdentical': bundled[0] == rebuilt[0],
                      'bundledSha256': hashlib.sha256(bundled[0]).hexdigest(),
                      'rebuiltSha256': hashlib.sha256(rebuilt[0]).hexdigest()}))


if __name__ == '__main__':
    main()
