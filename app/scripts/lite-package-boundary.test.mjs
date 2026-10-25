import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, copyFileSync, appendFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { assertLiteResources } from './lite-package-boundary.mjs';

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(),'imboard-lite-boundary-'));
  try { run(root); } finally { rmSync(root,{recursive:true,force:true}); }
}
test('allows DingTalk error symbols in the enterprise DWS binary',()=>fixture(root=>{
  writeFileSync(join(root,'dws.exe'),Buffer.concat([Buffer.from([0x4d,0x5a,0,0]),Buffer.from('github.com/DingTalk-Real-AI/dingtalk-workspace-cli/internal/apiclient.checkDingTalkError\0企业微信')]));
  assert.doesNotThrow(()=>assertLiteResources(root));
}));
test('still rejects personal message schema markers and forbidden runtime paths',()=>fixture(root=>{
  for(const marker of ['filehelper','微信邀请链接','talker','talker_name','"Talker"','wechat_cli','wxid_123','微信']) {
    writeFileSync(join(root,'payload.bin'),marker);
    assert.throws(()=>assertLiteResources(root),/微信源码标记/);
  }
  rmSync(join(root,'payload.bin'));
  mkdirSync(join(root,'python'));
  assert.throws(()=>assertLiteResources(root),/微信源码或运行时/);
}));

test('generic native file-cipher helper names are not WeChat identifiers',()=>fixture(root=>{
  writeFileSync(join(root,'payload.bin'),'encryptFileHelper decryptFileHelper');
  assert.doesNotThrow(()=>assertLiteResources(root));
}));

test('the vetted enterprise invitation metadata exception rejects a modified binary',()=>fixture(root=>{
  const folder=join(root,'runtime','dws','1.0.65-imboard.1','win-x64');mkdirSync(folder,{recursive:true});
  const target=join(folder,'dws.exe');
  copyFileSync(new URL('../runtime/dws/1.0.65-imboard.1/win-x64/dws.exe',import.meta.url),target);
  assert.doesNotThrow(()=>assertLiteResources(root));
  appendFileSync(target,'tampered');
  assert.throws(()=>assertLiteResources(root),/微信源码标记/);
}));

test('official enterprise API domain is allowed only in the host application binaries',()=>fixture(root=>{
  const host=join(root,'im_board_lib.dll');writeFileSync(host,'https://qyapi.weixin.qq.com/cgi-bin/aibot/cli/get_mcp_config');
  assert.doesNotThrow(()=>assertLiteResources(root));
  appendFileSync(host,' wechat_cli');assert.throws(()=>assertLiteResources(root),/微信源码标记/);
  rmSync(host);writeFileSync(join(root,'unreviewed.bin'),'weixin.qq.com');assert.throws(()=>assertLiteResources(root),/微信源码标记/);
}));
