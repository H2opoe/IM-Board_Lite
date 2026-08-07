import { createPortal } from "react-dom";
import type { CSSProperties, PointerEvent as ReactPointerEvent, RefObject } from "react";
import { CheckCircle2, GripVertical, Info, MessageCircle, Pause, Search, Settings2, Trash2 } from "lucide-react";
import { PlatformIcon } from "../../../components/shared/PlatformIcon";
import { APP_MESSAGES, EMPTY_STATE_MESSAGES, PROFILE_MESSAGES } from "../../../constants/messages";
import { PROFILE_STATUS_LABELS, type PlatformBindingOption } from "../../../constants/platforms";
import type { ImProfile, Platform } from "../model/types";
import { profileDisplayName } from "../../../utils/profiles";
import { profileAccountSubtitle, profilePlatformLabel } from "../model/profilePathUtils";
import type { ProfileDragPlacement, ProfileDragVisualState } from "../model/profileOrdering";

interface PlatformEntryPanelProps {
  orderedProfiles: ImProfile[];
  platformOptions: PlatformBindingOption[];
  onAddProfile: (platform: Platform) => void;
  onOpenAbout: () => void;
  onOpenDeveloperContact: () => void;
}

interface ProfileBatchToolbarProps {
  isAllProfilesSelected: boolean;
  selectedProfileCount: number;
  isBulkDeleteConfirming: boolean;
  deletingProfileId: string;
  bulkDeleteId: string;
  bulkDeleteButtonLabel: string;
  batchStatusAction: "enable" | "pause";
  onToggleAll: () => void;
  onBatchStatusChange: () => void;
  onTestSelectedRead: () => void;
  onBulkDelete: () => void;
}

interface ProfileAccountListProps {
  orderedProfiles: ImProfile[];
  visibleProfiles: ImProfile[];
  selectedProfileIds: Set<string>;
  highlightedProfileId: string;
  isBatchManaging: boolean;
  draggedProfileId: string;
  dragTargetProfileId: string;
  dragPlacement: ProfileDragPlacement;
  draggedProfile: ImProfile | null;
  dragVisualState: ProfileDragVisualState | null;
  pendingDeleteId: string;
  deletingProfileId: string;
  tableRef: RefObject<HTMLDivElement>;
  profileDragRowStyle: (profileId: string) => CSSProperties | undefined;
  onStartProfilePointerDrag: (profileId: string, event: ReactPointerEvent<HTMLButtonElement>) => void;
  onStartProfileRowPointerDrag: (profileId: string, event: ReactPointerEvent<HTMLDivElement>) => void;
  onToggleProfileSelection: (profileId: string) => void;
  onToggleProfile: (profile: ImProfile) => void;
  onEditProfile: (profile: ImProfile) => void;
  onTestRead: (profile: ImProfile) => void;
  onRemoveProfile: (profile: ImProfile) => void;
  onCancelDelete: () => void;
  onConfirmRemoveProfile: (profile: ImProfile) => void;
}

export function ProfilePlatformEntryPanel({ orderedProfiles, platformOptions, onAddProfile, onOpenAbout, onOpenDeveloperContact }: PlatformEntryPanelProps) {
  return (
    <article className="panel profile-bind-panel">
      <header className="panel-header">
        <div>
          <strong>绑定平台</strong>
          <span>绑定新账号</span>
        </div>
        <button className="icon-button about-entry-button" onClick={onOpenAbout} aria-label="关于IM-Board">
          <Info size={17} />
        </button>
      </header>
      <div className="bind-grid">
        {platformOptions.map((platform) => {
          const boundCount = orderedProfiles.filter((profile) => profile.platform === platform.id).length;
          const isDisabled = Boolean(platform.disabled);
          return (
            <div className={`bind-card ${isDisabled ? "disabled" : ""}`.trim()} key={platform.id} aria-disabled={isDisabled}>
              <span className="bind-count-badge">已绑定{boundCount}个</span>
              <PlatformIcon platform={platform.id} className="platform-icon-lg" />
              <div className="bind-card-title-row">
                <strong>{platform.label}</strong>
                {isDisabled && platform.disabledReason ? <span className="bind-paid-only-label">{platform.disabledReason}</span> : null}
              </div>
              <div className="bind-card-footer">
                {isDisabled ? (
                  <button className="secondary-button bind-contact-button" onClick={onOpenDeveloperContact}>
                    <MessageCircle size={16} />
                    加入付费群组
                  </button>
                ) : (
                  <span className="bind-auth-label">{platform.auth}</span>
                )}
              </div>
              {!isDisabled && <button className="bind-card-hit-area" onClick={() => onAddProfile(platform.id)} aria-label={`绑定${platform.label}`} />}
            </div>
          );
        })}
      </div>
    </article>
  );
}

