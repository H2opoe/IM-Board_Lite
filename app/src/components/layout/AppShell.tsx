import { ChevronRight, KeyRound, LayoutDashboard, Link2, Monitor, Moon, Settings, SunMedium, X } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import type { ThemeChoice, ThemeMode } from "../../hooks/useThemeController";
import type { ActionItem } from "../../features/dashboard/model/types";
import type { ImProfile } from "../../features/profiles/model/types";
import { AppIcon } from "../shared/AppIcon";
import { PlatformIcon } from "../shared/PlatformIcon";
import { PROFILE_STATUS_LABELS, platformLabel } from "../../constants/platforms";
import { formatRelativeDateTime } from "../../utils/dates";
import { profileRemark } from "../../utils/profiles";

const DRAWER_CLOSE_ANIMATION_MS = 300;

export type AppView = "dashboard" | "profiles" | "settings" | "appSettings";

interface Props {
  profiles: ImProfile[];
  activeProfileId: string;
  activeView: AppView;
  drawerItem: ActionItem | null;
  onSelectProfile: (profileId: string) => void;
  onSelectView: (view: AppView) => void;
  onCloseDrawer: () => void;
  themeChoice: ThemeChoice;
  effectiveThemeMode: ThemeMode;
  onThemeChange: (theme: ThemeChoice) => void;
  children: React.ReactNode;
}

function profileSubtitle(profile: ImProfile) {
  const remark = profileRemark(profile);
  const label = platformLabel(profile.platform);
  if (remark) return remark;
  return profile.label !== label ? profile.label : "";
}

function startWindowDrag(event: React.MouseEvent<HTMLElement>) {
  if (event.button !== 0) return;
  void getCurrentWindow().startDragging().catch(() => {});
}

