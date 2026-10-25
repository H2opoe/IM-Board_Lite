import { act, create } from 'react-test-renderer';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useSyncController } from './useSyncController';
import type { ImProfile } from '../../profiles/model/types';
import type { SyncJobSnapshot } from '../model/types';
const mocks=vi.hoisted(()=>({get:vi.fn(),watch:vi.fn(),run:vi.fn(),cancel:vi.fn(),wait:vi.fn(),dashboard:vi.fn()}));
vi.mock('../../../api/tauri',()=>({isTauri:true}));
vi.mock('../api/syncApi',()=>({getSyncJobState:mocks.get,watchSyncJobState:mocks.watch,runSyncJob:mocks.run,cancelSync:mocks.cancel,waitForSyncIdle:mocks.wait}));
vi.mock('../../dashboard/api/dashboardApi',()=>({getDashboard:mocks.dashboard}));
vi.mock('./useSyncFrequency',()=>({useSyncFrequency:()=>({syncFrequencyMinutes:15,setSyncFrequencyMinutes:vi.fn()})}));
const empty:SyncJobSnapshot={revision:0,jobId:'',profileId:'aggregate',mode:'incremental',running:false,busy:false,progress:null,result:null,error:null};
const result={profileId:'aggregate',syncStatus:'synced',aiStatus:'ready' as const,insertedMessages:2,analyzedMessages:1,warnings:[],startedAt:'start',finishedAt:'end'};
let current:SyncJobSnapshot;
let listener:(snapshot:SyncJobSnapshot)=>void;
let controller:ReturnType<typeof useSyncController>;
let view:ReturnType<typeof create>;
const setter=vi.fn();const stop=vi.fn();
const account: ImProfile = {id:'one',platform:'feishu',label:'fixture',enabled:true,configJson:{},status:'normal',sortOrder:0,createdAt:'',updatedAt:''};
function Harness({profile='aggregate',profiles=[account]}:{profile?:string;profiles?:ImProfile[]}) {controller=useSyncController({activeProfileId:profile,demoMode:false,profiles,setDashboard:setter});return null;}
function deferred<T>(){let resolve!:(value:T)=>void;const promise=new Promise<T>(r=>{resolve=r;});return {promise,resolve};}
async function emit(patch:Partial<SyncJobSnapshot>){await act(async()=>{current={...current,...patch};listener(current);});}
beforeEach(()=>{
 vi.clearAllMocks();current={...empty};
 const windowTarget=Object.assign(new EventTarget(),{setTimeout,clearTimeout,setInterval,clearInterval});
 vi.stubGlobal('window',windowTarget);vi.stubGlobal('document',Object.assign(new EventTarget(),{visibilityState:'visible'}));
 mocks.get.mockImplementation(async()=>({...current}));mocks.watch.mockImplementation(async(fn)=>{listener=fn;return stop;});
 mocks.dashboard.mockResolvedValue({syncStatus:'synced',aiStatus:'ready'});mocks.cancel.mockResolvedValue(true);mocks.wait.mockResolvedValue(undefined);
});
afterEach(()=>{if(view)act(()=>view.unmount());vi.unstubAllGlobals();});
describe('persistent backend sync observer',()=>{
 it('refreshes background completion once and ignores old or duplicate snapshots',async()=>{
  await act(async()=>{view=create(<Harness/>);});expect(mocks.run).not.toHaveBeenCalled();
  await emit({revision:1,jobId:'auto-1',running:true,busy:true});expect(controller.syncState).toBe('syncing');
  await emit({revision:3,running:false,busy:false,result});expect(controller.syncState).toBe('done');expect(mocks.dashboard).toHaveBeenCalledTimes(1);
  await act(async()=>{listener({...current});listener({...current,revision:2,running:true,busy:true,result:null});});
  expect(controller.syncState).toBe('done');expect(mocks.dashboard).toHaveBeenCalledTimes(1);
 });
 it('keeps the listener after a manual job and processes the next background job',async()=>{
  mocks.run.mockImplementation(async()=>{current={...empty,revision:2,jobId:'manual',result};listener(current);return result;});
  await act(async()=>{view=create(<Harness/>);});await act(async()=>{controller.handleSyncButtonClick();});
  expect(mocks.run).toHaveBeenCalledTimes(1);expect(mocks.watch).toHaveBeenCalledTimes(1);expect(stop).not.toHaveBeenCalled();expect(mocks.dashboard).toHaveBeenCalledTimes(1);
  await emit({revision:3,jobId:'auto',result:null,running:true,busy:true});
  await emit({revision:4,result,running:false,busy:false});expect(mocks.dashboard).toHaveBeenCalledTimes(2);expect(controller.syncState).toBe('done');
 });
 it('reconciles missed completion on focus',async()=>{
  await act(async()=>{view=create(<Harness/>);});
  current={...empty,revision:8,jobId:'missed',result};
  await act(async()=>{window.dispatchEvent(new Event('focus'));});
  expect(controller.syncState).toBe('done');expect(mocks.dashboard).toHaveBeenCalledTimes(1);
 });
 it('waits for backend termination before maintenance and does not require a local manual promise',async()=>{
  current={...empty,revision:1,jobId:'background',running:true,busy:true};
  const idle=deferred<void>();mocks.wait.mockReturnValue(idle.promise);
  mocks.run.mockImplementation(async()=>{current={...empty,revision:3,jobId:'maintenance',result};listener(current);return result;});
  await act(async()=>{view=create(<Harness/>);});
  let maintenance!:Promise<void>;
  await act(async()=>{maintenance=controller.runMaintenanceAction('full-resync');});
  expect(mocks.cancel).toHaveBeenCalledTimes(1);expect(mocks.run).not.toHaveBeenCalled();
  await act(async()=>{current={...empty,revision:2,jobId:'background',error:'同步已终止。'};idle.resolve();await maintenance;});
  expect(mocks.run).toHaveBeenCalledWith('aggregate','full_resync');
 });
 it('coalesces progress reads and rejects results for a previously selected account',async()=>{
  const first=deferred<any>();mocks.dashboard.mockReturnValueOnce(first.promise).mockResolvedValue({syncStatus:'synced',aiStatus:'ready'});
  await act(async()=>{view=create(<Harness profile="one"/>);});
  const progress={profileId:'one',profileLabel:'one',phase:'fetch_messages_done',message:'fixture',current:1,total:2,shouldRefreshDashboard:true};
  await emit({revision:1,jobId:'job',running:true,busy:true,progress});
  await emit({revision:2,progress});await emit({revision:3,progress});expect(mocks.dashboard).toHaveBeenCalledTimes(1);
  await act(async()=>{view.update(<Harness profile="two"/>);first.resolve({});});
  expect(mocks.dashboard).toHaveBeenCalledTimes(2);expect(setter.mock.calls.every(call=>call[1]==='two')).toBe(true);
 });
});

