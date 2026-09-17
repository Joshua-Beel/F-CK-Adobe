import type { DocumentInfo } from './model';

export type RecentFile = { path: string; name: string; pages: number; starred: boolean };
const key = 'pdf-workstation.recent-files.v1';
export function readRecentFiles(): RecentFile[] {
  try {
    const value: unknown = JSON.parse(window.localStorage.getItem(key) || '[]');
    if (!Array.isArray(value)) return [];
    const seen = new Set<string>();
    const result: RecentFile[] = [];
    for (const item of value) {
      if (!item || typeof item.path !== 'string' || !item.path.trim() || item.path.includes('\0') || typeof item.name !== 'string' || !item.name.trim() || !Number.isSafeInteger(item.pages) || item.pages < 1 || typeof item.starred !== 'boolean' || seen.has(item.path)) continue;
      result.push({ path: item.path, name: item.name, pages: item.pages, starred: item.starred });
      seen.add(item.path);
      if (result.length === 50) break;
    }
    return result;
  } catch { return []; }
}
export function saveRecentFiles(files: RecentFile[]): boolean {
  try { window.localStorage.setItem(key, JSON.stringify(files.slice(0, 50))); return true; }
  catch { return false; }
}
export function rememberFile(files: RecentFile[], document: DocumentInfo): RecentFile[] {
  return [{ path: document.path, name: document.name, pages: document.pages.length, starred: files.find(file => file.path === document.path)?.starred ?? false }, ...files.filter(file => file.path !== document.path)].slice(0, 50);
}
