#!/usr/bin/env python3
"""Offline Cargo candidate license material collector; never an artifact/link SBOM.

collect produces a gaps-preserved candidate directory. verify is read-only and
recomputes current inputs; --strict also requires complete reviewed evidence.
No network access, build scripts, application code, or credential APIs are used.
"""
from __future__ import annotations
import argparse
import contextlib
import hashlib
from html.parser import HTMLParser
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import uuid

PRODUCER = 'im-board-offline-rust-notices'
SCHEMA = 1
MAX_ARCHIVE = 64 * 1024 * 1024
MAX_EXPANDED = 256 * 1024 * 1024
MAX_TEXT = 2 * 1024 * 1024
MAX_MEMBERS = 20000
LICENSE_NAMES = ('license', 'licence', 'copying', 'notice', 'copyright', 'patents', 'authors')
CODE_SUFFIXES = {'.rs', '.c', '.h', '.cc', '.cpp', '.hpp', '.js', '.ts', '.py', '.json', '.toml', '.png', '.jpg', '.svg', '.ico', '.go'}
REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'
ENV_INPUTS = ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_BUILD_TARGET', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'RUSTC_BOOTSTRAP')

class Error(RuntimeError):
    pass

class StrictError(Error):
    pass

def sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()

def json_bytes(value) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode('utf-8')

def regular_bytes(path: Path, limit: int | None = None) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise Error(f'Expected regular input file: {path}')
    bound = 128 * MAX_TEXT if limit is None else limit
    if not isinstance(bound, int) or isinstance(bound, bool) or bound < 0:
        raise Error('Invalid byte limit')
    # stat is advisory: a concurrent writer may grow the file before reading.
    if path.stat().st_size > bound:
        raise Error(f'Input file exceeds limit: {path}')
    with path.open('rb') as stream:
        raw = stream.read(bound + 1)
    if len(raw) > bound:
        raise Error(f'Input grew beyond byte limit: {path}')
    return raw

def safe_relative(value: str) -> str:
    if not isinstance(value, str) or not value or '\\' in value or '\x00' in value:
        raise Error(f'Unsafe relative path: {value!r}')
    p = PurePosixPath(value)
    if p.as_posix() != value or p.is_absolute() or '..' in p.parts or '.' in value.split('/') or re.match(r'^[A-Za-z]:', value):
        raise Error(f'Unsafe relative path: {value!r}')
    return p.as_posix()

def file_set(root: Path) -> tuple[set[str], set[str]]:
    if root.is_symlink() or not root.is_dir():
        raise Error(f'Expected regular directory: {root}')
    files, directories = set(), set()
    for parent, dirs, names in os.walk(root, followlinks=False):
        for name in dirs + names:
            p = Path(parent) / name
            if p.is_symlink():
                raise Error(f'Symlink in input/output directory: {p}')
            rel = p.relative_to(root).as_posix()
            if p.is_dir(): directories.add(rel)
            elif p.is_file(): files.add(rel)
            else: raise Error(f'Special file in input/output directory: {p}')
    return files, directories

def parent_dirs(paths) -> set[str]:
    return {p.as_posix() for s in paths for p in PurePosixPath(s).parents if p.as_posix() != '.'}

def notice_file(path: str, explicit: str | None = None) -> bool:
    p = PurePosixPath(path)
    return path == explicit or (p.name.lower().startswith(LICENSE_NAMES) and p.suffix.lower() not in CODE_SUFFIXES)

def full_term_ids(text: str) -> set[str]:
    """Conservative structural recognition. Unknown terms remain unreviewed."""
    n = re.sub(r'\s+', ' ', text).lower().replace('’', "'").replace('“', '"').replace('”', '"')
    ids = set()
    def has(*phrases): return all(p in n for p in phrases)
    mit = has('permission is hereby granted, free of charge', 'the software is provided', 'the software or the use or other dealings in the software')
    mit = mit and has('use, copy, modify, merge, publish, distribute, sublicense', 'without warranty of any kind', 'merchantability', 'fitness for a particular purpose', 'noninfringement', 'whether in an action of contract, tort or otherwise') and len(n) >= 850
    if mit and has('the above copyright notice and this permission notice shall be included', 'all copies or substantial portions'): ids.add('MIT')
    if mit and 'the above copyright notice' not in n and 'permission notice shall be included' not in n: ids.add('MIT-0')
    if len(n) >= 8500 and has('apache license', 'version 2.0', 'grant of copyright license', 'grant of patent license', 'redistribution', 'submission of contributions', 'trademarks', 'disclaimer of warranty', 'limitation of liability', 'accepting warranty or additional liability', 'end of terms and conditions'): ids.add('Apache-2.0')
    bsd = has('redistribution and use in source and binary forms', 'redistributions of source code', 'redistributions in binary form', 'this software is provided', 'in no event')
    if bsd: ids.add('BSD-3-Clause' if ('neither the name' in n or 'names of its contributors' in n) else 'BSD-2-Clause')
    if has('permission to use, copy, modify, and', 'without fee', 'the software is provided', 'any special, direct, indirect'): ids.add('ISC')
    if has('permission to use, copy, modify, and', 'without fee', 'the software is provided') and 'all copies' not in n: ids.add('0BSD')
    if has('origin of this software must not be misrepresented', 'altered source versions', 'notice may not be removed or altered', 'this software is provided'): ids.add('Zlib')
    if has('mozilla public license version 2.0', 'definitions', 'limitations of liability', 'exhibit a', 'exhibit b'): ids.add('MPL-2.0')
    if has('boost software license', 'version 1.0', 'machine-executable object code', 'the software is provided'): ids.add('BSL-1.0')
    if has('this is free and unencumbered software released into the public domain', 'anyone is free to copy', 'the software is provided', 'unlicense.org'): ids.add('Unlicense')
    if len(n) >= 1800 and has('unicode license v3', 'permission is hereby granted, free of charge', 'this copyright and permission notice appear', 'the data files and software are provided', 'in no event shall the copyright holder', 'the name of a copyright holder shall not be used', 'prior written authorization'): ids.add('Unicode-3.0')
    if len(n) >= 2000 and has('community data license agreement', 'permissive', '2.0', 'provision of the data', 'conditions for sharing data', 'no restrictions on results', 'no warranty; limitation of liability', 'the data is provided on an', 'no data provider shall have any liability', 'definitions', 'machine learning models'): ids.add('CDLA-Permissive-2.0')
    if len(n) >= 6000 and has('creative commons legal code', 'cc0 1.0 universal', 'statement of purpose', '1. copyright and related rights', '2. waiver', '3. public license fallback', '4. limitations and disclaimers', 'creative commons is not a party to this document'): ids.add('CC0-1.0')
    return ids

