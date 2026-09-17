import { useCallback, useEffect, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Undo2, Redo2 } from 'lucide-react';
import { ArrowDownToLine, ArrowUpRight, Bookmark, ChevronDown, ChevronLeft, ChevronRight, ChevronsUpDown, CircleHelp, Combine, File, FileCheck2, FileImage, FileOutput, FilePenLine, FilePlus2, Files, FolderOpen, Hand, Highlighter, Home, LayoutGrid, List, Maximize, Menu, MessageSquare, Minus, MoreHorizontal, MousePointer2, PanelLeftClose, Pencil, Plus, Printer, RotateCw, Save, ScanLine, Search, ShieldCheck, Signature, SlidersHorizontal, Star, Sun, Type, X, ZoomIn, ZoomOut, type LucideIcon } from 'lucide-react';
import { closeDocument, native, openDocument, reopenDocument, editPages, saveCopy, splitDocument, type OpenResult, type SplitOutput } from './bridge';
import { clampPage, toolGroups, type DocumentInfo, type PageEdit } from './model';
import Viewer from './Viewer';
import Organizer from './Organizer';
import ConfirmDialog from './ConfirmDialog';
import Updates from './Updates';
import SearchPanel from './SearchPanel';
import BookmarksPanel from './BookmarksPanel';
import PageText from './PageText';
import PasswordDialog from './PasswordDialog';
import PrintDialog from './PrintDialog';
import DocumentProperties from './DocumentProperties';
import DependencyNotices from './DependencyNotices';
import { readPreferences, savePreferences } from './preferences';
import { readRecentFiles, saveRecentFiles, rememberFile } from './recentFiles';
import s from './Workspace.module.css';

const icons: Record<string, LucideIcon> = { 'Create a PDF': FilePlus2, 'Combine files': Combine, 'Organize pages': LayoutGrid, 'Edit a PDF': FilePenLine, 'Export a PDF': FileOutput, 'Scan & OCR': ScanLine, 'Fill & sign': Signature, 'Protect a PDF': ShieldCheck, 'Comment': MessageSquare, 'Compress a PDF': ArrowDownToLine };
function IconButton({ icon: Icon, label, onClick, disabled = false, active = false }: { icon: LucideIcon; label: string; onClick?: () => void; disabled?: boolean; active?: boolean }) {
  return <button className={`${s.iconButton} ${active ? s.activeIcon : ''}`} aria-label={label} title={disabled && !onClick ? `${label} — not implemented yet` : label} onClick={onClick} disabled={disabled}><Icon size={19} strokeWidth={1.7} /></button>;
}