export function AppShell({
  profiles,
  activeProfileId,
  activeView,
  drawerItem,
  onSelectProfile,
  onSelectView,
  onCloseDrawer,
  themeChoice,
  effectiveThemeMode,
  onThemeChange,
  children
}: Props) {
  const isTauri = "__TAURI_INTERNALS__" in window;
  const [renderedDrawerItem, setRenderedDrawerItem] = useState<ActionItem | null>(drawerItem);
  const [isDrawerClosing, setIsDrawerClosing] = useState(false);
  const drawerCloseTimer = useRef<number | null>(null);
  const drawerStateClass = isDrawerClosing ? "is-closing" : "is-open";

  useEffect(() => {
    if (drawerCloseTimer.current !== null) {
      window.clearTimeout(drawerCloseTimer.current);
      drawerCloseTimer.current = null;
    }

    if (drawerItem) {
      setRenderedDrawerItem(drawerItem);
      setIsDrawerClosing(false);
      return;
    }

    if (renderedDrawerItem) {
      setIsDrawerClosing(true);
      drawerCloseTimer.current = window.setTimeout(() => {
        setRenderedDrawerItem(null);
        setIsDrawerClosing(false);
        drawerCloseTimer.current = null;
      }, DRAWER_CLOSE_ANIMATION_MS);
    }
  }, [drawerItem, renderedDrawerItem]);

  useEffect(() => {
    return () => {
      if (drawerCloseTimer.current !== null) {
        window.clearTimeout(drawerCloseTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!drawerItem) return undefined;

    function closeDrawerOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCloseDrawer();
    }

    window.addEventListener("keydown", closeDrawerOnEscape);
    return () => window.removeEventListener("keydown", closeDrawerOnEscape);
  }, [drawerItem, onCloseDrawer]);

  return (
    <div className={`app-shell theme-${effectiveThemeMode} theme-choice-${themeChoice}${isTauri ? " tauri-window" : ""}`}>
      {isTauri && <div className="window-drag-region" data-tauri-drag-region onMouseDown={startWindowDrag} />}
      <aside className="sidebar">
        <div className="brand" data-tauri-drag-region={isTauri ? "" : undefined} onMouseDown={isTauri ? startWindowDrag : undefined}>
          <AppIcon className="brand-mark" variant={effectiveThemeMode} />
          <div>
            <strong className="brand-title">
              IM-Board{" "}
              <small className="lite-edition-badge">Lite版</small>
            </strong>
            <span>聊天汇总看板</span>
          </div>
        </div>

        <nav className="nav-section">
          <button
            className={activeView === "dashboard" && activeProfileId === "aggregate" ? "nav-item active" : "nav-item"}
            onClick={() => onSelectProfile("aggregate")}
          >
            <LayoutDashboard size={18} />
            聚合看板
          </button>
        </nav>

        <div className="section-title">已绑定平台</div>
        <nav className="profile-list">
          {profiles.map((profile) => {
            const subtitle = profileSubtitle(profile);
            const statusLabel = PROFILE_STATUS_LABELS[profile.status] ?? profile.status;
            return (
              <button
                key={profile.id}
                className={activeView === "dashboard" && activeProfileId === profile.id ? "nav-item active" : "nav-item"}
                onClick={() => onSelectProfile(profile.id)}
                title={`${platformLabel(profile.platform)}${subtitle ? `·${subtitle}` : ""}·${statusLabel}`}
              >
                <span className="platform-nav-icon">
                  <PlatformIcon platform={profile.platform} className="platform-icon-sm" />
                  <span className={`status-dot ${profile.status}`} />
                </span>
                <span className="profile-label">
                  {platformLabel(profile.platform)}
                  {subtitle && <small>{subtitle}</small>}
                </span>
                <ChevronRight size={16} />
              </button>
            );
          })}
        </nav>

        <div className="sidebar-actions">
          <button className={activeView === "profiles" ? "nav-item active" : "nav-item"} onClick={() => onSelectView("profiles")}>
            <Link2 size={18} />
            平台管理
          </button>
          <button className={activeView === "settings" ? "nav-item active" : "nav-item"} onClick={() => onSelectView("settings")}>
            <KeyRound size={18} />
            AI配置
          </button>
          <button className={activeView === "appSettings" ? "nav-item active" : "nav-item"} onClick={() => onSelectView("appSettings")}>
            <Settings size={18} />
            设置
          </button>
          <div className="theme-switcher" aria-label="主题切换">
            <button
              className={themeChoice === "auto" ? "theme-segment active" : "theme-segment"}
              onClick={() => onThemeChange("auto")}
              aria-pressed={themeChoice === "auto"}
              title="跟随系统设置"
            >
              <Monitor size={15} />
              <span>自动</span>
            </button>
            <button
              className={themeChoice === "light" ? "theme-segment active" : "theme-segment"}
              onClick={() => onThemeChange("light")}
              aria-pressed={themeChoice === "light"}
              title="浅色模式"
            >
              <SunMedium size={15} />
              <span>浅色</span>
            </button>
            <button
              className={themeChoice === "dark" ? "theme-segment active" : "theme-segment"}
              onClick={() => onThemeChange("dark")}
              aria-pressed={themeChoice === "dark"}
              title="深色模式"
            >
              <Moon size={15} />
              <span>深色</span>
            </button>
          </div>
        </div>
      </aside>

      <main className="main-pane">{children}</main>

      {renderedDrawerItem && (
        <>
          <div className={`drawer-dismiss-layer ${drawerStateClass}`} onMouseDown={onCloseDrawer} aria-hidden="true" />
          <aside
            className={`detail-drawer ${drawerStateClass}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="source-detail-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="drawer-header">
              <div>
                <strong id="source-detail-title">来源详情</strong>
                <span>{renderedDrawerItem.chatName}</span>
              </div>
              <button className="icon-button" onClick={onCloseDrawer} aria-label="关闭详情">
                <X size={17} />
              </button>
            </div>
            <div className="drawer-body">
              <section className="drawer-section">
                <div>
                  <h2>{renderedDrawerItem.title}</h2>
                  <p>{renderedDrawerItem.description}</p>
                </div>
              </section>
              <section className="drawer-source-meta" aria-label="来源信息">
                <span>
                  <strong>时间点</strong>
                  {formatRelativeDateTime(renderedDrawerItem.sourceMessageAt)}
                </span>
                <span>
                  <strong>来源平台</strong>
                  {renderedDrawerItem.platformRemark
                    ? `${renderedDrawerItem.platformLabel}（${renderedDrawerItem.platformRemark}）`
                    : renderedDrawerItem.platformLabel}
                </span>
              </section>
              <section className="drawer-block">
                <strong>证据摘要</strong>
                <p>{renderedDrawerItem.evidenceSummary}</p>
              </section>
            </div>
          </aside>
        </>
      )}
    </div>
  );
}