def actual_copyright_lines(text: str) -> list[tuple[int, str]]:
    result = []
    for number, line in enumerate(text.splitlines(), 1):
        clean = re.sub(r'^\s*(?://+|#|/\*|\*|<!--)\s*', '', line).strip()
        statement = re.search(r'(?i)(?:\bcopyright\b(?:\s*\([cC]\))?|©)\s*\d{4}', clean)
        spdx = re.match(r'(?i)^(?:SPDX-FileCopyrightText|PackageCopyrightText):\s*(.+)', clean)
        if spdx and spdx[1].strip().upper() in {'NONE', 'NOASSERTION'}: spdx = None
        if statement or spdx:
            if not re.search(r'(?i)<year>|<copyright holders>|\[year\]|\[yyyy\]|\[name', clean):
                result.append((number, line))
    return result

class Expression:
    def __init__(self, value: str):
        # Cargo explicitly used '/' as the legacy dual-license separator.
        self.legacy_slash = '/' in value
        value = value.replace('/', ' OR ')
        self.tokens = re.findall(r'[A-Za-z0-9][A-Za-z0-9.+-]*|[()]', value)
        if re.sub(r'\s+', '', ''.join(self.tokens)) != re.sub(r'\s+', '', value): raise Error('Unsupported license expression characters')
        self.index = 0
        self.tree = self.parse_or()
        if self.index != len(self.tokens): raise Error('Unexpected license expression token')
    def pop(self):
        if self.index >= len(self.tokens): raise Error('Incomplete license expression')
        value = self.tokens[self.index]; self.index += 1; return value
    def peek(self): return self.tokens[self.index] if self.index < len(self.tokens) else None
    def atom(self):
        token = self.pop()
        if token == '(':
            node = self.parse_or()
            if self.pop() != ')': raise Error('Unclosed license expression')
            return node
        if token in ('AND', 'OR', 'WITH', ')'): raise Error('Invalid license expression')
        if self.peek() == 'WITH':
            self.pop(); exception = self.pop()
            if exception in ('AND', 'OR', 'WITH', '(', ')'): raise Error('Invalid license exception')
            token += ' WITH ' + exception
        return ('id', token)
    def parse_and(self):
        node = self.atom()
        while self.peek() == 'AND': self.pop(); node = ('and', node, self.atom())
        return node
    def parse_or(self):
        node = self.parse_and()
        while self.peek() == 'OR': self.pop(); node = ('or', node, self.parse_and())
        return node
    def satisfy(self, ids: set[str]):
        def visit(n):
            if n[0] == 'id': return [n[1]] if n[1] in ids else None
            left, right = visit(n[1]), visit(n[2])
            if n[0] == 'or': return left if left is not None else right
            return sorted(set(left + right)) if left is not None and right is not None else None
        return visit(self.tree)

def archive_contents(raw: bytes, expected_name: str, expected_version: str) -> dict[str, bytes]:
    if len(raw) > MAX_ARCHIVE: raise Error('Crate archive exceeds compressed limit')
    root = expected_name + '-' + expected_version
    files, seen, expanded = {}, set(), 0
    try:
        with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
            for count, member in enumerate(archive, 1):
                if count > MAX_MEMBERS: raise Error('Crate archive has too many members')
                normalized = safe_relative(member.name.rstrip('/'))
                if normalized in seen: raise Error(f'Duplicate TAR member: {normalized}')
                seen.add(normalized)
                p = PurePosixPath(normalized)
                if not p.parts or p.parts[0] != root: raise Error(f'Wrong crate TAR root: {normalized}')
                if member.isdir(): continue
                if not member.isfile(): raise Error(f'Link/special TAR member: {normalized}')
                rel = p.relative_to(root).as_posix()
                if rel == '.': raise Error('Crate TAR root cannot be a file')
                expanded += member.size
                if member.size < 0 or expanded > MAX_EXPANDED: raise Error('Crate archive exceeds expanded limit')
                stream = archive.extractfile(member)
                data = stream.read(member.size + 1)
                if len(data) != member.size: raise Error('Truncated crate member')
                files[rel] = data
    except (tarfile.TarError, EOFError, OSError) as error:
        raise Error(f'Invalid crate archive: {error}') from error
    if 'Cargo.toml' not in files: raise Error('Crate archive lacks Cargo.toml')
    return files

