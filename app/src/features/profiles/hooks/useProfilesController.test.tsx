import { act, create } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useProfilesController } from "./useProfilesController";
import type { ImProfile } from "../model/types";
const mocks=vi.hoisted(()=>({create:vi.fn(),release:vi.fn(),open:vi.fn(),message:vi.fn(),save:vi.fn(),notice:vi.fn()}));
vi.mock("../api/profilesApi",()=>({createProfileDraft:mocks.create,releaseProfileDraft:mocks.release,upsertProfile:mocks.save,deleteProfile:vi.fn()}));
vi.mock("../../../api/profileReadApi",()=>({testProfileRead:vi.fn()}));
let controller:ReturnType<typeof useProfilesController>;
let view:ReturnType<typeof create>;
function Harness(){
  const params={orderedProfiles:[],selectedProfiles:[],onProfilesChange:async()=>{},openOfficialCliSetup:mocks.open,
    setDeletingProfileId:vi.fn(),setFormMessage:mocks.message,setPendingBulkDeleteIds:vi.fn(),setPendingDeleteId:vi.fn(),
    setProfileMessage:mocks.notice,setProfileMessageIsError:vi.fn(),setProfileNotice:vi.fn(),updateProfileReadNotice:vi.fn()};
  controller=useProfilesController(params);return null;
}
const profile=(id:string):ImProfile=>({id,platform:"wecom",label:id,enabled:true,configJson:{},status:"normal",sortOrder:0,createdAt:"fixture",updatedAt:"fixture"});
function pending(){let resolve!:(profile:ImProfile)=>void;let reject!:(error:Error)=>void;const promise=new Promise<ImProfile>((yes,no)=>{resolve=yes;reject=no});return{promise,resolve,reject};}
beforeEach(async()=>{vi.resetAllMocks();mocks.release.mockResolvedValue(undefined);await act(async()=>{view=create(<Harness/>);});});
afterEach(()=>{act(()=>view.unmount());});
it("releases a draft that arrives after page unmount",async()=>{
  const request=pending();mocks.create.mockReturnValue(request.promise);let task!:Promise<void>;
  act(()=>{task=controller.addProfile("wecom");});act(()=>view.unmount());
  await act(async()=>{request.resolve(profile("late"));await task;});
  expect(mocks.open).not.toHaveBeenCalled();expect(mocks.release).toHaveBeenCalledWith("late");
});
it("opens only the newest creation request and releases the superseded draft",async()=>{
  const first=pending();const second=pending();mocks.create.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  let a!:Promise<void>;let b!:Promise<void>;act(()=>{a=controller.addProfile("wecom");b=controller.addProfile("wecom");});
  await act(async()=>{second.resolve(profile("new"));await b;first.resolve(profile("old"));await a;});
  expect(mocks.open.mock.calls).toEqual([["wecom",profile("new")]]);expect(mocks.release.mock.calls).toEqual([["old"]]);
});
it("releases an unclaimed draft when opening its modal fails",async()=>{
  mocks.create.mockResolvedValue(profile("unclaimed"));mocks.open.mockImplementation(()=>{throw new Error("fixture open failure");});
  await act(async()=>{await controller.addProfile("wecom");});
  expect(mocks.release).toHaveBeenCalledWith("unclaimed");expect(mocks.message).toHaveBeenCalledWith(expect.stringContaining("fixture open failure"));
});
it("ignores a stale creation error after unmount",async()=>{
  const request=pending();mocks.create.mockReturnValue(request.promise);let task!:Promise<void>;
  act(()=>{task=controller.addProfile("wecom");});act(()=>view.unmount());mocks.message.mockClear();
  await act(async()=>{request.reject(new Error("late failure"));await task;});expect(mocks.message).not.toHaveBeenCalled();
});

it("preserves the read version when toggling account state",async()=>{
  const original=profile("saved");
  await act(async()=>{await controller.toggleProfile(original);});
  expect(mocks.save).toHaveBeenCalledWith({...original,enabled:false,status:"disabled"});
});
it("shows a save conflict without an unhandled rejection",async()=>{
  mocks.save.mockRejectedValue(new Error("账号配置已发生变化"));
  await act(async()=>{await controller.toggleProfile(profile("saved"));});
  expect(mocks.notice).toHaveBeenCalledWith(expect.stringContaining("账号配置已发生变化"));
});
