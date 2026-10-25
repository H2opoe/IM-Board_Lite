import { afterEach, describe, expect, it, vi } from 'vitest';
import { waitForSyncIdle } from './syncApi';
const invoke=vi.hoisted(()=>vi.fn());
vi.mock('@tauri-apps/api/core',()=>({invoke}));
vi.mock('../../../api/tauri',()=>({isTauri:true,requireTauri:vi.fn()}));
vi.mock('../../../api/demoMode',()=>({isDemoMode:()=>false}));
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals();invoke.mockReset();});
describe('wait for backend exit',()=>{
 it('keeps polling while busy and resolves only when backend releases its guard',async()=>{
  vi.useFakeTimers();vi.stubGlobal('window',{setTimeout});
  invoke.mockResolvedValueOnce({busy:true}).mockResolvedValueOnce({busy:true}).mockResolvedValue({busy:false});
  let completed=false;const wait=waitForSyncIdle().then(()=>{completed=true;});
  await vi.advanceTimersByTimeAsync(100);expect(completed).toBe(false);
  await vi.advanceTimersByTimeAsync(100);await wait;expect(completed).toBe(true);
  expect(invoke).toHaveBeenCalledTimes(3);expect(invoke).toHaveBeenCalledWith('get_sync_job_state');
 });
 it('times out without claiming the backend stopped',async()=>{
  vi.useFakeTimers();vi.stubGlobal('window',{setTimeout});invoke.mockResolvedValue({busy:true});
  const assertion=expect(waitForSyncIdle(100)).rejects.toThrow('未启动新的维护任务');
  await vi.advanceTimersByTimeAsync(100);await assertion;
 });
});
