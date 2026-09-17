import { useCallback, useEffect, useState } from 'react';
import { ArrowDownToLine, ArrowUpRight, Bookmark, ChevronDown, ChevronLeft, ChevronRight, ChevronsUpDown, CircleHelp, Combine, File, FileCheck2, FileImage, FileOutput, FilePenLine, FilePlus2, Files, FolderOpen, Hand, Highlighter, Home, LayoutGrid, List, Maximize, Menu, MessageSquare, Minus, MoreHorizontal, MousePointer2, PanelLeftClose, Pencil, Plus, Printer, RotateCw, Save, ScanLine, Search, ShieldCheck, Signature, SlidersHorizontal, Star, Sun, Type, X, ZoomIn, ZoomOut, type LucideIcon } from 'lucide-react';
import { closeDocument, native, openDocument } from './bridge';
import { clampPage, toolGroups, type DocumentInfo } from './model';
import Viewer from './Viewer';
import s from './Workspace.module.css';

const icons: Record<string, LucideIcon> = { 'Create a PDF': FilePlus2, 'Combine files': Combine, 'Organize pages': LayoutGrid, 'Edit a PDF': FilePenLine, 'Export a PDF': FileOutput, 'Scan & OCR': ScanLine, 'Fill & sign': Signature, 'Protect a PDF': ShieldCheck, 'Comment': MessageSquare, 'Compress a PDF': ArrowDownToLine };
function IconButton({ icon: Icon, label, onClick, disabled = false, active = false }: { icon: LucideIcon; label: string; onClick?: () => void; disabled?: boolean; active?: boolean }) {
  return <button className={`${s.iconButton} ${active ? s.activeIcon : ''}`} aria-label={label} title={disabled ? `${label} — not implemented yet` : label} onClick={onClick} disabled={disabled}><Icon size={19} strokeWidth={1.7} /></button>;
}

