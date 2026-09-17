import { Bot, Database, FileCode2, GitCompareArrows, Plus, Search, Settings2 } from 'lucide-react';
import logo from '../assets/redrob-mark-512.png';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

export function ActivityRail() {
  const navigatorOpen = useWorkspace((state) => state.navigatorOpen);
  const aiOpen = useWorkspace((state) => state.aiOpen);
  const mutations = useWorkspace((state) => state.mutations.length);
  const activeTabId = useWorkspace((state) => state.activeTabId);
  const openConnectionModal = useWorkspace((state) => state.openConnectionModal);
  const focusNavigatorSearch = useWorkspace((state) => state.focusNavigatorSearch);
  const setUi = useWorkspace((state) => state.setUi);
  return (
    <nav className="activity-rail" aria-label="Workspace tools">
      <img className="brand-mark" src={logo} alt="Redrob Data" />
      <div className="rail-group">
        <IconButton label="Connections" active={navigatorOpen} aria-pressed={navigatorOpen} onClick={() => setUi({ navigatorOpen: !navigatorOpen })}><Database size={18} /></IconButton>
        <IconButton label="New connection" data-testid="open-connection-modal" onClick={() => openConnectionModal()}><Plus size={18} /></IconButton>
        <IconButton label="Schema search" onClick={focusNavigatorSearch}><Search size={18} /></IconButton>
        <IconButton label="Query workspace" active aria-current="page" disabled={!activeTabId} onClick={() => document.querySelector<HTMLElement>('[data-testid="query-editor"] textarea')?.focus()}><FileCode2 size={18} /></IconButton>
        <IconButton label="Staged changes" badge={mutations} onClick={() => setUi({ changesOpen: true })}><GitCompareArrows size={18} /></IconButton>
      </div>
      <div className="rail-spacer" />
      <div className="rail-group">
        <IconButton label="Redrob AI" active={aiOpen} data-testid="toggle-ai" onClick={() => setUi({ aiOpen: !aiOpen })}><Bot size={18} /></IconButton>
        <IconButton label="Redrob settings" data-testid="open-ai-settings-rail" onClick={() => setUi({ aiSettingsOpen: true })}><Settings2 size={18} /></IconButton>
      </div>
      <div className="user-avatar" data-tooltip="Local workspace">AS</div>
    </nav>
  );
}