export function ProfileBatchToolbar({
  isAllProfilesSelected,
  selectedProfileCount,
  isBulkDeleteConfirming,
  deletingProfileId,
  bulkDeleteId,
  bulkDeleteButtonLabel,
  batchStatusAction,
  onToggleAll,
  onBatchStatusChange,
  onTestSelectedRead,
  onBulkDelete
}: ProfileBatchToolbarProps) {
  const isBatchEnableAction = batchStatusAction === "enable";
  return (
    <div className="profile-batch-toolbar">
      <label className="profile-batch-select-all">
        <input type="checkbox" checked={isAllProfilesSelected} onChange={onToggleAll} />
        <span>全选</span>
      </label>
      <span className="profile-batch-count">已选{selectedProfileCount}个</span>
      <button className="secondary-button" onClick={onBatchStatusChange} disabled={selectedProfileCount === 0}>
        {isBatchEnableAction ? <CheckCircle2 size={16} /> : <Pause size={16} />}
        {isBatchEnableAction ? "批量启用" : "批量暂停"}
      </button>
      <button className="secondary-button" onClick={onTestSelectedRead} disabled={selectedProfileCount === 0}>
        <Search size={16} />
        测试读取
      </button>
      <button
        className="danger-button"
        onClick={onBulkDelete}
        disabled={deletingProfileId === bulkDeleteId || (!isBulkDeleteConfirming && selectedProfileCount === 0)}
      >
        {!isBulkDeleteConfirming && <Trash2 size={16} />}
        {bulkDeleteButtonLabel}
      </button>
    </div>
  );
}

