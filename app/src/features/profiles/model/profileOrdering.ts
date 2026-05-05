import type { ImProfile } from "./types";

export type ProfileDragPlacement = "before" | "after";

export interface ProfileDragTarget {
  profileId: string;
  placement: ProfileDragPlacement;
}

export interface ProfileDragRect {
  top: number;
  height: number;
}

export interface ProfileDragVisualState {
  profileId: string;
  pointerX: number;
  pointerY: number;
  offsetX: number;
  offsetY: number;
  width: number;
  height: number;
}

interface ProfileDragTargetOptions {
  clientY: number;
  activeProfileId: string;
  orderedProfiles: ImProfile[];
  capturedRects: Map<string, ProfileDragRect>;
  table: HTMLDivElement | null;
}

interface ProfileDragShiftOptions {
  profileId: string;
  orderedProfiles: ImProfile[];
  draggedProfileId: string;
  dragTargetProfileId: string;
  dragPlacement: ProfileDragPlacement;
  dragVisualState: ProfileDragVisualState | null;
  rowGap: number;
}

export function profilesAfterDrag(
  orderedProfiles: ImProfile[],
  sourceProfileId: string,
  targetProfileId: string,
  placement: ProfileDragPlacement
): ImProfile[] | null {
  if (!sourceProfileId || sourceProfileId === targetProfileId) return null;
  const nextProfiles = [...orderedProfiles];
  const sourceIndex = nextProfiles.findIndex((profile) => profile.id === sourceProfileId);
  if (sourceIndex < 0) return null;
  const [sourceProfile] = nextProfiles.splice(sourceIndex, 1);
  const targetIndex = nextProfiles.findIndex((profile) => profile.id === targetProfileId);
  if (targetIndex < 0) return null;
  nextProfiles.splice(placement === "after" ? targetIndex + 1 : targetIndex, 0, sourceProfile);
  return nextProfiles;
}

export function profilesHaveSameOrder(nextProfiles: ImProfile[], currentProfiles: ImProfile[]): boolean {
  if (nextProfiles.length !== currentProfiles.length) return false;
  return nextProfiles.every((profile, index) => profile.id === currentProfiles[index]?.id);
}

export function profileDragTargetFromPointer({
  clientY,
  activeProfileId,
  orderedProfiles,
  capturedRects,
  table
}: ProfileDragTargetOptions): ProfileDragTarget | null {
  if (!activeProfileId) return null;
  const capturedRows = orderedProfiles
    .map((profile) => ({ profileId: profile.id, rect: capturedRects.get(profile.id) }))
    .filter((row): row is { profileId: string; rect: ProfileDragRect } => Boolean(row.rect) && row.profileId !== activeProfileId);

  if (capturedRows.length > 0) return targetFromRects(clientY, capturedRows);
  if (!table) return null;

  // 拖拽开始时优先使用行高快照，只有快照缺失时才读取 DOM，避免实时布局变化导致插入位置抖动。
  const rows = Array.from(table.querySelectorAll<HTMLElement>("[data-profile-row='true']")).filter(
    (row) => row.dataset.profileId && row.dataset.profileId !== activeProfileId
  );
  if (rows.length === 0) return null;
  const liveRows = rows.map((row) => ({
    profileId: row.dataset.profileId ?? "",
    rect: row.getBoundingClientRect()
  }));
  return targetFromRects(clientY, liveRows);
}

export function captureProfileDragRects(table: HTMLDivElement | null): Map<string, ProfileDragRect> {
  const rects = new Map<string, ProfileDragRect>();
  if (!table) return rects;

  for (const row of Array.from(table.querySelectorAll<HTMLElement>("[data-profile-row='true']"))) {
    const profileId = row.dataset.profileId;
    if (!profileId) continue;
    const rect = row.getBoundingClientRect();
    rects.set(profileId, { top: rect.top, height: rect.height });
  }
  return rects;
}

export function profileDragRowShift({
  profileId,
  orderedProfiles,
  draggedProfileId,
  dragTargetProfileId,
  dragPlacement,
  dragVisualState,
  rowGap
}: ProfileDragShiftOptions): number {
  if (!draggedProfileId || !dragTargetProfileId || !dragVisualState || profileId === draggedProfileId) return 0;
  const sourceIndex = orderedProfiles.findIndex((profile) => profile.id === draggedProfileId);
  const targetIndex = orderedProfiles.findIndex((profile) => profile.id === dragTargetProfileId);
  const rowIndex = orderedProfiles.findIndex((profile) => profile.id === profileId);
  if (sourceIndex < 0 || targetIndex < 0 || rowIndex < 0) return 0;

  const insertionIndex = dragPlacement === "after" ? targetIndex + 1 : targetIndex;
  const shiftDistance = dragVisualState.height + rowGap;
  if (sourceIndex < insertionIndex && rowIndex > sourceIndex && rowIndex < insertionIndex) {
    return -shiftDistance;
  }
  if (sourceIndex > insertionIndex && rowIndex >= insertionIndex && rowIndex < sourceIndex) {
    return shiftDistance;
  }
  return 0;
}

function targetFromRects(
  clientY: number,
  rows: Array<{ profileId: string; rect: ProfileDragRect }>
): ProfileDragTarget | null {
  for (const row of rows) {
    if (clientY < row.rect.top + row.rect.height / 2) {
      return { profileId: row.profileId, placement: "before" };
    }
  }
  const lastRow = rows[rows.length - 1];
  return lastRow ? { profileId: lastRow.profileId, placement: "after" } : null;
}
