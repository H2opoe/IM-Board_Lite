import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,rmSync} from 'node:fs';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {assertLiteResources} from './lite-macos-package-boundary.mjs';
test('Mac enterprise-domain exception is restricted to the host binary and retains other markers',()=>{
 const root=mkdtempSync(join(tmpdir(),'imboard-mac-gate-'));
 try {
  const bin=join(root,'Contents','MacOS');mkdirSync(bin,{recursive:true});const host=join(bin,'IM-Board-Lite');
  writeFileSync(host,'https://qyapi.weixin.qq.com/cgi-bin/aibot/cli/get_mcp_config');assert.doesNotThrow(()=>assertLiteResources(root));
  writeFileSync(host,'weixin.qq.com wechat_cli');assert.throws(()=>assertLiteResources(root),/微信源码标记/);
  writeFileSync(host,'enterprise');writeFileSync(join(bin,'other'),'weixin.qq.com');assert.throws(()=>assertLiteResources(root),/微信源码标记/);
 } finally {rmSync(root,{recursive:true,force:true});}
});
