import { useEffect } from "react";

interface Options {
  highlightedProfileId: string;
  isAboutOpen: boolean;
  isBatchManaging: boolean;
  officialCliOpen: boolean;
  closeOfficialCliSetup: () => void;
  exitBatchManagement: () => void;
  resetModalCompositionState: () => void;
  setHighlightedProfileId: (value: string) => void;
  setIsAboutOpen: (value: boolean) => void;
}

export function useProfilesPageLifecycle(options: Options) {
  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      if (!options.isAboutOpen && !options.officialCliOpen && !options.isBatchManaging) return;
      event.preventDefault();
      if (options.isAboutOpen) return options.setIsAboutOpen(false);
      if (options.officialCliOpen) return options.closeOfficialCliSetup();
      options.exitBatchManagement();
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [
    options.isAboutOpen,
    options.isBatchManaging,
    options.officialCliOpen,
    options.closeOfficialCliSetup,
    options.exitBatchManagement,
    options.setIsAboutOpen
  ]);

  useEffect(() => {
    if (options.officialCliOpen) return;
    options.resetModalCompositionState();
  }, [options.officialCliOpen, options.resetModalCompositionState]);

  useEffect(() => {
    if (!options.highlightedProfileId) return;
    const timer = window.setTimeout(() => options.setHighlightedProfileId(""), 5200);
    const row = document.querySelector<HTMLElement>(
      `[data-profile-id="${CSS.escape(options.highlightedProfileId)}"]`
    );
    row?.scrollIntoView({ block: "center", behavior: "smooth" });
    return () => window.clearTimeout(timer);
  }, [options.highlightedProfileId, options.setHighlightedProfileId]);
}
