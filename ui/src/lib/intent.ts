// Workspace-intent parser. Takes user-typed text + current store snapshot,
// returns a structured IntentResult the CommandBar can dispatch.
//
// Verbs:
//   pull up <project>[ and <project>...]
//   open <project>/<branch>
//   close <project|branch>
//   back to grid
//   show me what needs me / show me what needs attention
//   tree <project>
//   new worktree <branch>[ in <project>]

import type { ProjectInfo, WorktreeInfo } from './protocol'

export type IntentResult =
  | { kind: 'unknown'; reason: string }
  | { kind: 'back_to_grid' }
  | { kind: 'pull_up'; worktreeIds: string[] }
  | { kind: 'close'; worktreeIds: string[] }
  | { kind: 'show_needs_me' }
  | { kind: 'tree'; projectId: string }
  | { kind: 'new_worktree'; projectId: string; branch: string }

export interface IntentContext {
  projects: Record<string, ProjectInfo>
  worktrees: Record<string, WorktreeInfo>
}

export function parseIntent(input: string, ctx: IntentContext): IntentResult {
  const text = input.trim().toLowerCase()
  if (!text) return { kind: 'unknown', reason: 'empty input' }

  // back to grid
  if (text === 'back to grid' || text === 'grid' || text === 'back') {
    return { kind: 'back_to_grid' }
  }

  // show me what needs me / needs attention
  if (
    text.startsWith('show me what needs') ||
    text === 'what needs me' ||
    text === 'needs me' ||
    text === 'needs attention'
  ) {
    return { kind: 'show_needs_me' }
  }

  // tree <project>
  if (text.startsWith('tree ')) {
    const target = text.slice(5).trim()
    const project = findProject(target, ctx)
    if (!project) return { kind: 'unknown', reason: `no project matching "${target}"` }
    return { kind: 'tree', projectId: project.id }
  }

  // new worktree <branch>[ in <project>]
  if (text.startsWith('new worktree ')) {
    const rest = text.slice(13).trim()
    const inMatch = rest.match(/^(.+?)\s+in\s+(.+)$/)
    let branch: string
    let projectName: string | null
    if (inMatch) {
      branch = inMatch[1].trim()
      projectName = inMatch[2].trim()
    } else {
      branch = rest
      projectName = null
    }
    if (!branch) return { kind: 'unknown', reason: 'missing branch name' }

    const project = projectName
      ? findProject(projectName, ctx)
      : Object.values(ctx.projects).at(-1) ?? null
    if (!project) return { kind: 'unknown', reason: 'no project to create worktree in' }
    return { kind: 'new_worktree', projectId: project.id, branch }
  }

  // open <project>/<branch>
  if (text.startsWith('open ')) {
    const target = text.slice(5).trim()
    const wt = resolveWorktreeRef(target, ctx)
    if (!wt) return { kind: 'unknown', reason: `no worktree matching "${target}"` }
    return { kind: 'pull_up', worktreeIds: [wt.id] }
  }

  // close <name>
  if (text.startsWith('close ')) {
    const target = text.slice(6).trim()
    const ids = resolveAllMatching(target, ctx)
    if (ids.length === 0) return { kind: 'unknown', reason: `no worktree matching "${target}"` }
    return { kind: 'close', worktreeIds: ids }
  }

  // pull up <project>[ and <project>...]
  if (text.startsWith('pull up ')) {
    const rest = text.slice(8).trim()
    const targets = rest
      .split(/\s*,\s*|\s+and\s+/)
      .map(s => s.trim())
      .filter(Boolean)
    const ids: string[] = []
    for (const t of targets) {
      const wt = resolveWorktreeRef(t, ctx)
      if (wt) {
        ids.push(wt.id)
      } else {
        // Try matching by project — open the project's first/most-urgent worktree
        const proj = findProject(t, ctx)
        if (proj) {
          const projectWts = Object.values(ctx.worktrees).filter(w => w.project_id === proj.id)
          if (projectWts.length > 0) ids.push(projectWts[0].id)
        }
      }
    }
    if (ids.length === 0) return { kind: 'unknown', reason: `no worktrees matching "${rest}"` }
    return { kind: 'pull_up', worktreeIds: ids }
  }

  return { kind: 'unknown', reason: `don't recognize "${input}"` }
}

function findProject(name: string, ctx: IntentContext): ProjectInfo | null {
  const lower = name.toLowerCase()
  // exact name match first
  for (const p of Object.values(ctx.projects)) {
    if (p.name.toLowerCase() === lower) return p
  }
  // prefix match
  for (const p of Object.values(ctx.projects)) {
    if (p.name.toLowerCase().startsWith(lower)) return p
  }
  // substring match
  for (const p of Object.values(ctx.projects)) {
    if (p.name.toLowerCase().includes(lower)) return p
  }
  return null
}

/** Resolve a "project/branch" or "project" reference to one worktree. */
function resolveWorktreeRef(ref: string, ctx: IntentContext): WorktreeInfo | null {
  const lower = ref.toLowerCase()
  if (lower.includes('/')) {
    const [projName, branchName] = lower.split('/').map(s => s.trim())
    const project = findProject(projName, ctx)
    if (!project) return null
    return (
      Object.values(ctx.worktrees).find(
        w => w.project_id === project.id && w.branch.toLowerCase() === branchName,
      ) ?? null
    )
  }
  // Bare branch name match
  for (const w of Object.values(ctx.worktrees)) {
    if (w.branch.toLowerCase() === lower) return w
  }
  // Project name match → first worktree
  const project = findProject(ref, ctx)
  if (!project) return null
  return Object.values(ctx.worktrees).find(w => w.project_id === project.id) ?? null
}

/** Resolve a fuzzy reference into all matching worktree IDs (used by `close`). */
function resolveAllMatching(ref: string, ctx: IntentContext): string[] {
  const lower = ref.toLowerCase()
  // Specific project/branch
  const specific = resolveWorktreeRef(ref, ctx)
  if (specific) return [specific.id]

  // All worktrees in a project
  const project = findProject(ref, ctx)
  if (project) {
    return Object.values(ctx.worktrees)
      .filter(w => w.project_id === project.id)
      .map(w => w.id)
  }

  // All worktrees on a branch name
  const matches = Object.values(ctx.worktrees).filter(w => w.branch.toLowerCase() === lower)
  return matches.map(w => w.id)
}
