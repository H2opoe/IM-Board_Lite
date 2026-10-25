#!/usr/bin/env python3
"""Collect license texts for modules actually recorded in the built DWS executable."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def escaped(value):
    return ''.join('!' + c.lower() if c.isupper() else c for c in value)


def collect(go, binary, source, module_cache, output, allow_unverified_vendor_licenses=False):
    info = json.loads(subprocess.check_output([str(go), 'version', '-m', '-json', str(binary)], text=True, timeout=30))
    if info['Main']['Path'] != 'github.com/DingTalk-Real-AI/dingtalk-workspace-cli':
        raise ValueError('Unexpected executable module')
    go_root = Path(subprocess.check_output([str(go), 'env', 'GOROOT'], text=True, timeout=15).strip())
    entries = []
    sections = ['Third-party license texts for the bundled IM-Board DWS adaptation.\n'
                'This inventory follows executable build metadata; it is not a license compatibility opinion.\n']
    modules = [{'Path': info['Main']['Path'], 'Version': 'pinned-source', 'Dir': source},
               {'Path': 'Go standard library', 'Version': info['GoVersion'], 'Dir': go_root}]
    for dep in sorted(info.get('Deps', []), key=lambda value: value['Path']):
        replacement = dep.get('Replace')
        if replacement:
            # Upstream vendors patched modules. Resolve only local, source-bound
            # replacements; never follow an arbitrary external directory.
            name = replacement['Path']
            directory = (source / name).resolve()
            if not name.startswith('./') or not directory.is_relative_to(source.resolve()) or directory.is_symlink():
                raise ValueError('Unsafe replaced dependency: ' + dep['Path'])
            modules.append({**dep, 'Dir': directory})
        else:
            modules.append({**dep, 'Dir': module_cache / (escaped(dep['Path']) + '@' + escaped(dep['Version']))})
    for module in modules:
        directory = module['Dir']
        if not directory.is_dir():
            raise ValueError('Missing module source: ' + module['Path'])
        if module['Path'] == 'Go standard library':
            candidates = [directory / 'LICENSE', directory / 'PATENTS']
            candidates += list((directory / 'src/vendor').rglob('*'))
        else:
            candidates = list(directory.rglob('*'))
        declaration_only = False
        if module['Path'] == 'github.com/mattn/go-localereader' and module['Version'] == 'v0.0.1':
            readme = directory / 'README.md'
            if hashlib.sha256(readme.read_bytes()).hexdigest() != '0c5c52517f13becd7a1e1234f1e8a5ba370f5a5078cee9ac058c2d534e267fcb':
                raise ValueError('Pinned license declaration changed')
            candidates.append(readme)
            declaration_only = True
        if module['Path'] == 'gitlab.alibaba-inc.com/aes/aem-go-sdk' and module.get('Replace', {}).get('Path') == './third_party/aem-go-sdk':
            readme = directory / 'README.md'
            if hashlib.sha256(readme.read_bytes()).hexdigest() != 'c9aa4692957c68b444f484a9ced64929964bf40b41d6dc0169a493be2993b36f':
                raise ValueError('Pinned upstream redistribution declaration changed')
            candidates.append(readme)
            declaration_only = True
        files = []
        for path in sorted(set(candidates)):
            if not path.is_file() or not (path.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE', 'PATENTS')) or (declaration_only and path.name == 'README.md')):
                continue
            if path.is_symlink() or path.stat().st_size > 2 * 1024 * 1024:
                raise ValueError('Unsafe or oversized license file: ' + str(path))
            data = path.read_bytes()
            text = data.decode('utf-8')
            if not text.strip():
                continue
            relative = path.relative_to(directory).as_posix()
            files.append({'path': relative, 'sha256': hashlib.sha256(data).hexdigest()})
            sections.append('\n' + '=' * 72 + '\n' + module['Path'] + '@' + module['Version'] + '\n' + relative + '\n\n' + text + '\n')
        if not files:
            if not (allow_unverified_vendor_licenses and module['Path'] == 'safechat-go-sdk' and module.get('Replace', {}).get('Path') == './third_party/safechat-go-sdk'):
                raise ValueError('No license text found for ' + module['Path'])
            sections.append('\nUNVERIFIED UPSTREAM LICENSE: safechat-go-sdk. No license text was supplied in the pinned upstream source. This test-package inventory does not establish redistribution authorization.\n')
        entries.append({'module': module['Path'], 'version': module['Version'], 'goSum': module.get('Sum'), 'replacement': module.get('Replace'), 'licenses': files, 'licenseEvidence': 'upstream-authorization-unverified; no license file supplied' if not files else ('upstream-authorization-unverified; no license file supplied' if module['Path'] == 'gitlab.alibaba-inc.com/aes/aem-go-sdk' else 'declaration-only; upstream did not ship full terms') if declaration_only else 'license-text'})
    evidence = {'schemaVersion': 1, 'binarySha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'goVersion': info['GoVersion'], 'modules': entries}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.with_suffix('.dependencies.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + '\n', encoding='utf-8', newline='\n')
    output.with_suffix('.THIRD-PARTY-NOTICES.txt').write_text(''.join(sections), encoding='utf-8', newline='\n')
    return evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['go', 'binary', 'source', 'module-cache', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--allow-unverified-vendor-licenses', action='store_true', help='Test-package evidence only; does not establish redistribution rights')
    args = parser.parse_args()
    result = collect(args.go, args.binary, args.source, args.module_cache, args.output, args.allow_unverified_vendor_licenses)
    print(json.dumps({'modules': len(result['modules']), 'binarySha256': result['binarySha256']}))


if __name__ == '__main__':
    main()
