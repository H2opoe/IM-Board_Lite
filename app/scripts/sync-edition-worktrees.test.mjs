import {test} from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,readFileSync,rmSync,existsSync,chmodSync,statSync,symlinkSync,readdirSync} from 'node:fs';
import {join,dirname} from 'node:path';
import {tmpdir} from 'node:os';
import {spawnSync} from 'node:child_process';
import {createPlan,applyPlan,recoverBackup} from './sync-edition-worktrees.mjs';
function git(root,...args){const r=spawnSync('git',args,{cwd:root,encoding:'utf8'});assert.equal(r.status,0,r.stderr);}
function put(root,path,content){mkdirSync(dirname(join(root,path)),{recursive:true});writeFileSync(join(root,path),content);}
function fixture(){
 const workspace=mkdtempSync(join(tmpdir(),'edition-sync-test-'));const source=join(workspace,'Source');
 for(const name of ['Source','Target','Other']){
  const dir=join(workspace,name);mkdirSync(dir);git(dir,'init','-q');git(dir,'config','user.name','Fixture');git(dir,'config','user.email','fixture@example.invalid');git(dir,'branch','-M',name==='Source'?'main':'win');
  put(dir,'app/src/a.ts',name==='Source'?'new a\n':'old a\n');put(dir,'app/src/b.ts',name==='Source'?'new b\n':'old b\n');git(dir,'add','.');git(dir,'commit','-qm','fixture');
 }
 const manifest={schemaVersion:2,targets:{Target:{edition:'full',branch:'win'},Other:{edition:'full',branch:'win'}},files:['a','b'].map(name=>({source:`app/src/${name}.ts`,destination:`app/src/${name}.ts`,targets:['Target','Other']}))};
 return {workspace,source,manifest,target:join(workspace,'Target'),close:()=>rmSync(workspace,{recursive:true,force:true})};
}
function scoped(name,body){test(name,()=>{const f=fixture();try{body(f);}finally{f.close();}});}
scoped('plan is read-only and apply copies all targets after preflight',f=>{
 const plan=createPlan(f.source,f.manifest);assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
 assert.equal(applyPlan(plan,f.manifest).changed,4);for(const name of ['Target','Other'])assert.equal(readFileSync(join(f.workspace,name,'app/src/b.ts'),'utf8'),'new b\n');
});
scoped('missing late source does not copy earlier files',f=>{
 rmSync(join(f.source,'app/src/b.ts'));assert.throws(()=>createPlan(f.source,f.manifest),/Missing source/);assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
});
scoped('publication failure restores every already changed target and original permissions',f=>{
 chmodSync(join(f.target,'app/src/a.ts'),0o755);git(f.target,'add','.');git(f.target,'commit','--allow-empty','-qm','executable fixture');
 const plan=createPlan(f.source,f.manifest);assert.throws(()=>applyPlan(plan,f.manifest,{beforePublish:index=>{if(index===2)throw new Error('simulated disk failure');}}),/rolled back/);
 for(const name of ['Target','Other'])assert.equal(readFileSync(join(f.workspace,name,'app/src/a.ts'),'utf8'),'old a\n');
 if(process.platform!=='win32')assert.equal(statSync(join(f.target,'app/src/a.ts')).mode&0o777,0o755);
});
scoped('post-copy validation failure rolls back the whole batch',f=>{
 assert.throws(()=>applyPlan(createPlan(f.source,f.manifest),f.manifest,{verify:()=>{throw new Error('boundary violation');}}),/rolled back/);
 for(const name of ['Target','Other'])for(const file of ['a','b'])assert.equal(readFileSync(join(f.workspace,name,`app/src/${file}.ts`),'utf8'),`old ${file}\n`);
});
scoped('rollback removes newly created files and directories',f=>{
 f.manifest.files[0].destination='app/src/new/nested/a.ts';
 assert.throws(()=>applyPlan(createPlan(f.source,f.manifest),f.manifest,{verify:()=>{throw new Error('failure');}}),/rolled back/);
 assert.equal(existsSync(join(f.target,'app/src/new')),false);
});
scoped('dirty target is refused without changing user work',f=>{
 put(f.target,'app/src/a.ts','user edit\n');assert.throws(()=>applyPlan(createPlan(f.source,f.manifest),f.manifest),/uncommitted/);assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'user edit\n');
});
scoped('changed source, HEAD and destination invalidate reviewed plans',f=>{
 let plan=createPlan(f.source,f.manifest);put(f.source,'app/src/a.ts','changed again\n');assert.throws(()=>applyPlan(plan,f.manifest),/stale/);
 plan=createPlan(f.source,f.manifest);git(f.target,'commit','--allow-empty','-qm','new head');assert.throws(()=>applyPlan(plan,f.manifest),/stale/);
 plan=createPlan(f.source,f.manifest);put(f.target,'app/src/a.ts','concurrent edit\n');assert.throws(()=>applyPlan(plan,f.manifest),/stale/);
});
scoped('Demo cannot receive backend code, and traversal or duplicate destinations fail closed',f=>{
 f.manifest.targets.Target.edition='demo';assert.throws(()=>createPlan(f.source,f.manifest),/presentation/);
 f.manifest.targets.Target.edition='full';f.manifest.files[0].destination='../outside';assert.throws(()=>createPlan(f.source,f.manifest),/Unsafe/);
 f.manifest.files[0].destination=f.manifest.files[1].destination;assert.throws(()=>createPlan(f.source,f.manifest),/Duplicate/);
});
scoped('Demo static build inputs use a narrow allowlist and cannot carry backend destinations',f=>{
 f.manifest.targets.Target.edition='demo';
 const paths=[
  'app/scripts/frontend-notices.mjs',
  'app/scripts/collect-frontend-notices.mjs',
  'app/scripts/verify-frontend-notices.mjs',
  'app/scripts/frontend-notices.test.mjs',
  'app/scripts/rust-notices.py',
  'app/scripts/test_rust_notices.py',
  'app/scripts/RUST-NOTICES.md',
  'app/scripts/verify-release-config.mjs',
  'app/scripts/release-config.test.mjs',
  'app/scripts/application-license-sources/frontend/sources.json',
  'app/scripts/application-license-sources/frontend/component-1.0.0/LICENSE_MIT.txt'
 ];
 for(const path of paths)put(f.source,path,'license fixture\n');
 f.manifest.files=paths.map(path=>({source:path,destination:path,targets:['Target']}));
 const plan=createPlan(f.source,f.manifest);assert.equal(plan.operations.length,paths.length);
 for(const path of ['app/scripts/runtime.mjs','app/scripts/release-runtime.mjs','app/scripts/release-config.mjs','app/scripts/rust-runtime.py','app/scripts/unknown-rust-notices.py','app/scripts/application-license-sources/rust/runtime.py','app/src-tauri/src/lib.rs','app/scripts/application-license-sources/frontend/component/runtime.rs','app/scripts/application-license-sources/frontend/component/runtime.mjs']){
  f.manifest.files=[{source:paths[0],destination:path,targets:['Target']}];assert.throws(()=>createPlan(f.source,f.manifest),/presentation/);
  f.manifest.files=[{source:path,destination:paths[0],targets:['Target']}];assert.throws(()=>createPlan(f.source,f.manifest),/presentation/);
 }
});
scoped('target branch mismatches are refused',f=>{
 f.manifest.targets.Target.branch='different';assert.throws(()=>createPlan(f.source,f.manifest),/Wrong target branch/);
});
if(process.platform!=='win32')scoped('symlinked destination parents are never traversed',f=>{
 const outside=join(f.workspace,'outside');mkdirSync(outside);symlinkSync(outside,join(f.target,'linked'));f.manifest.files[0].destination='linked/a.ts';assert.throws(()=>createPlan(f.source,f.manifest),/Symlink/);assert.equal(existsSync(join(outside,'a.ts')),false);
});
scoped('recovery journal restores interruption state and refuses concurrent edits',f=>{
 let journal;
 assert.throws(()=>applyPlan(createPlan(f.source,f.manifest),f.manifest,{
  onBackup:path=>{journal=path;},
  beforePublish:index=>{if(index===1){put(f.target,'app/src/a.ts','concurrent content\n');throw new Error('interrupted');}}
 }),/backups retained/);
 try{
  assert.throws(()=>recoverBackup(journal,f.manifest,f.source),/Concurrent edit/);
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'concurrent content\n');
  put(f.target,'app/src/a.ts','new a\n');
  assert.equal(recoverBackup(journal,f.manifest,f.source).restored,1);
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
  assert.equal(existsSync(journal),false);
 }finally{if(journal)rmSync(journal,{recursive:true,force:true});}
});
scoped('a terminated child process can be recovered from its persisted journal',f=>{
 const moduleUrl=new URL('./sync-edition-worktrees.mjs',import.meta.url).href;
 const code=`import {createPlan,applyPlan} from ${JSON.stringify(moduleUrl)}; const manifest=${JSON.stringify(f.manifest)}; applyPlan(createPlan(${JSON.stringify(f.source)},manifest),manifest,{onBackup:path=>process.stdout.write(path),beforePublish:index=>{if(index===1)process.exit(23);}});`;
 const child=spawnSync(process.execPath,['--input-type=module','-e',code],{encoding:'utf8'});
 assert.equal(child.status,23,child.stderr);const journal=child.stdout;
 try{
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'new a\n');
  assert.equal(recoverBackup(journal,f.manifest,f.source).restored,1);
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
 }finally{rmSync(journal,{recursive:true,force:true});}
});

