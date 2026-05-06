import { Suspense, lazy, useState } from "react";
import { AppShell, type AppView } from "./components/layout/AppShell";
import { useDashboardStore } from "./features/dashboard/hooks/useDashboardStore";
import { useProfilesStore } from "./features/profiles/hooks/useProfilesStore";
import { useSyncController } from "./features/sync/hooks/useSyncController";
import { useThemeController } from "./hooks/useThemeController";
import type { ActionItem } from "./features/dashboard/model/types";

const DashboardPage = lazy(() => import("./pages/Dashboard/DashboardPage").then((module) => ({ default: module.DashboardPage })));
const ProfilesPage = lazy(() => import("./pages/Profiles/ProfilesPage").then((module) => ({ default: module.ProfilesPage })));
const SettingsPage = lazy(() => import("./pages/Settings/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const AppSettingsPage = lazy(() => import("./pages/AppSettings/AppSettingsPage").then((module) => ({ default: module.AppSettingsPage })));

function PageLoadingFallback() {
  return (
    <div className="page-surface view-loading-page" role="status" aria-live="polite">
      <div className="empty-state">正在加载页面…</div>
    </div>
  );
}

export function App() {
  const [activeProfileId, setActiveProfileId] = useState("aggregate");
  const [activeView, setActiveView] = useState<AppView>("dashboard");
  const [drawerItem, setDrawerItem] = useState<ActionItem | null>(null);
  const { profiles, refreshProfiles } = useProfilesStore();
  const { themeChoice, effectiveThemeMode, handleThemeChange } = useThemeController();
  const { dashboard, dashboardProfileId, setDashboard } = useDashboardStore(activeProfileId, activeView === "dashboard");
  const {
    syncState,
    syncMessage,
    syncMessagePages,
    syncProgressNotices,
    syncFrequencyMinutes,
    isSyncCancelArmed,
    handleSyncButtonClick,
    runMaintenanceAction,
    setSyncFrequencyMinutes,
    dismissSyncMessage,
    closeSyncProgressNotice
  } = useSyncController({ activeProfileId, setDashboard });

  return (
    <AppShell
      profiles={profiles}
      activeProfileId={activeProfileId}
      activeView={activeView}
      onSelectProfile={(profileId) => {
        setActiveProfileId(profileId);
        setActiveView("dashboard");
      }}
      onSelectView={setActiveView}
      drawerItem={drawerItem}
      onCloseDrawer={() => setDrawerItem(null)}
      themeChoice={themeChoice}
      effectiveThemeMode={effectiveThemeMode}
      onThemeChange={handleThemeChange}
    >
      <Suspense fallback={<PageLoadingFallback />}>
        {activeView === "dashboard" && !dashboard && <PageLoadingFallback />}
        {activeView === "dashboard" && dashboard && dashboardProfileId === activeProfileId && (
          <DashboardPage
            key={activeProfileId}
            data={dashboard}
            profiles={profiles}
            activeProfileId={activeProfileId}
            syncState={syncState}
            syncMessage={syncMessage}
            syncMessagePages={syncMessagePages}
            syncProgressNotices={syncProgressNotices}
            syncFrequencyMinutes={syncFrequencyMinutes}
            onOpenSource={setDrawerItem}
            onDashboardChange={setDashboard}
            isSyncCancelArmed={isSyncCancelArmed}
            onSyncNow={handleSyncButtonClick}
            onFullResync={() => runMaintenanceAction("full-resync", activeProfileId)}
            onRetryAiAnalysis={() => runMaintenanceAction("retry-analysis", activeProfileId)}
            onSyncFrequencyChange={setSyncFrequencyMinutes}
            onConfigureAi={() => setActiveView("settings")}
            onDismissSyncMessage={dismissSyncMessage}
            onDismissSyncProgressNotice={closeSyncProgressNotice}
          />
        )}
        {activeView === "profiles" && <ProfilesPage profiles={profiles} onProfilesChange={refreshProfiles} />}
        {activeView === "settings" && <SettingsPage />}
        {activeView === "appSettings" && <AppSettingsPage />}
      </Suspense>
    </AppShell>
  );
}
