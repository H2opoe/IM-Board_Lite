import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, readFileSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { stageReleaseResources } from "./windows-release-resources.mjs";
const write=(path,data)=>{mkdirSync(dirname(path),{recursive:true});writeFileSync(path,data);};
test("release sources replace stale staged data and copy only declared runtime files",()=>{
 const root=mkdtempSync(join(tmpdir(),"imboard-resource-test-")),app=join(root,"app"),stage=join(root,"stage");
 try {
  write(join(app,"runtime/current.bin"),"current");write(join(app,"reader/main.py"),"current source");write(join(app,"reader/private.log"),"SYNTHETIC_USAGE");write(join(stage,"_up_/app.sqlite"),"SYNTHETIC_OLD_DATABASE");
  stageReleaseResources(app,stage,["runtime",{path:"reader",extensions:[".py"]}],{verify:()=>{}});
  assert.equal(readFileSync(join(stage,"_up_/runtime/current.bin"),"utf8"),"current");
  assert.equal(readFileSync(join(stage,"_up_/reader/main.py"),"utf8"),"current source");
  assert.equal(existsSync(join(stage,"_up_/app.sqlite")),false);assert.equal(existsSync(join(stage,"_up_/reader/private.log")),false);
 }finally{rmSync(root,{recursive:true,force:true});}
});
test("source verification failure and escaped paths cannot become release inputs",()=>{
 const root=mkdtempSync(join(tmpdir(),"imboard-resource-test-")),app=join(root,"app"),stage=join(root,"stage");
 try {mkdirSync(app);assert.throws(()=>stageReleaseResources(app,stage,["../outside"],{verify:()=>{}}),/escapes/);assert.throws(()=>stageReleaseResources(app,stage,[],{verify:()=>{throw new Error("bad manifest");}}),/bad manifest/);}
 finally{rmSync(root,{recursive:true,force:true});}
});
