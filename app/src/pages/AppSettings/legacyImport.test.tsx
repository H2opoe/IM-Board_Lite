import { act, create } from 'react-test-renderer';
import { describe, expect, it, vi } from 'vitest';
import { AppSettingsPage } from './AppSettingsPage';
const api=vi.hoisted(()=>({importLegacyData:vi.fn(), getAppSettings:vi.fn(async()=>({cacheClearTime:'00:00'}))}));
vi.mock('../../api/appSettingsApi',()=>({...api,saveAppSettings:vi.fn(),exportDiagnosticPackage:vi.fn()}));
vi.mock('../../components/AboutModal',()=>({AboutModal:()=>null}));
vi.mock('../../components/DeveloperFeedbackModal',()=>({DeveloperFeedbackModal:()=>null}));
vi.mock('../../components/shared/FloatingNotice',()=>({FloatingNotice:({message}:{message:string})=><p>{message}</p>,FloatingNoticeStack:({children}:{children:React.ReactNode})=><div>{children}</div>}));
describe('legacy import entry',()=>{
 it('requires an explicit click and refreshes accounts only after successful import',async()=>{
  const dispatchEvent=vi.fn();vi.stubGlobal('window',{dispatchEvent});api.importLegacyData.mockResolvedValue({profiles:1,messages:2,actions:3,topics:0});
  let view!: ReturnType<typeof create>;
  try{
   await act(async()=>{view=create(<AppSettingsPage/>);});
   expect(api.importLegacyData).not.toHaveBeenCalled();
   const button=view.root.findAllByType('button').find(node=>node.children.includes('从旧版目录导入'))!;
   await act(async()=>{await button.props.onClick();});
   expect(api.importLegacyData).toHaveBeenCalledTimes(1);
   expect(dispatchEvent.mock.calls[0][0].type).toBe('im-board-profiles-changed');
   expect(view.root.findAllByType('p').some(node=>node.children.join('').includes('重新授权'))).toBe(true);
  }finally{if(view)act(()=>view.unmount());vi.unstubAllGlobals();api.importLegacyData.mockClear();}
 });
 it('reports failure without refreshing or claiming import succeeded',async()=>{
  const dispatchEvent=vi.fn();vi.stubGlobal('window',{dispatchEvent});api.importLegacyData.mockRejectedValue(new Error('测试导入失败'));
  let view!: ReturnType<typeof create>;
  try{
   await act(async()=>{view=create(<AppSettingsPage/>);});
   const button=view.root.findAllByType('button').find(node=>node.children.includes('从旧版目录导入'))!;
   await act(async()=>{await button.props.onClick();});
   expect(dispatchEvent).not.toHaveBeenCalled();
   expect(view.root.findAllByType('p').some(node=>node.children.join('').includes('测试导入失败'))).toBe(true);
  }finally{if(view)act(()=>view.unmount());vi.unstubAllGlobals();api.importLegacyData.mockClear();}
 });

 it('locks duplicate imports and refreshes the live account list after a completed import even if settings was closed',async()=>{
  const dispatchEvent=vi.fn();vi.stubGlobal('window',{dispatchEvent});
  let resolve!: (value:{profiles:number;messages:number;actions:number;topics:number})=>void;
  api.importLegacyData.mockReturnValue(new Promise(yes=>{resolve=yes;}));
  let view!: ReturnType<typeof create>;
  try{
   await act(async()=>{view=create(<AppSettingsPage/>);});
   const button=view.root.findAllByType('button').find(node=>node.children.includes('从旧版目录导入'))!;
   act(()=>{button.props.onClick();button.props.onClick();});
   expect(api.importLegacyData).toHaveBeenCalledTimes(1);
   expect(view.root.findByType('input').props.disabled).toBe(true);
   act(()=>view.unmount());
   await act(async()=>{resolve({profiles:1,messages:2,actions:3,topics:0});});
   expect(dispatchEvent).toHaveBeenCalledTimes(1);
   expect(dispatchEvent.mock.calls[0][0].type).toBe('im-board-profiles-changed');
  }finally{if(view)act(()=>view.unmount());vi.unstubAllGlobals();api.importLegacyData.mockClear();}
 });
});
