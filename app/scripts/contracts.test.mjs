import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const read = path => readFileSync(join(root, path), 'utf8');
const camel = name => name.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
function rustFields(source, name) {
  const declaration = source.match(new RegExp(`#\\[serde\\(rename_all = "camelCase"\\)\\]\\s*pub struct ${name} \\{([^}]+)\\}`));
  assert.ok(declaration, `${name} must use the verified camelCase serialization contract`);
  const body = declaration[1];
  assert.ok(!body.includes('#['), `${name}: field serde attributes need explicit contract support`);
  const fields = [...body.matchAll(/pub (\w+): ([^,]+),/g)];
  assert.ok(fields.length > 0);
  const types = { String:'string', bool:'boolean', i64:'number', usize:'number', u64:'number', 'Option<SyncProgress>':'SyncProgress|null', 'Option<SyncResult>':'SyncResult|null', 'Option<String>':'string|null', 'serde_json::Value':'object', 'Vec<String>':'string[]', "&'static str":'string', "&'static [&'static str]":'string[]' };
  return Object.fromEntries(fields.map(([, field, type]) => {
    assert.ok(types[type.trim()], `${name}.${field}: unhandled Rust type ${type}`);
    return [camel(field), types[type.trim()]];
  }));
}
function frontendFields(source, name) {
  const ast = ts.createSourceFile('contract.ts', source, ts.ScriptTarget.Latest, true);
  const declaration = ast.statements.find(node => ts.isInterfaceDeclaration(node) && node.name.text === name);
  assert.ok(declaration, `missing interface ${name}`);
  const simple = { 'SyncProgress | null':'SyncProgress|null', 'SyncResult | null':'SyncResult|null', 'string | null':'string|null', string:'string', number:'number', boolean:'boolean', 'string[]':'string[]', 'Record<string, unknown>':'object' };
  return Object.fromEntries(declaration.members.map(member => {
    assert.ok(ts.isPropertySignature(member) && member.type && !member.questionToken, `unhandled optional/member in ${name}`);
    const field = member.name.getText(ast);
    const spelling = member.type.getText(ast);
    // Intentional domain refinements of backend strings, checked explicitly.
    if (spelling === 'Platform') {
      const platform = ast.statements.find(node => ts.isTypeAliasDeclaration(node) && node.name.text === 'Platform');
      assert.ok(platform && ts.isUnionTypeNode(platform.type));
      assert.ok(platform.type.types.every(node => ts.isLiteralTypeNode(node) && ts.isStringLiteral(node.literal)));
      return [field, 'string'];
    }
    if (name === 'SyncResult' && field === 'aiStatus' && spelling === 'DashboardData["aiStatus"]') return [field, 'string'];
    assert.ok(simple[spelling], `${name}.${field}: unhandled frontend type ${spelling}`);
    return [field, simple[spelling]];
  }));
}
const contracts = [
  ['sync/status.rs','SyncJobSnapshot','features/sync/model/types.ts','SyncJobSnapshot'],
  ['storage/models.rs','ImProfile','features/profiles/model/types.ts','ImProfile'],
  ['storage/models.rs','SyncResult','features/sync/model/types.ts','SyncResult'],
  ['sync/job.rs','SyncProgress','features/sync/model/types.ts','SyncProgress'],
  ['connectors/mod.rs','ConnectorCapabilityDescriptor','features/profiles/model/types.ts','ConnectorCapability'],
];
for (const [rustPath, rustName, tsPath, tsName] of contracts) {
  test(`${rustName} serde fields match ${tsName} frontend fields and types`, () => {
    assert.deepEqual(frontendFields(read(`src/${tsPath}`), tsName), rustFields(read(`src-tauri/src/${rustPath}`), rustName));
  });
}
test('contract comparison detects renamed fields and incompatible primitive types', () => {
  const rust = rustFields('#[serde(rename_all = "camelCase")]\npub struct Fixture { pub account_id: String, pub enabled: bool, }', 'Fixture');
  assert.notDeepEqual(frontendFields('export interface Fixture { account: string; enabled: boolean; }', 'Fixture'), rust);
  assert.notDeepEqual(frontendFields('export interface Fixture { accountId: string; enabled: number; }', 'Fixture'), rust);
  assert.throws(() => rustFields('#[serde(rename_all = "camelCase")] pub struct Fixture { #[serde(skip)] pub account_id: String, }', 'Fixture'), /attributes/);
});

test('every literal frontend invoke has a registered Tauri command', () => {
  const backend = read('src-tauri/src/lib.rs');
  const handlers = backend.match(/tauri::generate_handler!\[([\s\S]*?)\]/)?.[1];
  assert.ok(handlers, 'missing command registration');
  const registered = new Set([...handlers.matchAll(/commands::\w+::(\w+)/g)].map(m => m[1]));
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) { visit(path); continue; }
      if (!/\.tsx?$/.test(path) || /\.test\./.test(path)) continue;
      const ast = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true);
      function walk(node) {
        if (ts.isCallExpression(node) && ts.isIdentifier(node.expression) && node.expression.text === 'invoke') {
          const command = node.arguments[0];
          assert.ok(command && ts.isStringLiteral(command), `${path}: dynamic invoke needs an explicit contract`);
          assert.ok(registered.has(command.text), `${path}: unregistered command ${command.text}`);
        }
        ts.forEachChild(node, walk);
      }
      walk(ast);
    }
  }
  visit(join(root, 'src'));
  assert.ok(registered.has('get_sync_job_state'));
});