def parse_tree(raw: bytes):
    try: text = raw.decode('utf-8')
    except UnicodeDecodeError as error: raise Error('Cargo tree is not UTF-8') from error
    rows = {}
    for line in text.splitlines():
        if not line: raise Error('Unexpected empty Cargo tree line')
        line = re.sub(r' \(\*\)$', '', line)
        parts = line.split('|')
        if len(parts) != 4: raise Error(f'Unparseable Cargo tree row: {line[:160]}')
        m = re.fullmatch(r'([A-Za-z0-9_-]+) v([^\s]+)( \(proc-macro\))?(?: \(([^\n]+)\))?', parts[0])
        if not m: raise Error(f'Unparseable Cargo package display: {parts[0]}')
        key = (m[1], m[2]); row = {'name': m[1], 'version': m[2], 'displaySource': m[4], 'procMacroDisplay': bool(m[3]), 'license': parts[1], 'repository': parts[2], 'features': sorted(set(filter(None, parts[3].split(','))))}
        if key in rows:
            prior = rows[key]
            if any(prior[x] != row[x] for x in ['displaySource', 'procMacroDisplay', 'license', 'repository']): raise Error(f'Ambiguous tree package: {key}')
            row['features'] = sorted(set(prior['features'] + row['features']))
        rows[key] = row
    if not rows: raise Error('Cargo tree is empty')
    return rows

def execute(args, cwd, env):
    result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, timeout=90, check=False)
    if result.returncode: raise Error(f'Offline command failed ({result.returncode}): {args[1:]}\n{result.stderr.decode(errors="replace")[:3000]}')
    return result.stdout, result.stderr

def config_inputs(options, manifests) -> dict[str, str | None]:
    paths = {options.lock, options.manifest, options.cargo, options.rustc, Path(__file__).resolve()}
    paths.update(manifests)
    for parent in [options.manifest.parent, *options.manifest.parent.parents]:
        paths.update([parent / '.cargo/config', parent / '.cargo/config.toml'])
    paths.update([options.cargo_home / 'config', options.cargo_home / 'config.toml'])
    return {str(p): sha(regular_bytes(p)) if p.exists() else None for p in sorted(paths, key=str)}

def directory_fingerprint(root):
    files, dirs = file_set(root)
    return sha(json_bytes({'files': sorted(files), 'directories': sorted(dirs)}))

def same_inputs(captured, directories=None):
    for path, expected in captured.items():
        p = Path(path)
        actual = sha(regular_bytes(p)) if p.exists() else None
        if actual != expected: raise Error(f'Input changed during collection: {path}')
    for root, expected in (directories or {}).items():
        if directory_fingerprint(Path(root)) != expected: raise Error(f'Input directory changed during collection: {root}')

def supplemental_inputs(path: Path, edition: str):
    raw = regular_bytes(path, MAX_TEXT)
    try: manifest = json.loads(raw)
    except (ValueError, UnicodeDecodeError) as error: raise Error('Invalid supplement manifest') from error
    if manifest.get('schemaVersion') != 1 or manifest.get('edition') != edition: raise Error('Supplement schema/edition mismatch')
    records, expected_files, fingerprints = {}, {'materials.json'}, {str(path): sha(raw)}
    if path.name != 'materials.json': raise Error('Supplement manifest must be named materials.json')
    for c in manifest.get('components', []):
        key = (c['name'], c['version'])
        if key in records: raise Error(f'Duplicate supplement component: {key}')
        if not re.fullmatch(r'[0-9a-f]{64}', c.get('registryChecksum', '')): raise Error('Invalid supplement component checksum')
        if not isinstance(c.get('gaps'), list) or not isinstance(c.get('additionalReview'), list): raise Error('Supplement must retain gap/review arrays')
        if c.get('complete') != (not c['gaps']): raise Error('Inconsistent supplement complete/gap state')
        materials = []
        for f in c.get('files', []):
            rel = safe_relative(f['path']); p = path.parent / rel
            if not p.resolve().is_relative_to(path.parent.resolve()): raise Error('Supplement path escaped input root')
            data = regular_bytes(p, MAX_TEXT)
            if sha(data) != f.get('sha256') or len(data) != f.get('bytes'): raise Error(f'Supplement material hash/size mismatch: {rel}')
            if not f.get('url') or not f.get('kind'): raise Error('Supplement material lacks provenance/kind')
            expected_files.add(rel); fingerprints[str(p)] = sha(data); materials.append((f, data))
        records[key] = (c, materials)
    actual, dirs = file_set(path.parent)
    if actual != expected_files or dirs != parent_dirs(expected_files): raise Error('Supplement directory has missing/extra files or directories')
    return records, fingerprints

def check_source_tree(base: Path, archive_files):
    actual, directories = file_set(base)
    captured_directory = sha(json_bytes({'files': sorted(actual), 'directories': sorted(directories)}))
    # Cargo's cache markers are not source authentication evidence.
    expected = set(archive_files)
    if actual - {'.cargo-ok', '.cargo-checksum.json'} != expected: raise Error(f'Registry source file set drift: {base.name}')
    for path, raw in archive_files.items():
        if regular_bytes(base / path) != raw: raise Error(f'Registry source bytes differ from locked archive: {base.name}/{path}')
    return sha(json_bytes({p: sha(raw) for p, raw in sorted(archive_files.items())})), captured_directory

