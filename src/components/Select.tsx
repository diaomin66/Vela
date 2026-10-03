import { useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { Check, ChevronDown, Search } from 'lucide-react';
import './editor.css';

export interface SelectOption { value: string; label: string; description?: string; disabled?: boolean }
interface SelectProps {
  id?: string;
  ariaLabel?: string;
  value: string;
  onChange: (value: string) => void;
  options: SelectOption[];
  disabled?: boolean;
  searchable?: boolean;
  placeholder?: string;
  className?: string;
}

export function Select({ id, ariaLabel, value, onChange, options, disabled = false, searchable = false, placeholder = '请选择', className = '' }: SelectProps) {
  const uid = useId();
  const triggerId = id ?? `${uid}-trigger`;
  const listId = `${uid}-list`;
  const button = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [placement, setPlacement] = useState<CSSProperties>({});
  const chosen = options.find((option) => option.value === value);
  const filtered = options.filter((option) => `${option.label} ${option.description ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
  const activeId = filtered[active] ? `${uid}-option-${active}` : undefined;

  function close(restoreFocus = false) {
    setOpen(false);
    popup.current?.hidePopover();
    if (restoreFocus) button.current?.focus();
  }

  function show(last = false) {
    if (disabled) return;
    setQuery('');
    const selected = options.findIndex((option) => option.value === value && !option.disabled);
    const enabled = options.map((option, index) => option.disabled ? -1 : index).filter((index) => index >= 0);
    setActive(last ? (enabled.at(-1) ?? 0) : selected >= 0 ? selected : (enabled[0] ?? 0));
    setOpen(true);
  }

  useLayoutEffect(() => {
    if (!open) return;
    const element = popup.current;
    if (!element) return;
    const place = () => {
      const bounds = button.current?.getBoundingClientRect();
      if (!bounds) return;
      const edge = 12;
      const width = Math.min(Math.max(bounds.width, 230), window.innerWidth - edge * 2);
      const below = window.innerHeight - bounds.bottom - edge - 7;
      const above = bounds.top - edge - 7;
      const upward = below < 210 && above > below;
      const available = Math.max(100, upward ? above : below);
      const maxHeight = Math.min(356, available);
      setPlacement({
        left: Math.min(Math.max(edge, bounds.left), window.innerWidth - width - edge), width,
        top: upward ? undefined : bounds.bottom + 7,
        bottom: upward ? window.innerHeight - bounds.top + 7 : undefined,
        maxHeight,
        '--select-list-height': `${Math.max(48, maxHeight - (searchable ? 61 : 16))}px`,
      } as CSSProperties);
    };
    place();
    element.showPopover();
    const frame = requestAnimationFrame(() => (searchable ? search.current : list.current)?.focus());
    const onScroll = (event: Event) => {
      if (event.target instanceof Node && element.contains(event.target)) return;
      const trigger = button.current;
      if (!trigger) return;
      const bounds = trigger.getBoundingClientRect();
      let clipTop = 0;
      let clipBottom = window.innerHeight;
      for (let ancestor = trigger.parentElement; ancestor; ancestor = ancestor.parentElement) {
        if (!/(auto|scroll|hidden|clip)/.test(getComputedStyle(ancestor).overflowY)) continue;
        const clip = ancestor.getBoundingClientRect();
        clipTop = Math.max(clipTop, clip.top);
        clipBottom = Math.min(clipBottom, clip.bottom);
      }
      if (bounds.bottom <= clipTop || bounds.top >= clipBottom) {
        setOpen(false);
        element.hidePopover();
      } else place();
    };
    window.addEventListener('resize', place);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [open, searchable]);

  useEffect(() => {
    if (open) document.getElementById(activeId ?? '')?.scrollIntoView({ block: 'nearest' });
  }, [activeId, open]);

  useEffect(() => {
    if (disabled) {
      setOpen(false);
      popup.current?.hidePopover();
    }
  }, [disabled]);

  function choose(index: number) {
    const option = filtered[index];
    if (!option || option.disabled) return;
    onChange(option.value);
    close(true);
  }

  function navigate(event: KeyboardEvent<HTMLElement>) {
    if (event.key === 'Escape') {
      event.preventDefault(); event.stopPropagation(); close(true); return;
    }
    if (event.key === 'Tab') { close(true); return; }
    if (event.key === 'Enter' || (event.key === ' ' && event.target === list.current)) {
      event.preventDefault(); choose(active); return;
    }
    const enabled = filtered.map((option, index) => option.disabled ? -1 : index).filter((index) => index >= 0);
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault();
      if (event.key === 'Home') setActive(enabled[0] ?? 0);
      else if (event.key === 'End') setActive(enabled.at(-1) ?? 0);
      else {
        const at = enabled.indexOf(active);
        const next = event.key === 'ArrowDown' ? (at + 1) % enabled.length : (at - 1 + enabled.length) % enabled.length;
        setActive(enabled[next] ?? 0);
      }
    }
  }

  return <div className={`vela-select ${className}`}>
    <button ref={button} id={triggerId} type="button" role="combobox" className="vela-select-trigger"
      aria-label={ariaLabel} aria-expanded={open} aria-controls={listId} aria-haspopup="listbox" disabled={disabled}
      onClick={() => open ? close(true) : show()}
      onKeyDown={(event) => {
        if (open) { navigate(event); return; }
        if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
          event.preventDefault(); show(event.key === 'End');
          if (event.key === 'Home') setActive(Math.max(0, options.findIndex((option) => !option.disabled)));
        }
      }}>
      <span className={!chosen ? 'vela-select-placeholder' : ''}>{chosen?.label ?? placeholder}</span><ChevronDown size={15} className={open ? 'is-open' : ''}/>
    </button>
    <div ref={popup} className="vela-select-popup" popover="auto" style={placement}
      onToggle={(event) => { if (event.newState === 'closed') setOpen(false); }} onKeyDown={navigate}>
      {searchable && <div className="vela-select-search"><Search size={15}/><input ref={search} value={query}
        aria-label={`搜索${ariaLabel ?? '选项'}`} role="combobox" aria-autocomplete="list" aria-expanded="true"
        aria-controls={listId} aria-activedescendant={activeId} placeholder="搜索…" autoComplete="off"
        onChange={(event) => { setQuery(event.target.value); setActive(0); }}/></div>}
      <div ref={list} id={listId} role="listbox" tabIndex={-1} className="vela-select-options"
        aria-label={ariaLabel} aria-labelledby={ariaLabel ? undefined : triggerId} aria-activedescendant={activeId}>
        {filtered.map((option, index) => <div id={`${uid}-option-${index}`} key={option.value} role="option"
          aria-selected={option.value === value} aria-disabled={option.disabled || undefined}
          className={`vela-select-option ${index === active ? 'is-active' : ''}`}
          onPointerMove={() => { if (!option.disabled) setActive(index); }}
          onMouseDown={(event) => event.preventDefault()} onClick={() => choose(index)}>
          <span><span>{option.label}</span>{option.description && <small>{option.description}</small>}</span>
          {option.value === value && <Check size={16}/>}
        </div>)}
        {!filtered.length && <p className="vela-select-empty" role="status">没有匹配选项</p>}
      </div>
    </div>
  </div>;
}
