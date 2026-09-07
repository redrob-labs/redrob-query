import { createContext, useContext, useRef, type PropsWithChildren } from 'react';
import { useStore } from 'zustand';
import type { DataBridge } from '../api/bridge';
import { createWorkspaceStore, type WorkspaceStore } from './workspaceStore';

const WorkspaceContext = createContext<WorkspaceStore | null>(null);

export function WorkspaceProvider({ bridge, children }: PropsWithChildren<{ bridge: DataBridge }>) {
  const storeRef = useRef<WorkspaceStore | null>(null);
  if (!storeRef.current) storeRef.current = createWorkspaceStore(bridge);
  return <WorkspaceContext.Provider value={storeRef.current}>{children}</WorkspaceContext.Provider>;
}

export function useWorkspace<T>(selector: (state: ReturnType<WorkspaceStore['getState']>) => T): T {
  const store = useContext(WorkspaceContext);
  if (!store) throw new Error('useWorkspace must be used inside WorkspaceProvider.');
  return useStore(store, selector);
}
