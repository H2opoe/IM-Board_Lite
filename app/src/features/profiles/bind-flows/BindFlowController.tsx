import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import type { PlatformCliDeploymentProgress } from "../../../api/bridgeApi";
import type { Platform } from "../model/types";
import type { OfficialCliBindState } from "./officialCli";
import { OfficialCliBindFlowModal } from "./officialCli";

interface SharedBindModalProps {
  updatingCliPlatform: Platform | null;
  cliDeploymentProgress: PlatformCliDeploymentProgress[];
  formMessage: ReactNode;
  onUpdateCli: (platform: Platform) => void;
  onOpenAbout: () => void;
  onCompositionStart: () => void;
  onCompositionEnd: () => void;
}

interface OfficialCliControllerProps {
  state: OfficialCliBindState;
  isCopied: boolean;
  onCopy: () => void;
  onRemarkChange: (value: string) => void;
  onClose: () => void;
  onSave: () => void;
  onKeyDown: (event: ReactKeyboardEvent<HTMLElement>) => void;
}

interface ProfileBindFlowControllerProps {
  shared: SharedBindModalProps;
  officialCli: OfficialCliControllerProps;
}

export function ProfileBindFlowController({
  shared,
  officialCli
}: ProfileBindFlowControllerProps) {
  return (
    <>
      <OfficialCliBindFlowModal
        state={officialCli.state}
        updatingCliPlatform={shared.updatingCliPlatform}
        cliDeploymentProgress={shared.cliDeploymentProgress}
        isCopied={officialCli.isCopied}
        formMessage={shared.formMessage}
        onUpdateCli={shared.onUpdateCli}
        onCopy={officialCli.onCopy}
        onRemarkChange={officialCli.onRemarkChange}
        onOpenAbout={shared.onOpenAbout}
        onClose={officialCli.onClose}
        onSave={officialCli.onSave}
        onKeyDown={officialCli.onKeyDown}
        onCompositionStart={shared.onCompositionStart}
        onCompositionEnd={shared.onCompositionEnd}
      />
    </>
  );
}