function terminateDuringTemporaryWrite(f, partial=false){
 const moduleUrl=new URL('./sync-edition-worktrees.mjs',import.meta.url).href;
 const code=`import {writeFileSync} from 'node:fs'; import {createPlan,applyPlan} from ${JSON.stringify(moduleUrl)}; const manifest=${JSON.stringify(f.manifest)}; applyPlan(createPlan(${JSON.stringify(f.source)},manifest),manifest,{onBackup:path=>process.stdout.write(path),afterTemporaryWrite:(index,path)=>{if(index===2){if(${partial})writeFileSync(path,'ne');process.exit(24);}}});`;
 const child=spawnSync(process.execPath,['--input-type=module','-e',code],{encoding:'utf8'});
 assert.equal(child.status,24,child.stderr);return child.stdout;
}
for(const partial of [false,true])scoped(`temporary write interruption recovers all files and removes ${partial?'partial':'complete'} staging file`,f=>{
 const journal=terminateDuringTemporaryWrite(f,partial);
 try{
  assert.equal(readdirSync(join(f.target,'app/src')).filter(n=>n.startsWith('.im-board-sync-')).length,1);
  assert.equal(recoverBackup(journal,f.manifest,f.source).restored,2);
  for(const name of ['Target','Other']){
   const dir=join(f.workspace,name,'app/src');
   assert.deepEqual(readdirSync(dir).sort(),['a.ts','b.ts']);
   assert.equal(readFileSync(join(dir,'a.ts'),'utf8'),'old a\n');
  }
 }finally{rmSync(journal,{recursive:true,force:true});}
});
scoped('temporary recovery refuses unrelated content before rolling back published files',f=>{
 const journal=terminateDuringTemporaryWrite(f);
 try{
  const dir=join(f.target,'app/src');const path=join(dir,readdirSync(dir).find(n=>n.startsWith('.im-board-sync-')));
  writeFileSync(path,'user data');
  assert.throws(()=>recoverBackup(journal,f.manifest,f.source),/Concurrent edit blocks temporary/);
  assert.equal(readFileSync(path,'utf8'),'user data');
  assert.equal(readFileSync(join(dir,'a.ts'),'utf8'),'new a\n');
  writeFileSync(path,'new b\n');
  assert.equal(recoverBackup(journal,f.manifest,f.source).restored,2);
 }finally{rmSync(journal,{recursive:true,force:true});}
});

