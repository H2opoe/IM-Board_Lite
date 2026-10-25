import { closeSync, openSync, chmodSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, rmdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

function statOrNull(path) {
  try { return lstatSync(path); } catch(error) { if(error.code==='ENOENT')return null; throw error; }
}
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const forbiddenLitePath = /wechat|weixin|wx[_-]?key|frida|bridge_main/i;
const demoUiPath = /^app\/src\/(components\/(cards|shared)\/|styles\/|utils\/|features\/(dashboard|profiles)\/model\/)/;
const demoLicensePath = /^(?:app\/scripts\/(?:frontend-notices|collect-frontend-notices|verify-frontend-notices)\.mjs|app\/scripts\/frontend-notices\.test\.mjs|app\/scripts\/application-license-sources\/frontend\/(?:sources\.json|[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+\.txt)|app\/scripts\/(?:rust-notices\.py|test_rust_notices\.py|RUST-NOTICES\.md))$/;
const demoMetadataPath = /^app\/scripts\/(?:verify-release-config\.mjs|release-config\.test\.mjs|release-build-env(?:\.test)?\.mjs)$/;
function run(command, args, cwd) {
  const result = spawnSync(command, args, {cwd, encoding:'utf8'});
  if (result.status !== 0) throw new Error(`${command} failed in ${cwd}: ${result.stderr || result.stdout || result.error}`);
  return result.stdout.trim();
}
function pathIn(root, name) {
  if (!name || name.includes('\\') || name.includes(':') || name.split('/').some(part => !part || part === '.' || part === '..') || name.split('/')[0] === '.git') throw new Error(`Unsafe relative path: ${name}`);
  const path = resolve(root, name);
  if (!path.startsWith(resolve(root) + sep)) throw new Error(`Path escapes worktree: ${name}`);
  let current = resolve(root);
  for (const part of name.split('/')) {
    current = join(current, part);
    if (statOrNull(current)?.isSymbolicLink()) throw new Error(`Symlink is not allowed: ${current}`);
  }
  return path;
}
function fileState(path) {
  const stat = statOrNull(path);
  if (!stat) return null;
  if (!stat.isFile() || stat.isSymbolicLink()) throw new Error(`Expected regular file: ${path}`);
  return {sha256:hash(readFileSync(path)), mode:stat.mode & 0o777};
}
const canonicalPath = path => realpathSync.native(path);
function sameDirectory(left, right) {
  const a = statSync(left, { bigint: true }), b = statSync(right, { bigint: true });
  return a.dev === b.dev && a.ino !== 0n && a.ino === b.ino;
}
function gitState(root) {
  const gitRoot = canonicalPath(run('git',['rev-parse','--show-toplevel'],root));
  if (gitRoot !== canonicalPath(root) && !sameDirectory(gitRoot, root)) throw new Error(`Not a worktree root: ${root}`);
  return {head:run('git',['rev-parse','HEAD'],root),branch:run('git',['branch','--show-current'],root),dirty:run('git',['status','--porcelain','--untracked-files=all'],root)};
}
function validateManifest(manifest) {
  if (manifest.schemaVersion !== 2 || !Array.isArray(manifest.files) || !manifest.targets) throw new Error('Unsupported edition sync manifest');
  for (const [name, target] of Object.entries(manifest.targets)) {
    if (!/^[A-Za-z0-9_-]+$/.test(name) || !['full','lite','demo'].includes(target.edition) || !target.branch) throw new Error(`Invalid target: ${name}`);
  }
  const seen=new Set();
  for (const item of manifest.files) {
    if (!Array.isArray(item.targets) || item.targets.length === 0) throw new Error('File must have explicit targets');
    for (const name of item.targets) {
      const target=manifest.targets[name];
      if (!target) throw new Error(`Unknown target: ${name}`);
      if (target.edition === 'lite' && (forbiddenLitePath.test(item.source) || forbiddenLitePath.test(item.destination))) throw new Error(`Forbidden Lite path: ${item.destination}`);
      if (target.edition === 'demo' && (![item.source,item.destination].every(path=>demoUiPath.test(path)||demoLicensePath.test(path)||demoMetadataPath.test(path)))) throw new Error(`Demo permits presentation files and explicit static build inputs only: ${item.destination}`);
      const key=`${name}:${item.destination}`;
      if (seen.has(key)) throw new Error(`Duplicate destination: ${key}`);
      seen.add(key);
    }
  }
}

export function createPlan(sourceRoot, manifest) {
  sourceRoot=canonicalPath(sourceRoot);validateManifest(manifest);
  const source=gitState(sourceRoot);
  const targets={};
  for (const [name, target] of Object.entries(manifest.targets)) {
    const root=pathIn(dirname(sourceRoot),name);
    if (sameDirectory(root, sourceRoot)) throw new Error('Source cannot also be a target');
    const state=gitState(root);
    if (state.branch !== target.branch) throw new Error(`Wrong target branch: ${name}: ${state.branch}, expected ${target.branch}`);
    targets[name]={...state,root,edition:target.edition};
  }
  const operations=[];
  for (const item of manifest.files) {
    const sourcePath=pathIn(sourceRoot,item.source);
    const sourceFile=fileState(sourcePath);
    if (!sourceFile) throw new Error(`Missing source: ${item.source}`);
    for (const target of item.targets) {
      const destination=pathIn(targets[target].root,item.destination);
      const original=fileState(destination);
      operations.push({target,source:item.source,destination:item.destination,sourceFile,original,action:JSON.stringify(original)===JSON.stringify(sourceFile)?'same':original?'replace':'create'});
    }
  }
  return {schemaVersion:2,toolHash:hash(readFileSync(fileURLToPath(import.meta.url))),sourceRoot,source,manifestHash:hash(JSON.stringify(manifest)),targets,operations,manualReview:manifest.manualReview ?? []};
}
function verifyTarget(root, edition) {
  run('git',['diff','--check'],root);
  if (edition === 'lite') run(process.execPath,['scripts/verify-lite-boundary.mjs'],join(root,'app'));
  if (edition === 'demo') run(process.execPath,['scripts/verify-demo-boundary.mjs'],join(root,'app'));
}

function writeOwnedTemporary(path, bytes, temporary) {
  const fd=openSync(path,'wx');
  temporary.push(path);
  try{writeFileSync(fd,bytes);}finally{closeSync(fd);}
}
function temporaryPath(destination, id, index, phase) {
  return join(dirname(destination), `.im-board-sync-${id}-${index}-${phase}`);
}
function saveDirectories(backup, directories) {
  // Publish the journal before mkdir; recovery tolerates a directory not yet created.
  const next=join(backup,'directories.next');
  writeFileSync(next,JSON.stringify(directories));
  renameSync(next,join(backup,'directories.json'));
}
function cleanRecordedTemporaries(backup, entries) {
  const metadata=join(backup,'temporary.json');
  if(!existsSync(metadata))return null; // Journals produced before temporary tracking.
  const {schemaVersion,id}=JSON.parse(readFileSync(metadata,'utf8'));
  if(schemaVersion!==1 || !/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(id))throw new Error('Invalid temporary journal');
  const removals=[];
  for(const {op,index,destination,previous} of entries){
    const next=readFileSync(join(backup,`${index}.new`));
    if(hash(next)!==op.sourceFile.sha256)throw new Error('Recovery staged hash mismatch');
    for(const phase of ['publish','rollback','recover']){
      const path=temporaryPath(destination,id,index,phase);
      if(!fileState(path))continue;
      const bytes=readFileSync(path);
      const expected=phase==='publish'?next:previous;
      // A terminated write may leave a prefix. Unrelated edits must be preserved.
      if(!expected || bytes.length>expected.length || !bytes.equals(expected.subarray(0,bytes.length)))throw new Error(`Concurrent edit blocks temporary cleanup: ${path}`);
      removals.push(path);
    }
  }
  for(const path of removals)rmSync(path);
  return id;
}

export function applyPlan(plan, manifest, {verify=verifyTarget, beforePublish=()=>{}, onBackup=()=>{}, afterTemporaryWrite=()=>{}}={}) {
  const fresh=createPlan(plan.sourceRoot,manifest);
  if (JSON.stringify(fresh)!==JSON.stringify(plan)) throw new Error('Plan is stale: HEAD, worktree, manifest, source or destination changed. Generate and review a new plan.');
  for (const [name,target] of Object.entries(plan.targets)) if (target.dirty) throw new Error(`Target worktree has uncommitted changes: ${name}`);
  const changed=plan.operations.filter(op=>op.action!=='same');
  if (!changed.length) return {changed:0};
  const backup=mkdtempSync(join(tmpdir(),'im-board-edition-sync-'));
  const written=[],temporary=[],createdDirs=[];
  const temporaryId=randomUUID();
  let keepBackup=false;
  try {
    // Read and stage every input before touching any target.
    const staged=changed.map((op,index)=>{
      const input=readFileSync(pathIn(plan.sourceRoot,op.source));
      if(hash(input)!==op.sourceFile.sha256)throw new Error(`Source changed while staging: ${op.source}`);
      const destination=pathIn(plan.targets[op.target].root,op.destination);
      const previous=op.original?readFileSync(destination):null;
      if(previous && hash(previous)!==op.original.sha256)throw new Error(`Target changed while staging: ${op.destination}`);
      writeFileSync(join(backup,`${index}.new`),input);
      if(previous)writeFileSync(join(backup,`${index}.old`),previous);
      return {op,index,destination};
    });
    writeFileSync(join(backup,'plan.json'),JSON.stringify(plan,null,2));
    writeFileSync(join(backup,'directories.json'),'[]');
    writeFileSync(join(backup,'temporary.json'),JSON.stringify({schemaVersion:1,id:temporaryId}));
    onBackup(backup);
    for(const entry of staged){
      const {op,index,destination}=entry;
      beforePublish(index);
      pathIn(plan.targets[op.target].root,op.destination);
      if(JSON.stringify(fileState(destination))!==JSON.stringify(op.original))throw new Error(`Concurrent target edit: ${destination}`);
      let parent=dirname(destination);const missing=[];
      while(!existsSync(parent)){missing.push(parent);parent=dirname(parent);}
      for(const dir of missing.reverse()){createdDirs.push(dir);saveDirectories(backup,createdDirs);mkdirSync(dir);}
      const tmp=temporaryPath(destination,temporaryId,index,'publish');
      writeOwnedTemporary(tmp,readFileSync(join(backup,`${index}.new`)),temporary);
      afterTemporaryWrite(index,tmp);
      chmodSync(tmp,op.sourceFile.mode);
      renameSync(tmp,destination);written.push(entry);
    }
    for(const name of new Set(changed.map(op=>op.target)))verify(plan.targets[name].root,plan.targets[name].edition);
    for(const entry of written)if(JSON.stringify(fileState(entry.destination))!==JSON.stringify(entry.op.sourceFile))throw new Error(`Post-write mismatch: ${entry.destination}`);
    return {changed:written.length};
  } catch(error) {
    const failures=[];
    for(const {op,index,destination} of written.reverse()){
      try{
        // Never overwrite a concurrent edit during rollback.
        if(JSON.stringify(fileState(destination))!==JSON.stringify(op.sourceFile))throw new Error('concurrent change; original retained in backup');
        if(op.original){
          const tmp=temporaryPath(destination,temporaryId,index,'rollback');
          writeOwnedTemporary(tmp,readFileSync(join(backup,`${index}.old`)),temporary);chmodSync(tmp,op.original.mode);renameSync(tmp,destination);
        }else rmSync(destination);
      }catch(rollbackError){failures.push(`${destination}: ${rollbackError.message}`);}
    }
    // A failure before rename still owns a temporary file in a newly made directory.
    // Remove it before attempting to remove that directory.
    for(const path of temporary){try{rmSync(path,{force:true});}catch(cleanupError){failures.push(`${path}: ${cleanupError.message}`);}}
    for(const dir of createdDirs.reverse()){try{rmdirSync(dir);}catch{ /* Keep any directory populated by concurrent work. */ }}
    if(failures.length){keepBackup=true;throw new Error(`${error.message}\nRollback needs recovery; backups retained at ${backup}\n${failures.join('\n')}`);}
    throw new Error(`${error.message}\nAll published files rolled back.`);
  } finally {
    for(const path of temporary)rmSync(path,{force:true});
    if(!keepBackup)rmSync(backup,{recursive:true,force:true});
  }
}

export function recoverBackup(backup, manifest, expectedSourceRoot) {
  const plan=JSON.parse(readFileSync(join(backup,'plan.json'),'utf8'));
  validateManifest(manifest);
  if(plan.schemaVersion!==2 || plan.sourceRoot!==canonicalPath(expectedSourceRoot) || plan.manifestHash!==hash(JSON.stringify(manifest)))throw new Error('Recovery plan does not match this source and manifest');
  const allowed=new Set(manifest.files.flatMap(item=>item.targets.map(name=>`${name}:${item.source}:${item.destination}`)));
  if(plan.operations.length!==allowed.size)throw new Error('Recovery operation list is incomplete');
  const seen=new Set();
  for(const op of plan.operations){
    const key=`${op.target}:${op.source}:${op.destination}`;
    if(!allowed.has(key) || seen.has(key))throw new Error('Unexpected recovery operation');
    seen.add(key);
  }
  for(const [name,target] of Object.entries(plan.targets)){
    if(!manifest.targets[name] || target.root!==pathIn(dirname(plan.sourceRoot),name))throw new Error('Invalid recovery target');
    const state=gitState(target.root);
    if(state.head!==target.head || state.branch!==target.branch)throw new Error(`Recovery target HEAD changed: ${name}`);
  }
  const entries=plan.operations.filter(op=>op.action!=='same').map((op,index)=>{
    const destination=pathIn(plan.targets[op.target].root,op.destination);
    const current=fileState(destination);
    if(JSON.stringify(current)!==JSON.stringify(op.sourceFile) && JSON.stringify(current)!==JSON.stringify(op.original))throw new Error(`Concurrent edit blocks recovery: ${destination}`);
    let previous=null;
    if(op.original){previous=readFileSync(join(backup,`${index}.old`));if(hash(previous)!==op.original.sha256)throw new Error('Recovery backup hash mismatch');}
    return {op,index,destination,current,previous};
  });
  const directories=JSON.parse(readFileSync(join(backup,'directories.json'),'utf8'));
  if(!Array.isArray(directories))throw new Error('Invalid recovery directories');
  for(const dir of directories){
    const target=typeof dir==='string' && Object.values(plan.targets).find(t=>dir.startsWith(t.root+sep));
    if(!target || !entries.some(entry=>entry.destination.startsWith(dir+sep)))throw new Error('Invalid recovery directory');
    pathIn(target.root,relative(target.root,dir).split(sep).join('/'));
  }
  const temporaryId=cleanRecordedTemporaries(backup,entries) ?? randomUUID();
  // Upgrade legacy journals before creating a recovery temporary file.
  if(!existsSync(join(backup,'temporary.json')))writeFileSync(join(backup,'temporary.json'),JSON.stringify({schemaVersion:1,id:temporaryId}));
  let restored=0;
  for(const {op,index,destination,current,previous} of entries.reverse()){
    if(JSON.stringify(current)===JSON.stringify(op.original))continue;
    if(JSON.stringify(fileState(destination))!==JSON.stringify(current))throw new Error(`Concurrent edit blocks recovery: ${destination}`);
    if(previous){
      const tmp=temporaryPath(destination,temporaryId,index,'recover');
      const owned=[];
      try{writeOwnedTemporary(tmp,previous,owned);chmodSync(tmp,op.original.mode);renameSync(tmp,destination);}finally{for(const path of owned)rmSync(path,{force:true});}
    }else rmSync(destination);
    restored++;
  }
  for(const dir of directories.reverse()){
    try{rmdirSync(dir);}catch{/* Preserve nonempty directories. */}
  }
  rmSync(backup,{recursive:true,force:true});
  return {restored};
}

if(process.argv[1] && resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const sourceRoot=canonicalPath(resolve(dirname(fileURLToPath(import.meta.url)),'../..'));
  const manifest=JSON.parse(readFileSync(join(sourceRoot,'app/edition-sync-manifest.json'),'utf8'));
  const args=process.argv.slice(2);
  if(args.length===2 && args[0]==='--apply'){
    const plan=JSON.parse(readFileSync(resolve(args[1]),'utf8'));
    if(resolve(plan.sourceRoot)!==sourceRoot)throw new Error('Plan belongs to another source worktree');
    console.log(JSON.stringify(applyPlan(plan,manifest,{onBackup:path=>console.error(`Recovery journal: ${path}`)}),null,2));
  }else if(args.length===2 && args[0]==='--recover'){
    console.log(JSON.stringify(recoverBackup(resolve(args[1]),manifest,sourceRoot),null,2));
  }else if(args.length===0 || (args.length===2 && args[0]==='--plan')){
    const result=JSON.stringify(createPlan(sourceRoot,manifest),null,2)+'\n';
    if(args.length)writeFileSync(resolve(args[1]),result);else process.stdout.write(result);
  }else throw new Error('Usage: sync-edition-worktrees.mjs [--plan FILE | --apply FILE | --recover JOURNAL_DIR]');
}
