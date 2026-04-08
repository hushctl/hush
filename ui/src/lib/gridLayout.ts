import { Delaunay } from 'd3-delaunay'
import { urgencyOrder, statusColor } from './status'
import type { WorktreeInfo } from './protocol'

const GRID_SPACING = 24
const GRAVITY_RADIUS = 55
const JITTER = 6  // px — deterministic per-site jitter to break hexagonal regularity

export interface VoronoiCell {
  polygon: Array<[number, number]>
  fill: string
}

export interface WorktreePosition {
  worktreeId: string
  x: number
  y: number
  dotSize: number
  color: string
  lastActivity: number
}

/** Cheap integer hash for deterministic per-site jitter. */
function siteHash(col: number, row: number): number {
  let h = (col * 92837111) ^ (row * 689287499)
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b)
  h = Math.imul(h ^ (h >>> 16), 0x45d9f3b)
  return h ^ (h >>> 16)
}

/**
 * Build a Voronoi tessellation that fills the viewport.
 *
 * Sites are placed on a ~GRID_SPACING lattice with a small deterministic jitter
 * so cells look organic rather than hexagonal. Each cell is coloured toward the
 * nearest worktree's status colour when it falls within GRAVITY_RADIUS of that
 * worktree's dot, using the same falloff curve as the old gravity-well logic.
 */
export function generateVoronoiCells(
  width: number,
  height: number,
  wtPositions: WorktreePosition[]
): VoronoiCell[] {
  if (width <= 0 || height <= 0) return []

  // Build jittered site array
  const cols = Math.ceil(width / GRID_SPACING) + 1
  const rows = Math.ceil(height / GRID_SPACING) + 1
  const points: number[] = []
  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < cols; c++) {
      const h = siteHash(c, r)
      const jx = ((h & 0xff) / 255 - 0.5) * 2 * JITTER
      const jy = (((h >>> 8) & 0xff) / 255 - 0.5) * 2 * JITTER
      points.push(
        c * GRID_SPACING + GRID_SPACING / 2 + jx,
        r * GRID_SPACING + GRID_SPACING / 2 + jy
      )
    }
  }

  const delaunay = Delaunay.from({ length: points.length / 2 } as ArrayLike<unknown>, (_, i) => points[i * 2], (_, i) => points[i * 2 + 1])
  const voronoi = delaunay.voronoi([0, 0, width, height])

  const cells: VoronoiCell[] = []
  const n = points.length / 2
  for (let i = 0; i < n; i++) {
    const poly = voronoi.cellPolygon(i)
    if (!poly || poly.length < 3) continue

    // Centroid of the polygon
    let cx = 0, cy = 0
    for (const [px, py] of poly) { cx += px; cy += py }
    cx /= poly.length
    cy /= poly.length

    // Gravity-well fill: blend toward the nearest worktree's status color
    let fill = '#0a0a0a'
    let bestFactor = 0
    let bestColor = fill
    for (const wt of wtPositions) {
      const dist = Math.hypot(cx - wt.x, cy - wt.y)
      if (dist < GRAVITY_RADIUS) {
        const factor = (1 - dist / GRAVITY_RADIUS) * 0.35
        if (factor > bestFactor) {
          bestFactor = factor
          bestColor = wt.color
        }
      }
    }
    if (bestFactor > 0) {
      fill = blendColor('#0a0a0a', bestColor, bestFactor)
    }

    cells.push({ polygon: poly as Array<[number, number]>, fill })
  }

  return cells
}

/**
 * Place worktrees on the grid using a true 2D scatter:
 *   X-axis = urgency  (needs_you → left, idle → right)
 *   Y-axis = recency  (most recent → top, oldest → bottom)
 *
 * Worktrees belonging to the same project cluster vertically.
 * Projects with multiple worktrees stack their dots with a fixed gap,
 * and the cluster's X is anchored by the project's most-urgent worktree.
 */