scoped('failure after staging removes temporary files before new directories',f=>{
 f.manifest.files[0].destination='app/src/new/nested/a.ts';
 assert.throws(()=>applyPlan(createPlan(f.source,f.manifest),f.manifest,{afterTemporaryWrite:()=>{throw new Error('disk failure before rename');}}),/rolled back/);
 assert.equal(existsSync(join(f.target,'app/src/new')),false);
});
if(process.platform!=='win32')scoped('temporary recovery rejects symlinks and leaves the external file unchanged',f=>{
 const journal=terminateDuringTemporaryWrite(f);
 try{
  const dir=join(f.target,'app/src');const path=join(dir,readdirSync(dir).find(n=>n.startsWith('.im-board-sync-')));
  const external=join(f.workspace,'external');writeFileSync(external,'new b\n');rmSync(path);symlinkSync(external,path);
  assert.throws(()=>recoverBackup(journal,f.manifest,f.source),/Expected regular file/);
  assert.equal(readFileSync(external,'utf8'),'new b\n');
  rmSync(path);recoverBackup(journal,f.manifest,f.source);
 }finally{rmSync(journal,{recursive:true,force:true});}
});

scoped('invalid directory journal is refused before recovery changes any target',f=>{
 const journal=terminateDuringTemporaryWrite(f);
 try{
  writeFileSync(join(journal,'directories.json'),JSON.stringify([join(f.target,'unrelated')]));
  assert.throws(()=>recoverBackup(journal,f.manifest,f.source),/Invalid recovery directory/);
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'new a\n');
  writeFileSync(join(journal,'directories.json'),'[]');recoverBackup(journal,f.manifest,f.source);
 }finally{rmSync(journal,{recursive:true,force:true});}
});

test('case aliases of the same source root remain usable without accepting nested directories', t=>{
 const f=fixture();try{
  const alias=join(f.workspace,'source');
  if(!existsSync(alias)||statSync(alias).ino!==statSync(f.source).ino){t.skip('fixture filesystem has distinct case aliases');return;}
  const plan=createPlan(alias,f.manifest);assert.equal(plan.operations.length,4);
  assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
 }finally{f.close();}
});
scoped('a directory nested inside a repository is not its worktree root',f=>{
 const nested=join(f.source,'nested');mkdirSync(nested);
 assert.throws(()=>createPlan(nested,f.manifest),/Not a worktree root/);
 assert.equal(readFileSync(join(f.target,'app/src/a.ts'),'utf8'),'old a\n');
});
