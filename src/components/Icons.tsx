export function BrandMark({ className = '' }: { className?: string }) {
  return <img className={className} src="/mark.svg" width={32} height={32} alt="" draggable={false} aria-hidden="true"/>;
}
