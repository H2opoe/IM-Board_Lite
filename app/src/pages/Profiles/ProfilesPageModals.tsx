import type { ComponentProps } from "react";
import { AboutModal } from "../../components/AboutModal";
import { ProfileBindFlowController } from "../../features/profiles/bind-flows/BindFlowController";

interface ProfilesPageModalsProps {
  bindFlow: ComponentProps<typeof ProfileBindFlowController>;
  isAboutOpen: boolean;
  onCloseAbout: () => void;
}

export function ProfilesPageModals({ bindFlow, isAboutOpen, onCloseAbout }: ProfilesPageModalsProps) {
  return (
    <>
      <ProfileBindFlowController {...bindFlow} />
      {isAboutOpen && <AboutModal onClose={onCloseAbout} />}
    </>
  );
}
