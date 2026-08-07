import { getVersion } from "@tauri-apps/api/app";

export async function getAppVersion(): Promise<string | null> {
  try {
    const version = (await getVersion()).trim();
    return version || null;
  } catch {
    return null;
  }
}

export function displayAppVersion(appVersion: string | null): string {
  if (__IM_BOARD_RELEASE_LABEL__ && __IM_BOARD_RELEASE_LABEL__ !== appVersion) {
    return __IM_BOARD_RELEASE_LABEL__;
  }
  return appVersion ?? "--";
}
