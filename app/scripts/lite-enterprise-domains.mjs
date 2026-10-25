// Enterprise Tencent API domains are not personal-account source markers.
// This exception is limited to the reviewed Wecom read adapter and its fixtures.
export function maskWecomOfficialDomain(path, text) {
  path = path.replaceAll("\\", "/");
  if (!/^app\/src-tauri\/src\/bridge_runner\/wecom_read_compat\/(?:mod|tests)\.rs$/.test(path)) return text;
  return text.replaceAll("weixin.qq.com", "enterprise-api-domain");
}
