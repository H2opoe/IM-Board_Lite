import { useEffect, useState } from "react";
import { PLATFORM_BINDING_OPTIONS, type PlatformBindingOption } from "../../../constants/platforms";
import { isDemoMode } from "../../../api/demoMode";
import { getConnectorCapabilities } from "../api/profilesApi";
import { logRecoverableError } from "../../../utils/logging";

export function useConnectorCapabilities() {
  const [options, setOptions] = useState<PlatformBindingOption[]>(PLATFORM_BINDING_OPTIONS);

  useEffect(() => {
    if (isDemoMode()) return;
    let active = true;
    void getConnectorCapabilities()
      .then((capabilities) => {
        if (!active) return;
        setOptions(
          capabilities.map((capability) => ({
            id: capability.platform,
            label: capability.label,
            auth: capability.authDescription,
            runtimeDependency: capability.runtimeDependency,
            permissions: capability.permissions,
            commands: capability.commands,
            healthCheck: capability.healthCheck
          }))
        );
      })
      .catch((error) => {
        logRecoverableError("读取平台能力失败，已使用内置平台列表", error);
      });
    return () => {
      active = false;
    };
  }, []);

  return options;
}
