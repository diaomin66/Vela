import { memo, useEffect, useMemo, useRef, useState } from 'react';
import { createArtifactDocument } from '../../lib/evaluation/artifact-document';
import { createNativeArtifact, nativeArtifacts, releaseNativeArtifact } from '../../lib/evaluation/artifact-api';
import './artifact-preview.css';

interface ArtifactPreviewProps {
  html: string;
  title: string;
  playing?: boolean;
  className?: string;
  interactive?: boolean;
  onReady?: () => void;
  onError?: (message: string) => void;
}

export const ArtifactPreview = memo(function ArtifactPreview({ html, title, playing = true, className = '', interactive = false, onReady, onError }: ArtifactPreviewProps) {
  const container = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  const callbacks = useRef({ onReady, onError });
  const playback = useRef(playing);
  const [width, setWidth] = useState(0);
  const [visible, setVisible] = useState(false);
  const [source, setSource] = useState<{ token: string; url: string }>();
  const [error, setError] = useState<string>();
  const [ready, setReady] = useState(false);
  const token = useMemo(() => crypto.randomUUID(), [html]);
  const document = useMemo(() => createArtifactDocument(html, token, false), [html, token]);
  callbacks.current = { onReady, onError };
  playback.current = playing;

  useEffect(() => {
    const node = container.current;
    if (!node) return;
    const resize = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    const intersection = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { rootMargin: '100px' });
    resize.observe(node);
    intersection.observe(node);
    return () => { resize.disconnect(); intersection.disconnect(); };
  }, []);

  useEffect(() => {
    setReady(false);
    setError(undefined);
    if (!nativeArtifacts) return;
    let active = true;
    let id: string | undefined;
    void createNativeArtifact(document).then((location) => {
      id = location.id;
      if (active) setSource({ token, url: location.url });
      else void releaseNativeArtifact(id).catch(() => {});
    }).catch((failure) => {
      if (!active) return;
      const message = String(failure);
      setError(message);
      callbacks.current.onError?.(message);
    });
    return () => { active = false; if (id) void releaseNativeArtifact(id).catch(() => {}); };
  }, [document, token]);

  useEffect(() => {
    const listen = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || event.data?.type !== 'ahax:artifact-ready' || event.data.token !== token) return;
      setReady(true);
      callbacks.current.onReady?.();
      frame.current?.contentWindow?.postMessage({ type: 'ahax:artifact-playback', token, playing: playback.current && visible }, '*');
    };
    window.addEventListener('message', listen);
    return () => window.removeEventListener('message', listen);
  }, [token, visible]);

  useEffect(() => {
    frame.current?.contentWindow?.postMessage({ type: 'ahax:artifact-playback', token, playing: playing && visible }, '*');
  }, [playing, ready, token, visible]);

  return <div ref={container} className={`artifact-preview ${className}`} data-ready={ready || undefined} data-interactive={interactive || undefined} aria-busy={!ready && !error}>
    {error ? <div className="artifact-preview-error" role="status">{error}</div> : (!nativeArtifacts || source?.token === token) ? <iframe
      key={token}
      ref={frame}
      title={title}
      tabIndex={interactive ? 0 : -1}
      sandbox="allow-scripts"
      referrerPolicy="no-referrer"
      allow="camera 'none'; microphone 'none'; geolocation 'none'; clipboard-read 'none'; clipboard-write 'none'"
      src={nativeArtifacts ? source?.url : undefined}
      srcDoc={nativeArtifacts ? undefined : document}
      style={{ transform: `scale(${width / 960})` }}
    /> : <div className="artifact-preview-loading" aria-label="正在载入作品" />}
  </div>;
});
