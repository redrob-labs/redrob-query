import { useEffect, useRef, useState } from 'react';
import { Loader, Menu } from '@redrob-labs/ui';
import { Icon } from '../ui/Icon';
import clsx from 'clsx';
import type { ConnectionProfile, DatabaseKind, MetadataNode } from '../domain/types';
import { useWorkspace } from '../store/WorkspaceProvider';
import { IconButton } from './IconButton';

const engineLabel: Record<DatabaseKind, string> = { postgresql: 'PostgreSQL', mysql: 'MySQL', sqlite: 'SQLite', mongodb: 'MongoDB', sqlserver: 'SQL Server' };
const engineGlyph: Record<DatabaseKind, string> = { postgresql: 'PG', mysql: 'MY', sqlite: 'SQ', mongodb: 'MO', sqlserver: 'MS' };
const NodeIcon = ({ node }: { node: MetadataNode }) => node.kind === 'database' ? <Icon name="database" /> : node.kind === 'table' ? <Icon name="grid" className="node-icon table" /> : node.kind === 'view' ? <Icon name="eye" className="node-icon view" /> : node.kind === 'column' ? <Icon name="columns" className="node-icon column" /> : <span className="schema-icon">S</span>;

// Beekeeper puts table actions in a right-click menu (TableListContextMenus.ts:50-180). A row menu
// button carries the same actions and is reachable by keyboard and touch, which a bare right-click is not.
const quoteIdentifier = (kind: DatabaseKind | undefined, name: string) =>
  kind === 'mysql' ? `\`${name.replace(/`/g, '``')}\`` : kind === 'sqlserver' ? `[${name.replace(/]/g, ']]')}]` : `"${name.replace(/"/g, '""')}"`;
export const selectStatement = (kind: DatabaseKind | undefined, table: string, schema?: string) =>
  `SELECT *\nFROM ${schema ? `${quoteIdentifier(kind, schema)}.` : ''}${quoteIdentifier(kind, table)}\n${kind === 'sqlserver' ? '' : 'LIMIT 100'}`.trimEnd();

// `schema` is threaded down rather than looked up: a node knows its parentId but not the parent's
// NAME, and the structure panel needs the name to qualify a table. The schema node passes its own
// name to its children, which is the only place that name is known for certain.
function TreeNode({ node, depth, filter, schema }: { node: MetadataNode; depth: number; filter: string; schema?: string }) {
  const expanded = useWorkspace((state) => state.expandedNodes.has(node.id));
  const children = useWorkspace((state) => state.metadata[node.id]);
  const toggleNode = useWorkspace((state) => state.toggleNode);
  const setUi = useWorkspace((state) => state.setUi);
  const hasChildren = Boolean(node.childCount);
  const kind = useWorkspace((state) => state.connections.find((item) => item.id === state.activeConnectionId)?.kind);
  const newTab = useWorkspace((state) => state.newTab);
  const updateQuery = useWorkspace((state) => state.updateQuery);
  const notify = useWorkspace((state) => state.notify);
  const isRelation = node.kind === 'table' || node.kind === 'view';
  const visibleChildren = children?.filter((child) => !filter || child.name.toLowerCase().includes(filter) || Boolean(child.childCount));
  if (filter && !hasChildren && !node.name.toLowerCase().includes(filter)) return null;
  return <><div className="tree-row"><button className={clsx('tree-node', node.kind === 'column' && 'is-column')} style={{ paddingLeft: 10 + depth * 14 }} onClick={() => { if (node.kind === 'table' || node.kind === 'view') { setUi({ structureTarget: { table: node.name, schema } }); } if (hasChildren) void toggleNode(node); }} aria-expanded={hasChildren ? expanded : undefined} data-testid={`metadata-${node.id}`}><span className="tree-chevron">{hasChildren ? (expanded ? <Icon name="chevronDown" /> : <Icon name="chevronRight" />) : <span />}</span><NodeIcon node={node} /><span className="tree-label">{node.name}</span>{node.dataType ? <span className="tree-type">{node.dataType}</span> : null}</button>{isRelation && kind !== 'mongodb' ? <Menu
      label={<><Icon name="more" /><span className="sr-only">Actions for {node.name}</span></>}
      variant="ghost" size="sm" align="right" className="tree-row-menu"
      items={[
        { id: 'select', label: 'Query this table', icon: <Icon name="play" />, onSelect: () => { newTab(); updateQuery(selectStatement(kind, node.name, schema)); } },
        { id: 'structure', label: 'View structure', icon: <Icon name="columns" />, onSelect: () => setUi({ structureTarget: { table: node.name, schema } }) },
        { type: 'separator' },
        { id: 'copy', label: 'Copy name', icon: <Icon name="copy" />, onSelect: () => { void navigator.clipboard.writeText(schema ? `${schema}.${node.name}` : node.name).then(() => notify('success', 'Copied', node.name), () => notify('error', 'Copy failed', 'The clipboard is not available here.')); } },
      ]} /> : null}</div>{expanded && !children ? <div className="tree-loading" style={{ paddingLeft: 32 + depth * 14 }}><Icon name="dot" /> Loading…</div> : null}{expanded && visibleChildren?.map((child) => <TreeNode key={child.id} node={child} depth={depth + 1} filter={filter} schema={node.kind === 'schema' ? node.name : schema} />)}</>;
}

