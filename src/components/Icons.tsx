export function BrandMark({ className = '' }: { className?: string }) {
  return <svg className={className} viewBox="0 0 48 48" fill="none" aria-hidden="true"><g transform="translate(24 24)" fill="currentColor"><rect x="-5" y="-21" width="10" height="42" rx="5" transform="rotate(45)"/><rect x="-5" y="-21" width="10" height="42" rx="5" transform="rotate(-45)"/><circle r="6.5" fill="var(--mark-cutout, #f7f8f4)"/></g></svg>;
}
