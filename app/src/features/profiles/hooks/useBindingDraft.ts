import { useCallback, useEffect, useRef } from "react";
import { releaseProfileDraft } from "../api/profilesApi";

export function releaseUnusedDraft(profileId: string) {
  void releaseProfileDraft(profileId).catch(() => {
    console.warn("绑定草稿释放失败，后端登记可能保留到应用退出。");
  });
}

// Register a modal's current draft and release its registry entry on every exit path.
// Backend release is idempotent and never deletes persisted accounts or files.
export function useBindingDraft() {
  const current = useRef<string | null>(null);
  const release = useCallback(() => {
    const id = current.current;
    current.current = null;
    if (id) releaseUnusedDraft(id);
  }, []);
  const track = useCallback((id: string) => {
    if (current.current === id) return;
    release();
    current.current = id;
  }, [release]);
  useEffect(() => release, [release]);
  return { track, release };
}