export function Navigator() {
  const connections = useWorkspace((state) => state.connections);
  const activeId = useWorkspace((state) => state.activeConnectionId);
  const roots = useWorkspace((state) => state.metadata.root);
  const setActive = useWorkspace((state) => state.setActiveConnection);
  const openConnectionModal = useWorkspace((state) => state.openConnectionModal);
  const connectConnection = useWorkspace((state) => state.connectConnection);
  const disconnectConnection = useWorkspace((state) => state.disconnectConnection);
  const removeConnection = useWorkspace((state) => state.removeConnection);
  const loadNode = useWorkspace((state) => state.loadNode);
  const searchRequest = useWorkspace((state) => state.navigatorSearchRequest);
  const searchRef = useRef<HTMLInputElement>(null);
  const [filter, setFilter] = useState('');
  const [pickerOpen, setPickerOpen] = useState(false);
  const [actionsOpen, setActionsOpen] = useState(false);
  const [pendingRemoval, setPendingRemoval] = useState<ConnectionProfile | null>(null);
  const [removing, setRemoving] = useState(false);
  useEffect(() => { if (searchRequest > 0) { searchRef.current?.focus(); searchRef.current?.select(); } }, [searchRequest]);
  const active = connections.find((item) => item.id === activeId);
  const closeRemoval = () => { if (!removing) setPendingRemoval(null); };
  const confirmRemoval = async () => {
    if (!pendingRemoval || removing) return;
    const profileId = pendingRemoval.id;
    setRemoving(true);
    await removeConnection(profileId);
    setRemoving(false);
    setPendingRemoval(null);
  };
  return (
    <aside className="navigator" aria-label="Connection and schema navigator" data-testid="navigator">
      <div className="panel-heading"><div><span className="eyebrow">Workspace</span><h1>Data explorer</h1></div><IconButton label="Add connection" onClick={() => openConnectionModal()}><Icon name="plus" /></IconButton></div>
      <label className="navigator-search"><Icon name="search" /><span className="sr-only">Filter schemas</span><input ref={searchRef} value={filter} onChange={(event) => setFilter(event.target.value.toLowerCase())} placeholder="Filter schemas…" /></label>
      <div className="connection-picker">
        <button className="connection-current" aria-label="Choose connection" aria-expanded={pickerOpen} onClick={() => setPickerOpen((value) => !value)}><span className="database-glyph">{active ? engineGlyph[active.kind] : 'DB'}</span><span><strong>{active?.name ?? 'No connection'}</strong><small>{active ? <i className={`state-${active.state}`} /> : null} {active?.isDemo ? 'Demo ' : ''}{active ? `${engineLabel[active.kind]} · ${active.state}` : 'Select a profile'}</small></span><Icon name="chevronDown" /></button>
        {pickerOpen && connections.length > 0 ? <div className="connection-options">{connections.map((connection) => <button key={connection.id} onClick={() => { setPickerOpen(false); void setActive(connection.id); }}><strong>{connection.name}</strong><small>{connection.state}</small></button>)}</div> : null}
      </div>
      <div className="tree-toolbar"><span>Objects</span><div><IconButton label="Refresh schema" disabled={!active || active.state !== 'connected'} onClick={() => void loadNode(null, true)}><Icon name="refresh" /></IconButton><div className="profile-actions"><IconButton label="Connection actions" disabled={!active} onClick={() => setActionsOpen((value) => !value)}><Icon name="more" /></IconButton>{actionsOpen && active ? <div className="profile-actions-menu" role="menu"><button disabled={Boolean(active.builtIn) || active.state === 'connecting'} title={active.builtIn ? 'Built-in profiles cannot be edited' : active.state === 'connecting' ? 'Wait for the connection attempt to finish' : undefined} onClick={() => { setActionsOpen(false); openConnectionModal(active.id); }}><Icon name="edit" /> Edit</button>{active.state === 'connected' ? <button onClick={() => { setActionsOpen(false); void disconnectConnection(active.id); }}><Icon name="plug" /> Disconnect</button> : <button disabled={active.state === 'connecting'} onClick={() => { setActionsOpen(false); void connectConnection(active.id); }}><Icon name="link" /> {active.state === 'connecting' ? 'Connecting…' : 'Connect'}</button>}<button className="danger" disabled={Boolean(active.builtIn) || active.state === 'connecting'} title={active.builtIn ? 'Built-in profiles cannot be removed' : active.state === 'connecting' ? 'Wait for the connection attempt to finish' : undefined} onClick={() => { setActionsOpen(false); setPendingRemoval(active); }}><Icon name="trash" /> Remove</button></div> : null}</div></div></div>
      <div className="schema-tree" role="tree" aria-label="Database objects">{!active ? <div className="navigator-state">Select a connection to view its status.</div> : active.state !== 'connected' ? <div className="navigator-state">{active.state === 'connecting' ? <><Loader size="sm" label="Connecting" /> Connecting…</> : 'Connect this profile to load its schema.'}</div> : !roots ? <div className="navigator-state"><Loader size="sm" label="Loading the schema" /> Loading schema…</div> : roots.length === 0 ? <div className="navigator-state">No objects found</div> : roots.map((node) => <TreeNode key={node.id} node={node} depth={0} filter={filter} />)}</div>
      {active?.state === 'connected' && roots ? <div className="navigator-footer"><span className="live-dot" /> Metadata synced <span>just now</span></div> : <div className="navigator-footer">{active ? `${active.state[0].toUpperCase()}${active.state.slice(1)}` : 'Waiting for connection selection'}</div>}
      {pendingRemoval ? <div className="modal-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) closeRemoval(); }}><section className="remove-confirmation" role="dialog" aria-modal="true" aria-labelledby="remove-connection-title" aria-describedby="remove-connection-description" onKeyDown={(event) => {
        if (event.key === 'Escape') closeRemoval();
        if (event.key !== 'Tab') return;
        const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled), [tabindex="0"]'));
        const first = controls[0]; const last = controls.at(-1);
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
        if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }}><div className="remove-confirmation-icon"><Icon name="trash" size={24} /></div><h2 id="remove-connection-title">Remove “{pendingRemoval.name}”?</h2><p id="remove-connection-description">This permanently removes the saved profile, its stored credential, open queries, and local query history for this connection.</p><div><button className="ghost-button" autoFocus onClick={closeRemoval} disabled={removing}>Cancel</button><button className="danger-button" onClick={() => void confirmRemoval()} disabled={removing}>{removing ? <Loader size="sm" label="Removing the connection" /> : <Icon name="trash" />} {removing ? 'Removing…' : `Remove ${pendingRemoval.name}`}</button></div></section></div> : null}
    </aside>
  );
}