export default function App() {
  const [documents, setDocuments] = useState<DocumentInfo[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [view, setView] = useState<'home' | 'tools' | 'document'>('home');
  const [section, setSection] = useState('Recent');
  const [query, setQuery] = useState('');
  const [toolsOpen, setToolsOpen] = useState(true);
  const [nav, setNav] = useState(false);
  const [menu, setMenu] = useState(false);
  const [dark, setDark] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [zoom, setZoom] = useState(100);
  const [fit, setFit] = useState(true);
  const [hand, setHand] = useState(true);
  const [page, setPage] = useState(0);
  const [target, setTarget] = useState({ page: 0, token: 0 });
  const [stars, setStars] = useState<number[]>([]);
  const doc = documents.find(d => d.id === active);
  const activate = (id: number) => { setActive(id); setView('document'); setPage(0); setTarget(v => ({ page: 0, token: v.token + 1 })); };
  const open = useCallback(async (example = false) => {
    if (busy) return;
    setBusy(true); setError(''); setMenu(false);
    try {
      const info = await openDocument(example);
      if (info) { setDocuments(list => [...list, info]); activate(info.id); }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }, [busy]);
  const close = async (id: number) => {
    try { await closeDocument(id); setDocuments(list => list.filter(d => d.id !== id)); if (active === id) { setActive(null); setView('home'); } } catch (e) { setError(String(e)); }
  };
  const go = (value: number) => { if (doc) { const next = clampPage(value, doc.pages.length); setPage(next); setTarget(v => ({ page: next, token: v.token + 1 })); } };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === 'o') { event.preventDefault(); void open(); }
      if ((event.target as HTMLElement).matches('input,select,textarea')) return;
      if (event.key === 'F4') { event.preventDefault(); event.shiftKey ? setToolsOpen(v => !v) : setNav(v => !v); }
      if (event.ctrlKey && event.key === '2') { event.preventDefault(); setFit(true); }
      if (event.ctrlKey && event.key === '1') { event.preventDefault(); setFit(false); setZoom(100); }
      if (view !== 'document' || !doc) return;
      if (event.key === 'PageDown') { event.preventDefault(); go(page + 1); }
      if (event.key === 'PageUp') { event.preventDefault(); go(page - 1); }
      if (event.key === 'Home') { event.preventDefault(); go(0); }
      if (event.key === 'End') { event.preventDefault(); go(doc.pages.length - 1); }
      if (event.key.toLowerCase() === 'h' && !event.ctrlKey) setHand(true);
      if (event.ctrlKey && event.key === 'Tab') { event.preventDefault(); const i = documents.findIndex(d => d.id === active); activate(documents[(i + 1) % documents.length].id); }
      if (event.ctrlKey && event.key.toLowerCase() === 'w') { event.preventDefault(); void close(doc.id); }
    };
    window.addEventListener('keydown', listener); return () => window.removeEventListener('keydown', listener);
  });
  const listed = documents.filter(d => d.name.toLowerCase().includes(query.toLowerCase()) && (section !== 'Starred' || stars.includes(d.id)));
  const changeZoom = (value: number) => { setFit(false); setZoom(Math.max(10, Math.min(400, value))); };
  const toolRow = (name: string, index: number) => {
    const Icon = icons[name] || FileCheck2;
    return <button key={name} className={s.toolRow} disabled title={`${name} — planned, not implemented yet`}><span className={s.toolIcon} style={{ color: ['#7361b3', '#277bb4', '#239576', '#bc6b25'][index % 4] }}><Icon size={21} strokeWidth={1.7} /></span><span>{name}</span></button>;
  };
  return <div className={`${s.app} ${dark ? s.dark : ''}`}>
    <header className={s.tabbar}>
      <div className={s.appMark}><Files size={21} /></div>
      <button className={s.menuButton} onClick={() => setMenu(v => !v)} aria-expanded={menu}><Menu size={17} /> Menu</button>
      <button className={`${s.homeTab} ${view === 'home' ? s.selectedTab : ''}`} aria-label="Home" onClick={() => setView('home')}><Home size={19} /></button>
      <div className={s.documentTabs}>{documents.map(document => <div key={document.id} className={`${s.documentTab} ${view === 'document' && active === document.id ? s.selectedTab : ''}`}><button onClick={() => activate(document.id)}><File size={15} /><span>{document.name}</span></button><IconButton icon={X} label={`Close ${document.name}`} onClick={() => void close(document.id)} /></div>)}</div>
      <button className={s.createButton} disabled title="Create a PDF — not implemented yet"><Plus size={17} /> Create</button>
      <span className={s.windowTitle}>PDF Workstation</span>
    </header>
    {menu && <div className={s.menuPopover}><button onClick={() => void open()}>Open… <kbd>Ctrl+O</kbd></button><button onClick={() => void open(true)}>Open sample PDF</button><hr /><button onClick={() => { setDark(v => !v); setMenu(false); }}>Switch to {dark ? 'light' : 'dark'} theme</button><button onClick={() => { setNotice('Viewer foundation: PDF opening, tabs, continuous scrolling, page navigation, pan, zoom, and fit width are available. Editing, text selection, search, saving, OCR, forms, and signatures are not implemented yet.'); setMenu(false); }}>About this build</button></div>}
    <div className={s.globalbar}>
      <nav className={s.primaryNav}><button className={toolsOpen && view !== 'home' ? s.selectedNav : ''} onClick={() => view === 'document' ? setToolsOpen(v => !v) : setView('tools')}>All tools</button><button disabled>Edit</button><button disabled>Convert</button><button disabled>E-sign</button></nav>
      <div className={s.globalActions}><IconButton icon={Search} label="Find text" disabled /><span className={s.divider} /><IconButton icon={Save} label="Save" disabled /><IconButton icon={Printer} label="Print" disabled /><IconButton icon={Sun} label="Toggle theme" onClick={() => setDark(v => !v)} /><IconButton icon={CircleHelp} label="Build information" onClick={() => setNotice('This is the first viewer milestone. Tools marked unavailable are planned for later milestones. Your source PDFs are opened read-only.')} /><button className={s.openButton} onClick={() => void open()} disabled={busy}><FolderOpen size={16} /> {busy ? 'Opening…' : 'Open a file'}</button></div>
    </div>
    {(error || notice) && <div className={`${s.banner} ${error ? s.error : ''}`} role={error ? 'alert' : 'status'}><span>{error || notice}</span><IconButton icon={X} label="Dismiss message" onClick={() => { setError(''); setNotice(''); }} /></div>}
    <main className={s.main}>
      {view === 'home' ? <>
        <aside className={s.homeSidebar}><h2>Home</h2>{['Recent', 'Starred'].map(item => <button key={item} className={section === item ? s.sidebarSelected : ''} onClick={() => setSection(item)}>{item === 'Recent' ? <Home size={18} /> : <Star size={18} />}{item}</button>)}<div className={s.sidebarCaption}>FILES</div><button onClick={() => void open()}><FolderOpen size={18} /> Your computer</button><div className={s.sidebarBottom}><ShieldCheck size={16} /><span>Local files. Yours to keep.</span></div></aside>
        <section className={s.homeContent}><div className={s.homeHeading}><div><p className={s.eyebrow}>YOUR WORKSPACE</p><h1>Work with your PDFs.</h1></div><button className={s.outlineButton} onClick={() => setView('tools')}>See all tools <ArrowUpRight size={16} /></button></div>
          <div className={s.quickCards}>{['Edit a PDF', 'Export a PDF', 'Combine files', 'Fill & sign'].map((name, i) => { const Icon = icons[name]; return <div className={s.quickCard} key={name}><span style={{ color: ['#7260b4', '#2c80b2', '#25876b', '#b86a25'][i] }}><Icon size={29} strokeWidth={1.5} /></span><h3>{name}</h3><p>{['Update text and images.', 'Convert to another format.', 'Bring documents together.', 'Complete your paperwork.'][i]}</p><span className={s.planned}>Planned</span></div>; })}</div>
          <div className={s.recentHeader}><h2>{section}</h2><div className={s.recentControls}><label className={s.search}><Search size={16} /><input aria-label="Search open files" placeholder="Search your files" value={query} onChange={e => setQuery(e.target.value)} /></label><IconButton icon={List} label="List view" active /></div></div>
          <div className={s.tableHeading}><span>NAME</span><span>LOCATION</span><span>PAGES</span><span /></div>
          {listed.map(document => <div className={s.fileRow} key={document.id}><button onClick={() => activate(document.id)}><FileImage size={25} /><span>{document.name}<small>PDF document</small></span></button><span title={document.path}>This computer</span><span>{document.pages.length}</span><IconButton icon={Star} label={`Star ${document.name}`} active={stars.includes(document.id)} onClick={() => setStars(list => list.includes(document.id) ? list.filter(id => id !== document.id) : [...list, document.id])} /></div>)}
          {!listed.length && <div className={s.empty}><div className={s.emptyIcon}><Files size={36} strokeWidth={1.25} /></div><h3>{query ? 'No matching files' : section === 'Starred' ? 'Keep important files close' : 'Your documents start here'}</h3><p>{query ? 'Try another file name.' : section === 'Starred' ? 'Star an open file to find it here.' : 'Open a PDF from your computer to start reading.'}</p>{!query && section !== 'Starred' && <><button className={s.openButton} onClick={() => void open()} disabled={busy}>Open a file</button><button className={s.textButton} onClick={() => void open(true)} disabled={busy}>Explore a sample PDF <ChevronRight size={15} /></button></>}</div>}
          <p className={s.foundationNote}>Viewer foundation · Advanced tools are in development{!native ? ' · Browser preview' : ''}</p>
        </section>
      </> : view === 'tools' ? <section className={s.toolsCatalog}><div className={s.catalogHeading}><div><p className={s.eyebrow}>THE COMPLETE WORKSPACE</p><h1>All tools</h1><p>Everything in one place. Advanced tools are planned for later milestones.</p></div><label className={s.search}><Search size={16} /><input aria-label="Search tools" placeholder="Find a tool" value={query} onChange={e => setQuery(e.target.value)} /></label></div>{toolGroups.map(group => <section key={group.name}><h2>{group.name}</h2><div className={s.catalogGrid}>{group.tools.filter(name => name.toLowerCase().includes(query.toLowerCase())).map((name, i) => <div className={s.catalogCard} key={name}>{toolRow(name, i)}<span className={s.planned}>Not available yet</span></div>)}</div></section>)}</section> : doc ? <>
        {toolsOpen && <aside className={s.toolsPanel}><div className={s.panelHeading}><h2>All tools</h2><IconButton icon={PanelLeftClose} label="Collapse all tools" onClick={() => setToolsOpen(false)} /></div>{['Export a PDF', 'Edit a PDF', 'Create a PDF', 'Combine files', 'Organize pages', 'Comment', 'Fill & sign', 'Scan & OCR', 'Protect a PDF', 'Compress a PDF'].map(toolRow)}<button className={s.textButton} onClick={() => setView('tools')}>View all tools <ChevronRight size={15} /></button><div className={s.panelNote}>Advanced tools are not available in this viewer build.</div></aside>}
        <div className={s.documentArea}><Viewer key={doc.id} document={doc} zoom={zoom} fit={fit} target={target} onPage={setPage} hand={hand} /><div className={s.quickToolbar}><IconButton icon={MousePointer2} label="Select text" disabled /><IconButton icon={Hand} label="Pan document" active={hand} onClick={() => setHand(v => !v)} /><span className={s.horizontalDivider} /><IconButton icon={MessageSquare} label="Add comment" disabled /><IconButton icon={Highlighter} label="Highlight text" disabled /><IconButton icon={Pencil} label="Draw" disabled /><IconButton icon={Type} label="Fill in text" disabled /><IconButton icon={Signature} label="Add signature" disabled /><span className={s.horizontalDivider} /><IconButton icon={MoreHorizontal} label="Customize quick tools" disabled /></div></div>
        {nav && <aside className={s.pagesPanel}><div className={s.panelHeading}><h2>Pages</h2><IconButton icon={X} label="Close pages" onClick={() => setNav(false)} /></div><div className={s.pageList}>{doc.pages.map((size, i) => <button className={i === page ? s.currentPage : ''} key={i} onClick={() => go(i)}><File size={24} /><span>Page {i + 1}<small>{(size.width / 72).toFixed(1)} × {(size.height / 72).toFixed(1)} in</small></span></button>)}</div></aside>}
        <aside className={s.rightRail}><div><IconButton icon={MessageSquare} label="Comments" disabled /><IconButton icon={Bookmark} label="Bookmarks" disabled /><IconButton icon={Files} label="Pages" active={nav} onClick={() => setNav(v => !v)} /></div><div className={s.pageControls}><IconButton icon={ChevronLeft} label="Previous page" disabled={page === 0} onClick={() => go(page - 1)} /><input aria-label="Page number" key={`${doc.id}-${page}`} type="number" min={1} max={doc.pages.length} defaultValue={page + 1} onKeyDown={e => { if (e.key === 'Enter') go(Number(e.currentTarget.value) - 1); }} onBlur={e => go(Number(e.currentTarget.value) - 1)} /><span className={s.pageCount}>/ {doc.pages.length}</span><IconButton icon={ChevronRight} label="Next page" disabled={page === doc.pages.length - 1} onClick={() => go(page + 1)} /><span className={s.horizontalDivider} /><IconButton icon={RotateCw} label="Rotate view" disabled /><IconButton icon={Maximize} label="Fit width" active={fit} onClick={() => setFit(true)} /><IconButton icon={ZoomIn} label="Zoom in" onClick={() => changeZoom(zoom + 25)} /><IconButton icon={ZoomOut} label="Zoom out" onClick={() => changeZoom(zoom - 25)} /></div></aside>
      </> : null}
    </main>
    <footer className={s.statusbar}><span>{view === 'document' && doc ? `${(doc.pages[page].width / 72).toFixed(2)} × ${(doc.pages[page].height / 72).toFixed(2)} in` : 'PDF Workstation'}</span><span>{busy ? 'Opening document…' : view === 'document' && doc ? `${doc.name} · Read-only` : 'Files stay on your computer'}</span>{view === 'document' ? <select aria-label="Zoom" value={fit ? 'fit' : zoom} onChange={e => e.target.value === 'fit' ? setFit(true) : changeZoom(Number(e.target.value))}><option value="fit">Fit width</option>{Array.from(new Set([10,25,50,75,100,125,150,200,300,400,zoom])).sort((a,b) => a-b).map(z => <option value={z} key={z}>{z}%</option>)}</select> : <span>Local workspace</span>}</footer>
  </div>;
}
