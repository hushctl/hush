import { TopBar } from './TopBar'
import { TrustCertBanner } from './TrustCertBanner'
import { Canvas } from '@/components/Canvas/Canvas'

export function TilingContainer() {
  return (
    <div className="flex flex-col h-full">
      <TopBar />
      <TrustCertBanner />
      <div className="flex-1 relative overflow-hidden">
        <Canvas />
      </div>
    </div>
  )
}
