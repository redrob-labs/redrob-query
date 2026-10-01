import { Icon } from '../ui/Icon';
import { Mark } from '@redrob-labs/ui';
import symbolDark from '../assets/brand/redrob-symbol-solid-white.png';
import symbolLight from '../assets/brand/redrob-symbol.png';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

export function ActivityRail() {
  const navigatorOpen = useWorkspace((state) => state.navigatorOpen);
  const aiOpen = useWorkspace((state) => state.aiOpen);
  const mutations = useWorkspace((state) => state.mutations.length);
  const activeTabId = useWorkspace((state) => state.activeTabId);
  const pluginViewId = useWorkspace((state) => state.pluginViewId);
  const openConnectionModal = useWorkspace((state) => state.openConnectionModal);
  const focusNavigatorSearch = useWorkspace((state) => state.focusNavigatorSearch);
  const setUi = useWorkspace((state) => state.setUi);
  return (
    <nav className="activity-rail" aria-label="Workspace tools">
      <Mark src={symbolLight} darkSrc={symbolDark} height={24} alt="Redrob Query" className="brand-mark" />
      <div className="rail-group">
        <IconButton label="Connections" active={navigatorOpen} aria-pressed={navigatorOpen} onClick={() => setUi({ navigatorOpen: !navigatorOpen })}><Icon name="database" size={24} /></IconButton>
        <IconButton label="New connection" data-testid="open-connection-modal" onClick={() => openConnectionModal()}><Icon name="plus" size={24} /></IconButton>
        <IconButton label="Schema search" onClick={focusNavigatorSearch}><Icon name="search" size={24} /></IconButton>
        <IconButton label="Query workspace" active aria-current="page" disabled={!activeTabId} onClick={() => document.querySelector<HTMLElement>('[data-testid="query-editor"] textarea')?.focus()}><Icon name="fileCode" size={24} /></IconButton>
        <IconButton label="Staged changes" badge={mutations} onClick={() => setUi({ changesOpen: true })}><Icon name="compare" size={24} /></IconButton>
        <IconButton label={pluginViewId ? 'Close ER diagram' : 'ER diagram'} data-testid="toggle-plugin-view" active={Boolean(pluginViewId)} aria-pressed={Boolean(pluginViewId)} onClick={() => setUi({ pluginViewId: pluginViewId ? null : 'bks-er-diagram/main-view' })}><Icon name="plug" size={24} /></IconButton>
      </div>
      <div className="rail-spacer" />
      <div className="rail-group">
        <IconButton label="Redrob AI" active={aiOpen} data-testid="toggle-ai" onClick={() => setUi({ aiOpen: !aiOpen })}><Icon name="sparkle" size={24} /></IconButton>
        <IconButton label="Redrob settings" data-testid="open-ai-settings-rail" onClick={() => setUi({ aiSettingsOpen: true })}><Icon name="settings" size={24} /></IconButton>
      </div>
      <div className="user-avatar" data-tooltip="Local workspace">AS</div>
    </nav>
  );
}