def add_material(files, name, version, source_path, data, kind, **metadata):
    suffix = re.sub(r'[^A-Za-z0-9._-]', '_', PurePosixPath(source_path).name)
    output = f'licenses/{name}-{version}/{sha(data)}-{suffix}'
    if output in files and files[output] != data: raise Error('Material output collision')
    files[output] = data
    try: text = data.decode('utf-8')
    except UnicodeDecodeError as error: raise Error(f'License material is not UTF-8: {name}/{source_path}') from error
    return {'path': output, 'sourcePath': source_path, 'sha256': sha(data), 'bytes': len(data), 'kind': kind, 'recognizedFullTerms': sorted(full_term_ids(text)), 'actualCopyrightLines': [{'line': n, 'text': s} for n, s in actual_copyright_lines(text)], **metadata}

def source_notices(archive_files):
    records = []
    for path, raw in sorted(archive_files.items()):
        if PurePosixPath(path).suffix.lower() not in {'.rs', '.c', '.h', '.cc', '.cpp', '.hpp', '.py', '.js', '.go'}: continue
        try: text = raw.decode('utf-8')
        except UnicodeDecodeError: continue
        lines = text.splitlines(keepends=True)
        # Preserve exact legal headers; do not copy API docs or code examples.
        end = 0
        for i, line in enumerate(lines[:80]):
            stripped = line.strip()
            if stripped.startswith(('///', '//!')): break
            if not stripped and end: end = i + 1; break
            if stripped.startswith(('//', '/*', '*', '#')): end = i + 1
            else: break
        header = ''.join(lines[:end]).rstrip() + '\n'
        if actual_copyright_lines(header) or 'source code form is subject' in header.lower() or 'licensed under' in header.lower():
            records.append({'sourcePath': path, 'firstLine': 1, 'lastLine': end, 'sourceSha256': sha(raw), 'text': header})
    return records

def options_dict(options):
    return {'edition': options.edition, 'package': options.package, 'target': options.target, 'host': options.host, 'profile': options.profile, 'featureMode': options.feature_mode, 'features': sorted(options.features), 'manifest': str(options.manifest), 'lock': str(options.lock), 'cargoHome': str(options.cargo_home), 'supplement': str(options.supplement), 'cargoExecutable': str(options.cargo), 'rustcExecutable': str(options.rustc)}

def platform(triple):
    for marker, value in [('windows', 'windows'), ('apple', 'apple'), ('linux', 'linux'), ('freebsd', 'freebsd')]:
        if marker in triple: return value
    return triple

def build_environment(env):
    names = set(ENV_INPUTS)
    names.update(key for key in env if key.startswith(('CARGO_BUILD_', 'CARGO_TARGET_', 'CARGO_PROFILE_', 'CARGO_UNSTABLE_', 'RUSTC_', 'RUSTUP_')))
    # Fingerprints preserve stale-input detection without exposing flag values.
    return {key: sha(env[key].encode()) if key in env else None for key in sorted(names)}

