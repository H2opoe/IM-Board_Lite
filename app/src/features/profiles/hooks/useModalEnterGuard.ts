import type { KeyboardEvent as ReactKeyboardEvent } from "react";
import { useRef } from "react";

const MODAL_COMPOSITION_ENTER_GUARD_MS = 160;

export function useModalEnterGuard() {
  const modalInputComposingRef = useRef(false);
  const modalCompositionEndedAtRef = useRef<number | null>(null);

  function resetModalCompositionState() {
    modalInputComposingRef.current = false;
    modalCompositionEndedAtRef.current = null;
  }

  function handleModalCompositionStart() {
    modalInputComposingRef.current = true;
  }

  function handleModalCompositionEnd() {
    modalInputComposingRef.current = false;
    modalCompositionEndedAtRef.current = performance.now();
  }

  function isModalInputMethodEnter(event: ReactKeyboardEvent<HTMLElement>) {
    const nativeEvent = event.nativeEvent as KeyboardEvent & { isComposing?: boolean };
    const legacyCompositionKeyCode = 229;
    const nativeKeyCode = nativeEvent.keyCode || nativeEvent.which;
    const isJustAfterComposition = modalCompositionEndedAtRef.current !== null
      && performance.now() - modalCompositionEndedAtRef.current < MODAL_COMPOSITION_ENTER_GUARD_MS;

    return nativeEvent.isComposing || modalInputComposingRef.current || nativeKeyCode === legacyCompositionKeyCode || isJustAfterComposition;
  }

  function shouldHandleModalEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key !== "Enter" || event.repeat || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) return false;
    if (isModalInputMethodEnter(event)) return false;
    const target = event.target as HTMLElement | null;
    if (!target) return true;
    if (target.tagName === "TEXTAREA" || target.tagName === "BUTTON" || target.closest("button")) return false;
    return true;
  }

  return {
    resetModalCompositionState,
    handleModalCompositionStart,
    handleModalCompositionEnd,
    shouldHandleModalEnter
  };
}