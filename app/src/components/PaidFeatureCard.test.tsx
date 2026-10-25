import { act, create } from "react-test-renderer";
import { expect, it, vi } from "vitest";
import { ProfilePlatformEntryPanel } from "../features/profiles/bind-flows/profileList";
import { PLATFORM_BINDING_OPTIONS } from "../constants/platforms";

it("keeps the paid contact entry alongside the three actual connectors", () => {
  const add = vi.fn(), contact = vi.fn();
  let view!: ReturnType<typeof create>;
  act(() => { view = create(<ProfilePlatformEntryPanel orderedProfiles={[]} platformOptions={PLATFORM_BINDING_OPTIONS}
    onAddProfile={add} onOpenAbout={vi.fn()} onOpenDeveloperContact={contact} />); });
  const buttons = view.root.findAllByType("button");
  const bindButtons = buttons.filter((button) => button.props.className === "bind-card-hit-area");
  expect(bindButtons).toHaveLength(3);
  const contactButton = buttons.find((button) => button.props.className?.includes("bind-contact-button"))!;
  act(() => contactButton.props.onClick());
  expect(contact).toHaveBeenCalledTimes(1);
  expect(add).not.toHaveBeenCalled();
  act(() => bindButtons[0].props.onClick());
  expect(add).toHaveBeenCalledWith("wecom");
  act(() => view.unmount());
});
