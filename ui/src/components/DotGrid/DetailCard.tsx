import { ProjectCard } from '@/components/ProjectCard/ProjectCard'
import type { WorktreeInfo } from '@/lib/protocol'

interface Props {
  worktree: WorktreeInfo
  onOpen: () => void
}

export function DetailCard({ worktree, onOpen }: Props) {
  return (
    <div className="bg-card border border-border">
      <ProjectCard worktree={worktree} onOpen={onOpen} />
    </div>
  )
}
