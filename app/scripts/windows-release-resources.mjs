import { cpSync, existsSync, lstatSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, extname, join, resolve, sep } from "node:path";
import { spawnSync } from "node:child_process";

// Assemble from declared source inputs, never from an old build/template tree.
export function stageReleaseResources(appRoot, stage, rules, { verify } = {}) {
  if (verify) verify();
  else {
    const r=spawnSync(process.execPath,[join(appRoot,"scripts/verify-runtime-manifest.mjs")],{cwd:appRoot,stdio:"inherit"});
    if(r.status!==0)throw new Error("Source runtime manifest verification failed");
  }
  const root=join(stage,"_up_");rmSync(root,{recursive:true,force:true});mkdirSync(root,{recursive:true});
  function copy(source,destination,extensions) {
    const info=lstatSync(source);
    if(info.isSymbolicLink())throw new Error("Release resources cannot contain symbolic links");
    if(info.isDirectory()) {
      mkdirSync(destination,{recursive:true});
      for(const name of readdirSync(source).sort()) {
        if(name===".DS_Store" || name==="__pycache__")continue;
        copy(join(source,name),join(destination,name),extensions);
      }
    } else if(info.isFile() && (!extensions || extensions.includes(extname(source)))) {
      mkdirSync(dirname(destination),{recursive:true});cpSync(source,destination);
    } else if(!info.isFile())throw new Error("Release resource is not a regular file");
  }
  for(const rule of rules) {
    const item=typeof rule==="string"?{path:rule}:rule;
    const source=resolve(appRoot,item.path),target=resolve(root,item.path);
    if(!source.startsWith(resolve(appRoot)+sep) || !target.startsWith(resolve(root)+sep))throw new Error("Release resource escapes its root");
    if(!existsSync(source))throw new Error(`Missing release source: ${item.path}`);
    // Check parent components as well as descendants before copying.
    let cursor=source;while(cursor!==resolve(appRoot)){if(lstatSync(cursor).isSymbolicLink())throw new Error("Release source path contains a link");cursor=dirname(cursor);}
    copy(source,target,item.extensions);
  }
}
