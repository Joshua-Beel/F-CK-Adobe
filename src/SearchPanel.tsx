import { useEffect, useRef, useState } from 'react';
import { pageText } from './bridge';
import type { DocumentInfo } from './model';
import s from './SearchPanel.module.css';

type Hit = { page: number; excerpt: string };
export default function SearchPanel({ document, go, close }: { document: DocumentInfo; go: (page: number) => void; close: () => void }) {
  const [query, setQuery] = useState('');
  const [matchCase, setMatchCase] = useState(false);
  const [hits, setHits] = useState<Hit[]>([]);
  const [status, setStatus] = useState('Search embedded text. Scanned images need OCR first.');
  const [running, setRunning] = useState(false);
  const [error, setError] = useState('');
  const generation = useRef(0);
  useEffect(() => () => { generation.current++; }, []);
  const cancel = () => { generation.current++; setRunning(false); setStatus('Search stopped. Results below may be incomplete.'); };
  const search = async () => {
    const needle = query.trim();
    if (!needle) return;
    const request = ++generation.current;
    setHits([]); setError(''); setRunning(true);
    const found: Hit[] = [];
    try {
      for (let page = 0; page < document.pages.length; page++) {
        if (request !== generation.current) return;
        setStatus(`Searching page ${page + 1} of ${document.pages.length}…`);
        const text = await pageText(document.id, page, document.revision);
        if (request !== generation.current) return;
        const pattern = needle.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
        const match = new RegExp(pattern, matchCase ? 'u' : 'iu').exec(text);
        if (match) {
          const start = Math.max(0, match.index - 70);
          const end = Math.min(text.length, match.index + match[0].length + 110);
          found.push({ page, excerpt: `${start ? '…' : ''}${text.slice(start, end)}${end < text.length ? '…' : ''}` });
          setHits([...found]);
        }
        if (found.length === 500) { setStatus('Showing the first 500 matching pages. Narrow your search to see fewer results.'); return; }
      }
      setStatus(found.length ? `${found.length} matching ${found.length === 1 ? 'page' : 'pages'}.` : 'No matching text found. Scanned images need OCR first.');
    } catch (e) {
      if (request === generation.current) { setError(String(e)); setStatus('Search could not finish. Results below may be incomplete.'); }
    } finally { if (request === generation.current) setRunning(false); }
  };
  return <aside className={s.panel} aria-label="Find in document">
    <div className={s.heading}><h2>Find text</h2><button aria-label="Close search" onClick={close}>×</button></div>
    <form onSubmit={event => { event.preventDefault(); void search(); }}>
      <input autoFocus aria-label="Find in document" value={query} maxLength={500} onChange={event => { cancel(); setQuery(event.target.value); setHits([]); setError(''); setStatus('Press Enter to search.'); }} />
      <label><input type="checkbox" checked={matchCase} onChange={event => { cancel(); setMatchCase(event.target.checked); setHits([]); setStatus('Press Enter to search.'); }} /> Match case</label>
      <div className={s.actions}><button type="submit" disabled={!query.trim()}>Search</button>{running && <button type="button" onClick={cancel}>Stop</button>}</div>
    </form>
    <p role="status">{status}</p>
    {error && <p role="alert">{error}</p>}
    <div className={s.results}>{hits.map(hit => <section key={hit.page}><button onClick={() => go(hit.page)}>Go to page {hit.page + 1}</button><p>{hit.excerpt}</p></section>)}</div>
  </aside>;
}
