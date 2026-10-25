import { describe, expect, it } from "vitest";
import type { PlatformDeployment } from "../../../api/bridgeApi";
import { commandLineToolName, joinNativePath } from "./profilePathUtils";

describe("account configuration paths", () => {
  it.each([
    ["C:\\账号\\", "C:\\账号\\cache"],
    ["C:\\账号\\\\", "C:\\账号\\cache"],
    ["\\\\server\\共享\\", "\\\\server\\共享\\cache"],
    ["C:\\", "C:\\cache"],
    ["C:/账号/", "C:/账号\\cache"],
    ["/tmp/账号/", "/tmp/账号/cache"],
    ["/", "/cache"]
  ])("joins %s without a repeated directory separator", (base, expected) => {
    expect(joinNativePath(base, "cache")).toBe(expected);
  });
});

describe("binding command shell", () => {
  it("uses Windows PowerShell when deployment metadata is unavailable", () => {
    expect(commandLineToolName()).toBe("Windows PowerShell");
  });

  it("uses the shell named by the deployment", () => {
    const deployment = { commandShell: "  指定终端  " } as PlatformDeployment;
    expect(commandLineToolName(deployment)).toBe("指定终端");
  });
});
