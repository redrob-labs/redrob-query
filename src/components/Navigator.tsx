import { useState } from 'react';
import { ChevronDown, ChevronRight, Circle, Columns3, Database, Eye, MoreHorizontal, Plus, RefreshCw, Search, Table2 } from 'lucide-react';
import clsx from 'clsx';
import type { DatabaseKind, MetadataNode } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const engineLabel: Record<DatabaseKind, string> = { postgresql: 'PostgreSQL', mysql: 'MySQL', sqlite: 'SQLite', mongodb: 'MongoDB', sqlserver: 'SQL Server' };
const engineGlyph: Record<DatabaseKind, string> = { postgresql: 'PG', mysql: 'MY', sqlite: 'SQ', mongodb: 'MO', sqlserver: 'MS' };
const NodeIcon = ({ node }: { node: MetadataNode }) => {
  if (node.kind === 'database') return <Database size={14} />;
  if (node.kind === 'table') return <Table2 size={14} className="node-icon table" />;
  if (node.kind === 'view') return <Eye size={14} className="node-icon view" />;
  if (node.kind === 'column') return <Columns3 size={13} className="node-icon column" />;
  return <span className="schema-icon">S</span>;
};

function TreeNode({ node, depth, filter }: { node: MetadataNode; depth: number; filter: string }) {
  const expanded = useWorkspace((state) => state.expandedNodes.has(node.id));
  const children = useWorkspace((state) => state.metadata[node.id]);
  const toggleNode = useWorkspace((state) => state.toggleNode);
  const hasChildren = Boolean(node.childCount);
  const visibleChildren = children?.filter((child) => !filter || child.name.toLowerCase().includes(filter) || Boolean(child.childCount));
  if (filter && !hasChildren && !node.name.toLowerCase().includes(filter)) return null;
  return (
    <>
      <button className={clsx('tree-node', node.kind === 'column' && 'is-column')} style={{ paddingLeft: 10 + depth * 14 }} onClick={() => hasChildren && void toggleNode(node)} aria-expanded={hasChildren ? expanded : undefined} data-testid={`metadata-${node.id}`}>
        <span className="tree-chevron">{hasChildren ? (expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} />) : <span />}</span>
        <NodeIcon node={node} /><span className="tree-label">{node.name}</span>{node.dataType ? <span className="tree-type">{node.dataType}</span> : null}
      </button>
      {expanded && !children ? <div className="tree-loading" style={{ paddingLeft: 32 + depth * 14 }}><Circle size={8} /> Loading…</div> : null}
      {expanded && visibleChildren?.map((child) => <TreeNode key={child.id} node={child} depth={depth + 1} filter={filter} />)}
    </>
  );
}

export function Navigator() {
  const connections = useWorkspace((state) => state.connections);
  const activeId = useWorkspace((state) => state.activeConnectionId);
  const roots = useWorkspace((state) => state.metadata.root);
  const setActive = useWorkspace((state) => state.setActiveConnection);
  const setUi = useWorkspace((state) => state.setUi);
  const loadNode = useWorkspace((state) => state.loadNode);
  const [filter, setFilter] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const active = connections.find((item) => item.id === activeId);
  return (
    <aside className="navigator" aria-label="Connection and schema navigator" data-testid="navigator">
      <div className="panel-heading"><div><span className="eyebrow">Workspace</span><h1>Data explorer</h1></div><IconButton label="Add connection" onClick={() => setUi({ connectionModalOpen: true })}><Plus size={16} /></IconButton></div>
      <label className="navigator-search"><Search size={14} /><span className="sr-only">Filter schemas</span><input value={filter} onChange={(event) => setFilter(event.target.value.toLowerCase())} placeholder="Filter schemas…" /></label>
      <div className="connection-picker">
        <button className="connection-current" aria-label="Choose connection" aria-expanded={pickerOpen} onClick={() => setPickerOpen((value) => !value)}>
          <span className="database-glyph">{active ? engineGlyph[active.kind] : 'DB'}</span>
          <span><strong>{active?.name ?? 'No connection'}</strong><small>{active ? <i /> : null} {active?.isDemo ? 'Demo ' : ''}{active ? engineLabel[active.kind] : 'Select a profile'}</small></span><ChevronDown size={14} />
        </button>
        {pickerOpen && connections.length > 0 ? <div className="connection-options">{connections.map((connection) => <button key={connection.id} onClick={() => { setPickerOpen(false); void setActive(connection.id); }}>{connection.name}</button>)}</div> : null}
      </div>
      <div className="tree-toolbar"><span>Objects</span><div><IconButton label="Refresh schema" disabled={!active} onClick={() => void loadNode(null, true)}><RefreshCw size={14} /></IconButton><IconButton label="Schema actions unavailable" disabled><MoreHorizontal size={15} /></IconButton></div></div>
      <div className="schema-tree" role="tree" aria-label="Database objects">
        {!active ? <div className="navigator-state">Select a connection to load its schema.</div> : !roots ? <div className="navigator-state"><span className="spinner" /> Loading schema…</div> : roots.length === 0 ? <div className="navigator-state">No objects found</div> : roots.map((node) => <TreeNode key={node.id} node={node} depth={0} filter={filter} />)}
      </div>
      {active && roots ? <div className="navigator-footer"><span className="live-dot" /> Metadata synced <span>just now</span></div> : <div className="navigator-footer">{active ? 'Loading metadata…' : 'Waiting for connection selection'}</div>}
    </aside>
  );
}
