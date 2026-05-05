import { type CSSProperties, type PointerEvent as ReactPointerEvent, useEffect, useRef, useState } from "react";
import type { ImProfile } from "../model/types";
import {
  captureProfileDragRects,
  profileDragRowShift as calculateProfileDragRowShift,
  profileDragTargetFromPointer as profileDragTargetFromPointerPosition,
  profilesAfterDrag as reorderProfilesAfterDrag,
  profilesHaveSameOrder,
  type ProfileDragPlacement,
  type ProfileDragRect,
  type ProfileDragTarget,
  type ProfileDragVisualState
} from "../model/profileOrdering";

const PROFILE_DRAG_ROW_GAP = 8;

interface UseProfileDragOrderingOptions {
  orderedProfiles: ImProfile[];
  isBatchManaging: boolean;
  saveProfileOrder: (profiles: ImProfile[]) => Promise<void>;
}

export function useProfileDragOrdering({ orderedProfiles, isBatchManaging, saveProfileOrder }: UseProfileDragOrderingOptions) {
  const [draggedProfileId, setDraggedProfileId] = useState("");
  const [dragTargetProfileId, setDragTargetProfileId] = useState("");
  const [dragPlacement, setDragPlacement] = useState<ProfileDragPlacement>("before");
  const [dragVisualState, setDragVisualState] = useState<ProfileDragVisualState | null>(null);
  const [previewProfiles, setPreviewProfiles] = useState<ImProfile[] | null>(null);
  const profileTableRef = useRef<HTMLDivElement>(null);
  const profileDragRectsRef = useRef<Map<string, ProfileDragRect>>(new Map());
  const saveProfileOrderRef = useRef(saveProfileOrder);
  const visibleProfiles = draggedProfileId ? orderedProfiles : previewProfiles ?? orderedProfiles;
  const draggedProfile = draggedProfileId ? orderedProfiles.find((profile) => profile.id === draggedProfileId) ?? null : null;

  useEffect(() => {
    saveProfileOrderRef.current = saveProfileOrder;
  }, [saveProfileOrder]);

  useEffect(() => {
    if (!draggedProfileId) return undefined;

    function handlePointerMove(event: PointerEvent) {
      event.preventDefault();
      setDragVisualState((currentState) =>
        currentState && currentState.profileId === draggedProfileId
          ? { ...currentState, pointerX: event.clientX, pointerY: event.clientY }
          : currentState
      );
      const target = resolveProfileDragTarget(event.clientY);
      if (!target) return;
      setDragTargetProfileId(target.profileId);
      setDragPlacement(target.placement);
      setPreviewProfiles(reorderProfilesAfterDrag(orderedProfiles, draggedProfileId, target.profileId, target.placement));
    }

    function handlePointerUp(event: PointerEvent) {
      if (event.type === "pointercancel") {
        resetProfileDragState();
        return;
      }
      const target = resolveProfileDragTarget(event.clientY);
      const nextProfiles = target ? reorderProfilesAfterDrag(orderedProfiles, draggedProfileId, target.profileId, target.placement) : previewProfiles;
      setDraggedProfileId("");
      setDragTargetProfileId("");
      setDragVisualState(null);
      profileDragRectsRef.current.clear();
      if (!nextProfiles || profilesHaveSameOrder(nextProfiles, orderedProfiles)) {
        setPreviewProfiles(null);
        return;
      }
      setPreviewProfiles(nextProfiles);
      void saveProfileOrderRef.current(nextProfiles);
    }

    window.addEventListener("pointermove", handlePointerMove, { passive: false });
    window.addEventListener("pointerup", handlePointerUp);
    window.addEventListener("pointercancel", handlePointerUp);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", handlePointerUp);
      window.removeEventListener("pointercancel", handlePointerUp);
    };
  }, [draggedProfileId, orderedProfiles, previewProfiles]);

  function resolveProfileDragTarget(clientY: number, activeProfileId = draggedProfileId): ProfileDragTarget | null {
    return profileDragTargetFromPointerPosition({
      clientY,
      activeProfileId,
      orderedProfiles,
      capturedRects: profileDragRectsRef.current,
      table: profileTableRef.current
    });
  }

  function startProfilePointerDrag(profileId: string, event: ReactPointerEvent<HTMLButtonElement>) {
    if (!isBatchManaging) return;
    beginProfilePointerDrag(profileId, event);
  }

  function startProfileRowPointerDrag(profileId: string, event: ReactPointerEvent<HTMLDivElement>) {
    if (!isBatchManaging) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("input, button, a, label")) return;
    beginProfilePointerDrag(profileId, event);
  }

  function beginProfilePointerDrag(profileId: string, event: ReactPointerEvent<HTMLElement>) {
    if (orderedProfiles.length < 2) return;
    event.preventDefault();
    event.stopPropagation();
    const row = event.currentTarget.closest<HTMLElement>("[data-profile-row='true']");
    if (!row) return;
    const rowRect = row.getBoundingClientRect();
    profileDragRectsRef.current = captureProfileDragRects(profileTableRef.current);
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setDraggedProfileId(profileId);
    setDragVisualState({
      profileId,
      pointerX: event.clientX,
      pointerY: event.clientY,
      offsetX: event.clientX - rowRect.left,
      offsetY: event.clientY - rowRect.top,
      width: rowRect.width,
      height: rowRect.height
    });
    setPreviewProfiles(orderedProfiles);
    const target = resolveProfileDragTarget(event.clientY, profileId);
    if (target) {
      setDragTargetProfileId(target.profileId);
      setDragPlacement(target.placement);
      setPreviewProfiles(reorderProfilesAfterDrag(orderedProfiles, profileId, target.profileId, target.placement) ?? orderedProfiles);
    }
  }

  function resetProfileDragState() {
    setDraggedProfileId("");
    setDragTargetProfileId("");
    setDragVisualState(null);
    setPreviewProfiles(null);
    profileDragRectsRef.current.clear();
  }

  function profileDragRowStyle(profileId: string): CSSProperties | undefined {
    const shift = calculateProfileDragRowShift({
      profileId,
      orderedProfiles,
      draggedProfileId,
      dragTargetProfileId,
      dragPlacement,
      dragVisualState,
      rowGap: PROFILE_DRAG_ROW_GAP
    });
    if (shift === 0) return undefined;
    return { transform: `translate3d(0, ${shift}px, 0)` };
  }

  return {
    visibleProfiles,
    draggedProfile,
    draggedProfileId,
    dragTargetProfileId,
    dragPlacement,
    dragVisualState,
    profileTableRef,
    profileDragRowStyle,
    resetProfileDragState,
    startProfilePointerDrag,
    startProfileRowPointerDrag
  };
}