def build_candidate(options, runner=execute):
    files = {}
    pre_inputs = config_inputs(options, [])
    records, supplement_hashes = supplemental_inputs(options.supplement, options.edition)
    env = os.environ.copy(); env['CARGO_HOME'] = str(options.cargo_home); env['RUSTC'] = str(options.rustc)
    captured_env = build_environment(env)
    feature_args = []
    if options.feature_mode == 'no-default': feature_args.append('--no-default-features')
    elif options.feature_mode == 'all': feature_args.append('--all-features')
    if options.features: feature_args += ['--features', ','.join(sorted(options.features))]
    base = ['--manifest-path', str(options.manifest), '--locked', '--offline']
    commands = []
    def run(tool, args, role):
        command = [str(tool), *args]; stdout, stderr = runner(command, options.manifest.parent, env)
        # A cache-lock wait is scheduling status, not graph/package evidence.
        # Emit it to the execution log; only this exact Cargo status is omitted.
        stable_stderr = []
        for line in stderr.splitlines(keepends=True):
            if re.fullmatch(rb'\s*Blocking waiting for file lock on package cache\s*', line):
                sys.stderr.write(line.decode('utf-8', errors='replace'))
            else: stable_stderr.append(line)
        stderr = b''.join(stable_stderr)
        files['inputs/' + role + '.txt'] = stdout
        if stderr: files['inputs/' + role + '.stderr.txt'] = stderr
        commands.append({'role': role, 'command': command, 'stdoutSha256': sha(stdout), 'stderrSha256': sha(stderr)})
        return stdout
    cargo_version = run(options.cargo, ['-Vv'], 'cargo-version').decode('utf-8')
    rustc_version = run(options.rustc, ['-Vv'], 'rustc-version').decode('utf-8')
    rustc_values = dict(re.findall(r'^([a-z-]+): (.+)$', rustc_version, re.MULTILINE))
    if rustc_values.get('host') != options.host or not rustc_values.get('commit-hash') or not rustc_values.get('release'): raise Error('Explicit host or compiler provenance does not match rustc -Vv')
    cargo_host = re.search(r'^host: (.+)$', cargo_version, re.MULTILINE)
    if not cargo_host or cargo_host[1] != options.host: raise Error('Explicit host does not match Cargo host')
    sysroot = Path(run(options.rustc, ['--print', 'sysroot'], 'rustc-sysroot').decode('utf-8').strip()).resolve()
    meta = []
    for triple, role in [(options.target, 'metadata-target'), (options.host, 'metadata-host')]:
        data = run(options.cargo, ['metadata', *base, '--format-version', '1', '--filter-platform', triple, *feature_args], role)
        try: meta.append(json.loads(data))
        except ValueError as error: raise Error('Cargo metadata is not JSON') from error
    mapped = {}
    for metadata in meta:
        if metadata.get('version') != 1: raise Error('Unsupported Cargo metadata version')
        for package in metadata['packages']:
            key = (package['name'], package['version']); prior = mapped.setdefault(key, {})
            if package['id'] in prior and prior[package['id']] != package: raise Error('Conflicting metadata package records')
            prior[package['id']] = package
    roots = [p for packages in mapped.values() for p in packages.values() if p['name'] == options.package and Path(p['manifest_path']).resolve() == options.manifest.resolve()]
    if len(roots) != 1: raise Error('Expected exactly one selected workspace package')
    root = roots[0]; root_key = (root['name'], root['version'])
    if any(Path(m['workspace_root']).resolve() / 'Cargo.lock' != options.lock.resolve() for m in meta): raise Error('Explicit lock is not the Cargo workspace lock used by the queries')
    all_rows = parse_tree(run(options.cargo, ['tree', *base, '--package', options.package, '--target', options.target, '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}|{l}|{r}|{f}', '--color', 'never', *feature_args], 'tree-normal-build'))
    normal = parse_tree(run(options.cargo, ['tree', *base, '--package', options.package, '--target', options.target, '--edges', 'normal', '--prefix', 'none', '--format', '{p}|{l}|{r}|{f}', '--color', 'never', *feature_args], 'tree-normal'))
    if root_key not in all_rows or root_key not in normal or not set(normal).issubset(all_rows): raise Error('Inconsistent tree scope or missing selected root')
    lock = tomllib.loads(regular_bytes(options.lock).decode('utf-8'))
    locked = {}
    for package in lock['package']:
        locked.setdefault((package['name'], package['version']), []).append(package)
    manifests = []
    source_inputs = {}
    input_directories = {str(options.supplement.parent): directory_fingerprint(options.supplement.parent)}
    components = []
    for key, row in sorted(all_rows.items()):
        choices = mapped.get(key, {})
        if len(choices) != 1: raise Error(f'Missing/ambiguous target+host metadata package: {key}')
        package = next(iter(choices.values())); manifests.append(Path(package['manifest_path']))
        if key == root_key: continue
        if row['procMacroDisplay'] != any('proc-macro' in t['kind'] for t in package.get('targets', [])): raise Error(f'Tree/metadata proc-macro role mismatch: {key}')
        if row['displaySource'] is not None: raise Error(f'Unexpected non-registry tree display source: {key}')
        if package.get('source') != REGISTRY: raise Error(f'Unsupported non-crates.io dependency identity: {key}')
        locked_choices = [p for p in locked.get(key, []) if p.get('source') == package['source']]
        if len(locked_choices) != 1 or len(locked.get(key, [])) != 1: raise Error(f'Ambiguous Cargo.lock identity: {key}')
        identity = locked_choices[0]; checksum = identity.get('checksum')
        if not re.fullmatch('[0-9a-f]{64}', checksum or ''): raise Error(f'Missing registry checksum: {key}')
        cached = Path(package['manifest_path']).parent
        source_root = options.cargo_home / 'registry/src'
        if not cached.resolve().is_relative_to(source_root.resolve()): raise Error(f'Registry metadata source outside selected Cargo home: {key}')
        relative = cached.relative_to(source_root)
        if len(relative.parts) != 2 or relative.parts[1] != key[0] + '-' + key[1]: raise Error(f'Unexpected registry source layout: {key}')
        archive_path = options.cargo_home / 'registry/cache' / relative.parts[0] / (key[0] + '-' + key[1] + '.crate')
        archive = regular_bytes(archive_path, MAX_ARCHIVE)
        if sha(archive) != checksum: raise Error(f'Locked crate archive checksum mismatch: {key}')
        archive_files = archive_contents(archive, *key)
        archive_manifest = tomllib.loads(archive_files['Cargo.toml'].decode('utf-8'))['package']
        if archive_manifest.get('name') != key[0] or archive_manifest.get('version') != key[1]: raise Error(f'Crate archive package identity mismatch: {key}')
        declared = archive_manifest.get('license')
        if package.get('license') != declared or row['license'] not in (declared or '',): raise Error(f'Cargo/locked-archive license declaration mismatch: {key}')
        source_hash, directory_hash = check_source_tree(cached, archive_files)
        source_inputs[str(archive_path)] = checksum
        source_inputs.update({str(cached / p): sha(raw) for p, raw in archive_files.items()})
        input_directories[str(cached)] = directory_hash
        component = {'name': key[0], 'version': key[1], 'packageId': package['id'], 'source': package['source'], 'registryChecksum': checksum, 'sourceArchiveUrl': f'https://static.crates.io/crates/{key[0]}/{key[0]}-{key[1]}.crate', 'sourceArchivePath': str(archive_path), 'sourceTreeSha256': source_hash, 'licenseDeclaration': declared, 'packageAuthors': archive_manifest.get('authors', []), 'authorsAreCopyrightEvidence': False, 'treeFeatures': row['features'], 'graphRoles': ['normal-candidate'] if key in normal else ['build-exclusive-candidate'], 'procMacro': any('proc-macro' in t['kind'] for t in package.get('targets', [])), 'artifactInclusion': 'not-established', 'materials': [], 'gaps': [], 'additionalReview': []}
        if component['procMacro']: component['graphRoles'].append('proc-macro-input')
        explicit = archive_manifest.get('license-file')
        if explicit is not None: explicit = safe_relative(explicit)
        for source_path, data in sorted(archive_files.items()):
            if notice_file(source_path, explicit):
                if len(data) > MAX_TEXT: raise Error(f'Oversized license text: {key}/{source_path}')
                component['materials'].append(add_material(files, *key, source_path, data, 'authenticated-crate-text', sourceArchiveSha256=checksum))
        if explicit and explicit not in archive_files: component['gaps'].append({'kind': 'declared-license-file-missing', 'path': explicit})
        for n, notice in enumerate(source_notices(archive_files)):
            material = add_material(files, *key, f'SOURCE-NOTICE-{n:05d}.txt', notice['text'].encode(), 'authenticated-source-notice', sourceArchiveSha256=checksum, excerpt={k: v for k, v in notice.items() if k != 'text'})
            component['materials'].append(material)
        supplement = records.get(key)
        if supplement:
            c, materials = supplement
            if c['registryChecksum'] != checksum or c['licenseDeclaration'] != declared: raise Error(f'Supplement locked component mismatch: {key}')
            if not c.get('upstream', {}).get('commit') or not re.fullmatch('[0-9a-f]{40}', c['upstream']['commit']): raise Error('Supplement lacks fixed upstream revision')
            contexts = c.get('applicableTargets', [])
            if not any(t.get('edition') == options.edition and t.get('target') == options.target for t in contexts): raise Error(f'Supplement historical edition/target mismatch: {key}')
            vcs = archive_files.get('.cargo_vcs_info.json')
            if vcs and json.loads(vcs)['git']['sha1'] != c['upstream']['commit']: raise Error('Supplement fixed revision differs from the locked package VCS revision')
            component['supplement'] = {'complete': c['complete'], 'selectedLicense': c['selectedLicense'], 'upstream': c['upstream'], 'historicalApplicabilityIsGraphAuthority': False}
            component['gaps'].extend(c['gaps']); component['additionalReview'].extend(c['additionalReview'])
            for f, data in materials:
                for excerpt in f.get('excerpts', []):
                    for occ in excerpt['occurrences']:
                        path = safe_relative(occ['packagePath'])
                        if path not in archive_files or sha(archive_files[path]) != occ['sourceFileSha256'] or occ['sourceArchiveSha256'] != checksum: raise Error('Supplement source excerpt provenance mismatch')
                        text = archive_files[path].decode('utf-8'); lines = text.splitlines(keepends=True)
                        first, last = occ['firstLine'], occ['lastLine']
                        if not isinstance(first, int) or not isinstance(last, int) or first < 1 or last < first or last > len(lines): raise Error('Invalid source excerpt range')
                        exact = ''.join(lines[first - 1:last]).rstrip() + '\n'
                        if sha(exact.encode()) != excerpt['excerptSha256']: raise Error('Supplement source excerpt byte mismatch')
                component['materials'].append(add_material(files, *key, f['path'], data, f['kind'], url=f['url'], upstreamSha256=f['sha256'], upstreamRevision=c['upstream']['commit'], declaredLicenseId=f.get('licenseId'), containsComponentCopyrightClaim=f.get('containsComponentCopyright', False)))
        associated = set()
        canonical = set()
        for f in component['materials']:
            if f['kind'] == 'canonical-license-terms': canonical.update(f['recognizedFullTerms'])
            elif f['kind'] in {'authenticated-crate-text', 'upstream-license-terms'}: associated.update(f['recognizedFullTerms'])
        if supplement and supplement[0].get('licenseTextStatus') == 'source-linked-official-terms':
            selected = supplement[0]['selectedLicense']
            source_link = {'Apache-2.0': 'apache.org/licenses/LICENSE-2.0', 'MIT': 'opensource.org/licenses/MIT'}.get(selected)
            if source_link and selected in canonical and any(f['kind'] == 'authenticated-source-notice' and f['actualCopyrightLines'] and source_link in files[f['path']].decode('utf-8') for f in component['materials']): associated.add(selected)
        component['canonicalTerms'] = sorted(canonical)
        component['associatedFullTerms'] = sorted(associated)
        try:
            expression = Expression(declared or '')
            selected = expression.satisfy(associated)
            component['legacySlashMeansOr'] = expression.legacy_slash
            component['selectedLicenses'] = selected
            if selected is None: component['gaps'].append({'kind': 'declared-license-expression-not-covered', 'expression': declared, 'associatedFullTerms': sorted(associated)})
        except Error as error:
            component['selectedLicenses'] = None; component['gaps'].append({'kind': 'unreviewed-license-expression', 'expression': declared, 'reason': str(error)})
        component['copyrightEvidence'] = {'status': 'published-notices-retained' if any(f['actualCopyrightLines'] for f in component['materials'] if f['kind'] != 'canonical-license-terms') else 'not-published-in-reviewed-materials', 'authorsAreCopyrightEvidence': False}
        component['licenseMaterialsReady'] = not component['gaps'] and not component['additionalReview']
        components.append(component)
    post_inputs = config_inputs(options, manifests)
    for p, before in pre_inputs.items():
        if post_inputs[p] != before: raise Error(f'Cargo input drift during tool queries: {p}')
    post_inputs.update(supplement_hashes)
    post_inputs.update(source_inputs)
    same_inputs(post_inputs, input_directories)
    stdlib_candidates = [sysroot / 'share/doc/rust/COPYRIGHT-library.html', sysroot / 'share/doc/rust/html/COPYRIGHT-library.html']
    for p in stdlib_candidates: post_inputs[str(p)] = sha(regular_bytes(p)) if p.exists() else None
    existing = [p for p in stdlib_candidates if p.exists()]
    stdlib = {'scope': 'Exact compiler sysroot standard-library license materials; not complete compiler notices or linked-component proof', 'rustcRelease': rustc_values['release'], 'rustcCommit': rustc_values['commit-hash'], 'sysroot': str(sysroot), 'gaps': []}
    if len(existing) != 1: stdlib['gaps'].append({'kind': 'matching-sysroot-library-notice-missing-or-ambiguous'})
    else:
        raw = regular_bytes(existing[0], 8 * MAX_TEXT)
        text = raw.decode('utf-8')
        class PlainText(HTMLParser):
            def __init__(self): super().__init__(); self.parts = []
            def handle_data(self, data): self.parts.append(data)
        plain = PlainText(); plain.feed(text)
        if 'Copyright notices for The Rust Standard Library' not in text or '</html>' not in text or not {'MIT', 'Apache-2.0'}.issubset(full_term_ids(' '.join(plain.parts))): raise Error('Sysroot COPYRIGHT-library is not complete standard-library notice HTML')
        files['stdlib/COPYRIGHT-library.html'] = raw; stdlib.update({'path': str(existing[0]), 'sha256': sha(raw), 'bytes': len(raw), 'versionAssociation': 'Read from the actual rustc-reported sysroot; no alternate-version document fallback'})
        post_inputs[str(existing[0])] = sha(raw)
    native_platform = platform(options.host) == platform(options.target)
    context = {**options_dict(options), 'cargoVersion': cargo_version, 'rustcVersionVerbose': rustc_version, 'cargoExecutableSha256': sha(regular_bytes(Path(options.cargo))), 'rustcExecutableSha256': sha(regular_bytes(Path(options.rustc))), 'environment': captured_env, 'commands': commands, 'stderrPolicy': 'Only exact Cargo package-cache lock-wait scheduling status is emitted to execution stderr and omitted from reproducible graph evidence; every other diagnostic byte is retained', 'nativePlatformContext': native_platform, 'profileEvidence': 'Declared build intent; Cargo tree does not establish the final link or build profile'}
    issues = [{'component': c['name'] + '@' + c['version'], 'gaps': c['gaps'], 'additionalReview': c['additionalReview']} for c in components if not c['licenseMaterialsReady']]
    if stdlib['gaps']: issues.append({'component': 'rust-standard-library', 'gaps': stdlib['gaps']})
    if not native_platform: issues.append({'component': 'build-context', 'gaps': [{'kind': 'cross-platform-candidate-graph-is-not-native-release-context'}]})
    overview = {'components': len(components), 'normalCandidates': sum((c['name'], c['version']) in normal for c in components), 'buildExclusiveCandidates': sum((c['name'], c['version']) not in normal for c in components), 'procMacroInputs': sum(c['procMacro'] for c in components), 'licenseMaterialsReady': sum(c['licenseMaterialsReady'] for c in components), 'componentsWithGapsOrReview': sum(not c['licenseMaterialsReady'] for c in components), 'strictCandidateMaterialGate': 'pass' if not issues else 'fail', 'binaryArtifactInclusion': 'not-established'}
    notice_lines = ['Rust candidate dependency license materials', 'Scope: exact host/target/features normal+build Cargo tree. This is not a linked binary SBOM.', f'Host: {options.host}; target: {options.target}; feature mode: {options.feature_mode}; features: {",".join(sorted(options.features))}', 'Unresolved items remain in manifest.json and block strict candidate verification.', '']
    for c in components:
        notice_lines += ['=' * 72, c['name'] + ' ' + c['version'], f'Declared license: {c["licenseDeclaration"]}', 'Source: ' + c['sourceArchiveUrl'], 'Registry checksum: ' + c['registryChecksum'], '']
        for f in c['materials']:
            notice_lines += [f'--- {f["kind"]}: {f["sourcePath"]} ---']
            if f['kind'] == 'canonical-license-terms': notice_lines += ['Canonical terms/template; placeholders are not actual component copyright.']
            notice_lines += [files[f['path']].decode('utf-8'), '']
    files['THIRD-PARTY-NOTICES.txt'] = ('\n'.join(notice_lines) + '\n').encode('utf-8')
    inventory = {p: {'sha256': sha(raw), 'bytes': len(raw)} for p, raw in sorted(files.items())}
    manifest = {'producer': PRODUCER, 'schemaVersion': SCHEMA, 'generatorSha256': sha(Path(__file__).read_bytes()), 'scope': 'Cargo normal/build candidates for an explicit context; not a linked binary or installation SBOM', 'context': context, 'inputFingerprints': post_inputs, 'inputDirectoryFingerprints': input_directories, 'components': components, 'stdlib': stdlib, 'summary': overview, 'strictIssues': issues, 'inventory': inventory}
    files['manifest.json'] = json_bytes(manifest)
    same_inputs(post_inputs, input_directories)
    if build_environment({**os.environ, 'CARGO_HOME': str(options.cargo_home), 'RUSTC': str(options.rustc)}) != captured_env: raise Error('Build environment changed during collection')
    return manifest, files