export function computeWorktreePositions(
  worktrees: WorktreeInfo[],
  width: number,
  height: number,
  lastActivities: Map<string, number>
): WorktreePosition[] {
  if (worktrees.length === 0) return []

  const now = Date.now()
  const PADDING = 100
  const CLUSTER_V_GAP = 32  // px between stacked dots in the same project
  const usableW = width - 2 * PADDING
  const usableH = height - 2 * PADDING

  // X anchor for each urgency level (fraction of usable width)
  // needs_you=0 → far left; idle=3 → far right
  const URGENCY_X = [0.08, 0.32, 0.62, 0.84]

  // Normalise recency: 0 = most recent (→ top), 1 = oldest (→ bottom)
  const times = worktrees.map(wt => lastActivities.get(wt.id) ?? now)
  const minT = Math.min(...times)
  const maxT = Math.max(...times)
  const timeRange = maxT - minT || 1

  function recencyNorm(id: string): number {
    const t = lastActivities.get(id) ?? now
    // Invert: most recent (high t) → 0 (top)
    return 1 - (t - minT) / timeRange
  }

  // Group worktrees by project
  const byProject = new Map<string, WorktreeInfo[]>()
  for (const wt of worktrees) {
    const arr = byProject.get(wt.project_id) ?? []
    arr.push(wt)
    byProject.set(wt.project_id, arr)
  }

  // Sort each project's worktrees: most urgent first, then most recent
  for (const wts of byProject.values()) {
    wts.sort((a, b) => {
      const uDiff = urgencyOrder(a.status) - urgencyOrder(b.status)
      if (uDiff !== 0) return uDiff
      return recencyNorm(a.id) - recencyNorm(b.id) // recent first in cluster
    })
  }

  // Sort projects by their best (most urgent) worktree, then recency
  const sortedProjects = [...byProject.entries()].sort((a, b) => {
    const uA = urgencyOrder(a[1][0].status)
    const uB = urgencyOrder(b[1][0].status)
    if (uA !== uB) return uA - uB
    return recencyNorm(a[1][0].id) - recencyNorm(b[1][0].id)
  })

  // Group projects by urgency band so we can distribute Y evenly within each band.
  // This prevents all idle (or all running) projects from collapsing to the same Y
  // when their recency scores are identical.
  const byBand = new Map<number, typeof sortedProjects>()
  for (const entry of sortedProjects) {
    const band = urgencyOrder(entry[1][0].status)
    const arr = byBand.get(band) ?? []
    arr.push(entry)
    byBand.set(band, arr)
  }

  const positions: WorktreePosition[] = []
  // Track which project each position belongs to (for cross-project collision avoidance)
  const posProjectId: string[] = []

  for (const [band, bandProjects] of byBand.entries()) {
    const anchorXFrac = URGENCY_X[band]
    const n = bandProjects.length

    bandProjects.forEach(([, wts], bandIdx) => {
      // Distribute projects evenly across Y within their band.
      // Even one project gets placed at the midpoint of its slot.
      const yFrac = (bandIdx + 0.5) / n
      const baseY = PADDING + yFrac * usableH

      // Hash-based X nudge — wider spread (±50px) to separate labels
      const projectHash = wts[0].project_id
        .split('')
        .reduce((acc, c) => acc + c.charCodeAt(0), 0)
      const xNudge = ((projectHash * 31) % 100) - 50  // ±50px

      const baseX = PADDING + anchorXFrac * usableW + xNudge

      // Stack worktrees in the project vertically, centred around baseY
      const totalHeight = (wts.length - 1) * CLUSTER_V_GAP
      const startY = baseY - totalHeight / 2

      for (let i = 0; i < wts.length; i++) {
        const wt = wts[i]
        const urgency = urgencyOrder(wt.status)
        const xShift = (urgency - band) * 24

        const x = Math.max(PADDING, Math.min(width - PADDING, baseX + xShift))
        const y = Math.max(PADDING, Math.min(height - PADDING, startY + i * CLUSTER_V_GAP))
        const dotSize = urgency === 0 ? 14 : urgency === 1 ? 11 : urgency === 2 ? 9 : 7

        positions.push({
          worktreeId: wt.id,
          x,
          y,
          dotSize,
          color: statusColor(wt.status),
          lastActivity: lastActivities.get(wt.id) ?? now,
        })
        posProjectId.push(wts[0].project_id)
      }
    })
  }

  // Cross-project collision avoidance: ensure dots from different projects stay
  // at least MIN_SEP apart (accounts for label widths). Same-project dots are
  // allowed to stay close (they are already stacked with CLUSTER_V_GAP).
  const MIN_SEP = 110
  for (let iter = 0; iter < 20; iter++) {
    for (let i = 0; i < positions.length; i++) {
      for (let j = i + 1; j < positions.length; j++) {
        if (posProjectId[i] === posProjectId[j]) continue  // same project — keep tight
        const dx = positions[j].x - positions[i].x
        const dy = positions[j].y - positions[i].y
        const dist = Math.hypot(dx, dy)
        if (dist < MIN_SEP && dist > 0.01) {
          const push = (MIN_SEP - dist) / 2 + 1
          const nx = dx / dist
          const ny = dy / dist
          // Push primarily along Y to preserve urgency-X column grouping
          positions[i].x = Math.max(PADDING, Math.min(width - PADDING, positions[i].x - nx * push * 0.2))
          positions[i].y = Math.max(PADDING, Math.min(height - PADDING, positions[i].y - ny * push))
          positions[j].x = Math.max(PADDING, Math.min(width - PADDING, positions[j].x + nx * push * 0.2))
          positions[j].y = Math.max(PADDING, Math.min(height - PADDING, positions[j].y + ny * push))
        }
      }
    }
  }

  return positions
}

/** Hex color blend: a * (1-t) + b * t */
function blendColor(a: string, b: string, t: number): string {
  const ra = parseInt(a.slice(1, 3), 16)
  const ga = parseInt(a.slice(3, 5), 16)
  const ba = parseInt(a.slice(5, 7), 16)
  const rb = parseInt(b.slice(1, 3), 16)
  const gb = parseInt(b.slice(3, 5), 16)
  const bb = parseInt(b.slice(5, 7), 16)
  const r = Math.round(ra + (rb - ra) * t)
  const g = Math.round(ga + (gb - ga) * t)
  const bl = Math.round(ba + (bb - ba) * t)
  return `rgb(${r},${g},${bl})`
}