export function ProfileAccountList({
  orderedProfiles,
  visibleProfiles,
  selectedProfileIds,
  highlightedProfileId,
  isBatchManaging,
  draggedProfileId,
  dragTargetProfileId,
  dragPlacement,
  draggedProfile,
  dragVisualState,
  pendingDeleteId,
  deletingProfileId,
  tableRef,
  profileDragRowStyle,
  onStartProfilePointerDrag,
  onStartProfileRowPointerDrag,
  onToggleProfileSelection,
  onToggleProfile,
  onEditProfile,
  onTestRead,
  onRemoveProfile,
  onCancelDelete,
  onConfirmRemoveProfile
}: ProfileAccountListProps) {
  return (
    <div className={`profile-table ${draggedProfileId ? "dragging" : ""}`.trim()} ref={tableRef}>
      {orderedProfiles.length === 0 ? (
        <div className="empty-state">{EMPTY_STATE_MESSAGES.noProfiles}</div>
      ) : (
        visibleProfiles.map((profile) => {
          const rowClasses = [
            "profile-row",
            isBatchManaging ? "batch-mode" : "",
            selectedProfileIds.has(profile.id) ? "selected" : "",
            highlightedProfileId === profile.id ? "profile-row-updated" : "",
            draggedProfileId === profile.id ? "dragging" : "",
            dragTargetProfileId === profile.id ? `drag-over-${dragPlacement}` : ""
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <div
              className={rowClasses}
              key={profile.id}
              data-profile-row="true"
              data-profile-id={profile.id}
              style={profileDragRowStyle(profile.id)}
              onPointerDown={(event) => onStartProfileRowPointerDrag(profile.id, event)}
            >
              {isBatchManaging && (
                <label className="profile-select">
                  <input
                    type="checkbox"
                    checked={selectedProfileIds.has(profile.id)}
                    onChange={() => onToggleProfileSelection(profile.id)}
                    aria-label={`选择${profileDisplayName(profile)}`}
                  />
                </label>
              )}
              <div className="profile-platform">
                <PlatformIcon platform={profile.platform} className="platform-icon-md" />
                <div>
                  <strong>{profilePlatformLabel(profile)}</strong>
                  <span>{profileAccountSubtitle(profile)}</span>
                </div>
              </div>
              <span className={`profile-status ${profile.status}`}>{PROFILE_STATUS_LABELS[profile.status] ?? profile.status}</span>
              {isBatchManaging ? (
                <button
                  type="button"
                  className="profile-sort-handle"
                  title="按住拖拽排序"
                  aria-label={`拖拽排序${profileDisplayName(profile)}`}
                  onPointerDown={(event) => onStartProfilePointerDrag(profile.id, event)}
                >
                  <GripVertical size={17} />
                  <span>拖拽排序</span>
                </button>
              ) : (
                <div className="profile-actions">
                  <button
                    className="secondary-button"
                    onClick={() => onToggleProfile(profile)}
                    title={profile.enabled ? "暂停同步" : "启用同步"}
                  >
                    {profile.enabled ? <Pause size={16} /> : <CheckCircle2 size={16} />}
                    <span className="profile-action-label">{profile.enabled ? "暂停同步" : "启用同步"}</span>
                  </button>
                  <button className="icon-button" onClick={() => onEditProfile(profile)} aria-label="修改配置">
                    <Settings2 size={16} />
                  </button>
                  <button className="secondary-button" onClick={() => onTestRead(profile)} title="测试读取">
                    <Search size={16} />
                    <span className="profile-action-label">测试读取</span>
                  </button>
                  <button className="icon-button danger" onClick={() => onRemoveProfile(profile)} aria-label="删除账号配置">
                    <Trash2 size={16} />
                  </button>
                </div>
              )}
              {pendingDeleteId === profile.id && (
                <div className="profile-delete-confirm">
                  <span>{PROFILE_MESSAGES.deleteImpact}</span>
                  <button className="secondary-button" onClick={onCancelDelete} disabled={deletingProfileId === profile.id}>
                    {APP_MESSAGES.cancel}
                  </button>
                  <button className="danger-button" onClick={() => onConfirmRemoveProfile(profile)} disabled={deletingProfileId === profile.id}>
                    {deletingProfileId === profile.id ? APP_MESSAGES.deleteInProgress : APP_MESSAGES.confirmDelete}
                  </button>
                </div>
              )}
            </div>
          );
        })
      )}
      <ProfileDragFloating
        draggedProfile={draggedProfile}
        dragVisualState={dragVisualState}
        isSelected={Boolean(draggedProfile && selectedProfileIds.has(draggedProfile.id))}
      />
    </div>
  );
}

function ProfileDragFloating({
  draggedProfile,
  dragVisualState,
  isSelected
}: {
  draggedProfile: ImProfile | null;
  dragVisualState: ProfileDragVisualState | null;
  isSelected: boolean;
}) {
  if (!dragVisualState || !draggedProfile) return null;
  const themeMode = document.documentElement.dataset.theme === "light" ? "light" : "dark";
  const floatingStyle: CSSProperties = {
    width: dragVisualState.width,
    minHeight: dragVisualState.height,
    transform: `translate3d(${dragVisualState.pointerX - dragVisualState.offsetX}px, ${
      dragVisualState.pointerY - dragVisualState.offsetY
    }px, 0) scale(1.025)`
  };

  return createPortal(
    <div className={`profile-drag-floating-layer profile-drag-layer-${themeMode}`}>
      <div className={`profile-row batch-mode profile-drag-floating ${isSelected ? "selected" : ""}`} style={floatingStyle} aria-hidden="true">
        <span className="profile-select profile-select-preview">
          <span className={`profile-checkbox-preview ${isSelected ? "checked" : ""}`} />
        </span>
        <div className="profile-platform">
          <PlatformIcon platform={draggedProfile.platform} className="platform-icon-md" />
          <div>
            <strong>{profilePlatformLabel(draggedProfile)}</strong>
            <span>{profileAccountSubtitle(draggedProfile)}</span>
          </div>
        </div>
        <span className={`profile-status ${draggedProfile.status}`}>{PROFILE_STATUS_LABELS[draggedProfile.status] ?? draggedProfile.status}</span>
        <div className="profile-sort-handle">
          <GripVertical size={17} />
          <span>拖拽排序</span>
        </div>
      </div>
    </div>,
    document.body
  );
}