def verify_directory(output: Path):
    raw = regular_bytes(output / 'manifest.json', 64 * MAX_TEXT)
    try: manifest = json.loads(raw)
    except (ValueError, UnicodeDecodeError) as error: raise Error('Invalid output manifest') from error
    if manifest.get('producer') != PRODUCER or manifest.get('schemaVersion') != SCHEMA: raise Error('Output is not an owned collector directory')
    expected = {'manifest.json', *manifest['inventory']}
    for path, record in manifest['inventory'].items():
        safe_relative(path)
        if not isinstance(record.get('bytes'), int) or isinstance(record['bytes'], bool) or not 0 <= record['bytes'] <= 128 * MAX_TEXT: raise Error('Invalid output inventory byte budget')
        data = regular_bytes(output / path, record['bytes'])
        if sha(data) != record['sha256'] or len(data) != record['bytes']: raise Error(f'Output hash/size mismatch: {path}')
    actual, dirs = file_set(output)
    if actual != expected or dirs != parent_dirs(expected): raise Error('Output directory has missing/extra files or directories')
    return manifest

@contextlib.contextmanager
def output_lock(output: Path):
    lock = output.parent / ('.' + output.name + '.rust-notices-lock')
    try: lock.mkdir()
    except FileExistsError as error: raise Error('Collector output is already locked; do not run concurrent writers') from error
    token = uuid.uuid4().hex
    owner = lock / 'owner'
    try:
        owner.write_text(token)
        yield
    finally:
        if owner.is_file() and owner.read_text() == token:
            owner.unlink(); lock.rmdir()

