import { useState } from 'react'
import { useStore, splitKey } from '@/store'
import { Button } from '@/components/ui/button'
import { parseIntent, type IntentResult } from '@/lib/intent'

/**
 * Workspace-intent command bar.
 *
 * NOT a chat input. Conversation with Claude Code happens in the embedded
 * terminal panes — typing here only changes workspace state (which terminals
 * are open, where they sit, view mode).
 *
 * Verbs (v1):
 *   pull up <project>[ and <project>...]   → open one terminal per match
 *   open <project>/<branch>                 → open a specific worktree
 *   close <project|branch>                  → close that pane
 *   back to grid                            → close all panes, return to grid
 *   show me what needs me                   → open every needs_you worktree
 *   tree <project>                          → open project tree view
 *   new worktree <branch>[ in <project>]    → create + open new worktree
 */
export function CommandBar() {
  const [value, setValue] = useState('')
  const [hint, setHint] = useState<string | null>(null)
  const [showSetup, setShowSetup] = useState(false)
  const [setupStep, setSetupStep] = useState<'project' | 'worktree' | 'daemon' | null>(null)
  const [setupData, setSetupData] = useState({
    projectPath: '', projectName: '', branch: '',
    daemonUrl: 'wss://localhost:9111/ws', daemonName: '',
    machineId: '', permissionMode: 'plan',
  })

  const send = useStore(s => s.send)
  const projects = useStore(s => s.projects)
  const worktrees = useStore(s => s.worktrees)
  const daemons = useStore(s => s.daemons)
  const layoutMode = useStore(s => s.layoutMode)
  const daemonError = useStore(s => s.daemonError)
  const clearDaemonError = useStore(s => s.clearDaemonError)
  const openPane = useStore(s => s.openPane)
  const closePane = useStore(s => s.closePane)
  const switchToGrid = useStore(s => s.switchToGrid)
  const openProjectTree = useStore(s => s.openProjectTree)
  const addDaemon = useStore(s => s.addDaemon)
  const pendingCreate = useStore(s => s.pendingCreate)
  const clearPendingCreate = useStore(s => s.clearPendingCreate)
  const openDaemonDetail = useStore(s => s.openDaemonDetail)

  /** ID of the daemon to use for workspace mutations.
   *  Prefers the first connected daemon; falls back to first registered. */
  function targetMachineId(): string {
    const connected = Object.values(daemons).find(d => d.connected)
    if (connected) return connected.id
    return Object.keys(daemons)[0] ?? 'localhost'
  }

  function handleSubmit() {
    const text = value.trim()
    if (!text) return

    const intent = parseIntent(text, { projects, worktrees, daemons })

    const result = dispatchIntent(intent)
    if (result.ok) {
      setValue('')
      setHint(null)
    } else {
      setHint(result.error ?? 'unknown intent — try "pull up <project>" or "back to grid"')
    }
  }

  function dispatchIntent(intent: IntentResult): { ok: boolean; error?: string } {
    switch (intent.kind) {
      case 'unknown':
        return { ok: false, error: intent.reason }

      case 'back_to_grid':
        switchToGrid()
        return { ok: true }

      case 'pull_up': {
        if (intent.worktreeIds.length === 0) return { ok: false, error: 'no matching worktrees' }
        for (const id of intent.worktreeIds) openPane(id)
        return { ok: true }
      }

      case 'close': {
        if (intent.worktreeIds.length === 0) return { ok: false, error: 'no matching worktrees to close' }
        for (const id of intent.worktreeIds) closePane(id)
        return { ok: true }
      }

      case 'show_needs_me': {
        const ids = Object.values(worktrees).filter(w => w.status === 'needs_you').map(w => w.id)
        if (ids.length === 0) return { ok: false, error: 'nothing needs you right now' }
        for (const id of ids) openPane(id)
        return { ok: true }
      }

      case 'tree':
        openProjectTree(intent.projectId)
        return { ok: true }

      case 'new_worktree': {
        // intent.projectId is the namespaced key — split it
        const [mid, rawProjId] = splitKey(intent.projectId)
        send(mid || targetMachineId(), {
          type: 'create_worktree',
          project_id: rawProjId || intent.projectId,
          branch: intent.branch,
          permission_mode: setupData.permissionMode,
        })
        return { ok: true }
      }

      case 'inspect_daemon':
        openDaemonDetail(intent.machineId)
        return { ok: true }
    }
  }

  function handleRegisterProject() {
    const { projectPath, projectName, machineId } = setupData
    if (!projectPath.trim()) return
    send(machineId || targetMachineId(), {
      type: 'register_project',
      path: projectPath.trim(),
      name: (projectName.trim() || projectPath.split('/').pop()) ?? 'project',
    })
    setSetupStep('worktree')
  }

  function handleCreateWorktree() {
    const { branch, permissionMode } = setupData
    const lastProject = Object.values(projects).at(-1)
    if (!lastProject || !branch.trim()) return
    const [mid, rawProjId] = splitKey(lastProject.id)
    send(mid || targetMachineId(), {
      type: 'create_worktree',
      project_id: rawProjId || lastProject.id,
      branch: branch.trim(),
      permission_mode: permissionMode,
    })
    setShowSetup(false)
    setSetupStep(null)
    setSetupData({ projectPath: '', projectName: '', branch: '', daemonUrl: 'wss://localhost:9111/ws', daemonName: '', machineId: '', permissionMode: 'plan' })
  }

  function handleAddDaemon() {
    const { daemonUrl, daemonName } = setupData
    if (!daemonUrl.trim()) return
    // Use URL as temporary id; real machine_id will be learned on first message
    const tempId = daemonUrl.trim()
    addDaemon({
      id: tempId,
      name: daemonName.trim() || tempId,
      url: daemonUrl.trim(),
    })
    setShowSetup(false)
    setSetupStep(null)
    setSetupData({ projectPath: '', projectName: '', branch: '', daemonUrl: 'wss://localhost:9111/ws', daemonName: '', machineId: '' })
  }

  const placeholder =
    layoutMode === 'grid'
      ? 'workspace intent — "pull up kinobi", "show me what needs me"…'
      : '"close kinobi", "back to grid", "pull up rangoli"…'

  const connectedCount = Object.values(daemons).filter(d => d.connected).length

  return (
    <div data-testid="command-bar" className="border-t border-border bg-background shrink-0">
      {/* Main input row */}
      <div className="flex items-center gap-2 px-3 py-2">
        <span className="text-xs font-mono text-muted-foreground shrink-0">▸</span>
        <input
          data-testid="command-input"
          className="flex-1 bg-transparent text-sm font-normal outline-none placeholder:text-muted-foreground"
          placeholder={placeholder}
          value={value}
          onChange={e => { setValue(e.target.value); setHint(null) }}
          onKeyDown={e => { if (e.key === 'Enter') handleSubmit() }}
        />
        {/* Connection status indicator */}
        <span className="text-xs font-mono text-muted-foreground shrink-0 flex items-center gap-1">
          <span
            className="inline-block w-1.5 h-1.5"
            style={{ backgroundColor: connectedCount > 0 ? '#22c55e' : '#ef4444' }}
          />
          {connectedCount}/{Object.keys(daemons).length} {Object.keys(daemons).length === 1 ? 'daemon' : 'daemons'}
        </span>
        {value.trim() && (
          <Button
            data-testid="command-send"
            size="sm"
            className="rounded-none shadow-none font-normal shrink-0"
            onClick={handleSubmit}
          >
            Run
          </Button>
        )}
        <Button
          data-testid="add-project-btn"
          variant="outline"
          size="sm"
          className="rounded-none shadow-none font-normal shrink-0"
          onClick={() => {
            const firstConnected = Object.values(daemons).find(d => d.connected)
            setSetupData(d => ({ ...d, machineId: firstConnected?.id ?? targetMachineId() }))
            setShowSetup(v => !v)
            setSetupStep('project')
          }}
        >
          + project
        </Button>
        <Button
          data-testid="add-daemon-btn"
          variant="outline"
          size="sm"
          className="rounded-none shadow-none font-normal shrink-0"
          onClick={() => { setShowSetup(v => !v); setSetupStep('daemon') }}
        >
          + daemon
        </Button>
      </div>

      {hint && (
        <div data-testid="command-hint" className="px-3 pb-2 text-xs font-mono text-amber-500">
          {hint}
        </div>
      )}

      {daemonError && (
        <div className="px-3 pb-2 text-xs font-mono text-red-500 flex items-center justify-between">
          <span>error: {daemonError}</span>
          <button onClick={clearDaemonError} className="ml-4 text-red-400 hover:text-red-300">✕</button>
        </div>
      )}

      {pendingCreate && (
        <div className="border-t border-border px-3 py-2 bg-muted flex items-center gap-3 text-xs font-mono">
          <span className="text-muted-foreground shrink-0">
            <span className="text-amber-500">directory not found:</span> {pendingCreate.path}
          </span>
          <span className="text-muted-foreground">— create it and run git init?</span>
          <Button
            size="sm"
            className="rounded-none shadow-none font-normal shrink-0"
            onClick={() => {
              send(pendingCreate.machineId, {
                type: 'create_and_register_project',
                path: pendingCreate.path,
                name: pendingCreate.name,
              })
              clearPendingCreate()
              setShowSetup(false)
              setSetupStep(null)
            }}
          >
            Yes
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="rounded-none shadow-none font-normal shrink-0"
            onClick={() => { clearPendingCreate(); setShowSetup(false); setSetupStep(null) }}
          >
            No
          </Button>
        </div>
      )}

      {/* Project setup flow — onboarding only, not part of intent verbs */}
      {showSetup && (
        <div data-testid="setup-flow" className="border-t border-border px-3 py-2 bg-muted space-y-2">
          {setupStep === 'project' && (
            <div className="space-y-2">
            {Object.values(daemons).filter(d => d.connected).length >= 2 && (
              <div className="flex items-center gap-2">
                <span className="text-xs font-mono text-muted-foreground shrink-0">daemon:</span>
                {Object.values(daemons).filter(d => d.connected).map(d => (
                  <button
                    key={d.id}
                    className={`px-2 py-1 text-xs font-mono border ${setupData.machineId === d.id ? 'border-foreground' : 'border-border text-muted-foreground'} hover:border-foreground transition-colors`}
                    onClick={() => setSetupData(sd => ({ ...sd, machineId: d.id }))}
                  >
                    {d.name}
                  </button>
                ))}
              </div>
            )}
            <div className="flex items-center gap-2">
              <span className="text-xs font-mono text-muted-foreground shrink-0">path:</span>
              <input
                data-testid="setup-path-input"
                className="flex-1 bg-background border border-border px-2 py-1 text-sm font-mono outline-none"
                placeholder="/Users/you/code/my-project"
                value={setupData.projectPath}
                onChange={e => setSetupData(d => ({ ...d, projectPath: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') handleRegisterProject() }}
                autoFocus
              />
              <input
                className="w-32 bg-background border border-border px-2 py-1 text-sm font-mono outline-none"
                placeholder="name (opt)"
                value={setupData.projectName}
                onChange={e => setSetupData(d => ({ ...d, projectName: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') handleRegisterProject() }}
              />
              <Button size="sm" className="rounded-none shadow-none font-normal" onClick={handleRegisterProject}>
                Register
              </Button>
            </div>
            </div>
          )}
          {setupStep === 'worktree' && (
            <div className="flex items-center gap-2 flex-wrap">
              <span className="text-xs font-mono text-muted-foreground shrink-0">branch:</span>
              <input
                data-testid="setup-branch-input"
                className="w-40 bg-background border border-border px-2 py-1 text-sm font-mono outline-none"
                placeholder="main"
                value={setupData.branch}
                onChange={e => setSetupData(d => ({ ...d, branch: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') handleCreateWorktree() }}
                autoFocus
              />
              <span className="text-xs font-mono text-muted-foreground shrink-0">mode:</span>
              {(['plan', 'default', 'auto', 'dangerously-skip-permissions'] as const).map(mode => (
                <button
                  key={mode}
                  className={`px-2 py-1 text-xs font-mono border ${setupData.permissionMode === mode ? 'border-foreground' : 'border-border text-muted-foreground'} hover:border-foreground transition-colors`}
                  onClick={() => setSetupData(d => ({ ...d, permissionMode: mode }))}
                >
                  {mode}
                </button>
              ))}
              <Button size="sm" className="rounded-none shadow-none font-normal" onClick={handleCreateWorktree}>
                Create worktree
              </Button>
              <Button variant="ghost" size="sm" className="rounded-none shadow-none font-normal" onClick={() => { setShowSetup(false); setSetupStep(null) }}>
                Cancel
              </Button>
            </div>
          )}
          {setupStep === 'daemon' && (
            <div className="flex items-center gap-2">
              <span className="text-xs font-mono text-muted-foreground shrink-0">ws url:</span>
              <input
                data-testid="setup-daemon-url-input"
                className="flex-1 bg-background border border-border px-2 py-1 text-sm font-mono outline-none"
                placeholder="wss://machine.ts.net:9111/ws"
                value={setupData.daemonUrl}
                onChange={e => setSetupData(d => ({ ...d, daemonUrl: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') handleAddDaemon() }}
                autoFocus
              />
              <input
                className="w-28 bg-background border border-border px-2 py-1 text-sm font-mono outline-none"
                placeholder="name (opt)"
                value={setupData.daemonName}
                onChange={e => setSetupData(d => ({ ...d, daemonName: e.target.value }))}
                onKeyDown={e => { if (e.key === 'Enter') handleAddDaemon() }}
              />
              <Button size="sm" className="rounded-none shadow-none font-normal" onClick={handleAddDaemon}>
                Add
              </Button>
              <Button variant="ghost" size="sm" className="rounded-none shadow-none font-normal" onClick={() => { setShowSetup(false); setSetupStep(null) }}>
                Cancel
              </Button>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
