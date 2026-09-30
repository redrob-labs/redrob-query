import { useEffect, useMemo, useState } from 'react';
import type { IconName } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
import { useWorkspace } from '../store/WorkspaceProvider';

const commandList = [
  { id: 'run', label: 'Run current query', group: 'Query', hint: '⌘ ↵', icon: 'play' },
  { id: 'new-query', label: 'Create new query', group: 'Query', hint: '⌘ N', icon: 'filePlus' },
  { id: 'connect', label: 'Add a database connection', group: 'Workspace', hint: '', icon: 'database' },
  { id: 'ai', label: 'Ask Redrob AI', group: 'AI', hint: '⌘ I', icon: 'sparkle' },
  { id: 'changes', label: 'Review staged changes', group: 'Data', hint: '', icon: 'compare' },
] as const satisfies readonly { id: string; label: string; group: string; hint: string; icon: IconName }[];

export function CommandPalette() {
  const open = useWorkspace((state) => state.commandPaletteOpen);
  const setUi = useWorkspace((state) => state.setUi);
  const openConnectionModal = useWorkspace((state) => state.openConnectionModal);
  const runQuery = useWorkspace((state) => state.runQuery);
  const newTab = useWorkspace((state) => state.newTab);
  const [search, setSearch] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const commands = useMemo(() => commandList.filter((command) => command.label.toLowerCase().includes(search.toLowerCase())), [search]);
  useEffect(() => { if (open) { setSearch(''); setSelectedIndex(0); } }, [open]);
  useEffect(() => { setSelectedIndex(0); }, [search]);
  if (!open) return null;
  const execute = (id: typeof commandList[number]['id']) => {
    setUi({ commandPaletteOpen: false });
    if (id === 'run') void runQuery();
    if (id === 'new-query') newTab();
    if (id === 'connect') openConnectionModal();
    if (id === 'ai') setUi({ aiOpen: true });
    if (id === 'changes') setUi({ changesOpen: true });
  };
  return (
    <div className="palette-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setUi({ commandPaletteOpen: false }); }}>
      <section className="command-palette" role="dialog" aria-modal="true" aria-label="Command palette" data-testid="command-palette">
        <label className="palette-search"><Icon name="search" size={24} /><span className="sr-only">Search commands</span><input autoFocus value={search} onChange={(event) => setSearch(event.target.value)} onKeyDown={(event) => { if (event.key === 'Escape') setUi({ commandPaletteOpen: false }); if (event.key === 'ArrowDown') { event.preventDefault(); setSelectedIndex((index) => Math.min(commands.length - 1, index + 1)); } if (event.key === 'ArrowUp') { event.preventDefault(); setSelectedIndex((index) => Math.max(0, index - 1)); } if (event.key === 'Enter' && commands[selectedIndex]) execute(commands[selectedIndex].id); }} placeholder="Type a command or search…" /><kbd>ESC</kbd></label>
        <div className="palette-results">{commands.length ? commands.map((command, index) => <button key={command.id} className={index === selectedIndex ? 'is-selected' : ''} aria-selected={index === selectedIndex} onMouseEnter={() => setSelectedIndex(index)} onClick={() => execute(command.id)}><span><Icon name={command.icon} /></span><div><strong>{command.label}</strong><small>{command.group}</small></div>{command.hint ? <kbd>{command.hint}</kbd> : null}</button>) : <div className="palette-empty"><Icon name="sparkle" size={24} /> No commands found</div>}</div>
        <footer><span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span><span><kbd>↵</kbd> Select</span><span>Redrob Query</span></footer>
      </section>
    </div>
  );
}