def publish(output: Path, files, options):
    output = output.absolute()
    forbidden = [options.manifest.parent, options.cargo_home, options.supplement.parent]
    if any(output.resolve().is_relative_to(p.resolve()) or p.resolve().is_relative_to(output.resolve()) for p in forbidden): raise Error('Output overlaps a source, cache, or supplement input directory')
    output.parent.mkdir(parents=True, exist_ok=True)
    with output_lock(output):
        if output.exists(): verify_directory(output)
        stage = Path(tempfile.mkdtemp(prefix='.' + output.name + '.stage-', dir=output.parent))
        backup = output.parent / ('.' + output.name + '.backup-' + uuid.uuid4().hex)
        moved_old = installed = False
        try:
            for rel, raw in sorted(files.items()):
                safe_relative(rel); p = stage / rel; p.parent.mkdir(parents=True, exist_ok=True)
                with p.open('xb') as stream: stream.write(raw); stream.flush(); os.fsync(stream.fileno())
            verify_directory(stage)
            snapshot = json.loads(files['manifest.json'])
            same_inputs(snapshot['inputFingerprints'], snapshot['inputDirectoryFingerprints'])
            if output.exists(): os.replace(output, backup); moved_old = True
            os.replace(stage, output); installed = True
        except BaseException:
            if installed and output.exists(): shutil.rmtree(output)
            if moved_old and backup.exists(): os.replace(backup, output)
            raise
        finally:
            if stage.exists(): shutil.rmtree(stage)
        if backup.exists(): shutil.rmtree(backup)

