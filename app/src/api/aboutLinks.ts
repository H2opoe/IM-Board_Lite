import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./tauri";

export async function openAboutLink(event: { preventDefault: () => void }, target: "profile" | "email"): Promise<void> {
  // Browser previews retain normal anchor behavior. Desktop WebViews dispatch
  // explicitly to the OS instead of trying to navigate a new embedded window.
  if (!isTauri) return;
  event.preventDefault();
  await invoke("open_about_link", { target });
}