export default function App() {
  const [preferences] = useState(readPreferences);
  const [documents, setDocuments] = useState<DocumentInfo[]>([]);
  const [active, setActive] = useState<number | null>(null);
  const [view, setView] = useState<'home' | 'tools' | 'document'>('home');
  const [section, setSection] = useState('Recent');
  const [query, setQuery] = useState('');
  const [toolsOpen, setToolsOpen] = useState(preferences.toolsOpen);
  const [nav, setNav] = useState(preferences.nav);
  const [searchOpen, setSearchOpen] = useState(false);
  const [bookmarksOpen, setBookmarksOpen] = useState(false);
  const [pageTextOpen, setPageTextOpen] = useState(false);
  const [printOpen, setPrintOpen] = useState(false);
  const [propertiesOpen, setPropertiesOpen] = useState(false);
  const [noticesOpen, setNoticesOpen] = useState(false);
  const [passwordRequest, setPasswordRequest] = useState<{ challenge: Extract<OpenResult, { status: 'password_required' }>; organize: boolean } | null>(null);
  useEffect(() => { if (searchOpen) setBookmarksOpen(false); }, [searchOpen]);
  const [menu, setMenu] = useState(false);
  const [updatesOpen, setUpdatesOpen] = useState(false);
  const [dark, setDark] = useState(preferences.dark);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [zoom, setZoom] = useState(preferences.zoom);
  const [fit, setFit] = useState(preferences.fit);
  const [hand, setHand] = useState(preferences.hand);
  const [page, setPage] = useState(0);
  const readingPages = useRef(new Map<number, number>());
  const [target, setTarget] = useState({ page: 0, token: 0 });
  const [recentFiles, setRecentFiles] = useState(readRecentFiles);
  const [organizing, setOrganizing] = useState(false);
  const [pendingClose, setPendingClose] = useState<number | 'window' | null>(null);
  const closingDocument = useRef<number | null>(null);
  useEffect(() => {
    if (!savePreferences({ dark, zoom, fit, hand, toolsOpen, nav })) setNotice('Your reading preferences could not be saved. They will last for this session only.');
  }, [dark, zoom, fit, hand, toolsOpen, nav]);
  useEffect(() => {
    if (!saveRecentFiles(recentFiles)) setNotice('Recent files and stars could not be saved. They will last for this session only.');
  }, [recentFiles]);
  const latest = useRef({ documents, busy, active });
  latest.current = { documents, busy, active };
  useEffect(() => {
    if (!native) return;
    const unlisten = getCurrentWindow().onCloseRequested(event => {
      if (latest.current.busy || closingDocument.current !== null) { event.preventDefault(); setNotice('Wait for the current operation to finish before closing.'); }
      else if (latest.current.documents.some(document => document.dirty)) { event.preventDefault(); setPendingClose('window'); }
    });
    return () => { void unlisten.then(stop => stop()); };
  }, []);
  const doc = documents.find(d => d.id === active);
  const activate = (id: number) => {
    if (busy || closingDocument.current !== null) return;
    const document = documents.find(item => item.id === id);
    if (!document) return;
    const next = clampPage(readingPages.current.get(id) ?? 0, document.pages.length);
    setActive(id); setView('document'); setPage(next); setTarget(value => ({ page: next, token: value.token + 1 }));
  };
  const trackPage = (value: number) => { if (doc) { const next = clampPage(value, doc.pages.length); readingPages.current.set(doc.id, next); setPage(next); } };
  const opened = (info: DocumentInfo, organize: boolean) => {
    setDocuments(list => [...list, info]); setRecentFiles(list => rememberFile(list, info)); setOrganizing(organize);
    setActive(info.id); setView('document'); setPage(0); setTarget(value => ({ page: 0, token: value.token + 1 }));
  };
  const acceptOpen = (result: OpenResult | null, organize: boolean) => {
    if (result?.status === 'opened') opened(result.document, organize);
    else if (result?.status === 'password_required') setPasswordRequest({ challenge: result, organize });
  };
  const open = useCallback(async (example = false, organize = false) => {
    if (busy || closingDocument.current !== null) return;
    setBusy(true); setError(''); setMenu(false);
    try {
      acceptOpen(await openDocument(example), organize);
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  }, [busy]);
  const reopen = async (path: string) => {
    if (busy || closingDocument.current !== null) return;
    const existing = documents.find(document => document.path === path);
    if (existing) { activate(existing.id); return; }
    setBusy(true); setError('');
    try {
      acceptOpen(await reopenDocument(path), false);
    } catch (e) { setError(`Could not reopen this file. It may have moved or been deleted. ${String(e)}`); }
    finally { setBusy(false); }
  };
  const close = async (id: number, discard = false) => {
    if (busy || closingDocument.current !== null) return;
    if (!discard && documents.find(document => document.id === id)?.dirty) { setPendingClose(id); return; }
    closingDocument.current = id; setBusy(true); setError('');
    try {
      await closeDocument(id); readingPages.current.delete(id); setDocuments(list => list.filter(d => d.id !== id));
      if (latest.current.active === id) { setActive(null); setView('home'); }
    } catch (e) { setError(String(e)); }
    finally { closingDocument.current = null; setBusy(false); }
  };
  const updateDocument = (info: DocumentInfo) => {
    setDocuments(list => list.map(document => document.id === info.id ? info : document));
    const next = clampPage(page, info.pages.length); readingPages.current.set(info.id, next); setPage(next); setTarget(value => ({ page: next, token: value.token + 1 }));
  };
  const edit = async (action: PageEdit): Promise<boolean> => {
    if (!doc || busy || closingDocument.current !== null) return false;
    setBusy(true); setError(''); setNotice('');
    try { updateDocument(await editPages(doc.id, action)); return true; }
    catch (e) { setError(String(e)); return false; } finally { setBusy(false); }
  };
  const save = async (pages?: number[]) => {
    if (!doc || busy || closingDocument.current !== null) return;
    setBusy(true); setError(''); setNotice('');
    try {
      const result = await saveCopy(doc.id, pages);
      if (result) { updateDocument(result.document); setNotice(`Saved ${pages ? 'selected pages' : 'a copy'} to ${result.path}`); }
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const split = async (pagesPerFile: number): Promise<SplitOutput | null> => {
    if (!doc || busy || closingDocument.current !== null) return null;
    setBusy(true); setError(''); setNotice('');
    try { return await splitDocument(doc.id, doc.revision, pagesPerFile); }
    finally { setBusy(false); }
  };
  const launchOrganizer = () => { if (busy || closingDocument.current !== null) return; if (doc) { setOrganizing(true); setView('document'); } else void open(false, true); };
  const go = (value: number) => { if (doc) { const next = clampPage(value, doc.pages.length); trackPage(next); setTarget(v => ({ page: next, token: v.token + 1 })); } };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (updatesOpen || pageTextOpen || printOpen || propertiesOpen || noticesOpen || passwordRequest || pendingClose !== null) return;
      if ((event.target as HTMLElement | null)?.closest?.('dialog')) return;
      if (event.ctrlKey && event.key.toLowerCase() === 'o') { event.preventDefault(); void open(); }
      if (event.ctrlKey && event.key.toLowerCase() === 'f' && doc) { event.preventDefault(); setView('document'); setOrganizing(false); setSearchOpen(true); return; }
      if (event.ctrlKey && event.key.toLowerCase() === 'p' && doc) { event.preventDefault(); if (!busy) setPrintOpen(true); return; }
      if (event.ctrlKey && event.key.toLowerCase() === 'd' && doc) { event.preventDefault(); if (!busy) setPropertiesOpen(true); return; }
      if (event.key === 'Escape' && searchOpen) { setSearchOpen(false); return; }
      if ((event.target as HTMLElement).matches('input,select,textarea')) return;
      if (event.key === 'F4') { event.preventDefault(); event.shiftKey ? setToolsOpen(v => !v) : setNav(v => !v); }
      if (event.ctrlKey && event.key === '2') { event.preventDefault(); setFit(true); }
      if (event.ctrlKey && event.key === '1') { event.preventDefault(); setFit(false); setZoom(100); }
      if (view !== 'document' || !doc) return;
      if (event.ctrlKey && event.key.toLowerCase() === 's') { event.preventDefault(); void save(); return; }
      if (event.ctrlKey && event.key.toLowerCase() === 'z') { event.preventDefault(); void edit({ kind: event.shiftKey ? 'redo' : 'undo' }); return; }
      if (event.ctrlKey && event.key.toLowerCase() === 'y') { event.preventDefault(); void edit({ kind: 'redo' }); return; }
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
  const listed = recentFiles.filter(d => d.name.toLowerCase().includes(query.toLowerCase()) && (section !== 'Starred' || d.starred));
  const changeZoom = (value: number) => { setFit(false); setZoom(Math.max(10, Math.min(400, value))); };
  const toolRow = (name: string, index: number) => {
    const Icon = icons[name] || FileCheck2;
    return <button key={name} className={s.toolRow} disabled={name !== 'Organize pages' || busy} onClick={launchOrganizer} title={name === 'Organize pages' ? name : `${name} — planned, not implemented yet`}><span className={s.toolIcon} style={{ color: ['#7361b3', '#277bb4', '#239576', '#bc6b25'][index % 4] }}><Icon size={21} strokeWidth={1.7} /></span><span>{name}</span></button>;
  };
  return <div className={`${s.app} ${dark ? s.dark : ''}`}>
    <header className={s.tabbar}>
      <div className={s.appMark}><Files size={21} /></div>
      <button className={s.menuButton} onClick={() => setMenu(v => !v)} aria-expanded={menu}><Menu size={17} /> Menu</button>
      <button className={`${s.homeTab} ${view === 'home' ? s.selectedTab : ''}`} aria-label="Home" onClick={() => setView('home')}><Home size={19} /></button>
      <div className={s.documentTabs}>{documents.map(document => <div key={document.id} className={`${s.documentTab} ${view === 'document' && active === document.id ? s.selectedTab : ''}`}><button disabled={busy} onClick={() => activate(document.id)}><File size={15} /><span>{document.name}{document.dirty ? ' *' : ''}</span></button><IconButton icon={X} label={`Close ${document.name}`} disabled={busy} onClick={() => void close(document.id)} /></div>)}</div>
      <button className={s.createButton} disabled title="Create a PDF — not implemented yet"><Plus size={17} /> Create</button>
      <span className={s.windowTitle}>PDF Workstation</span>
    </header>
    {menu && <div className={s.menuPopover}>
      <button onClick={() => void open()}>Open… <kbd>Ctrl+O</kbd></button><button onClick={() => void open(true)}>Open sample PDF</button>
      <button disabled={!doc || busy} onClick={() => { setMenu(false); void save(); }}>Save a copy… <kbd>Ctrl+S</kbd></button>
      <button onClick={() => { setMenu(false); launchOrganizer(); }}>Organize pages</button>
      <button disabled={!doc || busy} onClick={() => { setMenu(false); setPropertiesOpen(true); }}>Document properties… <kbd>Ctrl+D</kbd></button><hr />
      <button onClick={() => { setDark(v => !v); setMenu(false); }}>Switch to {dark ? 'light' : 'dark'} theme</button>
      <button disabled={busy} onClick={() => { setMenu(false); setUpdatesOpen(true); }}>Check for updates…</button>
      <button disabled={busy} onClick={() => { setMenu(false); setNoticesOpen(true); }}>Third-party notices…</button>
      <button onClick={() => { setNotice('PDF viewing, embedded-text search, and Organize Pages are available. Rotate, reorder, delete, extract, undo/redo, and save a new copy. Text editing, OCR, forms, and signatures are not implemented yet.'); setMenu(false); }}>About this build</button>
    </div>}
    {updatesOpen && <Updates dirty={documents.some(document => document.dirty)} busy={busy} setBusy={setBusy} close={() => setUpdatesOpen(false)} />}
    {pageTextOpen && doc && <PageText key={`${doc.id}-${doc.revision}-${page}`} document={doc} page={page} close={() => setPageTextOpen(false)} />}
    {passwordRequest && <PasswordDialog key={passwordRequest.challenge.request_id} challenge={passwordRequest.challenge} onOpened={info => { opened(info, passwordRequest.organize); setPasswordRequest(null); }} onClose={() => setPasswordRequest(null)} />}
    {printOpen && doc && <PrintDialog document={doc} page={page} setBusy={setBusy} close={() => setPrintOpen(false)} />}
    {propertiesOpen && doc && <DocumentProperties key={`${doc.id}-${doc.revision}`} document={doc} close={() => setPropertiesOpen(false)} />}
    {noticesOpen && <DependencyNotices close={() => setNoticesOpen(false)} />}
    <div className={s.globalbar}>
      <nav className={s.primaryNav}><button className={toolsOpen && view !== 'home' ? s.selectedNav : ''} onClick={() => view === 'document' ? setToolsOpen(v => !v) : setView('tools')}>All tools</button><button disabled>Edit</button><button disabled>Convert</button><button disabled>E-sign</button></nav>
      <div className={s.globalActions}><IconButton icon={Search} label="Find text" disabled={!doc || busy} active={searchOpen} onClick={() => { setView('document'); setOrganizing(false); setSearchOpen(v => !v); }} /><span className={s.divider} /><IconButton icon={Undo2} label="Undo" disabled={busy || !doc?.can_undo} onClick={() => void edit({ kind: 'undo' })} /><IconButton icon={Redo2} label="Redo" disabled={busy || !doc?.can_redo} onClick={() => void edit({ kind: 'redo' })} /><IconButton icon={Save} label="Save a copy" disabled={busy || !doc} onClick={() => void save()} /><IconButton icon={Printer} label="Print" disabled={busy || !doc} onClick={() => setPrintOpen(true)} /><IconButton icon={Sun} label="Toggle theme" onClick={() => setDark(v => !v)} /><IconButton icon={CircleHelp} label="Build information" onClick={() => setNotice('Organize Pages and document search are available. Other tools marked unavailable are planned for later milestones. Save a Copy writes a new file and preserves your original.')} /><button className={s.openButton} onClick={() => void open()} disabled={busy}><FolderOpen size={16} /> {busy ? 'Working…' : 'Open a file'}</button></div>
    </div>
    {(error || notice) && <div className={`${s.banner} ${error ? s.error : ''}`} role={error ? 'alert' : 'status'}><span>{error || notice}</span><IconButton icon={X} label="Dismiss message" onClick={() => { setError(''); setNotice(''); }} /></div>}
    <main className={s.main}>
      {view === 'home' ? <>
        <aside className={s.homeSidebar}><h2>Home</h2>{['Recent', 'Starred'].map(item => <button key={item} className={section === item ? s.sidebarSelected : ''} onClick={() => setSection(item)}>{item === 'Recent' ? <Home size={18} /> : <Star size={18} />}{item}</button>)}<div className={s.sidebarCaption}>FILES</div><button onClick={() => void open()}><FolderOpen size={18} /> Your computer</button><div className={s.sidebarBottom}><ShieldCheck size={16} /><span>Local files. Yours to keep.</span></div></aside>
        <section className={s.homeContent}><div className={s.homeHeading}><div><p className={s.eyebrow}>YOUR WORKSPACE</p><h1>Work with your PDFs.</h1></div><button className={s.outlineButton} onClick={() => setView('tools')}>See all tools <ArrowUpRight size={16} /></button></div>
          <div className={s.quickCards}>{['Edit a PDF', 'Export a PDF', 'Combine files', 'Fill & sign'].map((name, i) => { const Icon = icons[name]; return <div className={s.quickCard} key={name}><span style={{ color: ['#7260b4', '#2c80b2', '#25876b', '#b86a25'][i] }}><Icon size={29} strokeWidth={1.5} /></span><h3>{name}</h3><p>{['Update text and images.', 'Convert to another format.', 'Bring documents together.', 'Complete your paperwork.'][i]}</p><span className={s.planned}>Planned</span></div>; })}</div>
          <div className={s.recentHeader}><h2>{section}</h2><div className={s.recentControls}><button className={s.textButton} disabled={!recentFiles.length} onClick={() => setRecentFiles([])}>Clear file history</button><label className={s.search}><Search size={16} /><input aria-label="Search recent files" placeholder="Search your files" value={query} onChange={e => setQuery(e.target.value)} /></label><IconButton icon={List} label="List view" active /></div></div>
          <div className={s.tableHeading}><span>NAME</span><span>LOCATION</span><span>PAGES</span><span /></div>
          {listed.map(file => <div className={s.fileRow} key={file.path}><button disabled={busy} onClick={() => void reopen(file.path)}><FileImage size={25} /><span>{file.name}<small>PDF document</small></span></button><span title={file.path}>This computer</span><span>{file.pages}</span><IconButton icon={Star} label={`${file.starred ? 'Unstar' : 'Star'} ${file.name}`} active={file.starred} onClick={() => setRecentFiles(list => list.map(item => item.path === file.path ? { ...item, starred: !item.starred } : item))} /></div>)}
          {!listed.length && <div className={s.empty}><div className={s.emptyIcon}><Files size={36} strokeWidth={1.25} /></div><h3>{query ? 'No matching files' : section === 'Starred' ? 'Keep important files close' : 'Your documents start here'}</h3><p>{query ? 'Try another file name.' : section === 'Starred' ? 'Star an open file to find it here.' : 'Open a PDF from your computer to start reading.'}</p>{!query && section !== 'Starred' && <><button className={s.openButton} onClick={() => void open()} disabled={busy}>Open a file</button><button className={s.textButton} onClick={() => void open(true)} disabled={busy}>Explore a sample PDF <ChevronRight size={15} /></button></>}</div>}
          <p className={s.foundationNote}>Viewer + Organize Pages · More tools are in development{!native ? ' · Browser preview' : ''}</p>
        </section>
      </> : view === 'tools' ? <section className={s.toolsCatalog}><div className={s.catalogHeading}><div><p className={s.eyebrow}>THE COMPLETE WORKSPACE</p><h1>All tools</h1><p>Organize Pages is ready. Other advanced tools are planned for later milestones.</p></div><label className={s.search}><Search size={16} /><input aria-label="Search tools" placeholder="Find a tool" value={query} onChange={e => setQuery(e.target.value)} /></label></div>{toolGroups.map(group => <section key={group.name}><h2>{group.name}</h2><div className={s.catalogGrid}>{group.tools.filter(name => name.toLowerCase().includes(query.toLowerCase())).map((name, i) => <div className={s.catalogCard} key={name}>{toolRow(name, i)}<span className={s.planned}>{name === 'Organize pages' ? 'Available' : 'Not available yet'}</span></div>)}</div></section>)}</section> : doc ? <>
        {toolsOpen && <aside className={s.toolsPanel}><div className={s.panelHeading}><h2>All tools</h2><IconButton icon={PanelLeftClose} label="Collapse all tools" onClick={() => setToolsOpen(false)} /></div>{['Export a PDF', 'Edit a PDF', 'Create a PDF', 'Combine files', 'Organize pages', 'Comment', 'Fill & sign', 'Scan & OCR', 'Protect a PDF', 'Compress a PDF'].map(toolRow)}<button className={s.textButton} onClick={() => setView('tools')}>View all tools <ChevronRight size={15} /></button><div className={s.panelNote}>Organize Pages is available. More tools are in development.</div></aside>}
        {organizing ? <Organizer key={doc.id} document={doc} busy={busy} edit={edit} save={save} split={split} close={() => setOrganizing(false)} /> : <div className={s.documentArea}><Viewer key={`${doc.id}-${doc.revision}`} document={doc} zoom={zoom} fit={fit} target={target} onPage={trackPage} hand={hand} /><div className={s.quickToolbar}><IconButton icon={MousePointer2} label="Select text on page" active={!hand} onClick={() => setHand(false)} /><IconButton icon={Hand} label="Pan document" active={hand} onClick={() => setHand(true)} /><IconButton icon={Type} label="Read and copy page text" onClick={() => setPageTextOpen(true)} /><span className={s.horizontalDivider} /><IconButton icon={MessageSquare} label="Add comment" disabled /><IconButton icon={Highlighter} label="Highlight text" disabled /><IconButton icon={Pencil} label="Draw" disabled /><IconButton icon={Type} label="Fill in text" disabled /><IconButton icon={Signature} label="Add signature" disabled /><span className={s.horizontalDivider} /><IconButton icon={MoreHorizontal} label="Customize quick tools" disabled /></div></div>}
        {!organizing && bookmarksOpen && <BookmarksPanel key={`${doc.id}-${doc.revision}`} document={doc} go={go} close={() => setBookmarksOpen(false)} />}
        {!organizing && searchOpen && !bookmarksOpen && <SearchPanel key={`${doc.id}-${doc.revision}`} document={doc} go={go} close={() => setSearchOpen(false)} />}
        {!organizing && nav && !searchOpen && !bookmarksOpen && <aside className={s.pagesPanel}><div className={s.panelHeading}><h2>Pages</h2><IconButton icon={X} label="Close pages" onClick={() => setNav(false)} /></div><button onClick={() => setBookmarksOpen(true)}>Bookmarks</button><div className={s.pageList}>{doc.pages.map((size, i) => <button className={i === page ? s.currentPage : ''} key={i} onClick={() => go(i)}><File size={24} /><span>Page {i + 1}<small>{(size.width / 72).toFixed(1)} × {(size.height / 72).toFixed(1)} in</small></span></button>)}</div></aside>}
        <aside className={s.rightRail}><div><IconButton icon={MessageSquare} label="Comments" disabled /><IconButton icon={Bookmark} label="Bookmarks" active={bookmarksOpen} onClick={() => { setSearchOpen(false); setOrganizing(false); setBookmarksOpen(v => !v); }} /><IconButton icon={Files} label="Pages" active={nav && !bookmarksOpen && !searchOpen} onClick={() => { setSearchOpen(false); setBookmarksOpen(false); setNav(v => !v); }} /></div><div className={s.pageControls}><IconButton icon={ChevronLeft} label="Previous page" disabled={page === 0} onClick={() => go(page - 1)} /><input aria-label="Page number" key={`${doc.id}-${page}`} type="number" min={1} max={doc.pages.length} defaultValue={page + 1} onKeyDown={e => { if (e.key === 'Enter') go(Number(e.currentTarget.value) - 1); }} onBlur={e => go(Number(e.currentTarget.value) - 1)} /><span className={s.pageCount}>/ {doc.pages.length}</span><IconButton icon={ChevronRight} label="Next page" disabled={page === doc.pages.length - 1} onClick={() => go(page + 1)} /><span className={s.horizontalDivider} /><IconButton icon={RotateCw} label="Rotate view" disabled /><IconButton icon={Maximize} label="Fit width" active={fit} onClick={() => setFit(true)} /><IconButton icon={ZoomIn} label="Zoom in" onClick={() => changeZoom(zoom + 25)} /><IconButton icon={ZoomOut} label="Zoom out" onClick={() => changeZoom(zoom - 25)} /></div></aside>
      </> : null}
    </main>
    <footer className={s.statusbar}><span>{view === 'document' && doc ? `${(doc.pages[clampPage(page, doc.pages.length)].width / 72).toFixed(2)} × ${(doc.pages[clampPage(page, doc.pages.length)].height / 72).toFixed(2)} in` : 'PDF Workstation'}</span><span>{busy ? 'Working…' : view === 'document' && doc ? `${doc.name} · ${doc.dirty ? 'Unsaved changes' : 'Source preserved'}` : 'Files stay on your computer'}</span>{view === 'document' ? <select aria-label="Zoom" value={fit ? 'fit' : zoom} onChange={e => e.target.value === 'fit' ? setFit(true) : changeZoom(Number(e.target.value))}><option value="fit">Fit width</option>{Array.from(new Set([10,25,50,75,100,125,150,200,300,400,zoom])).sort((a,b) => a-b).map(z => <option value={z} key={z}>{z}%</option>)}</select> : <span>Local workspace</span>}</footer>
    {pendingClose !== null && <ConfirmDialog title="Discard unsaved page edits?" message="Save a copy before closing to keep your changes. Your original PDF has not been modified." confirmLabel="Discard and close" onCancel={() => setPendingClose(null)} onConfirm={() => { const pending = pendingClose; setPendingClose(null); if (pending === 'window') void getCurrentWindow().destroy(); else void close(pending, true); }} />}
  </div>;
}