def verify(options, output: Path, strict=False, runner=execute):
    output = output.absolute()
    with output_lock(output):
        actual = verify_directory(output)
        expected, files = build_candidate(options, runner)
        if actual != expected: raise Error('Current lock/context/source/config/material inputs differ from generated candidate')
        for path, data in files.items():
            if regular_bytes(output / path) != data: raise Error(f'Current reproduced output differs: {path}')
        if strict and expected['strictIssues']: raise StrictError(f'Strict candidate material gate failed: {len(expected["strictIssues"])} unresolved component/context records')
    return actual

def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['collect', 'verify'])
    for name in ['manifest', 'lock', 'cargo-home', 'supplement', 'cargo', 'rustc', 'output']:
        parser.add_argument('--' + name, required=True, type=Path)
    for name in ['edition', 'package', 'target', 'host', 'profile']:
        parser.add_argument('--' + name, required=True)
    parser.add_argument('--feature-mode', choices=['default', 'no-default', 'all'], required=True)
    parser.add_argument('--features', default='')
    parser.add_argument('--strict', action='store_true', help='Only for verify; fail any preserved gap, additional review, missing stdlib notice, or cross-platform release context')
    options = parser.parse_args(argv)
    options.features = sorted(set(filter(None, options.features.split(','))))
    if any(not re.fullmatch(r'[A-Za-z0-9_:+./-]+', f) for f in options.features): parser.error('Unsupported feature argument')
    for name in ['manifest', 'lock', 'cargo_home', 'supplement', 'cargo', 'rustc']: setattr(options, name, getattr(options, name).resolve())
    if options.strict and options.command != 'verify': parser.error('--strict only applies to read-only verify')
    try:
        if options.command == 'collect':
            manifest, files = build_candidate(options); publish(options.output, files, options)
        else: manifest = verify(options, options.output, options.strict)
        print(json.dumps({'command': options.command, **manifest['summary']}, sort_keys=True))
        return 0
    except StrictError as error:
        print(str(error), file=sys.stderr); return 2
    except (Error, OSError, ValueError, KeyError, subprocess.TimeoutExpired) as error:
        print(str(error), file=sys.stderr); return 1

if __name__ == '__main__':
    sys.exit(main())
