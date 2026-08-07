import { useEffect, useRef, useState } from "react";
import type { Platform } from "../model/types";

const COMMAND_COPIED_RESET_MS = 1800;

export function useCopiedCommandStatus() {
  const [copiedCommandPlatform, setCopiedCommandPlatform] = useState<Platform | null>(null);
  const copiedCommandResetTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => clearCopiedCommandResetTimer();
  }, []);

  function clearCopiedCommandResetTimer() {
    if (copiedCommandResetTimerRef.current === null) return;
    window.clearTimeout(copiedCommandResetTimerRef.current);
    copiedCommandResetTimerRef.current = null;
  }

  function resetCopiedCommandStatus() {
    clearCopiedCommandResetTimer();
    setCopiedCommandPlatform(null);
  }

  function scheduleCopiedCommandReset(platform: Platform) {
    clearCopiedCommandResetTimer();
    copiedCommandResetTimerRef.current = window.setTimeout(() => {
      setCopiedCommandPlatform((currentPlatform) => (currentPlatform === platform ? null : currentPlatform));
      copiedCommandResetTimerRef.current = null;
    }, COMMAND_COPIED_RESET_MS);
  }

  return {
    copiedCommandPlatform,
    resetCopiedCommandStatus,
    scheduleCopiedCommandReset,
    setCopiedCommandPlatform
  };
}
