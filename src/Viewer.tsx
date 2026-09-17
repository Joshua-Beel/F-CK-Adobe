import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { renderPage } from './bridge';
import { pageOffsets, visiblePages, type DocumentInfo } from './model';
import styles from './Workspace.module.css';

function Page({ id, index, width, height, scale }: { id: number; index: number; width: number; height: number; scale: number }) {
  const [url, setUrl] = useState('');
  const [error, setError] = useState('');
  useEffect(() => {
    let disposed = false;
    let objectUrl = '';
    setUrl(''); setError('');
    const timer = window.setTimeout(() => {
      renderPage(id, index, Math.min(3000, Math.round(width * scale * window.devicePixelRatio))).then(result => {
        objectUrl = result;
        if (disposed) URL.revokeObjectURL(result); else setUrl(result);
      }).catch(e => { if (!disposed) setError(String(e)); });
    }, 35);
    return () => { disposed = true; clearTimeout(timer); if (objectUrl) URL.revokeObjectURL(objectUrl); };
  }, [id, index, width, scale]);
  return <div className={styles.paper} style={{ width: width * scale, height: height * scale }} aria-label={`Page ${index + 1}`}>
    {url ? <img src={url} alt={`Page ${index + 1}`} draggable={false} /> : <div className={styles.pageLoading}>{error || `Rendering page ${index + 1}…`}</div>}
  </div>;
}

export default function Viewer({ document, zoom, fit, target, onPage, hand }: { document: DocumentInfo; zoom: number; fit: boolean; target: { page: number; token: number }; onPage: (page: number) => void; hand: boolean }) {
  const viewport = useRef<HTMLDivElement>(null);
  const [bounds, setBounds] = useState({ width: 900, height: 800 });
  const [top, setTop] = useState(0);
  const drag = useRef<{ x: number; y: number; top: number; left: number } | null>(null);
  useEffect(() => {
    const element = viewport.current!;
    const observer = new ResizeObserver(([entry]) => setBounds({ width: entry.contentRect.width, height: entry.contentRect.height }));
    observer.observe(element); return () => observer.disconnect();
  }, []);
  const scale = fit ? Math.max(0.1, (bounds.width - 144) / Math.max(...document.pages.map(p => p.width))) : zoom / 100 * 96 / 72;
  const offsets = useMemo(() => pageOffsets(document.pages, scale), [document, scale]);
  const previousLayout = useRef({ offsets, scale });
  const visible = visiblePages(offsets, document.pages, scale, top, bounds.height);
  const height = offsets.at(-1)! + document.pages.at(-1)!.height * scale + 24;
  useLayoutEffect(() => {
    const element = viewport.current!;
    const old = previousLayout.current;
    if (old.scale !== scale) {
      let anchor = old.offsets.findIndex((offset, i) => offset + document.pages[i].height * old.scale > element.scrollTop);
      anchor = Math.max(0, anchor);
      element.scrollTop = offsets[anchor] + (element.scrollTop - old.offsets[anchor]) * scale / old.scale;
      setTop(element.scrollTop);
    }
    previousLayout.current = { offsets, scale };
  }, [offsets, scale, document]);
  useEffect(() => { viewport.current?.scrollTo({ top: offsets[target.page] - 24 }); }, [target]);
  return <div ref={viewport} className={`${styles.viewport} ${hand ? styles.hand : ''}`} onScroll={event => {
    const scrollTop = event.currentTarget.scrollTop;
    setTop(scrollTop);
    let page = offsets.findIndex((offset, i) => offset + document.pages[i].height * scale > scrollTop + 80);
    onPage(page < 0 ? document.pages.length - 1 : page);
  }} onPointerDown={event => {
    if (!hand || event.button !== 0) return;
    const element = viewport.current!;
    drag.current = { x: event.clientX, y: event.clientY, top: element.scrollTop, left: element.scrollLeft };
    element.setPointerCapture(event.pointerId);
  }} onPointerMove={event => {
    if (!drag.current) return;
    viewport.current!.scrollTop = drag.current.top - event.clientY + drag.current.y;
    viewport.current!.scrollLeft = drag.current.left - event.clientX + drag.current.x;
  }} onPointerUp={() => { drag.current = null; }} onLostPointerCapture={() => { drag.current = null; }}>
    <div className={styles.pageStack} style={{ height, minWidth: Math.max(...document.pages.map(p => p.width)) * scale + 144 }}>
      {visible.map(index => <div key={`${document.id}-${index}`} className={styles.pagePosition} style={{ top: offsets[index] }}>
        <Page id={document.id} index={index} {...document.pages[index]} scale={scale} />
      </div>)}
    </div>
  </div>;
}
