import test from 'node:test';
import assert from 'node:assert/strict';
import { maskWecomOfficialDomain } from './lite-enterprise-domains.mjs';
const file='app/src-tauri/src/bridge_runner/wecom_read_compat/mod.rs';
test('reviewed enterprise adapter may use its official HTTPS domain',()=>assert.ok(!/weixin/i.test(maskWecomOfficialDomain(file,'https://qyapi.weixin.qq.com/api'))));
test('exception does not apply to other source files or personal API domains',()=>{
 assert.match(maskWecomOfficialDomain('app/src-tauri/src/other.rs','https://qyapi.weixin.qq.com/api'),/weixin/);
 assert.match(maskWecomOfficialDomain(file,'https://weixin.example.test/api'),/weixin/);
});
test('personal source markers are still scanned inside the adapter',()=>{
 assert.match(maskWecomOfficialDomain(file,'wechat frida wxid personal_weixin_reader'),/wechat frida wxid personal_weixin_reader/);
});
