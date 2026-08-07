import { useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";

const MODAL_COMPOSITION_ENTER_GUARD_MS = 120;

export function useModalEnterGuard() {
  const modalInputComposingRef = useRef(false);
  const modalCompositionEndedAtRef = useRef(0);

  function resetModalCompositionState() {
    modalInputComposingRef.current = false;
    modalCompositionEndedAtRef.current = 0;
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
    const isJustAfterComposition = performance.now() - modalCompositionEndedAtRef.current < MODAL_COMPOSITION_ENTER_GUARD_MS;

    // macOS 中文输入法用回车放弃候选词时，不同输入法对 isComposing 的上报不稳定，这里同时记录弹窗内的 composition 生命周期。
    return nativeEvent.isComposing || modalInputComposingRef.current || nativeKeyCode === legacyCompositionKeyCode || isJustAfterComposition;
  }

  function shouldHandleModalEnter(event: ReactKeyboardEvent<HTMLElement>) {
    if (event.key !== "Enter" || event.repeat || event.shiftKey || event.metaKey || event.ctrlKey || event.altKey) return false;
    if (isModalInputMethodEnter(event)) return false;
    const target = event.target as HTMLElement | null;
    if (!target) return true;
    const tagName = target.tagName.toLowerCase();
    if (tagName === "textarea" || tagName === "button" || target.closest("button")) return false;
    return true;
  }

  return {
    resetModalCompositionState,
    handleModalCompositionStart,
    handleModalCompositionEnd,
    shouldHandleModalEnter
  };
}
