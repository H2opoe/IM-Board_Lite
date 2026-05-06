import { useCallback, useEffect, useState } from "react";
import { listProfiles } from "../api/profilesApi";
import type { ImProfile } from "../model/types";

export function useProfilesStore() {
  const [profiles, setProfiles] = useState<ImProfile[]>([]);

  const refreshProfiles = useCallback(async () => {
    setProfiles(await listProfiles());
  }, []);

  useEffect(() => {
    void refreshProfiles();
  }, [refreshProfiles]);

  return {
    profiles,
    refreshProfiles
  };
}