describe('observer reconciliation races',()=>{
 it('does not let an earlier status query replace a newer event',async()=>{
  const query=deferred<SyncJobSnapshot>();mocks.get.mockReturnValueOnce(query.promise);
  await act(async()=>{view=create(<Harness/>);});
  await emit({revision:9,jobId:'newest',running:false,busy:false,result});
  await act(async()=>{query.resolve({...empty,revision:1,jobId:'old',running:true,busy:true});});
  expect(controller.syncState).toBe('done');expect(mocks.dashboard).toHaveBeenCalledTimes(1);
 });
 it('continues status reconciliation if event subscription fails',async()=>{
  const warn=vi.spyOn(console,'warn').mockImplementation(()=>{});
  mocks.watch.mockRejectedValueOnce(new Error('fixture subscription failure'));
  current={...empty,revision:2,jobId:'completed',result};
  try{await act(async()=>{view=create(<Harness/>);});expect(controller.syncState).toBe('done');expect(mocks.dashboard).toHaveBeenCalledTimes(1);}finally{warn.mockRestore();}
 });
 it('releases a listener that finishes subscribing after unmount',async()=>{
  const registration=deferred<()=>void>();mocks.watch.mockReturnValueOnce(registration.promise);
  await act(async()=>{view=create(<Harness/>);});act(()=>view.unmount());
  await act(async()=>{registration.resolve(stop);});expect(stop).toHaveBeenCalledTimes(1);expect(mocks.get).not.toHaveBeenCalled();
 });
});

describe('first launch sync guidance',()=>{
 it('keeps an empty startup idle and guides manual sync without starting a job',async()=>{
  await act(async()=>{view=create(<Harness profiles={[]}/>);});
  expect(controller.syncState).toBe('idle');expect(controller.syncMessage).toBe('');
  await act(async()=>{controller.handleSyncButtonClick();});
  expect(mocks.run).not.toHaveBeenCalled();expect(controller.syncState).toBe('idle');
  expect(controller.syncMessage).toContain('尚未绑定账号');
  await act(async()=>{await controller.runMaintenanceAction('full-resync');});
  expect(mocks.run).not.toHaveBeenCalled();
 });
 it('guides disabled accounts and permits syncing after an account is enabled',async()=>{
  await act(async()=>{view=create(<Harness profiles={[{...account,enabled:false}]}/>);});
  await act(async()=>{controller.handleSyncButtonClick();});
  expect(mocks.run).not.toHaveBeenCalled();expect(controller.syncMessage).toContain('尚未启用任何账号');
  await act(async()=>{view.update(<Harness/>);});
  await act(async()=>{controller.handleSyncButtonClick();});
  expect(mocks.run).toHaveBeenCalledWith('aggregate','incremental');
 });
 it('does not turn an idle backend result into a success or error notice',async()=>{
  await act(async()=>{view=create(<Harness profiles={[]}/>);});
  await emit({revision:1,jobId:'empty',result:{...result,syncStatus:'idle',insertedMessages:0,analyzedMessages:0}});
  expect(controller.syncState).toBe('idle');expect(controller.syncMessage).toBe('');
 });
 it('keeps actual all-account failures visible',async()=>{
  await act(async()=>{view=create(<Harness/>);});
  await emit({revision:1,jobId:'failed',result:{...result,syncStatus:'failed',warnings:['飞书：未授权，请重新绑定']}});
  expect(controller.syncState).toBe('failed');expect(controller.syncProgressNotices[0].variant).toBe('error');
 });
});
